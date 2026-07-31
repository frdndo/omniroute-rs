use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProvider {
    pub id: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub auth_header: Option<String>,
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
}

static RAW_JSON: &str = include_str!("../data/providers.json");

pub static REGISTRY: Lazy<HashMap<String, RegistryProvider>> = Lazy::new(|| {
    let providers: Vec<RegistryProvider> =
        serde_json::from_str(RAW_JSON).expect("providers.json must be valid");
    providers.into_iter().map(|p| (p.id.clone(), p)).collect()
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
    for (pid, p) in REGISTRY.iter() {
        if p.models.iter().any(|m| m.id == model) {
            return Some(pid.as_str());
        }
    }
    let lower = model.to_lowercase();
    for (pid, p) in REGISTRY.iter() {
        if p.models
            .iter()
            .any(|m| lower.starts_with(&m.id.to_lowercase()))
        {
            return Some(pid.as_str());
        }
    }
    None
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
}
