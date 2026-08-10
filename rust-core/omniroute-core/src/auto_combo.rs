//! Auto-combo (parity OmniRoute autoCombo — versi sederhana).
//!
//! Asli punya 12 scoring factors (quota, health, costInv, latencyInv,
//! taskFit, stability, tierPriority, tierAffinity, specificityMatch,
//! contextAffinity, resetWindowAffinity, connectionDensity). Kita pakai
//! subset yang punya data nyata dari telemetry sendiri:
//!   health (1 - error_rate) · latencyInv · costInv (free/noauth) ·
//!   stability (evidence dari jumlah request) · connection (configured).
//!
//! Output: ranking provider per model + chain combo (model asli + 2
//! fallback gratis terbaik) → dibuatkan combo `auto-{model}`.

use omniroute_db::repos::request_log_repo;

/// Score factors weights (mirip DEFAULT_WEIGHTS asli, dinormalisasi).
const W_HEALTH: f64 = 0.30;
const W_LATENCY: f64 = 0.20;
const W_COST: f64 = 0.15;
const W_STABILITY: f64 = 0.10;
const W_CONNECTION: f64 = 0.25;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderRank {
    pub provider: String,
    pub score: f64,
    pub health: f64,
    pub latency_inv: f64,
    pub cost_inv: f64,
    pub stability: f64,
    pub connected: bool,
    pub requests: i64,
    pub avg_duration_ms: f64,
    pub error_rate: f64,
    pub reason: String,
}

/// Rank providers owning `model` — hanya yang configured atau noauth
/// (yang beneran bisa dipakai routing).
pub fn rank_providers_for_model(
    model: &str,
    conn: Option<&rusqlite::Connection>,
) -> Vec<ProviderRank> {
    let owners = omniroute_providers::providers_for_model(model);
    let stats: Vec<serde_json::Value> = conn
        .map(request_log_repo::provider_stats)
        .unwrap_or_default();
    let stats_by_provider: std::collections::HashMap<&str, &serde_json::Value> = stats
        .iter()
        .map(|s| (s["provider"].as_str().unwrap_or(""), s))
        .collect();
    let installed: Vec<String> = conn
        .and_then(|c| crate::free_providers::installed_ids(c).ok())
        .unwrap_or_default();

    let mut ranks: Vec<ProviderRank> = Vec::new();
    for provider in owners {
        let is_noauth = crate::free_providers::is_noauth(provider);
        let connected = installed.contains(&provider.to_string());
        if !connected && !is_noauth {
            continue; // gak bisa dipakai routing → skip
        }
        let st = stats_by_provider.get(provider).copied();
        let requests = st.and_then(|s| s["requests"].as_i64()).unwrap_or(0);
        let avg_ms = st
            .and_then(|s| s["avg_duration_ms"].as_f64())
            .unwrap_or(0.0);
        let error_rate = st.and_then(|s| s["error_rate"].as_f64()).unwrap_or(0.0);

        // Neutral jika belum ada telemetry (belum pernah dipakai)
        let health = if requests > 0 { 1.0 - error_rate } else { 0.6 };
        let latency_inv = if avg_ms > 0.0 {
            1.0 / (1.0 + avg_ms / 1000.0)
        } else {
            0.5
        };
        let cost_inv = if is_noauth { 1.0 } else { 0.6 }; // noauth = gratis penuh
        let stability = (requests as f64 / 10.0).min(1.0);
        let connected_f = if connected { 1.0 } else { 0.4 }; // noauth tanpa connection dapat 0.4

        let score = W_HEALTH * health
            + W_LATENCY * latency_inv
            + W_COST * cost_inv
            + W_STABILITY * stability
            + W_CONNECTION * connected_f;

        let reason = if connected {
            "terkonfigurasi + telemetry OK".to_string()
        } else {
            "noauth gratis".to_string()
        };
        ranks.push(ProviderRank {
            provider: provider.to_string(),
            score,
            health,
            latency_inv,
            cost_inv,
            stability,
            connected,
            requests,
            avg_duration_ms: avg_ms,
            error_rate,
            reason,
        });
    }
    ranks.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranks
}

/// Chain combo: model asli + 2 fallback GRATIS terbaik (dari katalog free,
/// telemetry ranked). Max 3 entries.
pub fn suggest_chain(model: &str, conn: Option<&rusqlite::Connection>) -> Vec<String> {
    let mut chain = vec![model.to_string()];
    let mut fallbacks: Vec<(String, f64)> = Vec::new();
    for fp in crate::free_providers::catalog() {
        if fp.provider == model {
            continue;
        }
        for m in fp.models {
            if m == model {
                continue;
            }
            // cek provider-nya usable (configured atau noauth)
            let usable = fp.category == "noauth"
                || conn
                    .and_then(|c| crate::free_providers::installed_ids(c).ok())
                    .map(|v| v.contains(&fp.provider))
                    .unwrap_or(false);
            if !usable {
                continue;
            }
            let rank = rank_providers_for_model(&m, conn);
            let score = rank.first().map(|r| r.score).unwrap_or(0.0);
            fallbacks.push((m, score));
        }
    }
    fallbacks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (m, _) in fallbacks.into_iter().take(2) {
        chain.push(m);
    }
    chain
}

/// Create the combo row (engine-agnostic: caller handles DB + reload).
pub fn create_combo_row(
    conn: &rusqlite::Connection,
    combo_repo: &impl Fn(&rusqlite::Connection, &omniroute_db::models::Combo) -> Result<(), String>,
    model: &str,
) -> Result<omniroute_db::models::Combo, String> {
    let name = format!("auto-{model}");
    let chain = suggest_chain(model, Some(conn));
    let now = chrono::Utc::now().to_rfc3339();
    let combo = omniroute_db::models::Combo {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.clone(),
        kind: "auto".into(),
        models: chain,
        created_at: now.clone(),
        updated_at: now,
    };
    combo_repo(conn, &combo)?;
    Ok(combo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rank_filters_unusable() {
        // Tanpa DB: noauth opencode tetap muncul, provider ber-key (tanpa
        // connection) tersaring.
        let ranks = rank_providers_for_model("deepseek-v4-flash-free", None);
        assert!(ranks.iter().any(|r| r.provider == "opencode"));
        for r in &ranks {
            assert!(r.connected || crate::free_providers::is_noauth(&r.provider));
        }
    }

    #[test]
    fn test_suggest_chain_max_3() {
        let chain = suggest_chain("gpt-4o", None);
        assert_eq!(chain[0], "gpt-4o");
        assert!(chain.len() <= 3);
    }
}
