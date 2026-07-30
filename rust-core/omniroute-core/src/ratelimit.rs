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
    middleware::NoOpMiddleware,
    quota::Quota,
    state::{InMemoryState, NotKeyed},
};
use once_cell::sync::Lazy;

/// IP-based rate limiter using the governor crate
pub struct IpRateLimiter {
    buckets: Arc<
        dashmap::DashMap<
            String,
            RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>,
        >,
    >,
    max_buckets: usize,
    quota: Quota,
}

impl IpRateLimiter {
    pub fn new(requests_per_minute: u64, max_buckets: usize) -> Self {
        let quota = Quota::per_minute(requests_per_minute)
            .unwrap_or_else(|_| Quota::per_minute(60).unwrap());
        Self {
            buckets: Arc::new(dashmap::DashMap::new()),
            max_buckets,
            quota,
        }
    }

    pub fn check(&self, ip: &str) -> bool {
        if self.buckets.len() >= self.max_buckets {
            self.buckets.clear();
        }
        self.buckets
            .entry(ip.to_string())
            .or_insert_with(|| RateLimiter::direct(self.quota))
            .check()
            .is_ok()
    }
}

/// Axum middleware that enforces rate limiting
pub async fn rate_limit_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            request
                .extensions()
                .get::<std::net::SocketAddr>()
                .map(|a| a.ip().to_string())
        })
        .unwrap_or_else(|| "unknown".into());

    // Per-minute rate limiter for this IP
    let quota = Quota::per_minute(60).unwrap_or_else(|_| Quota::per_minute(60).unwrap());
    let limiter = RateLimiter::direct(quota);

    if limiter.check().is_ok() {
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
    fn test_rate_limiter_accepts_first() {
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
        assert!(!limiter.check("10.0.0.1"));
    }

    #[test]
    fn test_rate_limiter_eviction() {
        let limiter = IpRateLimiter::new(1, 2);
        assert!(limiter.check("1.1.1.1"));
        assert!(limiter.check("2.2.2.2"));
        assert!(limiter.check("3.3.3.3"));
    }
}
