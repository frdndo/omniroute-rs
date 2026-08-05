use serde::Serialize;

/// M12: Free providers — parity with OmniRoute FE free-provider catalog.
///
/// Two categories mirroring the original:
/// - `noauth`: no API key needed (public endpoints)
/// - `apikey`: free-tier providers that need a (free) API key
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

/// Curated free-tier catalog (kept deliberately small & stable — no
/// reverse-engineered endpoints like DDG/Felo which break without notice).
pub fn catalog() -> Vec<FreeProvider> {
    vec![
        FreeProvider {
            id: "opencode".into(),
            name: "OpenCode Free".into(),
            category: "noauth".into(),
            provider: "opencode".into(),
            format: "openai".into(),
            base_url: "https://opencode.ai/zen/v1".into(),
            free_note: "No API key required — public OpenCode endpoint (Kimi, GLM, Qwen, MiniMax). Rate limits apply.".into(),
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
