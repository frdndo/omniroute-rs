use anyhow::Result;
use rusqlite::{Connection, params};

/// Response cache entry (M5).
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub key: String,
    pub model: String,
    pub response: String,
    pub hits: i64,
    pub created_at: String,
    pub expires_at: String,
}

pub fn get(conn: &Connection, key: &str) -> Result<Option<CacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT key, model, response, hits, created_at, expires_at FROM cache_entries WHERE key=?1",
    )?;
    let mut rows = stmt.query_map(params![key], |row| {
        Ok(CacheEntry {
            key: row.get(0)?,
            model: row.get(1)?,
            response: row.get(2)?,
            hits: row.get(3)?,
            created_at: row.get(4)?,
            expires_at: row.get(5)?,
        })
    })?;
    match rows.next() {
        Some(Ok(e)) => Ok(Some(e)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn set(
    conn: &Connection,
    key: &str,
    model: &str,
    response: &str,
    expires_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO cache_entries (key, model, response, expires_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET response=?3, expires_at=?4, hits=0, created_at=datetime('now')",
        params![key, model, response, expires_at],
    )?;
    Ok(())
}

pub fn record_hit(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "UPDATE cache_entries SET hits = hits + 1 WHERE key=?1",
        params![key],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM cache_entries WHERE key=?1", params![key])?;
    Ok(())
}

pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM cache_entries", [])?;
    Ok(())
}

/// Remove expired entries; returns number purged.
pub fn purge_expired(conn: &Connection) -> Result<i64> {
    let n = conn.execute(
        "DELETE FROM cache_entries WHERE expires_at < datetime('now')",
        [],
    )?;
    Ok(n as i64)
}

pub fn stats(conn: &Connection) -> Result<serde_json::Value> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM cache_entries", [], |r| r.get(0))?;
    let hits: i64 = conn.query_row("SELECT COALESCE(SUM(hits),0) FROM cache_entries", [], |r| {
        r.get(0)
    })?;
    Ok(serde_json::json!({ "entries": total, "total_hits": hits }))
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
    fn test_cache_crud_and_hits() {
        let c = conn();
        assert!(get(&c, "k1").unwrap().is_none());

        set(
            &c,
            "k1",
            "gpt-4o",
            r#"{"choices":[{"message":{"content":"hi"}}]}"#,
            "2099-01-01 00:00:00",
        )
        .unwrap();
        let e = get(&c, "k1").unwrap().unwrap();
        assert_eq!(e.model, "gpt-4o");
        assert_eq!(e.hits, 0);

        record_hit(&c, "k1").unwrap();
        assert_eq!(get(&c, "k1").unwrap().unwrap().hits, 1);

        // upsert resets hits
        set(
            &c,
            "k1",
            "gpt-4o",
            r#"{"choices":[{"message":{"content":"bye"}}]}"#,
            "2099-01-01 00:00:00",
        )
        .unwrap();
        assert_eq!(get(&c, "k1").unwrap().unwrap().hits, 0);

        delete(&c, "k1").unwrap();
        assert!(get(&c, "k1").unwrap().is_none());
    }

    #[test]
    fn test_expiry_and_clear() {
        let c = conn();
        set(&c, "old", "m", "{}", "2000-01-01 00:00:00").unwrap();
        set(&c, "new", "m", "{}", "2099-01-01 00:00:00").unwrap();
        let purged = purge_expired(&c).unwrap();
        assert_eq!(purged, 1);
        assert!(get(&c, "old").unwrap().is_none());
        assert!(get(&c, "new").unwrap().is_some());

        clear(&c).unwrap();
        let s = stats(&c).unwrap();
        assert_eq!(s["entries"], 0);
    }
}
