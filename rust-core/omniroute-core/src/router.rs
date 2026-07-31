use crate::account::{AccountManager, AccountOutcome};
use crate::executor::{ExecutorError, ProviderExecutor};
use omniroute_db::repos::provider_connection_repo;
use std::collections::HashMap;

/// Route resolution result: which provider + executor to use
#[derive(Debug)]
pub struct RouteTarget {
    pub provider_id: String,
    pub account_key: String,
    pub executor: ProviderExecutor,
}

/// Configuration for the routing engine — API keys per provider
#[derive(Debug, Default, Clone)]
pub struct RouterConfig {
    pub api_keys: HashMap<String, String>,
    pub accounts: AccountManager,
    /// Optional upstream base URL overrides (tests, self-hosted mirrors)
    pub base_urls: HashMap<String, String>,
}

impl RouterConfig {
    /// Build a RouterConfig from environment:
    ///   OMNIROUTE_PROVIDER_KEYS="openai=sk-1;claude=sk-2"  (multi keys: openai=sk-1,sk-2)
    ///   OMNIROUTE_BASE_URL_<PROVIDER>=<url>
    pub fn from_env() -> Self {
        let mut cfg = RouterConfig::default();
        if let Ok(keys_str) = std::env::var("OMNIROUTE_PROVIDER_KEYS") {
            for part in keys_str.split(';') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Some((provider, keys)) = part.split_once('=') {
                    let provider = provider.trim();
                    let key_list: Vec<String> = keys
                        .split(',')
                        .map(|k| k.trim().to_string())
                        .filter(|k| !k.is_empty())
                        .collect();
                    if key_list.len() > 1 {
                        cfg = cfg.with_pool(provider, key_list);
                    } else if let Some(k) = key_list.first() {
                        cfg = cfg.with_key(provider, k);
                    }
                }
            }
        }
        for (provider, url) in crate::config::base_urls_from_env() {
            cfg = cfg.with_base_url(&provider, &url);
        }
        cfg
    }

    /// Register a single API key for a provider (shorthand).
    pub fn with_key(mut self, provider: &str, key: &str) -> Self {
        self.api_keys.insert(provider.to_string(), key.to_string());
        self.accounts.add_key(provider, key);
        self
    }

    /// Register multiple API keys for a provider (rotation pool).
    pub fn with_pool(mut self, provider: &str, keys: Vec<String>) -> Self {
        self.api_keys.insert(
            provider.to_string(),
            keys.first().cloned().unwrap_or_default(),
        );
        self.accounts.add_pool(provider, keys);
        self
    }

    /// Override the upstream base URL for a provider.
    pub fn with_base_url(mut self, provider: &str, url: &str) -> Self {
        self.base_urls.insert(provider.to_string(), url.to_string());
        self
    }

    /// Enable SQLite persistence of account health (cooldown/backoff).
    pub fn with_db_persistence(mut self, db: std::sync::Arc<omniroute_db::Database>) -> Self {
        self.accounts = std::mem::take(&mut self.accounts).with_persistence(db);
        self
    }

    /// Load active provider connections from SQLite into account pools.
    /// Priority-ordered; rate-limited connections get an initial cooldown.
    /// DB is the primary source — env keys remain as fallback for providers
    /// without DB entries (matches OmniRoute: env = virtual connections).
    pub fn load_from_db(&mut self, db: &omniroute_db::Database) -> Result<(), String> {
        let connections = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            provider_connection_repo::get_active(&conn).map_err(|e| e.to_string())?
        };

        for c in connections {
            let Some(key) = c.api_key.as_deref().filter(|k| !k.is_empty()) else {
                continue;
            };
            let cooldown_secs = c
                .rate_limited_until
                .as_deref()
                .and_then(|t| {
                    chrono::DateTime::parse_from_rfc3339(t)
                        .ok()
                        .map(|d| d.with_timezone(&chrono::Utc))
                })
                .map(|t| (t - chrono::Utc::now()).num_seconds().max(0) as u64)
                .unwrap_or(0);

            if cooldown_secs > 0 {
                self.accounts
                    .add_connection_cooled(&c.provider, key, &c.id, cooldown_secs);
            } else {
                self.accounts.add_connection(&c.provider, key, &c.id);
            }
        }
        Ok(())
    }
}

/// Resolves a requested model to a provider + executor, with
/// multi-account rotation support.
#[derive(Clone)]
pub struct RoutingEngine {
    config: RouterConfig,
}

impl RoutingEngine {
    pub fn new(config: RouterConfig) -> Self {
        Self { config }
    }

    /// Resolve the provider id that owns a model.
    pub fn resolve_provider(&self, model: &str) -> Result<&'static str, ExecutorError> {
        // 1. Known prefixes — deterministic, resolves ambiguity when many
        //    providers carry the same model (e.g. gpt-4o appears in 8+)
        let lower = model.to_lowercase();
        let known: &[(&str, &str)] = &[
            ("gpt-", "openai"),
            ("o1-", "openai"),
            ("o3-", "openai"),
            ("text-embedding", "openai"),
            ("claude-", "claude"),
            ("gemini-", "gemini"),
            ("deepseek-", "deepseek"),
        ];
        for (prefix, provider) in known {
            if lower.starts_with(prefix) {
                return Ok(provider);
            }
        }

        // 2. Registry match (exact, then prefix) for everything else
        if let Some(pid) = omniroute_providers::resolve_provider_for_model(model) {
            return Ok(pid);
        }

        Err(ExecutorError::UnsupportedProvider(format!(
            "no provider registered for model '{}'",
            model
        )))
    }

    /// Resolve model → RouteTarget using the next available account.
    pub fn route(&mut self, model: &str) -> Result<RouteTarget, ExecutorError> {
        let provider_id = self.resolve_provider(model)?;

        // Prefer the account pool (rotation); fall back to single key
        let account_key = if self.config.accounts.has_provider(provider_id) {
            self.config.accounts.next_key(provider_id).ok_or_else(|| {
                if self.config.accounts.all_cooling_down(provider_id) {
                    ExecutorError::RateLimited(429)
                } else {
                    ExecutorError::AuthFailed(0)
                }
            })?
        } else {
            self.config
                .api_keys
                .get(provider_id)
                .cloned()
                .unwrap_or_default()
        };

        if account_key.is_empty() {
            return Err(ExecutorError::AuthFailed(0));
        }

        let executor = ProviderExecutor::from_provider_id_with_base(
            provider_id,
            &account_key,
            self.config.base_urls.get(provider_id).map(|s| s.as_str()),
        )?;
        Ok(RouteTarget {
            provider_id: provider_id.to_string(),
            account_key,
            executor,
        })
    }

    /// Resolve model → RouteTarget, preferring a specific account key
    /// (session affinity). Falls back to normal rotation when the preferred
    /// account is cooling down or missing.
    pub fn route_prefer(
        &mut self,
        model: &str,
        preferred_account: Option<&str>,
    ) -> Result<RouteTarget, ExecutorError> {
        let provider_id = self.resolve_provider(model)?;

        let account_key = if self.config.accounts.has_provider(provider_id) {
            if let Some(pref) = preferred_account {
                if let Some(k) = self.config.accounts.prefer_key(provider_id, pref) {
                    k
                } else {
                    // preferred account unavailable → normal rotation
                    self.config.accounts.next_key(provider_id).ok_or_else(|| {
                        if self.config.accounts.all_cooling_down(provider_id) {
                            ExecutorError::RateLimited(429)
                        } else {
                            ExecutorError::AuthFailed(0)
                        }
                    })?
                }
            } else {
                self.config.accounts.next_key(provider_id).ok_or_else(|| {
                    if self.config.accounts.all_cooling_down(provider_id) {
                        ExecutorError::RateLimited(429)
                    } else {
                        ExecutorError::AuthFailed(0)
                    }
                })?
            }
        } else {
            self.config
                .api_keys
                .get(provider_id)
                .cloned()
                .unwrap_or_default()
        };

        if account_key.is_empty() {
            return Err(ExecutorError::AuthFailed(0));
        }

        let executor = ProviderExecutor::from_provider_id_with_base(
            provider_id,
            &account_key,
            self.config.base_urls.get(provider_id).map(|s| s.as_str()),
        )?;
        Ok(RouteTarget {
            provider_id: provider_id.to_string(),
            account_key,
            executor,
        })
    }

    /// Health peek for the auto-combo scorer: (account available?, backoff).
    pub fn peek_health(&self, provider_id: &str) -> (bool, u32) {
        self.config.accounts.peek_health(provider_id)
    }

    /// Report an upstream outcome so the account pool can rotate/cooldown.
    pub fn report(&mut self, provider_id: &str, key: &str, outcome: AccountOutcome) {
        self.config.accounts.report(provider_id, key, outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::ApiFormat;

    fn engine_with_keys() -> RoutingEngine {
        let config = RouterConfig::default()
            .with_key("openai", "sk-openai")
            .with_key("claude", "sk-ant")
            .with_key("gemini", "AIza-gem")
            .with_key("deepseek", "sk-deep");
        RoutingEngine::new(config)
    }

    #[test]
    fn test_resolve_exact_model() {
        let engine = engine_with_keys();
        assert_eq!(engine.resolve_provider("gpt-4o").unwrap(), "openai");
        assert_eq!(
            engine.resolve_provider("deepseek-chat").unwrap(),
            "deepseek"
        );
    }

    #[test]
    fn test_resolve_prefix_model() {
        let engine = engine_with_keys();
        assert_eq!(
            engine.resolve_provider("claude-sonnet-4").unwrap(),
            "claude"
        );
        assert_eq!(engine.resolve_provider("gemini-2.5-pro").unwrap(), "gemini");
    }

    #[test]
    fn test_resolve_unknown_model() {
        let engine = engine_with_keys();
        let err = engine
            .resolve_provider("nonexistent-model-xyz")
            .unwrap_err();
        assert!(matches!(err, ExecutorError::UnsupportedProvider(_)));
    }

    #[test]
    fn test_route_missing_api_key() {
        let mut engine = RoutingEngine::new(RouterConfig::default());
        let err = engine.route("gpt-4o").unwrap_err();
        assert!(matches!(err, ExecutorError::AuthFailed(_)));
    }

    #[test]
    fn test_route_success() {
        let mut engine = engine_with_keys();
        let target = engine.route("claude-sonnet-4").unwrap();
        assert_eq!(target.provider_id, "claude");
        assert_eq!(target.account_key, "sk-ant");
        assert_eq!(target.executor.api_format(), ApiFormat::Claude);
    }

    #[test]
    fn test_load_from_db_creates_pools() {
        // Temp DB with two active openai connections (priority order)
        let dir = std::env::temp_dir().join(format!("omniroute-rt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let db = omniroute_db::Database::open(&path).unwrap();
        {
            let conn = db.conn.lock().unwrap();
            use omniroute_db::models::ProviderConnection;
            use omniroute_db::repos::provider_connection_repo as r;
            let now = "2026-01-01T00:00:00Z".to_string();
            for (id, prio) in [("c1", 1), ("c2", 2)] {
                r::insert(
                    &conn,
                    &ProviderConnection {
                        id: id.into(),
                        provider: "openai".into(),
                        auth_type: Some("apikey".into()),
                        name: None,
                        email: None,
                        api_key: Some(format!("sk-{id}")),
                        is_active: true,
                        priority: Some(prio),
                        data: serde_json::json!({}),
                        rate_limited_until: None,
                        backoff_level: Some(0),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    },
                )
                .unwrap();
            }
        }

        let mut cfg = RouterConfig::default();
        cfg.load_from_db(&db).unwrap();
        assert_eq!(cfg.accounts.pool_len("openai"), 2);
        assert_eq!(cfg.accounts.next_key("openai").unwrap(), "sk-c1");
        assert_eq!(cfg.accounts.next_key("openai").unwrap(), "sk-c2");
    }

    #[test]
    fn test_load_from_db_skips_inactive_and_sets_cooldown() {
        let dir = std::env::temp_dir().join(format!("omniroute-rt2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let db = omniroute_db::Database::open(&path).unwrap();
        {
            let conn = db.conn.lock().unwrap();
            use omniroute_db::models::ProviderConnection;
            use omniroute_db::repos::provider_connection_repo as r;
            let now = "2026-01-01T00:00:00Z".to_string();
            // inactive → skipped
            r::insert(
                &conn,
                &ProviderConnection {
                    id: "dead".into(),
                    provider: "openai".into(),
                    auth_type: None,
                    name: None,
                    email: None,
                    api_key: Some("sk-dead".into()),
                    is_active: false,
                    priority: Some(1),
                    data: serde_json::json!({}),
                    rate_limited_until: None,
                    backoff_level: Some(0),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                },
            )
            .unwrap();
            // rate-limited in the future → starts in cooldown
            let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
            let now2 = now.clone();
            r::insert(
                &conn,
                &ProviderConnection {
                    id: "rl".into(),
                    provider: "openai".into(),
                    auth_type: None,
                    name: None,
                    email: None,
                    api_key: Some("sk-rl".into()),
                    is_active: true,
                    priority: Some(1),
                    data: serde_json::json!({}),
                    rate_limited_until: Some(future),
                    backoff_level: Some(2),
                    created_at: now2,
                    updated_at: now,
                },
            )
            .unwrap();
        }

        let mut cfg = RouterConfig::default();
        cfg.load_from_db(&db).unwrap();
        assert_eq!(cfg.accounts.pool_len("openai"), 1, "inactive skipped");
        assert!(
            cfg.accounts.next_key("openai").is_none(),
            "rate-limited account should be in cooldown"
        );
    }

    #[test]
    fn test_route_prefer_picks_preferred_account() {
        let config =
            RouterConfig::default().with_pool("openai", vec!["sk-1".into(), "sk-2".into()]);
        let mut engine = RoutingEngine::new(config);

        let t = engine.route_prefer("gpt-4o", Some("sk-2")).unwrap();
        assert_eq!(t.account_key, "sk-2");

        // Preferred account unavailable → normal rotation
        engine.report("openai", "sk-2", AccountOutcome::RateLimited);
        let t = engine.route_prefer("gpt-4o", Some("sk-2")).unwrap();
        assert_ne!(t.account_key, "sk-2");
    }

    #[test]
    fn test_peek_health() {
        let config =
            RouterConfig::default().with_pool("openai", vec!["sk-1".into(), "sk-2".into()]);
        let engine = RoutingEngine::new(config);
        let (available, backoff) = engine.peek_health("openai");
        assert!(available);
        assert_eq!(backoff, 0);
    }

    #[test]
    fn test_route_rotates_accounts() {
        let config =
            RouterConfig::default().with_pool("openai", vec!["sk-1".into(), "sk-2".into()]);
        let mut engine = RoutingEngine::new(config);

        let t1 = engine.route("gpt-4o").unwrap();
        assert_eq!(t1.account_key, "sk-1");
        engine.report("openai", "sk-1", AccountOutcome::RateLimited);

        // sk-1 cooling down → sk-2
        let t2 = engine.route("gpt-4o").unwrap();
        assert_eq!(t2.account_key, "sk-2");
        engine.report("openai", "sk-2", AccountOutcome::Success);

        // sk-1 still cooling → sk-2 again
        let t3 = engine.route("gpt-4o").unwrap();
        assert_eq!(t3.account_key, "sk-2");
    }

    #[test]
    fn test_route_all_cooldown_returns_rate_limited() {
        let config = RouterConfig::default().with_key("openai", "sk-1");
        let mut engine = RoutingEngine::new(config);
        let t1 = engine.route("gpt-4o").unwrap();
        engine.report("openai", &t1.account_key, AccountOutcome::RateLimited);
        let err = engine.route("gpt-4o").unwrap_err();
        assert!(matches!(err, ExecutorError::RateLimited(_)));
    }
}
