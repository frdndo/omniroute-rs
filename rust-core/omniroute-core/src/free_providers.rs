use serde::Serialize;

/// M12: Free providers — parity with OmniRoute FE free-provider catalog.
///
/// Two categories mirroring the original:
/// - `noauth`: no API key needed (public endpoints)
/// - `apikey`: free-tier providers that need a (free) API key
///
/// OmniRoute asli: 104 free providers (9 noauth + 95 apikey free-tier).
/// Kita: 41 (4 noauth + 37 apikey) — subset yang endpoint-nya stabil &
/// beneran free tier. Yang di-skip: reverse-engineered web-scrape
/// (duckduckgo-web, felo-web, veoaifree-web, auggie, chipotle) yang rawan
/// berubah tanpa notice, plus regional CN providers.
///
/// Rankings use OUR telemetry (request_logs) instead of OmniRoute's
/// external ELO scores — real observed latency/error rate per provider.

#[derive(Debug, Clone, Serialize)]
pub struct FreeProvider {
    pub id: String,
    pub name: String,
    pub category: String, // "noauth" | "apikey"
    pub provider: String, // registry/provider id for the executor
    pub format: String,
    pub base_url: String,
    pub free_note: String,
    pub auth_hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signup_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_url: Option<String>,
    pub models: Vec<String>,
}

/// Model ids "free" dari registry provider (fallback: 3 pertama).
fn registry_free_models(models: &[omniroute_providers::RegistryModel]) -> Vec<String> {
    let free: Vec<String> = models
        .iter()
        .filter(|m| m.id.to_lowercase().contains("free"))
        .map(|m| m.id.clone())
        .collect();
    if !free.is_empty() {
        return free.into_iter().take(6).collect();
    }
    models.iter().take(3).map(|m| m.id.clone()).collect()
}

/// Builder: format/base_url/models diambil dari registry curated (220
/// provider) — konsisten dengan executor. Models auto-fill (free dulu).
fn mk(
    id: &str,
    name: &str,
    category: &str,
    free_note: &str,
    auth_hint: &str,
    signup: Option<&str>,
    key_url: Option<&str>,
) -> FreeProvider {
    let reg = omniroute_providers::get_provider(id);
    let format = reg
        .and_then(|p| p.format.clone())
        .unwrap_or_else(|| "openai".into());
    let base_url = reg.and_then(|p| p.base_url.clone()).unwrap_or_default();
    let models = reg
        .map(|p| registry_free_models(&p.models))
        .unwrap_or_default();
    FreeProvider {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        provider: id.into(),
        format,
        base_url,
        free_note: free_note.into(),
        auth_hint: auth_hint.into(),
        signup_url: signup.map(String::from),
        api_key_url: key_url.map(String::from),
        models,
    }
}

/// Curated free-tier catalog.
pub fn catalog() -> Vec<FreeProvider> {
    vec![
        // ── NOAUTH (tanpa key) ─────────────────────────────────────────
        FreeProvider {
            id: "opencode".into(),
            name: "OpenCode Free".into(),
            category: "noauth".into(),
            provider: "opencode".into(),
            format: "openai".into(),
            base_url: "https://opencode.ai/zen/v1".into(),
            free_note: "No API key required — public OpenCode endpoint (DeepSeek, MiMo, HY3, Big Pickle). Rate limits apply.".into(),
            auth_hint: "No credentials needed — langsung aktif dengan tombol Add.".into(),
            signup_url: None,
            api_key_url: None,
            models: vec![
                "deepseek-v4-flash-free".into(),
                "mimo-v2.5-free".into(),
                "hy3-free".into(),
                "big-pickle".into(),
            ],
        },
        mk(
            "mimocode",
            "Mimocode",
            "noauth",
            "Public free endpoint — coding models tanpa API key.",
            "No credentials needed.",
            None,
            None,
        ),
        mk(
            "theoldllm",
            "TheOldLLM",
            "noauth",
            "Public free endpoint — open models tanpa API key.",
            "No credentials needed.",
            None,
            None,
        ),
        mk(
            "aihorde",
            "AI Horde",
            "noauth",
            "Komunitas distributed inference — gratis, antrean berdasarkan kudos.",
            "No credentials needed (kudos opsional untuk prioritas).",
            None,
            None,
        ),
        // ── APIKEY free-tier ───────────────────────────────────────────
        FreeProvider {
            id: "gemini".into(),
            name: "Google Gemini Free Tier".into(),
            category: "apikey".into(),
            provider: "gemini".into(),
            format: "gemini".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            free_note: "Gemini 2.x Flash gratis di free tier — key dari Google AI Studio.".into(),
            auth_hint: "Pakai API key gratis dari Google AI Studio (aistudio.google.com/apikey).".into(),
            signup_url: Some("https://aistudio.google.com".into()),
            api_key_url: Some("https://aistudio.google.com/apikey".into()),
            models: vec![
                "gemini-2.5-flash".into(),
                "gemini-2.5-flash-lite".into(),
                "gemini-2.0-flash".into(),
            ],
        },
        FreeProvider {
            id: "groq".into(),
            name: "Groq (Free Tier)".into(),
            category: "apikey".into(),
            provider: "groq".into(),
            format: "openai".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            free_note: "Llama & Qwen gratis, rate limit 30 RPM — key dari console.groq.com.".into(),
            auth_hint: "API key gratis di console.groq.com/keys.".into(),
            signup_url: Some("https://console.groq.com".into()),
            api_key_url: Some("https://console.groq.com/keys".into()),
            models: vec![
                "llama-3.3-70b-versatile".into(),
                "llama-3.1-8b-instant".into(),
                "qwen-2.5-32b".into(),
            ],
        },
        FreeProvider {
            id: "mistral".into(),
            name: "Mistral (Free Tier)".into(),
            category: "apikey".into(),
            provider: "mistral".into(),
            format: "openai".into(),
            base_url: "https://api.mistral.ai/v1".into(),
            free_note: "mistral-small & open models gratis (experiment tier) — key dari console.mistral.ai.".into(),
            auth_hint: "API key gratis di console.mistral.ai/api-keys.".into(),
            signup_url: Some("https://console.mistral.ai".into()),
            api_key_url: Some("https://console.mistral.ai/api-keys".into()),
            models: vec![
                "mistral-small-latest".into(),
                "open-mistral-7b".into(),
            ],
        },
        FreeProvider {
            id: "cerebras".into(),
            name: "Cerebras (Free Tier)".into(),
            category: "apikey".into(),
            provider: "cerebras".into(),
            format: "openai".into(),
            base_url: "https://api.cerebras.ai/v1".into(),
            free_note: "Llama 3.3-70B gratis, sangat cepat — key dari cloud.cerebras.ai.".into(),
            auth_hint: "API key gratis di cloud.cerebras.ai — menu API Keys.".into(),
            signup_url: Some("https://cloud.cerebras.ai".into()),
            api_key_url: None,
            models: vec!["llama-3.3-70b".into(), "llama-3.1-8b".into()],
        },
        FreeProvider {
            id: "huggingface".into(),
            name: "HuggingFace Inference".into(),
            category: "apikey".into(),
            provider: "huggingface".into(),
            format: "openai".into(),
            base_url: "https://router.huggingface.co/v1".into(),
            free_note: "Akses banyak model open-source gratis via HF Inference Providers — key dari huggingface.co/settings/tokens.".into(),
            auth_hint: "Free access token (read role) dari HF settings.".into(),
            signup_url: Some("https://huggingface.co/join".into()),
            api_key_url: Some("https://huggingface.co/settings/tokens".into()),
            models: vec![
                "meta-llama/Llama-3.3-70B-Instruct".into(),
                "Qwen/Qwen2.5-72B-Instruct".into(),
                "mistralai/Mistral-7B-Instruct-v0.3".into(),
            ],
        },
        mk(
            "deepseek",
            "DeepSeek",
            "apikey",
            "Model DeepSeek murah banget, kadang free-tier promo — key dari platform.deepseek.com.",
            "API key di platform.deepseek.com/api_keys.",
            Some("https://platform.deepseek.com"),
            Some("https://platform.deepseek.com/api_keys"),
        ),
        mk(
            "openrouter",
            "OpenRouter (Free Models)",
            "apikey",
            "Ribuan model, banyak yang gratis (suffix ':free') — satu key buat semua.",
            "API key di openrouter.ai/keys.",
            Some("https://openrouter.ai"),
            Some("https://openrouter.ai/keys"),
        ),
        mk(
            "fireworks",
            "Fireworks AI",
            "apikey",
            "Hosting model open-source super cepat, free tier tersedia.",
            "API key di fireworks.ai/login.",
            Some("https://fireworks.ai"),
            None,
        ),
        mk(
            "nvidia",
            "NVIDIA NIM",
            "apikey",
            "Llama & Qwen via NVIDIA NIM — free credits untuk mulai.",
            "API key di build.nvidia.com.",
            Some("https://build.nvidia.com"),
            None,
        ),
        mk(
            "siliconflow",
            "SiliconFlow",
            "apikey",
            "Model open-source Cina & global, free tier murah.",
            "API key di cloud.siliconflow.cn.",
            Some("https://cloud.siliconflow.cn"),
            None,
        ),
        mk(
            "github-models",
            "GitHub Models",
            "apikey",
            "Akses model frontier (GPT, Claude, Gemini) via GitHub — rate limit gratis.",
            "Butuh GitHub account + token — docs.github.com/models.",
            Some("https://github.com/marketplace/models"),
            None,
        ),
        mk(
            "deepinfra",
            "DeepInfra",
            "apikey",
            "Inference open-source murah, ada free tier.",
            "API key di deepinfra.com/dash.",
            Some("https://deepinfra.com"),
            Some("https://deepinfra.com/dash/api_keys"),
        ),
        mk(
            "sambanova",
            "SambaNova",
            "apikey",
            "Llama 4 & DeepSeek cepat — free tier preview.",
            "API key di cloud.sambanova.ai.",
            Some("https://cloud.sambanova.ai"),
            None,
        ),
        mk(
            "nebius",
            "Nebius AI",
            "apikey",
            "Studio model open-source — free credits untuk mulai.",
            "API key di studio.nebius.ai.",
            Some("https://studio.nebius.ai"),
            None,
        ),
        mk(
            "hyperbolic",
            "Hyperbolic",
            "apikey",
            "Inference open-source murah, free credits awal.",
            "API key di app.hyperbolic.xyz.",
            Some("https://app.hyperbolic.xyz"),
            None,
        ),
        mk(
            "ollama-cloud",
            "Ollama Cloud",
            "apikey",
            "Model open-source via Ollama Cloud — ada free tier.",
            "API key di ollama.com/cloud.",
            Some("https://ollama.com/cloud"),
            None,
        ),
        mk(
            "puter",
            "Puter AI",
            "apikey",
            "Free credits bulanan untuk model frontier.",
            "API key di developer.puter.com.",
            Some("https://developer.puter.com"),
            None,
        ),
        mk(
            "pollinations",
            "Pollinations AI",
            "apikey",
            "Model gratis via Pollinations — open & community.",
            "Bisa tanpa key, atau API key di pollinations.ai.",
            Some("https://pollinations.ai"),
            None,
        ),
        mk(
            "cohere",
            "Cohere",
            "apikey",
            "Command R gratis (trial API key) — key dari dashboard.cohere.com.",
            "API key di dashboard.cohere.com/api-keys.",
            Some("https://dashboard.cohere.com"),
            Some("https://dashboard.cohere.com/api-keys"),
        ),
        mk(
            "reka",
            "Reka AI",
            "apikey",
            "Flash & open models — $10/bulan free API credits.",
            "API key di platform.reka.ai.",
            Some("https://platform.reka.ai"),
            None,
        ),
        mk(
            "ai21",
            "AI21 Labs",
            "apikey",
            "Jamba & Jurassic — free tier tersedia.",
            "API key di ai21.com/studio.",
            Some("https://www.ai21.com/studio"),
            None,
        ),
        mk(
            "nous-research",
            "Nous Research",
            "apikey",
            "Hermes & open models via Nous — free credits.",
            "API key di nousresearch.com.",
            Some("https://nousresearch.com"),
            None,
        ),
        mk(
            "liquid",
            "Liquid AI",
            "apikey",
            "LFM models efisien — free tier.",
            "API key di liquid.ai.",
            Some("https://www.liquid.ai"),
            None,
        ),
        mk(
            "dgrid",
            "DGrid",
            "apikey",
            "Router open-source model — free tier.",
            "API key di dgrid.site.",
            Some("https://dgrid.site"),
            None,
        ),
        mk(
            "novita",
            "Novita AI",
            "apikey",
            "Inference murah, $0.5 credit untuk mulai.",
            "API key di novita.ai.",
            Some("https://novita.ai"),
            None,
        ),
        mk(
            "morph",
            "Morph",
            "apikey",
            "Open-source inference, free credits.",
            "API key di morph.so.",
            Some("https://morph.so"),
            None,
        ),
        mk(
            "blackbox",
            "Blackbox AI",
            "apikey",
            "Coding models — free tier.",
            "API key di blackbox.ai.",
            Some("https://www.blackbox.ai"),
            None,
        ),
        mk(
            "moonshot",
            "Moonshot AI",
            "apikey",
            "Kimi models — free credits awal.",
            "API key di platform.moonshot.cn.",
            Some("https://platform.moonshot.cn"),
            None,
        ),
        mk(
            "together",
            "Together AI",
            "apikey",
            "Hosting open-source, $1 free credit awal.",
            "API key di api.together.ai/settings/api-keys.",
            Some("https://www.together.ai"),
            Some("https://api.together.ai/settings/api-keys"),
        ),
        mk(
            "xai",
            "xAI (Grok)",
            "apikey",
            "Grok via xAI — free tier terbatas.",
            "API key di console.x.ai.",
            Some("https://console.x.ai"),
            None,
        ),
        mk(
            "qwen-cloud",
            "Qwen Cloud",
            "apikey",
            "Qwen via Alibaba Cloud — free quota pemula.",
            "API key di bailian.console.aliyun.com.",
            Some("https://bailian.console.aliyun.com"),
            None,
        ),
        mk(
            "trae",
            "Trae",
            "apikey",
            "Coding models via Trae — free tier.",
            "API key di trae.ai.",
            Some("https://www.trae.ai"),
            None,
        ),
        mk(
            "friendliai",
            "FriendliAI",
            "apikey",
            "Inference serverless — free tier.",
            "API key di friendli.ai/signup.",
            Some("https://friendli.ai"),
            None,
        ),
        mk(
            "monsterapi",
            "MonsterAPI",
            "apikey",
            "Open-source models — free credits awal.",
            "API key di monsterapi.ai.",
            Some("https://monsterapi.ai"),
            None,
        ),
        mk(
            "featherless-ai",
            "Featherless AI",
            "apikey",
            "Ribuan model open-source, free tier.",
            "API key di featherless.ai.",
            Some("https://featherless.ai"),
            None,
        ),
        mk(
            "nscale",
            "nScale",
            "apikey",
            "Open-source inference — free tier.",
            "API key di nscale.ai.",
            Some("https://nscale.ai"),
            None,
        ),
        mk(
            "baseten",
            "Baseten",
            "apikey",
            "Model hosting — free credits.",
            "API key di baseten.ai.",
            Some("https://baseten.ai"),
            None,
        ),
    ]
}

pub fn get(id: &str) -> Option<FreeProvider> {
    catalog().into_iter().find(|p| p.id == id)
}

/// True if a provider id is a no-auth provider (no API key needed).
pub fn is_noauth(provider_id: &str) -> bool {
    catalog()
        .iter()
        .any(|p| p.category == "noauth" && p.provider == provider_id)
}

/// Provider ids that are already configured (from DB connections).
pub fn installed_ids(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    use omniroute_db::repos::provider_connection_repo;
    let all = provider_connection_repo::get_all(conn).map_err(|e| e.to_string())?;
    Ok(all.iter().map(|c| c.provider.clone()).collect())
}

/// Build the response rows: catalog + installed flag + telemetry ranking.
pub fn list_with_telemetry(
    conn: Option<&rusqlite::Connection>,
    category: Option<&str>,
    configured_only: bool,
) -> Vec<serde_json::Value> {
    use omniroute_db::repos::request_log_repo;
    let installed: Vec<String> = conn.and_then(|c| installed_ids(c).ok()).unwrap_or_default();
    let telemetry: Vec<serde_json::Value> = conn
        .map(request_log_repo::provider_stats)
        .unwrap_or_default();

    catalog()
        .into_iter()
        .filter(|p| category.is_none_or(|c| c == p.category))
        .filter(|p| !configured_only || installed.contains(&p.provider))
        .map(|p| {
            let is_installed = installed.contains(&p.provider);
            let stats = telemetry
                .iter()
                .find(|t| t["provider"] == p.provider)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "category": p.category,
                "provider": p.provider,
                "format": p.format,
                "base_url": p.base_url,
                "free_note": p.free_note,
                "auth_hint": p.auth_hint,
                "signup_url": p.signup_url,
                "api_key_url": p.api_key_url,
                "models": p.models,
                "installed": is_installed,
                "telemetry": stats,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_entries_valid() {
        let c = catalog();
        assert!(!c.is_empty());
        for p in &c {
            assert!(!p.id.is_empty());
            assert!(p.category == "noauth" || p.category == "apikey");
            assert!(!p.models.is_empty());
        }
    }

    #[test]
    fn test_get_finds_entry() {
        assert!(get("groq").is_some());
        assert!(get("tidak-ada").is_none());
    }

    #[test]
    fn test_list_without_db() {
        let rows = list_with_telemetry(None, None, false);
        assert_eq!(rows.len(), catalog().len());
        assert!(rows.iter().all(|r| r["installed"] == false));
    }

    #[test]
    fn test_filter_by_category() {
        let rows = list_with_telemetry(None, Some("noauth"), false);
        assert!(rows.iter().all(|r| r["category"] == "noauth"));
    }
}
