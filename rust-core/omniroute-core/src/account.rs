use std::collections::HashMap;
use std::time::{Duration, Instant};

/// One API credential for a provider
#[derive(Debug, Clone)]
pub struct Account {
    pub key: String,
    pub enabled: bool,
    pub cooldown_until: Option<Instant>,
    pub consecutive_errors: u32,
    pub error_count: u64,
}

impl Account {
    fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            enabled: true,
            cooldown_until: None,
            consecutive_errors: 0,
            error_count: 0,
        }
    }

    fn available(&self, now: Instant) -> bool {
        self.enabled
            && match self.cooldown_until {
                Some(t) => now >= t,
                None => true,
            }
    }
}

/// Outcome of an upstream call, used to update account health
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountOutcome {
    Success,
    RateLimited,
    AuthFailed,
}

/// Round-robin pool of accounts for a single provider
#[derive(Debug, Clone)]
pub struct AccountPool {
    pub provider_id: String,
    accounts: Vec<Account>,
    next_idx: usize,
    /// How long a rate-limited account rests before retry
    pub cooldown_secs: u64,
    /// Disable an account after this many consecutive auth failures
    pub max_consecutive_auth_failures: u32,
}

impl AccountPool {
    pub fn new(provider_id: &str, keys: &[String]) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            accounts: keys.iter().map(|k| Account::new(k)).collect(),
            next_idx: 0,
            cooldown_secs: 60,
            max_consecutive_auth_failures: 2,
        }
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    /// Next available account (round-robin, skipping cooldown/disabled).
    /// Returns None if every account is cooling down or disabled.
    pub fn next_key(&mut self) -> Option<&Account> {
        let now = Instant::now();
        let n = self.accounts.len();
        if n == 0 {
            return None;
        }
        for _ in 0..n {
            let idx = self.next_idx % n;
            self.next_idx = (self.next_idx + 1) % n;
            if self.accounts[idx].available(now) {
                return Some(&self.accounts[idx]);
            }
        }
        None
    }

    /// Report the outcome for a given account key.
    pub fn report(&mut self, key: &str, outcome: AccountOutcome) {
        let now = Instant::now();
        for acc in self.accounts.iter_mut() {
            if acc.key != key {
                continue;
            }
            match outcome {
                AccountOutcome::Success => {
                    acc.consecutive_errors = 0;
                    acc.cooldown_until = None;
                }
                AccountOutcome::RateLimited => {
                    acc.consecutive_errors += 1;
                    acc.error_count += 1;
                    acc.cooldown_until = Some(now + Duration::from_secs(self.cooldown_secs));
                }
                AccountOutcome::AuthFailed => {
                    acc.consecutive_errors += 1;
                    acc.error_count += 1;
                    if acc.consecutive_errors >= self.max_consecutive_auth_failures {
                        acc.enabled = false;
                    }
                }
            }
            break;
        }
    }

    pub fn all_cooling_down(&self) -> bool {
        let now = Instant::now();
        !self.accounts.is_empty() && self.accounts.iter().all(|a| !a.available(now))
    }
}

/// Manages account pools across providers
#[derive(Debug, Clone)]
pub struct AccountManager {
    pools: HashMap<String, AccountPool>,
}

impl AccountManager {
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
        }
    }

    /// Register a pool for a provider.
    pub fn add_pool(&mut self, provider_id: &str, keys: Vec<String>) {
        self.pools.insert(
            provider_id.to_string(),
            AccountPool::new(provider_id, &keys),
        );
    }

    /// Register a single-key pool (shorthand).
    pub fn add_key(&mut self, provider_id: &str, key: &str) {
        self.add_pool(provider_id, vec![key.to_string()]);
    }

    pub fn has_provider(&self, provider_id: &str) -> bool {
        self.pools.contains_key(provider_id)
    }

    pub fn pool_len(&self, provider_id: &str) -> usize {
        self.pools.get(provider_id).map(|p| p.len()).unwrap_or(0)
    }

    /// Pick the next available account key for a provider.
    pub fn next_key(&mut self, provider_id: &str) -> Option<String> {
        let pool = self.pools.get_mut(provider_id)?;
        pool.next_key().map(|a| a.key.clone())
    }

    /// Report outcome for a provider+key.
    pub fn report(&mut self, provider_id: &str, key: &str, outcome: AccountOutcome) {
        if let Some(pool) = self.pools.get_mut(provider_id) {
            pool.report(key, outcome);
        }
    }

    pub fn all_cooling_down(&self, provider_id: &str) -> bool {
        self.pools
            .get(provider_id)
            .map(|p| p.all_cooling_down())
            .unwrap_or(false)
    }
}

impl Default for AccountManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_with(keys: &[&str]) -> AccountPool {
        let keys: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
        AccountPool::new("openai", &keys)
    }

    #[test]
    fn test_round_robin_rotation() {
        let mut pool = pool_with(&["k1", "k2", "k3"]);
        assert_eq!(pool.next_key().unwrap().key, "k1");
        assert_eq!(pool.next_key().unwrap().key, "k2");
        assert_eq!(pool.next_key().unwrap().key, "k3");
        assert_eq!(pool.next_key().unwrap().key, "k1"); // wraps
    }

    #[test]
    fn test_rate_limited_cooldown_skips() {
        let mut pool = pool_with(&["k1", "k2"]);
        assert_eq!(pool.next_key().unwrap().key, "k1");
        pool.report("k1", AccountOutcome::RateLimited);
        // k1 cooling down → k2 next
        assert_eq!(pool.next_key().unwrap().key, "k2");
        // k2 used, k1 still cooling → wraps to k2 again
        assert_eq!(pool.next_key().unwrap().key, "k2");
    }

    #[test]
    fn test_auth_failure_disables_after_threshold() {
        let mut pool = pool_with(&["k1", "k2"]);
        pool.report("k1", AccountOutcome::AuthFailed);
        pool.report("k1", AccountOutcome::AuthFailed);
        // k1 disabled after 2 failures → only k2 rotates
        assert_eq!(pool.next_key().unwrap().key, "k2");
        assert_eq!(pool.next_key().unwrap().key, "k2");
    }

    #[test]
    fn test_success_resets_errors() {
        let mut pool = pool_with(&["k1"]);
        pool.report("k1", AccountOutcome::AuthFailed);
        pool.report("k1", AccountOutcome::Success);
        // reset → k1 available again
        assert_eq!(pool.next_key().unwrap().key, "k1");
    }

    #[test]
    fn test_all_cooling_down_returns_none() {
        let mut pool = pool_with(&["k1"]);
        pool.report("k1", AccountOutcome::RateLimited);
        assert!(pool.next_key().is_none());
        assert!(pool.all_cooling_down());
    }

    #[test]
    fn test_empty_pool() {
        let mut pool = pool_with(&[]);
        assert!(pool.next_key().is_none());
        assert!(pool.is_empty());
    }

    #[test]
    fn test_manager_multi_provider() {
        let mut mgr = AccountManager::new();
        mgr.add_key("openai", "sk-o1");
        mgr.add_pool("claude", vec!["sk-a1".into(), "sk-a2".into()]);
        assert_eq!(mgr.pool_len("openai"), 1);
        assert_eq!(mgr.pool_len("claude"), 2);
        assert_eq!(mgr.next_key("openai").unwrap(), "sk-o1");
        assert_eq!(mgr.next_key("claude").unwrap(), "sk-a1");
        mgr.report("claude", "sk-a1", AccountOutcome::RateLimited);
        assert_eq!(mgr.next_key("claude").unwrap(), "sk-a2");
    }

    #[test]
    fn test_manager_unknown_provider() {
        let mut mgr = AccountManager::new();
        assert!(mgr.next_key("nope").is_none());
        assert!(!mgr.all_cooling_down("nope"));
    }
}
