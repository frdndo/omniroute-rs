use std::collections::HashMap;

/// M3 costs: spend computed from telemetry token totals × pricing table,
/// plus monthly budget status per provider.
pub struct Costs;

impl Costs {
    /// Compute spend for a month ("2026-08"):
    /// per provider: tokens, estimated USD (pricing join), budget usage.
    /// Models without pricing entries are estimated at $0 (flagged).
    pub fn report(db: &omniroute_db::Database, month: &str) -> serde_json::Value {
        let Ok(conn) = db.conn.lock() else {
            return serde_json::json!({"error": "db locked"});
        };
        let pricing = omniroute_db::repos::pricing_repo::get_all(&conn).unwrap_or_default();
        let budgets = omniroute_db::repos::pricing_repo::get_budgets(&conn).unwrap_or_default();
        let totals = omniroute_db::repos::request_log_repo::token_totals(&conn, month);

        // pricing lookup: (provider, model) -> (input, output) per MTok
        let mut price_map: HashMap<(String, String), (f64, f64)> = HashMap::new();
        for p in &pricing {
            price_map.insert(
                (p.provider.clone(), p.model.clone()),
                (p.input_per_mtok, p.output_per_mtok),
            );
        }
        // provider-level pricing fallback: provider -> (input, output)
        let mut provider_price: HashMap<String, (f64, f64)> = HashMap::new();
        for p in &pricing {
            if p.model == "*" {
                provider_price.insert(p.provider.clone(), (p.input_per_mtok, p.output_per_mtok));
            }
        }

        let mut per_provider: Vec<serde_json::Value> = Vec::new();
        let mut total_spend = 0.0f64;
        let mut total_prompt = 0i64;
        let mut total_completion = 0i64;
        let mut missing_pricing = 0usize;

        for t in &totals {
            let provider = t["provider"].as_str().unwrap_or("unknown");
            let model = t["model"].as_str().unwrap_or("");
            let prompt = t["prompt_tokens"].as_i64().unwrap_or(0);
            let completion = t["completion_tokens"].as_i64().unwrap_or(0);
            total_prompt += prompt;
            total_completion += completion;

            let price = price_map
                .get(&(provider.to_string(), model.to_string()))
                .copied()
                .or_else(|| provider_price.get(provider).copied());
            let (spend, priced) = match price {
                Some((i, o)) => (
                    omniroute_db::repos::request_log_repo::cost_usd(i, o, prompt, completion),
                    true,
                ),
                None => {
                    missing_pricing += 1;
                    (0.0, false)
                }
            };
            total_spend += spend;
            per_provider.push(serde_json::json!({
                "provider": provider,
                "model": model,
                "prompt_tokens": prompt,
                "completion_tokens": completion,
                "spend_usd": (spend * 1000.0).round() / 1000.0,
                "priced": priced,
            }));
        }

        // budget status per provider (current month)
        let budget_rows: Vec<serde_json::Value> = budgets
            .iter()
            .filter(|b| b.month == month)
            .map(|b| {
                let spent: f64 = per_provider
                    .iter()
                    .filter(|p| p["provider"] == b.provider)
                    .map(|p| p["spend_usd"].as_f64().unwrap_or(0.0))
                    .sum();
                let pct = if b.limit_usd > 0.0 {
                    (spent / b.limit_usd) * 100.0
                } else {
                    0.0
                };
                serde_json::json!({
                    "provider": b.provider,
                    "month": b.month,
                    "limit_usd": b.limit_usd,
                    "spent_usd": (spent * 1000.0).round() / 1000.0,
                    "used_pct": (pct * 10.0).round() / 10.0,
                })
            })
            .collect();

        serde_json::json!({
            "month": month,
            "total_spend_usd": (total_spend * 1000.0).round() / 1000.0,
            "total_tokens": total_prompt + total_completion,
            "prompt_tokens": total_prompt,
            "completion_tokens": total_completion,
            "missing_pricing_models": missing_pricing,
            "per_provider": per_provider,
            "budgets": budget_rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omniroute_db::repos::{pricing_repo, request_log_repo};
    #[test]
    fn test_report_computes_spend_and_budget() {
        let db = omniroute_db::Database::open_in_memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            // pricing: gpt-4o $2.5/$10 per MTok
            pricing_repo::upsert(
                &conn,
                &pricing_repo::PricingRow {
                    id: "p1".into(),
                    provider: "openai".into(),
                    model: "gpt-4o".into(),
                    input_per_mtok: 2.5,
                    output_per_mtok: 10.0,
                },
            )
            .unwrap();
            pricing_repo::upsert_budget(
                &conn,
                &pricing_repo::BudgetRow {
                    id: "b1".into(),
                    provider: "openai".into(),
                    month: "2026-08".into(),
                    limit_usd: 10.0,
                },
            )
            .unwrap();
            // 1M prompt + 0.5M completion → $2.5 + $5 = $7.5
            request_log_repo::insert(
                &conn,
                "POST",
                "/v1/chat/completions",
                200,
                100,
                Some("openai"),
                Some("gpt-4o"),
                1_000_000,
                500_000,
            )
            .unwrap();
        }
        let r = Costs::report(&db, "2026-08");
        assert_eq!(r["total_spend_usd"], 7.5);
        assert_eq!(r["per_provider"][0]["priced"], true);
        assert_eq!(r["budgets"][0]["spent_usd"], 7.5);
        assert_eq!(r["budgets"][0]["used_pct"], 75.0);
    }

    #[test]
    fn test_unpriced_models_flagged() {
        let db = omniroute_db::Database::open_in_memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            request_log_repo::insert(
                &conn,
                "POST",
                "/v1/chat/completions",
                200,
                50,
                Some("unknown-provider"),
                Some("mystery-model"),
                1000,
                500,
            )
            .unwrap();
        }
        let r = Costs::report(&db, "2026-08");
        assert_eq!(r["missing_pricing_models"], 1);
        assert_eq!(r["per_provider"][0]["priced"], false);
        assert_eq!(r["total_spend_usd"], 0.0);
    }
}
