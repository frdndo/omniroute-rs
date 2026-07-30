use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
pub struct IpRateLimiter {
    buckets: Mutex<HashMap<String, Vec<Instant>>>,
    max_requests: usize,
    window_secs: u64,
    max_ips: usize,
}

impl IpRateLimiter {
    pub fn new(max_requests: usize, window_secs: u64, max_ips: usize) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_requests,
            window_secs,
            max_ips,
        }
    }

    pub fn check(&self, ip: &str) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap();

        // Evict old entries
        if buckets.len() >= self.max_ips {
            buckets.clear();
        }

        // Remove expired timestamps
        let window = std::time::Duration::from_secs(self.window_secs);
        let timestamps = buckets.entry(ip.to_string()).or_default();
        timestamps.retain(|t| now.duration_since(*t) < window);

        // Check rate limit
        if timestamps.len() >= self.max_requests {
            false
        } else {
            timestamps.push(now);
            true
        }
    }
}

impl Default for IpRateLimiter {
    fn default() -> Self {
        Self::new(60, 60, 10000)
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

    let limiter = IpRateLimiter::default();
    if limiter.check(&ip) {
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
    fn test_accepts_first() {
        let limiter = IpRateLimiter::new(100, 60, 100);
        assert!(limiter.check("127.0.0.1"));
    }

    #[test]
    fn test_different_ips() {
        let limiter = IpRateLimiter::new(5, 60, 100);
        assert!(limiter.check("1.1.1.1"));
        assert!(limiter.check("2.2.2.2"));
    }

    #[test]
    fn test_blocks_after_limit() {
        let limiter = IpRateLimiter::new(3, 60, 100);
        assert!(limiter.check("10.0.0.1"));
        assert!(limiter.check("10.0.0.1"));
        assert!(limiter.check("10.0.0.1"));
        assert!(!limiter.check("10.0.0.1"));
    }

    #[test]
    fn test_eviction() {
        let limiter = IpRateLimiter::new(1, 60, 2);
        assert!(limiter.check("1.1.1.1"));
        assert!(limiter.check("2.2.2.2"));
        assert!(limiter.check("3.3.3.3")); // triggers eviction
    }
}
