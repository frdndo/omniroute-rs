use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::chat::{ChatRequest, ChatResponse, Usage};
use crate::ratelimit::with_rate_limit;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub version: String,
}

impl AppState {
    pub fn new(version: &str) -> Self {
        Self {
            started_at: chrono::Utc::now(),
            version: version.to_string(),
        }
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

/// Chat completion handler (non-streaming)
async fn handle_chat(
    State(_state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let model = req.model.clone();
    let user_msg = req
        .messages
        .iter()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_ref())
        .map(|c| match c {
            crate::chat::Content::Text(t) => t.clone(),
            crate::chat::Content::Parts(_) => "...".into(),
        })
        .unwrap_or_default();

    // Mock response — will be replaced by actual routing engine
    let response = ChatResponse::new(
        &model,
        &format!("Echo: {}", user_msg),
        Some(Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        }),
    );

    Ok(Json(response))
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
    use axum::{
        body::Body,
        http::{Method, Request},
    };
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
        assert!(json["data"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_chat_endpoint() {
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
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["object"], "chat.completion");
        assert!(
            json["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .contains("Hello")
        );
    }

    #[tokio::test]
    async fn test_chat_without_stream() {
        let state = AppState::new("test");
        let app = build_router(state);

        let req_body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Non-stream test"}],
            "stream": false
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
        assert_eq!(response.status(), StatusCode::OK);
    }
}
