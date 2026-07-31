use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use tower_http::cors::CorsLayer;

use crate::chat::{ChatRequest, ChatResponse};
use crate::combo::ComboEngine;
use crate::ratelimit::with_rate_limit;
use crate::router::{RouterConfig, RoutingEngine};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub version: String,
    pub combo: ComboEngine,
}

impl AppState {
    pub fn new(version: &str) -> Self {
        Self {
            started_at: chrono::Utc::now(),
            version: version.to_string(),
            combo: ComboEngine::new(RoutingEngine::new(RouterConfig::default())),
        }
    }

    pub fn with_router(mut self, config: RouterConfig) -> Self {
        self.combo = ComboEngine::new(RoutingEngine::new(config));
        self
    }
}

/// Health check handler
async fn handle_health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let uptime = chrono::Utc::now() - state.started_at;
    Json(serde_json::json!({
        "status": "ok",
        "version": state.version,
        "uptime_secs": uptime.num_seconds(),
    }))
}

/// Chat completion handler — routes via combo engine with fallback
async fn handle_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<serde_json::Value>)> {
    match state.combo.execute(&req).await {
        Ok(result) => {
            let mut resp = result.response;
            resp.model = result.used_model;
            Ok(Json(resp))
        }
        Err(e) => {
            let status = StatusCode::BAD_GATEWAY;
            let last = e.attempts.last();
            let provider = last.and_then(|a| a.provider.clone()).unwrap_or_default();
            Err((
                status,
                Json(serde_json::json!({
                    "error": {
                        "message": e.to_string(),
                        "type": "upstream_error",
                        "provider": provider,
                        "attempts": e.attempts.iter().map(|a| serde_json::json!({
                            "model": a.model,
                            "provider": a.provider,
                            "error": a.error,
                        })).collect::<Vec<_>>(),
                    }
                })),
            ))
        }
    }
}

/// List available models
async fn handle_models() -> Json<serde_json::Value> {
    let providers = omniroute_providers::list_providers();
    let models: Vec<serde_json::Value> = providers
        .iter()
        .flat_map(|p| {
            p.models.iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "object": "model",
                    "owned_by": p.id,
                    "created": 0,
                })
            })
        })
        .collect();
    Json(serde_json::json!({ "object": "list", "data": models }))
}

/// Build the HTTP proxy router
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handle_health))
        .route("/v1/chat/completions", axum::routing::post(handle_chat))
        .route("/v1/models", get(handle_models))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Start the proxy server on the given port
pub async fn start_server(port: u16, version: &str) {
    let state = AppState::new(version);
    let app = with_rate_limit(build_router(state));

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("🚀 Proxy server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Method, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = AppState::new("test");
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "test");
    }

    #[tokio::test]
    async fn test_models_endpoint() {
        let state = AppState::new("test");
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["object"], "list");
        assert!(!json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_chat_endpoint_unknown_model() {
        let state = AppState::new("test");
        let app = build_router(state);

        let req_body = serde_json::json!({
            "model": "unknown-model-xyz",
            "messages": [{"role": "user", "content": "Hello!"}]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "upstream_error");
        assert!(!json["error"]["attempts"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_chat_endpoint_missing_api_key() {
        // Default state has no API keys configured
        let state = AppState::new("test");
        let app = build_router(state);

        let req_body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello!"}]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Missing API key surfaces as upstream error (502) with attempts detail
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "upstream_error");
    }
}
