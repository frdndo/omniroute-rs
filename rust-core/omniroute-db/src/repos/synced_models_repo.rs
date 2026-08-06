//! Live-synced model list per provider (parity OmniRoute modelsUrl sync).
//! Synced models complement the static registry; they are preferred when
//! both sources have the same model id.
use rusqlite::Connection;

pub fn upsert_many(
    conn: &Connection,
    provider: &str,
    models: &[(String, Option<String>)],
) -> Result<usize, rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO synced_models (provider, model_id, name, raw, updated_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(provider, model_id) DO UPDATE SET
               name = excluded.name,
               raw = excluded.raw,
               updated_at = datetime('now')",
        )?;
        for (id, name) in models {
            stmt.execute(rusqlite::params![provider, id, name, None::<String>])?;
        }
    }
    tx.commit()?;
    Ok(models.len())
}

/// (model_id, name) for a provider, newest-first is irrelevant — ordered by id.
pub fn list_for_provider(
    conn: &Connection,
    provider: &str,
) -> Result<Vec<(String, Option<String>)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT model_id, name FROM synced_models WHERE provider = ?1 ORDER BY model_id",
    )?;
    let rows = stmt.query_map([provider], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

pub fn clear_provider(conn: &Connection, provider: &str) -> Result<usize, rusqlite::Error> {
    conn.execute("DELETE FROM synced_models WHERE provider = ?1", [provider])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::run_migrations;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn upsert_and_list() {
        let conn = test_conn();
        upsert_many(
            &conn,
            "opencode",
            &[("m1".into(), Some("Model 1".into())), ("m2".into(), None)],
        )
        .unwrap();
        upsert_many(
            &conn,
            "opencode",
            &[("m1".into(), Some("Model 1 updated".into()))],
        )
        .unwrap();
        let rows = list_for_provider(&conn, "opencode").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&("m1".into(), Some("Model 1 updated".into()))));
        clear_provider(&conn, "opencode").unwrap();
        assert!(list_for_provider(&conn, "opencode").unwrap().is_empty());
    }
}
