use crate::models::*;
use once_cell::sync::Lazy;
use std::collections::HashMap;

pub static REGISTRY: Lazy<HashMap<String, Provider>> = Lazy::new(|| {
    let mut m = HashMap::new();
    // OpenAI
    m.insert(
        "openai".into(),
        Provider {
            id: "openai".into(),
            name: "OpenAI".into(),
            auth_types: vec![AuthType::ApiKey],
            models: vec![
                ProviderModel {
                    id: "gpt-4o".into(),
                    name: "GPT-4o".into(),
                    context_window: Some(128000),
                    max_output: Some(16384),
                    pricing: Some(Pricing {
                        input: 2.50,
                        output: 10.00,
                        free: None,
                    }),
                },
                ProviderModel {
                    id: "gpt-4o-mini".into(),
                    name: "GPT-4o Mini".into(),
                    context_window: Some(128000),
                    max_output: Some(16384),
                    pricing: Some(Pricing {
                        input: 0.15,
                        output: 0.60,
                        free: None,
                    }),
                },
            ],
            base_url: "https://api.openai.com/v1".into(),
            status: ProviderStatus::Active,
        },
    );
    // Claude
    m.insert(
        "claude".into(),
        Provider {
            id: "claude".into(),
            name: "Anthropic Claude".into(),
            auth_types: vec![AuthType::ApiKey],
            models: vec![
                ProviderModel {
                    id: "claude-sonnet-4-20250514".into(),
                    name: "Claude Sonnet 4".into(),
                    context_window: Some(200000),
                    max_output: Some(8192),
                    pricing: Some(Pricing {
                        input: 3.00,
                        output: 15.00,
                        free: None,
                    }),
                },
                ProviderModel {
                    id: "claude-haiku-3-5-20241022".into(),
                    name: "Claude Haiku 3.5".into(),
                    context_window: Some(200000),
                    max_output: Some(8192),
                    pricing: Some(Pricing {
                        input: 0.80,
                        output: 4.00,
                        free: None,
                    }),
                },
            ],
            base_url: "https://api.anthropic.com/v1".into(),
            status: ProviderStatus::Active,
        },
    );
    // Gemini
    m.insert(
        "gemini".into(),
        Provider {
            id: "gemini".into(),
            name: "Google Gemini".into(),
            auth_types: vec![AuthType::ApiKey],
            models: vec![
                ProviderModel {
                    id: "gemini-2.5-flash".into(),
                    name: "Gemini 2.5 Flash".into(),
                    context_window: Some(1000000),
                    max_output: Some(8192),
                    pricing: Some(Pricing {
                        input: 0.075,
                        output: 0.30,
                        free: None,
                    }),
                },
                ProviderModel {
                    id: "gemini-2.5-pro".into(),
                    name: "Gemini 2.5 Pro".into(),
                    context_window: Some(1000000),
                    max_output: Some(8192),
                    pricing: Some(Pricing {
                        input: 1.25,
                        output: 5.00,
                        free: None,
                    }),
                },
            ],
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            status: ProviderStatus::Active,
        },
    );
    // DeepSeek
    m.insert(
        "deepseek".into(),
        Provider {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            auth_types: vec![AuthType::ApiKey],
            models: vec![ProviderModel {
                id: "deepseek-chat".into(),
                name: "DeepSeek V3".into(),
                context_window: Some(128000),
                max_output: Some(4096),
                pricing: Some(Pricing {
                    input: 0.27,
                    output: 1.10,
                    free: None,
                }),
            }],
            base_url: "https://api.deepseek.com/v1".into(),
            status: ProviderStatus::Active,
        },
    );
    m
});

pub fn get_provider(id: &str) -> Option<&'static Provider> {
    REGISTRY.get(id)
}

pub fn list_providers() -> Vec<&'static Provider> {
    REGISTRY.values().collect()
}

pub fn list_provider_ids() -> Vec<&'static str> {
    REGISTRY.keys().map(|s| s.as_str()).collect()
}
