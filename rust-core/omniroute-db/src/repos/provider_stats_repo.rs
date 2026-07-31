use anyhow::Result;
use rusqlite::{Connection, params};

/// Per-provider runtime stats persisted for the auto-combo scorer.
#[derive(Debug, Clone, Default)]
pub struct ProviderStatsRow {
    pub provider: String,
    pub latency_ema_ms: f64,
    pub total_requests: u64,
    pub failed_requests: u64,
}

pub fn get_all(conn: &Connection) -> Result<Vec<ProviderStatsRow>> {
    let mut stmt = conn.prepare(
        "SELECT provider, latency_ema_ms, total_requests, failed_requests FROM provider_stats",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ProviderStatsRow {
            provider: row.get(0)?,
            latency_ema_ms: row.get(1)?,
            total_requests: row.get(2)?,
            failed_requests: row.get(3)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn upsert(
    conn: &Connection,
    provider: &str,
    latency_ema_ms: f64,
    total_requests: u64,
    failed_requests: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO provider_stats (provider, latency_ema_ms, total_requests, failed_requests)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(provider) DO UPDATE SET
           latency_ema_ms=?2, total_requests=?3, failed_requests=?4",
        params![
            provider,
            latency_ema_ms,
            total_requests as i64,
            failed_requests as i64
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;

    #[test]
    fn test_stats_upsert() {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run_migrations(&conn).unwrap();

        upsert(&conn, "openai", 120.5, 10, 2).unwrap();
        let all = get_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].latency_ema_ms, 120.5);
        assert_eq!(all[0].total_requests, 10);

        // Upsert replaces
        upsert(&conn, "openai", 90.0, 15, 3).unwrap();
        let all = get_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].latency_ema_ms, 90.0);
        assert_eq!(all[0].total_requests, 15);
    }
}
