use std::collections::HashMap;

/// Per-provider runtime stats used by the auto-combo scorer.
#[derive(Debug, Default, Clone)]
pub struct ProviderStats {
    /// Exponential moving average of upstream latency (ms)
    pub latency_ema_ms: f64,
    /// Requests currently in flight
    pub active_requests: u32,
    /// Total requests observed
    pub total_requests: u64,
    /// Failed requests (rate limited / errors)
    pub failed_requests: u64,
}

/// Scores candidate models so the combo engine tries the best option first.
/// Mirrors OmniRoute's autoCombo scoring (health, latency, concurrency).
#[derive(Debug, Default, Clone)]
pub struct ComboScorer {
    stats: HashMap<String, ProviderStats>,
    /// EMA smoothing factor (0.0-1.0); higher = reacts faster
    pub alpha: f64,
}

impl ComboScorer {
    pub fn new() -> Self {
        Self {
            stats: HashMap::new(),
            alpha: 0.3,
        }
    }

    pub fn stats(&self) -> &HashMap<String, ProviderStats> {
        &self.stats
    }

    /// Record a completed upstream call for a provider.
    pub fn record_latency(&mut self, provider: &str, latency_ms: f64) {
        let s = self.stats.entry(provider.to_string()).or_default();
        s.total_requests += 1;
        if s.total_requests == 1 {
            s.latency_ema_ms = latency_ms;
        } else {
            s.latency_ema_ms = self.alpha * latency_ms + (1.0 - self.alpha) * s.latency_ema_ms;
        }
    }

    pub fn record_failure(&mut self, provider: &str) {
        self.stats
            .entry(provider.to_string())
            .or_default()
            .failed_requests += 1;
    }

    pub fn begin_request(&mut self, provider: &str) {
        self.stats
            .entry(provider.to_string())
            .or_default()
            .active_requests += 1;
    }

    pub fn end_request(&mut self, provider: &str) {
        if let Some(s) = self.stats.get_mut(provider) {
            s.active_requests = s.active_requests.saturating_sub(1);
        }
    }

    /// Score a candidate provider for ordering (higher = better).
    /// Factors (mirror OmniRoute autoCombo):
    ///   - health: account cooling/backoff heavily penalized
    ///   - latency: EMA penalty (slower = lower score)
    ///   - concurrency: in-flight penalty (saturated at 4)
    ///   - reliability: failure ratio penalty
    pub fn score(&self, provider: &str, account_available: bool, backoff_level: u32) -> f64 {
        let mut score = 100.0;

        // Health: account unavailable → disqualify-ish
        if !account_available {
            score -= 60.0;
        }
        score -= (backoff_level.min(5) as f64) * 8.0;

        if let Some(s) = self.stats.get(provider) {
            // Latency: penalty grows with EMA, capped at 40
            let lat_penalty = (s.latency_ema_ms / 50.0).min(40.0);
            score -= lat_penalty;

            // Concurrency: -6 per in-flight, capped at -24
            let conc_penalty = (s.active_requests as f64 * 6.0).min(24.0);
            score -= conc_penalty;

            // Reliability: failure ratio penalty, capped at 30
            if s.total_requests > 0 {
                let ratio = s.failed_requests as f64 / s.total_requests as f64;
                score -= (ratio * 60.0).min(30.0);
            }
        }

        score.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healthy_provider_scores_higher() {
        let scorer = ComboScorer::new();
        let healthy = scorer.score("a", true, 0);
        let cooling = scorer.score("a", false, 0);
        assert!(healthy > cooling);
    }

    #[test]
    fn test_backoff_penalty() {
        let scorer = ComboScorer::new();
        let fresh = scorer.score("a", true, 0);
        let backoff = scorer.score("a", true, 5);
        assert!(fresh > backoff);
        assert!((fresh - backoff - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_latency_and_concurrency_penalty() {
        let mut scorer = ComboScorer::new();
        scorer.record_latency("slow", 2000.0);
        scorer.begin_request("slow");
        scorer.begin_request("slow");

        let slow = scorer.score("slow", true, 0);
        let fast = scorer.score("fast", true, 0);
        assert!(fast > slow, "fast={} slow={}", fast, slow);
    }

    #[test]
    fn test_failure_ratio_penalty() {
        let mut scorer = ComboScorer::new();
        for _ in 0..10 {
            scorer.record_latency("unstable", 100.0);
        }
        for _ in 0..8 {
            scorer.record_failure("unstable");
        }
        let unstable = scorer.score("unstable", true, 0);
        let stable = scorer.score("stable", true, 0);
        assert!(stable > unstable);
    }

    #[test]
    fn test_ema_smoothing() {
        let mut scorer = ComboScorer::new();
        scorer.record_latency("p", 100.0);
        scorer.record_latency("p", 300.0);
        // EMA: 100 then 0.3*300 + 0.7*100 = 160
        let ema = scorer.stats.get("p").unwrap().latency_ema_ms;
        assert!((ema - 160.0).abs() < 0.001, "ema={}", ema);
    }
}
