use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use futures::StreamExt;
use tower_http::cors::CorsLayer;

use crate::auth::{AllowedHosts, GatewayKeys};
use crate::chat::ChatRequest;
use crate::combo::ComboEngine;
use crate::executor::ExecutorError;
use crate::ratelimit::with_rate_limit;
use crate::router::{RouterConfig, RoutingEngine};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub version: String,
    pub combo: ComboEngine,
    pub gateway_keys: GatewayKeys,
    pub allowed_hosts: AllowedHosts,
}

impl AppState {
    pub fn new(version: &str) -> Self {
        Self {
            started_at: chrono::Utc::now(),
            version: version.to_string(),
            combo: ComboEngine::new(RoutingEngine::new(RouterConfig::default())),
            gateway_keys: GatewayKeys::default(),
            allowed_hosts: AllowedHosts::default(),
        }
    }

    pub fn with_router(mut self, config: RouterConfig) -> Self {
        self.combo = ComboEngine::new(RoutingEngine::new(config));
        self
    }

    pub fn with_gateway_keys(mut self, keys: Vec<String>) -> Self {
        self.gateway_keys = GatewayKeys::new(keys);
        self
    }

    pub fn with_allowed_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allowed_hosts = AllowedHosts::new(hosts);
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

/// Chat completion handler — routes via combo engine with fallback.
/// Honors `stream: true` by returning an SSE response.
async fn handle_chat(
    State(mut state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    if req.is_streaming() {
        return handle_chat_stream(state, req).await;
    }

    match state.combo.execute(&req).await {
        Ok(result) => {
            let mut resp = crate::sanitize::sanitize_response(result.response);
            resp.model = result.used_model;
            Ok(axum::Json(resp).into_response())
        }
        Err(e) => Err(combo_error_response(e)),
    }
}

/// SSE streaming chat handler
async fn handle_chat_stream(
    mut state: AppState,
    req: ChatRequest,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let attempt = match state.combo.execute_stream(&req).await {
        Ok(a) => a,
        Err(e) => return Err(combo_error_response(e)),
    };

    // Map normalized chunks → SSE events, terminate with [DONE]
    let stream = attempt.stream.filter_map(|chunk| {
        let data = match chunk {
            Ok(crate::executor::streaming::StreamChunk::Data(d)) => d,
            Ok(crate::executor::streaming::StreamChunk::Done) => "[DONE]".to_string(),
            Err(e) => format!("[ERROR] {}", e),
        };
        futures::future::ready(Some(Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default().data(data),
        )))
    });

    Ok(axum::response::sse::Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
        )
        .into_response())
}

/// Build an OpenAI-style error response from a combo failure
fn combo_error_response(e: crate::combo::ComboError) -> (StatusCode, Json<serde_json::Value>) {
    let last = e.attempts.last();
    let provider = last.and_then(|a| a.provider.clone()).unwrap_or_default();

    // Map status from the last error if it's a direct executor error
    let (status, body) = if let Some(err) = last {
        if let Ok(parsed) = parse_executor_error(&err.error) {
            crate::sanitize::error_to_response(&parsed, Some(&provider))
        } else {
            (
                StatusCode::BAD_GATEWAY,
                serde_json::json!({
                    "error": {
                        "message": err.error,
                        "type": "upstream_error",
                        "provider": provider,
                    }
                }),
            )
        }
    } else {
        (
            StatusCode::BAD_GATEWAY,
            serde_json::json!({
                "error": {
                    "message": e.to_string(),
                    "type": "upstream_error",
                    "provider": provider,
                }
            }),
        )
    };

    // Attach attempts detail
    let mut body = body;
    body["error"]["attempts"] = serde_json::json!(
        e.attempts
            .iter()
            .map(|a| serde_json::json!({
                "model": a.model,
                "provider": a.provider,
                "error": a.error,
            }))
            .collect::<Vec<_>>()
    );

    (status, Json(body))
}

/// Best-effort parse of an ExecutorError from its Display string.
/// Falls back to Network if unparseable.
fn parse_executor_error(s: &str) -> Result<ExecutorError, ()> {
    if s.contains("rate limited") {
        Ok(ExecutorError::RateLimited(429))
    } else if s.contains("authentication failed") {
        Ok(ExecutorError::AuthFailed(401))
    } else if s.contains("timeout") {
        Ok(ExecutorError::Timeout(120))
    } else if s.contains("network error") {
        Ok(ExecutorError::Network(s.to_string()))
    } else if s.contains("no provider registered") {
        Ok(ExecutorError::UnsupportedProvider(s.to_string()))
    } else if s.contains("upstream error") {
        Ok(ExecutorError::Upstream(502, s.to_string()))
    } else {
        Err(())
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
    let base = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/chat/completions", axum::routing::post(handle_chat))
        .route("/v1/models", get(handle_models))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    crate::auth::harden_router(
        base,
        state.gateway_keys.clone(),
        state.allowed_hosts.clone(),
    )
}

/// Start the proxy server on the given port
pub async fn start_server(port: u16, version: &str) {
    let state = AppState::new(version)
        .with_router(RouterConfig::from_env())
        .with_gateway_keys(crate::config::gateway_keys_from_env())
        .with_allowed_hosts(crate::config::allowed_hosts_from_env());
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

        let body = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
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
        // Unknown model → 400 invalid_request_error (UnsupportedProvider)
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "invalid_request_error");
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
        // Missing API key → 401 authentication_error (AuthFailed)
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "authentication_error");
    }

    #[tokio::test]
    async fn test_auth_requires_key() {
        let state = AppState::new("test").with_gateway_keys(vec!["sk-gateway".into()]);
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_accepts_valid_key() {
        let state = AppState::new("test").with_gateway_keys(vec!["sk-gateway".into()]);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .method(Method::GET)
                    .header("Authorization", "Bearer sk-gateway")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_rejects_wrong_key() {
        let state = AppState::new("test").with_gateway_keys(vec!["sk-gateway".into()]);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .method(Method::GET)
                    .header("Authorization", "Bearer wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_host_guard_blocks_spoofed_host() {
        // Only localhost allowed
        let state = AppState::new("test").with_allowed_hosts(vec!["localhost".into()]);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .method(Method::GET)
                    .header("Host", "evil.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_host_guard_allows_localhost() {
        let state = AppState::new("test").with_allowed_hosts(vec!["localhost".into()]);
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .method(Method::GET)
                    .header("Host", "localhost:20128")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
