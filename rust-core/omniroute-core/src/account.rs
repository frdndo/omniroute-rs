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
    /// DB connection id (Some when loaded from provider_connections).
    /// Enables health persistence (rate_limited_until / backoff_level).
    pub connection_id: Option<String>,
}

impl Account {
    fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            enabled: true,
            cooldown_until: None,
            consecutive_errors: 0,
            error_count: 0,
            connection_id: None,
        }
    }

    fn with_connection(mut self, id: &str) -> Self {
        self.connection_id = Some(id.to_string());
        self
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
    pub(crate) accounts: Vec<Account>,
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

    /// Add an account with a DB connection id (health-persisted).
    pub fn add_connection(&mut self, key: &str, connection_id: &str) {
        self.accounts
            .push(Account::new(key).with_connection(connection_id));
    }

    /// Add a DB account that is currently rate-limited (initial cooldown).
    pub fn add_connection_cooled(&mut self, key: &str, connection_id: &str, cooldown_secs: u64) {
        let mut acc = Account::new(key).with_connection(connection_id);
        acc.cooldown_until = Some(Instant::now() + Duration::from_secs(cooldown_secs));
        self.accounts.push(acc);
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
#[derive(Clone)]
pub struct AccountManager {
    pools: HashMap<String, AccountPool>,
    /// Optional shared DB handle — when set, health changes persist to
    /// provider_connections (rate_limited_until / backoff_level).
    pub persist_db: Option<std::sync::Arc<omniroute_db::Database>>,
}

impl std::fmt::Debug for AccountManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountManager")
            .field("pools", &self.pools)
            .field("persist_db", &self.persist_db.is_some())
            .finish()
    }
}

impl AccountManager {
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
            persist_db: None,
        }
    }

    /// Enable SQLite persistence of account health.
    pub fn with_persistence(mut self, db: std::sync::Arc<omniroute_db::Database>) -> Self {
        self.persist_db = Some(db);
        self
    }

    /// Register a pool for a provider.
    pub fn add_pool(&mut self, provider_id: &str, keys: Vec<String>) {
        self.pools.insert(
            provider_id.to_string(),
            AccountPool::new(provider_id, &keys),
        );
    }

    /// Add a DB-backed account (with connection id) to a provider pool.
    /// Creates the pool if missing.
    pub fn add_connection(&mut self, provider_id: &str, key: &str, connection_id: &str) {
        self.pools
            .entry(provider_id.to_string())
            .or_insert_with(|| AccountPool::new(provider_id, &[]))
            .add_connection(key, connection_id);
    }

    /// Add a DB-backed account that starts in cooldown (rate-limited at load).
    pub fn add_connection_cooled(
        &mut self,
        provider_id: &str,
        key: &str,
        connection_id: &str,
        cooldown_secs: u64,
    ) {
        self.pools
            .entry(provider_id.to_string())
            .or_insert_with(|| AccountPool::new(provider_id, &[]))
            .add_connection_cooled(key, connection_id, cooldown_secs);
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
        let mut conn_id: Option<String> = None;
        let mut backoff_level: i32 = 0;
        if let Some(pool) = self.pools.get_mut(provider_id) {
            pool.report(key, outcome);
            // Find the account to get connection_id + backoff info
            for acc in pool.accounts.iter() {
                if acc.key == key {
                    conn_id = acc.connection_id.clone();
                    backoff_level = acc.consecutive_errors.min(5) as i32;
                    break;
                }
            }
        }

        // Persist health to SQLite (matches OmniRoute flow)
        if let (Some(db), Some(cid)) = (&self.persist_db, conn_id) {
            let rlu = match outcome {
                AccountOutcome::RateLimited => {
                    Some(chrono::Utc::now() + chrono::Duration::seconds(60))
                }
                _ => None,
            };
            if let Ok(conn) = db.conn.lock() {
                let _ = omniroute_db::repos::provider_connection_repo::update_health(
                    &conn,
                    &cid,
                    rlu.map(|t| t.to_rfc3339()).as_deref(),
                    backoff_level,
                );
            }
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
