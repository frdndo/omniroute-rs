use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProvider {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub auth_header: Option<String>,
    #[serde(default)]
    pub auth_prefix: Option<String>,
    #[serde(default)]
    pub models_url: Option<String>,
    #[serde(default)]
    pub model_count: Option<usize>,
    #[serde(default)]
    pub models: Vec<RegistryModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryModel {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub supports_reasoning: Option<bool>,
}

static RAW_JSON: &str = include_str!("../data/providers.json");

/// Model ids that are obviously not real models — scraped error messages /
/// duplicated display names in the raw catalog (e.g. opencode entries like
/// "reasoning_content ... must be passed back", "Model X is not supported").
fn is_junk_model(id: &str) -> bool {
    if id.chars().any(|c| c.is_whitespace()) {
        return true;
    }
    let l = id.to_lowercase();
    l.contains("must be passed")
        || l.contains("not supported")
        || l.contains("reasoning_content")
        || l.starts_with("model x")
}

/// Provider list in catalog (file) order — deterministic iteration
/// (REGISTRY is a HashMap with random order). Junk models are filtered out.
pub static PROVIDER_LIST: Lazy<Vec<RegistryProvider>> = Lazy::new(|| {
    let mut providers: Vec<RegistryProvider> =
        serde_json::from_str(RAW_JSON).expect("providers.json must be valid");
    for p in &mut providers {
        p.models.retain(|m| !is_junk_model(&m.id));
    }
    providers
});

pub static REGISTRY: Lazy<HashMap<String, RegistryProvider>> = Lazy::new(|| {
    PROVIDER_LIST
        .iter()
        .map(|p| (p.id.clone(), p.clone()))
        .collect()
});

pub fn get_provider(id: &str) -> Option<&'static RegistryProvider> {
    REGISTRY.get(id)
}

pub fn list_providers() -> Vec<&'static RegistryProvider> {
    REGISTRY.values().collect()
}

pub fn list_provider_ids() -> Vec<&'static str> {
    REGISTRY.keys().map(|s| s.as_str()).collect()
}

pub fn provider_count() -> usize {
    REGISTRY.len()
}

pub fn model_count() -> usize {
    REGISTRY.values().map(|p| p.models.len()).sum()
}

/// Resolve the provider id that owns a model (exact match first, then prefix).
pub fn resolve_provider_for_model(model: &str) -> Option<&'static str> {
    providers_for_model(model).into_iter().next()
}

/// ALL providers that carry a model — exact matches first, then prefix
/// matches, in catalog (file) order — deterministic, mirrors OmniRoute's
/// `pool` (all providers registering a model).
pub fn providers_for_model(model: &str) -> Vec<&'static str> {
    let lower = model.to_lowercase();
    let mut exact = Vec::new();
    let mut prefix = Vec::new();
    for p in PROVIDER_LIST.iter() {
        if p.models.iter().any(|m| m.id == model) {
            exact.push(p.id.as_str());
        } else if p
            .models
            .iter()
            .any(|m| lower.starts_with(&m.id.to_lowercase()))
        {
            prefix.push(p.id.as_str());
        }
    }
    exact.into_iter().chain(prefix).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_loaded() {
        assert!(
            provider_count() >= 100,
            "expected 100+ providers, got {}",
            provider_count()
        );
        assert!(
            model_count() > 500,
            "expected 500+ models, got {}",
            model_count()
        );
    }

    #[test]
    fn test_known_providers_present() {
        for id in [
            "openai",
            "claude",
            "anthropic",
            "gemini",
            "deepseek",
            "groq",
            "mistral",
        ] {
            assert!(REGISTRY.contains_key(id), "missing provider: {}", id);
        }
    }

    #[test]
    fn test_resolve_model() {
        // gpt-4o exists in many providers — registry returns one of them
        assert!(resolve_provider_for_model("gpt-4o").is_some());
        // Unknown model → None
        assert!(resolve_provider_for_model("nonexistent-xyz").is_none());
    }

    #[test]
    fn test_provider_has_models() {
        let p = get_provider("openai").expect("openai exists");
        assert!(!p.models.is_empty());
        assert!(p.models.iter().any(|m| m.id == "gpt-4o"));
    }

    #[test]
    fn test_format_field() {
        let claude = get_provider("claude").expect("claude exists");
        assert_eq!(claude.format.as_deref(), Some("claude"));
    }

    #[test]
    fn test_no_junk_models() {
        // Scraped error messages / display-name duplicates must be filtered.
        for p in PROVIDER_LIST.iter() {
            for m in &p.models {
                assert!(!is_junk_model(&m.id), "junk model: {}", m.id);
            }
        }
        let oc = get_provider("opencode").expect("opencode exists");
        assert!(oc.models.iter().any(|m| m.id == "deepseek-v4-flash-free"));
        assert!(
            !oc.models
                .iter()
                .any(|m| m.id.contains("must be passed") || m.id == "Model X is not supported")
        );
    }
}
