use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};
use governor::{
    RateLimiter,
    clock::DefaultClock,
    quota::Quota,
    state::{InMemoryState, NotKeyed},
};
use once_cell::sync::Lazy;

/// IP-based rate limiter using the governor crate
pub struct IpRateLimiter {
    buckets: Arc<dashmap::DashMap<String, RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
    max_buckets: usize,
    requests_per_minute: u64,
}

impl IpRateLimiter {
    pub fn new(requests_per_minute: u64, max_buckets: usize) -> Self {
        Self {
            buckets: Arc::new(dashmap::DashMap::new()),
            max_buckets,
            requests_per_minute,
        }
    }

    pub fn check(&self, ip: &str) -> bool {
        // Evict old entries if over limit
        if self.buckets.len() >= self.max_buckets {
            self.buckets.retain(|_, _| false);
        }

        let quota = Quota::per_minute(self.requests_per_minute)
            .unwrap_or_else(|_| Quota::per_minute(60).unwrap());
        let limiter = self
            .buckets
            .entry(ip.to_string())
            .or_insert_with(|| RateLimiter::direct(quota));
        limiter.check().is_ok()
    }
}

impl Default for IpRateLimiter {
    fn default() -> Self {
        Self::new(60, 10000)
    }
}

/// Global rate limiter instance (60 req/min per IP, 10k max IPs)
pub static GLOBAL_RATE_LIMITER: Lazy<IpRateLimiter> = Lazy::new(IpRateLimiter::default);

/// Axum middleware that enforces rate limiting
pub async fn rate_limit_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .or_else(|| {
            request
                .extensions()
                .get::<std::net::SocketAddr>()
                .map(|a| a.ip().to_string().as_str().to_string())
                .as_deref()
        })
        .unwrap_or("unknown");

    if GLOBAL_RATE_LIMITER.check(ip) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

/// Apply rate limiting middleware to a router
pub fn with_rate_limit(router: Router) -> Router {
    router.layer(middleware::from_fn(rate_limit_middleware))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_accepts_first_request() {
        let limiter = IpRateLimiter::new(1000, 100);
        assert!(limiter.check("127.0.0.1"));
    }

    #[test]
    fn test_rate_limiter_different_ips() {
        let limiter = IpRateLimiter::new(5, 100);
        assert!(limiter.check("1.1.1.1"));
        assert!(limiter.check("2.2.2.2"));
    }

    #[test]
    fn test_rate_limiter_blocks_after_limit() {
        let limiter = IpRateLimiter::new(3, 100);
        assert!(limiter.check("10.0.0.1"));
        assert!(limiter.check("10.0.0.1"));
        assert!(limiter.check("10.0.0.1"));
        // 4th request within same minute should be denied
        assert!(!limiter.check("10.0.0.1"));
    }

    #[test]
    fn test_rate_limiter_eviction() {
        let limiter = IpRateLimiter::new(1, 2);
        assert!(limiter.check("1.1.1.1"));
        assert!(limiter.check("2.2.2.2"));
        // Adding 3rd IP should trigger eviction
        assert!(limiter.check("3.3.3.3"));
    }

    #[test]
    fn test_global_limiter_exists() {
        let _ = &*GLOBAL_RATE_LIMITER;
    }
}
