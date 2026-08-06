use anyhow::Result;
use rusqlite::{Connection, params};

/// Persistent request telemetry (M2) — powers the Analytics dashboard.
#[allow(clippy::too_many_arguments)]
pub fn insert(
    conn: &Connection,
    method: &str,
    uri: &str,
    status: i64,
    duration_ms: i64,
    provider: Option<&str>,
    model: Option<&str>,
    prompt_tokens: i64,
    completion_tokens: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO request_logs (ts, method, uri, status, duration_ms, provider, model, prompt_tokens, completion_tokens)
         VALUES (datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![method, uri, status, duration_ms, provider, model, prompt_tokens, completion_tokens],
    )?;
    Ok(())
}

pub fn count_total(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM request_logs", [], |r| r.get(0))
        .unwrap_or(0)
}

/// Count requests whose status starts with `class` (e.g. 2 → 2xx).
pub fn count_status_class(conn: &Connection, class: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM request_logs WHERE status / 100 = ?1",
        params![class],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

pub fn avg_duration(conn: &Connection) -> f64 {
    conn.query_row("SELECT AVG(duration_ms) FROM request_logs", [], |r| {
        r.get::<_, Option<f64>>(0)
    })
    .unwrap_or(None)
    .unwrap_or(0.0)
}

/// Requests + avg latency per provider (chat calls only).
pub fn by_provider(conn: &Connection) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(
        "SELECT COALESCE(provider, 'non-chat (health/admin/models)'), COUNT(*), AVG(duration_ms)
         FROM request_logs GROUP BY COALESCE(provider, 'non-chat (health/admin/models)')
         ORDER BY COUNT(*) DESC",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = match stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "provider": r.get::<_, String>(0)?,
            "requests": r.get::<_, i64>(1)?,
            "avg_duration_ms": r.get::<_, f64>(2)?,
        }))
    }) {
        Ok(rows) => rows,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Per-provider health stats with error rate (chat calls) — powers the
/// free-provider rankings. Returns JSON rows: provider, requests,
/// avg_duration_ms, errors, error_rate.
pub fn provider_stats(conn: &Connection) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(
        "SELECT COALESCE(provider, 'unknown'),
                COUNT(*),
                AVG(duration_ms),
                SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END)
         FROM request_logs
         WHERE provider IS NOT NULL
         GROUP BY provider
         ORDER BY COUNT(*) DESC",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = match stmt.query_map([], |r| {
        let requests = r.get::<_, i64>(1)?;
        let errors = r.get::<_, i64>(3)?;
        Ok(serde_json::json!({
            "provider": r.get::<_, String>(0)?,
            "requests": requests,
            "avg_duration_ms": r.get::<_, f64>(2)?,
            "errors": errors,
            "error_rate": if requests > 0 { errors as f64 / requests as f64 } else { 0.0 },
        }))
    }) {
        Ok(rows) => rows,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Bucket counts for the last `hours` hours (0-filled).
pub fn hourly_counts(conn: &Connection, hours: i64) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(
        "SELECT strftime('%Y-%m-%d %H:00', ts) AS bucket, COUNT(*)
         FROM request_logs
         WHERE ts >= datetime('now', ?1)
         GROUP BY bucket ORDER BY bucket",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let shift = format!("-{} hours", hours);
    let rows = match stmt.query_map(params![shift], |r| {
        Ok(serde_json::json!({
            "bucket": r.get::<_, String>(0)?,
            "count": r.get::<_, i64>(1)?,
        }))
    }) {
        Ok(rows) => rows,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Token totals per provider+model for a month prefix ("2026-08").
pub fn token_totals(conn: &Connection, month: &str) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(
        "SELECT COALESCE(provider,'unknown'), COALESCE(model,''), SUM(prompt_tokens), SUM(completion_tokens)
         FROM request_logs WHERE substr(ts,1,7)=?1 GROUP BY provider, model",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = match stmt.query_map(params![month], |r| {
        Ok(serde_json::json!({
            "provider": r.get::<_, String>(0)?,
            "model": r.get::<_, String>(1)?,
            "prompt_tokens": r.get::<_, i64>(2)?,
            "completion_tokens": r.get::<_, i64>(3)?,
        }))
    }) {
        Ok(rows) => rows,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Cost of a request in USD given pricing (per million tokens).
pub fn cost_usd(input_per_mtok: f64, output_per_mtok: f64, prompt: i64, completion: i64) -> f64 {
    (prompt as f64 / 1_000_000.0) * input_per_mtok
        + (completion as f64 / 1_000_000.0) * output_per_mtok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrations::run_migrations(&c).unwrap();
        c
    }

    #[test]
    fn test_insert_and_aggregates() {
        let c = conn();
        insert(
            &c,
            "POST",
            "/v1/chat/completions",
            200,
            40,
            Some("openai"),
            Some("gpt-4o"),
            1000,
            200,
        )
        .unwrap();
        insert(&c, "GET", "/health", 200, 1, None, None, 0, 0).unwrap();
        insert(
            &c,
            "POST",
            "/v1/chat/completions",
            429,
            3,
            Some("openai"),
            Some("gpt-4o"),
            500,
            50,
        )
        .unwrap();

        assert_eq!(count_total(&c), 3);
        assert_eq!(count_status_class(&c, 2), 2);
        assert_eq!(count_status_class(&c, 4), 1);
        assert!(avg_duration(&c) > 0.0);

        let by_prov = by_provider(&c);
        assert_eq!(by_prov.len(), 2, "openai + non-chat (health row)");
        let openai = by_prov.iter().find(|p| p["provider"] == "openai").unwrap();
        assert_eq!(openai["requests"], 2);

        let hourly = hourly_counts(&c, 24);
        assert!(!hourly.is_empty());
        assert_eq!(hourly[0]["count"], 3);
    }
}
