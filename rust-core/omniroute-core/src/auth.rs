use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};

/// Gateway API keys accepted on `/v1/*` routes (client auth).
#[derive(Debug, Default, Clone)]
pub struct GatewayKeys {
    keys: Vec<String>,
}

impl GatewayKeys {
    pub fn new(keys: Vec<String>) -> Self {
        Self { keys }
    }

    pub fn with_key(mut self, key: &str) -> Self {
        self.keys.push(key.to_string());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn validate(&self, token: Option<&str>) -> bool {
        match token {
            Some(t) => self.keys.iter().any(|k| k == t),
            None => false,
        }
    }
}

/// Hosts allowed to reach the proxy. Empty = allow all (dev mode).
#[derive(Debug, Default, Clone)]
pub struct AllowedHosts {
    hosts: Vec<String>,
}

impl AllowedHosts {
    pub fn new(hosts: Vec<String>) -> Self {
        Self { hosts }
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    pub fn allows(&self, host: &str) -> bool {
        self.hosts.iter().any(|h| h == host)
    }
}

/// Extract the Bearer token from an Authorization header.
pub fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
}

/// Auth middleware: requires a valid gateway API key on protected routes.
/// `/admin/*` is excluded — it has its own stricter auth.
pub async fn auth_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    // Admin routes are protected by admin auth (separate key)
    if request.uri().path().starts_with("/admin") {
        return Ok(next.run(request).await);
    }

    let keys = request
        .extensions()
        .get::<GatewayKeys>()
        .cloned()
        .unwrap_or_default();

    // If no keys configured, auth is disabled (dev mode)
    if keys.is_empty() {
        return Ok(next.run(request).await);
    }

    let token = bearer_token(request.headers());
    if keys.validate(token.as_deref()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Host header guard: rejects requests whose Host is not allowed.
pub async fn host_guard_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let allowed = request
        .extensions()
        .get::<AllowedHosts>()
        .cloned()
        .unwrap_or_default();

    // No allowlist configured → allow all (dev mode)
    if allowed.is_empty() {
        return Ok(next.run(request).await);
    }

    let host = request
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Strip port if present (Host: localhost:20128 → localhost)
    let hostname = host.split(':').next().unwrap_or(host);

    if allowed.allows(hostname) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Apply host guard + auth to a router.
/// Extension layers go OUTSIDE the middleware so they're set before
/// the middleware inspects the request.
pub fn harden_router(router: Router, keys: GatewayKeys, hosts: AllowedHosts) -> Router {
    router
        .layer(middleware::from_fn(host_guard_middleware))
        .layer(middleware::from_fn(auth_middleware))
        .layer(axum::Extension(keys))
        .layer(axum::Extension(hosts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn test_bearer_token_extraction() {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_static("Bearer sk-123"));
        assert_eq!(bearer_token(&h).unwrap(), "sk-123");
    }

    #[test]
    fn test_bearer_token_missing() {
        let h = HeaderMap::new();
        assert!(bearer_token(&h).is_none());
    }

    #[test]
    fn test_bearer_token_wrong_scheme() {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_static("Basic abc"));
        assert!(bearer_token(&h).is_none());
    }

    #[test]
    fn test_gateway_keys_validate() {
        let keys = GatewayKeys::new(vec!["sk-a".into(), "sk-b".into()]);
        assert!(keys.validate(Some("sk-a")));
        assert!(keys.validate(Some("sk-b")));
        assert!(!keys.validate(Some("sk-c")));
        assert!(!keys.validate(None));
    }

    #[test]
    fn test_allowed_hosts() {
        let hosts = AllowedHosts::new(vec!["localhost".into(), "127.0.0.1".into()]);
        assert!(hosts.allows("localhost"));
        assert!(hosts.allows("127.0.0.1"));
        assert!(!hosts.allows("evil.com"));
    }
}
