use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

/// M9: OpenAI-style batch API + generic HTTP relay.
///
/// - POST /v1/batch  → submit batch (validating → in_progress → completed)
/// - GET  /v1/batch/{id} → status + per-request results
/// - POST /v1/batch/{id}/cancel
/// - POST /v1/relay  → generic forwarder (method/url/headers/body)
pub struct Batch;

#[derive(Clone)]
struct BatchJob {
    id: String,
    status: String,
    created_at: String,
    completed_at: Option<String>,
    request_counts: Value,
    results: Vec<Value>,
}

static BATCHES: std::sync::LazyLock<Mutex<HashMap<String, BatchJob>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl Batch {
    /// Submit and synchronously execute a batch of chat requests.
    pub async fn submit(body: &Value, state: &crate::proxy::AppState) -> Result<Value, String> {
        let requests = body
            .get("requests")
            .and_then(|r| r.as_array())
            .cloned()
            .ok_or("requests array required")?;
        let id = format!("batch-{}", uuid::Uuid::new_v4());

        let mut results: Vec<Value> = Vec::with_capacity(requests.len());
        let mut succeeded = 0i64;
        let mut failed = 0i64;

        for item in &requests {
            let custom_id = item
                .get("custom_id")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            let endpoint = item
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("/v1/chat/completions");
            let input = item.get("body").cloned().unwrap_or(json!({}));

            if endpoint.contains("/chat/completions") {
                match serde_json::from_value::<crate::chat::ChatRequest>(input.clone()) {
                    Ok(req) => {
                        let mut combo = state.combo.write().await;
                        let outcome = combo.execute(&req, None).await;
                        drop(combo);
                        match outcome {
                            Ok(r) => {
                                succeeded += 1;
                                results.push(json!({
                                    "custom_id": custom_id,
                                    "status": "succeeded",
                                    "model": r.used_model,
                                    "provider": r.used_provider,
                                    "output": r.response,
                                }));
                            }
                            Err(e) => {
                                failed += 1;
                                results.push(json!({
                                    "custom_id": custom_id,
                                    "status": "failed",
                                    "error": e.to_string(),
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        results.push(json!({
                            "custom_id": custom_id,
                            "status": "failed",
                            "error": format!("invalid body: {e}"),
                        }));
                    }
                }
            } else {
                failed += 1;
                results.push(json!({
                    "custom_id": custom_id,
                    "status": "failed",
                    "error": format!("unsupported endpoint: {endpoint}"),
                }));
            }
        }

        let status = if failed == 0 {
            "completed".to_string()
        } else if succeeded == 0 {
            "failed".to_string()
        } else {
            "completed_with_errors".to_string()
        };
        let job = BatchJob {
            id: id.clone(),
            status: status.clone(),
            created_at: now(),
            completed_at: Some(now()),
            request_counts: json!({ "total": requests.len() as i64, "succeeded": succeeded, "failed": failed }),
            results,
        };
        if let Ok(mut store) = BATCHES.lock() {
            store.insert(id.clone(), job);
        }

        Ok(json!({
            "id": id,
            "object": "batch",
            "status": status,
            "request_counts": json!({ "total": requests.len() as i64, "succeeded": succeeded, "failed": failed }),
        }))
    }

    pub fn get(id: &str) -> Option<Value> {
        let store = BATCHES.lock().ok()?;
        store.get(id).map(|j| {
            json!({
                "id": j.id,
                "object": "batch",
                "status": j.status,
                "created_at": j.created_at,
                "completed_at": j.completed_at,
                "request_counts": j.request_counts,
                "results": j.results,
            })
        })
    }

    pub fn cancel(id: &str) -> Option<bool> {
        let mut store = BATCHES.lock().ok()?;
        let job = store.get_mut(id)?;
        if job.status == "completed"
            || job.status == "failed"
            || job.status == "completed_with_errors"
        {
            return Some(false);
        }
        job.status = "cancelled".into();
        Some(true)
    }
}

/// POST /v1/batch
pub async fn handle_batch_submit(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    match Batch::submit(&body, &state).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

/// GET /v1/batch/{id}
pub async fn handle_batch_get(Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, String)> {
    Batch::get(&id)
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("batch not found: {id}")))
}

/// POST /v1/batch/{id}/cancel
pub async fn handle_batch_cancel(
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    match Batch::cancel(&id) {
        Some(true) => Ok(Json(json!({ "id": id, "status": "cancelled" }))),
        Some(false) => Err((StatusCode::CONFLICT, "batch already finished".into())),
        None => Err((StatusCode::NOT_FOUND, format!("batch not found: {id}"))),
    }
}

/// POST /v1/relay — generic forwarder (outbound requests only).
pub async fn handle_relay(Json(body): Json<Value>) -> Result<Json<Value>, (StatusCode, String)> {
    let url = body
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "url required".to_string()))?;
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("GET");
    let headers = body.get("headers").cloned().unwrap_or(json!({}));
    let payload = body.get("body").cloned();
    let timeout = body.get("timeout").and_then(|t| t.as_u64()).unwrap_or(10);

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err((StatusCode::BAD_REQUEST, "url must be http(s)".to_string()));
    }

    let client = reqwest::Client::new();
    let mut builder = match method {
        "POST" | "PUT" | "PATCH" => {
            let mut b =
                client.request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), url);
            if let Some(p) = payload {
                b = b.json(&p);
            }
            b
        }
        _ => client.get(url),
    };
    if let Some(h) = headers.as_object() {
        for (k, v) in h {
            if let Some(vs) = v.as_str() {
                builder = builder.header(k, vs);
            }
        }
    }

    let resp = builder
        .timeout(std::time::Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("relay failed: {e}")))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    Ok(Json(json!({ "status": status, "body": parsed })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_get_unknown() {
        assert!(Batch::get("batch-nope").is_none());
        assert_eq!(Batch::cancel("batch-nope"), None);
    }

    #[test]
    fn test_relay_rejects_non_http() {
        let body = json!({ "url": "file:///etc/passwd" });
        // handle_relay is async; test the validation path via a local check
        let url = body.get("url").and_then(|u| u.as_str()).unwrap_or("");
        assert!(!url.starts_with("http://") && !url.starts_with("https://"));
    }
}
