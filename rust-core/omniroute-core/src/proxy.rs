use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
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
    pub combo: std::sync::Arc<tokio::sync::RwLock<ComboEngine>>,
    pub gateway_keys: std::sync::Arc<std::sync::RwLock<GatewayKeys>>,
    pub allowed_hosts: AllowedHosts,
    pub admin_keys: crate::admin::AdminKeys,
    pub db_path: String,
    pub db: Option<std::sync::Arc<omniroute_db::Database>>,
}

impl AppState {
    pub fn new(version: &str) -> Self {
        Self {
            started_at: chrono::Utc::now(),
            version: version.to_string(),
            combo: std::sync::Arc::new(tokio::sync::RwLock::new(ComboEngine::new(
                RoutingEngine::new(RouterConfig::default()),
            ))),
            gateway_keys: std::sync::Arc::new(std::sync::RwLock::new(GatewayKeys::default())),
            allowed_hosts: AllowedHosts::default(),
            admin_keys: crate::admin::AdminKeys::default(),
            db_path: "./data/omniroute.db".to_string(),
            db: None,
        }
    }

    /// Rebuild the routing engine from env config + DB connections.
    /// Call after admin CRUD so new connections take effect immediately
    /// (matches OmniRoute: connections are picked up per-request).
    pub fn reload_accounts(&self) {
        let mut config = RouterConfig::from_env();
        if let Some(db) = &self.db {
            if let Err(e) = config.load_from_db(db) {
                tracing::warn!("reload accounts failed: {}", e);
            }
            config = config.with_db_persistence(db.clone());
        }
        if let Ok(mut combo) = self.combo.try_write() {
            let mut new_combo = ComboEngine::new(RoutingEngine::new(config));
            #[allow(clippy::collapsible_if)]
            if let Some(db) = &self.db {
                if let Err(e) = new_combo.load_combos_from_db(db) {
                    tracing::warn!("reload combos failed: {}", e);
                }
                new_combo = new_combo.with_db(db.clone());
            }
            *combo = new_combo;
        }
        self.reload_gateway_keys();
    }

    pub fn with_router(mut self, config: RouterConfig) -> Self {
        self.combo = std::sync::Arc::new(tokio::sync::RwLock::new(ComboEngine::new(
            RoutingEngine::new(config),
        )));
        self
    }

    pub fn with_gateway_keys(mut self, keys: Vec<String>) -> Self {
        self.gateway_keys = std::sync::Arc::new(std::sync::RwLock::new(GatewayKeys::new(keys)));
        self
    }

    /// Rebuild gateway keys from env + DB (apiKeys table). Hot-swappable.
    fn reload_gateway_keys(&self) {
        let keys = match &self.db {
            Some(db) => GatewayKeys::from_db(db),
            None => GatewayKeys::new(crate::config::gateway_keys_from_env()),
        };
        if let Ok(mut g) = self.gateway_keys.write() {
            *g = keys;
        }
    }

    pub fn with_allowed_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allowed_hosts = AllowedHosts::new(hosts);
        self
    }

    pub fn with_admin_keys(mut self, keys: crate::admin::AdminKeys) -> Self {
        self.admin_keys = keys;
        self
    }

    pub fn with_db_path(mut self, path: &str) -> Self {
        self.db_path = path.to_string();
        self
    }

    pub fn with_db(mut self, db: Option<std::sync::Arc<omniroute_db::Database>>) -> Self {
        self.db = db;
        self
    }
}

/// Build the /admin sub-router for an AppState (admin keys + db path).
fn admin_router_from_state(state: &AppState) -> Router {
    crate::admin::build_admin_router(state.clone(), state.admin_keys.clone())
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
/// Reads `X-Session-Id` header for G3 session affinity.
async fn handle_chat(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    if req.stream.unwrap_or(false) {
        return handle_chat_stream(state, req, session_id.as_deref()).await;
    }

    let mut combo = state.combo.write().await;
    // Telemetry context: provider/model diisi setelah route (dari result)
    let result = combo.execute(&req, session_id.as_deref()).await;
    match result {
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
    state: AppState,
    req: ChatRequest,
    session_id: Option<&str>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let mut combo = state.combo.write().await;
    let attempt = match combo.execute_stream(&req, session_id).await {
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
    use tower_http::trace::TraceLayer;

    let base = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/chat/completions", axum::routing::post(handle_chat))
        .route("/v1/models", get(handle_models))
        .route("/mcp", post(crate::mcp::handle_mcp))
        .route("/a2a", post(crate::a2a::handle_a2a))
        .route(
            "/.well-known/agent-card.json",
            get(crate::a2a::handle_agent_card),
        )
        .nest_service("/admin", admin_router_from_state(&state))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let hardened = crate::auth::harden_router(
        base,
        state.gateway_keys.clone(),
        state.allowed_hosts.clone(),
    );

    // TraceLayer OUTERMOST → logs every request incl. auth rejections (401/403)
    hardened.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &axum::extract::Request| {
                let method = request.method().to_string();
                let uri = request.uri().path().to_string();
                let span = tracing::info_span!(
                    "request",
                    method = %method,
                    uri = %uri,
                    status = tracing::field::Empty,
                    duration_ms = tracing::field::Empty
                );
                // Stage the entry for the dashboard log buffer
                if let Some(id) = span.id() {
                    crate::logs::stage_request(id.into_u64(), &method, &uri);
                }
                span
            })
            .on_response(|response: &axum::http::Response<axum::body::Body>, latency: std::time::Duration, span: &tracing::Span| {
                let status = response.status().as_u16();
                let duration_ms = latency.as_millis() as u64;
                span.record("status", status);
                span.record("duration_ms", duration_ms);
                #[allow(clippy::redundant_closure)]
                let span_id = span.id().map(|id| id.into_u64());
                let (method, uri) = match span_id.and_then(crate::logs::peek_request) {
                    Some(e) => (e.method, e.uri),
                    None => ("?".into(), "?".into()),
                };
                // Persistent telemetry (M2): chat requests are recorded by the
                // combo engine (has provider/model); everything else here.
                if !uri.starts_with("/v1/chat/completions") {
                    crate::telemetry::TELEMETRY.record(&method, &uri, status, duration_ms);
                }
                if let Some(id) = span_id {
                    crate::logs::finalize_request(id, status, duration_ms);
                }
                tracing::info!(
                    parent: span,
                    "→ {} ({} ms)",
                    status,
                    duration_ms
                );
            })
            .on_failure(|_error, latency: std::time::Duration, span: &tracing::Span| {
                span.record("duration_ms", latency.as_millis() as u64);
                tracing::error!(parent: span, "✗ request failed after {} ms", latency.as_millis());
            }),
    )
}

/// Start the proxy server on the given port
pub async fn start_server(port: u16, version: &str) {
    let db_path =
        std::env::var("OMNIROUTE_DB_PATH").unwrap_or_else(|_| "./data/omniroute.db".into());

    // Open shared SQLite DB once, then load active connections into routing
    // (DB is primary source; env keys fall back for providers without rows).
    let mut config = RouterConfig::from_env();
    let db = omniroute_db::Database::open(std::path::Path::new(&db_path));
    let db = match db {
        Ok(d) => {
            tracing::info!("SQLite ready: {}", db_path);
            let arc = std::sync::Arc::new(d);
            // Attach telemetry (M2): every request → request_logs table
            crate::telemetry::TELEMETRY.attach(arc.clone());
            if let Err(e) = config.load_from_db(&arc) {
                tracing::warn!("failed loading provider connections from DB: {}", e);
            }
            config = config.with_db_persistence(arc.clone());
            Some(arc)
        }
        Err(e) => {
            tracing::warn!("SQLite unavailable ({}), routing from env only", e);
            None
        }
    };

    let state = AppState::new(version)
        .with_router(config)
        .with_gateway_keys(crate::config::gateway_keys_from_env())
        .with_allowed_hosts(crate::config::allowed_hosts_from_env())
        .with_admin_keys(crate::admin::AdminKeys::from_env())
        .with_db_path(&db_path)
        .with_db(db);
    // Full reload: accounts (DB) + combos (DB) + env config
    state.reload_accounts();
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

    // ── Admin CRUD tests (temp SQLite DB) ──────────────────────────

    fn admin_state() -> (AppState, String) {
        let dir = std::env::temp_dir().join(format!("omniroute-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db").to_string_lossy().to_string();
        let state = AppState::new("test")
            .with_db_path(&db_path)
            .with_admin_keys(crate::admin::AdminKeys::new(vec!["admin-1".into()]));
        (state, db_path)
    }

    #[tokio::test]
    async fn test_admin_disabled_without_keys() {
        let state = AppState::new("test");
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/providers")
                    .method(Method::GET)
                    .header("Authorization", "Bearer whatever")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Fail closed: no admin keys → 503
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_admin_requires_valid_key() {
        let (state, _db) = admin_state();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/providers")
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
    async fn test_admin_crud_flow() {
        let (state, _db) = admin_state();
        let app = build_router(state);

        // Create provider connection
        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/providers")
                    .method(Method::POST)
                    .header("Authorization", "Bearer admin-1")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "provider": "openai",
                            "name": "primary",
                            "api_key": "sk-super-secret-12345678",
                            "is_active": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::CREATED);

        // List → key must be masked
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/providers")
                    .method(Method::GET)
                    .header("Authorization", "Bearer admin-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let body = axum::body::to_bytes(list.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        let key = json["data"][0]["api_key"].as_str().unwrap();
        assert!(key.contains("****"), "key must be masked, got {}", key);
        assert!(!key.contains("super-secret"), "raw key leaked!");
    }

    #[tokio::test]
    async fn test_admin_create_api_key_returns_full_key_once() {
        let (state, _db) = admin_state();
        let app = build_router(state);

        let create = app
            .oneshot(
                Request::builder()
                    .uri("/admin/api-keys")
                    .method(Method::POST)
                    .header("Authorization", "Bearer admin-1")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "client-a"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(create.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["key"].as_str().unwrap().starts_with("sk-"));
    }
}
