use crate::account::{AccountManager, AccountOutcome};
use crate::executor::{ExecutorError, ProviderExecutor};
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
}

impl RouterConfig {
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
        // 1. Exact model match in registry
        for (provider_id, provider) in omniroute_providers::REGISTRY.iter() {
            if provider.models.iter().any(|m| m.id == model) {
                return Ok(provider_id.as_str());
            }
        }

        // 2. Prefix match
        let lower = model.to_lowercase();
        for (provider_id, provider) in omniroute_providers::REGISTRY.iter() {
            if provider
                .models
                .iter()
                .any(|m| lower.starts_with(&m.id.to_lowercase()))
            {
                return Ok(provider_id.as_str());
            }
        }

        // 3. Known prefixes
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

        let executor = ProviderExecutor::from_provider_id(provider_id, &account_key)?;
        Ok(RouteTarget {
            provider_id: provider_id.to_string(),
            account_key,
            executor,
        })
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
