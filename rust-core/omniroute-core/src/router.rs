use crate::executor::{ExecutorError, ProviderExecutor};
use std::collections::HashMap;

/// Route resolution result: which provider + executor to use
#[derive(Debug)]
pub struct RouteTarget {
    pub provider_id: String,
    pub executor: ProviderExecutor,
}

/// Configuration for the routing engine — API keys per provider
#[derive(Debug, Default, Clone)]
pub struct RouterConfig {
    pub api_keys: HashMap<String, String>,
}

impl RouterConfig {
    pub fn with_key(mut self, provider: &str, key: &str) -> Self {
        self.api_keys.insert(provider.to_string(), key.to_string());
        self
    }
}

/// Resolves a requested model to a provider + executor.
#[derive(Clone)]
pub struct RoutingEngine {
    config: RouterConfig,
}

impl RoutingEngine {
    pub fn new(config: RouterConfig) -> Self {
        Self { config }
    }

    /// Resolve the provider id that owns a model.
    ///
    /// Strategy:
    /// 1. Exact model match against the provider registry
    /// 2. Prefix match ("gpt-4o" → provider whose models start with "gpt")
    /// 3. Common known prefixes as last resort
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

    /// Resolve model → RouteTarget (executor ready to call).
    pub fn route(&self, model: &str) -> Result<RouteTarget, ExecutorError> {
        let provider_id = self.resolve_provider(model)?;
        let api_key = self
            .config
            .api_keys
            .get(provider_id)
            .cloned()
            .unwrap_or_default();

        if api_key.is_empty() {
            return Err(ExecutorError::AuthFailed(0));
        }

        let executor = ProviderExecutor::from_provider_id(provider_id, &api_key)?;
        Ok(RouteTarget {
            provider_id: provider_id.to_string(),
            executor,
        })
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
        // "claude-sonnet-4-20250514" exact; prefix fallback for newer
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
        let engine = RoutingEngine::new(RouterConfig::default());
        let err = engine.route("gpt-4o").unwrap_err();
        assert!(matches!(err, ExecutorError::AuthFailed(_)));
    }

    #[test]
    fn test_route_success() {
        let engine = engine_with_keys();
        let target = engine.route("claude-sonnet-4").unwrap();
        assert_eq!(target.provider_id, "claude");
        assert_eq!(target.executor.api_format(), ApiFormat::Claude);
    }
}
