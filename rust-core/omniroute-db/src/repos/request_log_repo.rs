use anyhow::Result;
use rusqlite::{Connection, params};

/// Persistent request telemetry (M2) — powers the Analytics dashboard.
pub fn insert(
    conn: &Connection,
    method: &str,
    uri: &str,
    status: i64,
    duration_ms: i64,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO request_logs (ts, method, uri, status, duration_ms, provider, model)
         VALUES (datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6)",
        params![method, uri, status, duration_ms, provider, model],
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
        "SELECT COALESCE(provider, 'unknown'), COUNT(*), AVG(duration_ms)
         FROM request_logs GROUP BY COALESCE(provider, 'unknown')
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
        )
        .unwrap();
        insert(&c, "GET", "/health", 200, 1, None, None).unwrap();
        insert(
            &c,
            "POST",
            "/v1/chat/completions",
            429,
            3,
            Some("openai"),
            Some("gpt-4o"),
        )
        .unwrap();

        assert_eq!(count_total(&c), 3);
        assert_eq!(count_status_class(&c, 2), 2);
        assert_eq!(count_status_class(&c, 4), 1);
        assert!(avg_duration(&c) > 0.0);

        let by_prov = by_provider(&c);
        assert_eq!(by_prov.len(), 2, "openai + unknown (health row)");
        let openai = by_prov.iter().find(|p| p["provider"] == "openai").unwrap();
        assert_eq!(openai["requests"], 2);

        let hourly = hourly_counts(&c, 24);
        assert!(!hourly.is_empty());
        assert_eq!(hourly[0]["count"], 3);
    }
}
