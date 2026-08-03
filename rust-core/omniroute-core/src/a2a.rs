use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

/// M7: Google A2A (Agent2Agent) protocol — JSON-RPC over HTTP.
///
/// Exposes the router to other agents with the same 6 skills OmniRoute
/// advertises: providerDiscovery, smartRouting, quotaManagement,
/// costAnalysis, healthReport, listCapabilities.
pub struct A2a;

const PROTOCOL_VERSION: &str = "0.3";

/// In-memory task store (submitted → working → completed/failed).
static TASKS: std::sync::LazyLock<Mutex<HashMap<String, Value>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn rpc_ok(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_err(id: Option<Value>, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub fn agent_card() -> Value {
    json!({
        "name": "omniroute-rs",
        "description": "Smart AI provider router: fallback, auto-combo scoring, session affinity, caching, budgets",
        "url": "/a2a",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": PROTOCOL_VERSION,
        "skills": [
            { "id": "listCapabilities", "name": "List Capabilities", "description": "List all skills this agent supports" },
            { "id": "providerDiscovery", "name": "Provider Discovery", "description": "List available model providers and model counts" },
            { "id": "smartRouting", "name": "Smart Routing", "description": "Inspect routing candidates for a model (fallback order)" },
            { "id": "quotaManagement", "name": "Quota Management", "description": "Per-provider usage, cooldowns and backoff levels" },
            { "id": "costAnalysis", "name": "Cost Analysis", "description": "Monthly spend, token totals, budget status per provider" },
            { "id": "healthReport", "name": "Health Report", "description": "Proxy health, uptime, error rates, per-provider latency" }
        ]
    })
}

async fn dispatch_skill(skill: &str, state: &crate::proxy::AppState) -> Result<String, String> {
    match skill {
        "listCapabilities" => {
            let skills: Vec<String> = agent_card()["skills"]
                .as_array()
                .map(|s| {
                    s.iter()
                        .filter_map(|x| x["id"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Ok(format!("Skills: {}", skills.join(", ")))
        }
        "providerDiscovery" => {
            let providers = omniroute_providers::list_providers();
            let lines: Vec<String> = providers
                .iter()
                .map(|p| format!("{}: {} models", p.id, p.models.len()))
                .collect();
            Ok(format!(
                "{} providers\n{}",
                providers.len(),
                lines.join("\n")
            ))
        }
        "smartRouting" => {
            let model = "gpt-4o";
            let combo = state.combo.read().await;
            let candidates = combo.candidates(model);
            if candidates.is_empty() {
                return Ok(format!(
                    "No candidate connections for {model} (add a provider first)"
                ));
            }
            Ok(format!(
                "{model} candidate chain:\n{}",
                candidates.join(" → ")
            ))
        }
        "quotaManagement" => {
            let stats = crate::telemetry::TELEMETRY.stats();
            let prov = stats
                .get("by_provider")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            if prov.is_empty() {
                return Ok("No provider usage recorded yet".into());
            }
            let lines: Vec<String> = prov
                .iter()
                .map(|p| {
                    format!(
                        "{}: {} req, avg {}ms",
                        p["provider"], p["requests"], p["avg_duration_ms"]
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }
        "costAnalysis" => {
            let month = chrono::Utc::now().format("%Y-%m").to_string();
            let Some(db) = &state.db else {
                return Err("no database".into());
            };
            let report = crate::costs::Costs::report(db, &month);
            Ok(serde_json::to_string_pretty(&report).unwrap_or_default())
        }
        "healthReport" => {
            let stats = crate::telemetry::TELEMETRY.stats();
            let uptime = stats.get("uptime_seconds").cloned().unwrap_or(json!(0));
            Ok(format!(
                "uptime: {}s | requests: {} | errors: {} | avg latency: {}ms",
                uptime,
                stats.get("total_requests").unwrap_or(&json!(0)),
                stats.get("total_errors").unwrap_or(&json!(0)),
                stats.get("avg_duration_ms").unwrap_or(&json!(0.0)),
            ))
        }
        _ => Err(format!("unknown skill: {skill}")),
    }
}

impl A2a {
    #[allow(clippy::collapsible_if)]
    pub async fn handle(req: &Value, state: &crate::proxy::AppState) -> Value {
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

        match method {
            "agent/getCard" => rpc_ok(id, agent_card()),
            "skills/call" => {
                let skill = params.get("skill").and_then(|s| s.as_str()).unwrap_or("");
                match dispatch_skill(skill, state).await {
                    Ok(text) => rpc_ok(id, json!({ "skill": skill, "result": text })),
                    Err(e) => rpc_err(id, -32602, e),
                }
            }
            "message/send" => {
                let task_id = format!("task-{}", uuid::Uuid::new_v4());
                let message = params
                    .get("message")
                    .and_then(|m| m.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let model = params
                    .get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("gpt-4o");

                if message.is_empty() {
                    return rpc_err(id, -32602, "message.text required".into());
                }

                // record task as working
                if let Ok(mut tasks) = TASKS.lock() {
                    tasks.insert(
                        task_id.clone(),
                        json!({ "id": task_id, "status": "working", "model": model }),
                    );
                }

                let req = crate::chat::ChatRequest {
                    model: model.to_string(),
                    messages: vec![crate::chat::Message {
                        role: "user".into(),
                        content: Some(crate::chat::Content::Text(message.to_string())),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    }],
                    stream: Some(false),
                    max_tokens: None,
                    temperature: None,
                    top_p: None,
                    stop: None,
                    tools: None,
                    tool_choice: None,
                    extra: None,
                    cache: false,
                    cache_ttl: None,
                };

                let mut combo = state.combo.write().await;
                let outcome = combo.execute(&req, None).await;
                drop(combo);

                match outcome {
                    Ok(r) => {
                        let text = r
                            .response
                            .choices
                            .first()
                            .and_then(|c| c.message.content.clone())
                            .map(|c| c.to_string())
                            .unwrap_or_default();
                        if let Ok(mut tasks) = TASKS.lock() {
                            if let Some(t) = tasks.get_mut(&task_id) {
                                t["status"] = json!("completed");
                                t["artifacts"] = json!([{ "name": "response", "text": text }]);
                                t["provider"] = json!(r.used_provider);
                            }
                        }
                        rpc_ok(
                            id,
                            json!({
                                "taskId": task_id,
                                "status": "completed",
                                "artifacts": [{ "name": "response", "text": text }],
                                "provider": r.used_provider,
                            }),
                        )
                    }
                    Err(e) => {
                        if let Ok(mut tasks) = TASKS.lock() {
                            if let Some(t) = tasks.get_mut(&task_id) {
                                t["status"] = json!("failed");
                                t["error"] = json!(e.to_string());
                            }
                        }
                        rpc_err(id, -32000, format!("routing failed: {e}"))
                    }
                }
            }
            "message/get" => {
                let task_id = params.get("taskId").and_then(|t| t.as_str()).unwrap_or("");
                if let Ok(tasks) = TASKS.lock() {
                    if let Some(t) = tasks.get(task_id) {
                        return rpc_ok(id, t.clone());
                    }
                }
                rpc_err(id, -32001, format!("task not found: {task_id}"))
            }
            _ => rpc_err(id, -32601, format!("method not found: {method}")),
        }
    }
}

/// axum handler for POST /a2a.
pub async fn handle_a2a(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    Ok(Json(A2a::handle(&body, &state).await))
}

/// GET /.well-known/agent-card.json — A2A agent discovery.
pub async fn handle_agent_card() -> Json<Value> {
    Json(agent_card())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_card() {
        let card = agent_card();
        assert_eq!(card["name"], "omniroute-rs");
        let skills = card["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 6);
        let ids: Vec<&str> = skills.iter().filter_map(|s| s["id"].as_str()).collect();
        assert!(ids.contains(&"providerDiscovery"));
        assert!(ids.contains(&"costAnalysis"));
        assert!(ids.contains(&"listCapabilities"));
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let state = crate::proxy::AppState::new("test");
        let resp = A2a::handle(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "nope" }),
            &state,
        )
        .await;
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn test_message_get_unknown_task() {
        let state = crate::proxy::AppState::new("test");
        let resp = A2a::handle(
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "message/get", "params": { "taskId": "task-nope" } }),
            &state,
        )
        .await;
        assert_eq!(resp["error"]["code"], -32001);
    }

    #[tokio::test]
    async fn test_message_send_requires_text() {
        let state = crate::proxy::AppState::new("test");
        let resp = A2a::handle(
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "message/send", "params": { "message": {} } }),
            &state,
        )
        .await;
        assert_eq!(resp["error"]["code"], -32602);
    }
}
