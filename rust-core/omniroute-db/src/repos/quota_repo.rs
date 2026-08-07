//! Quota per API key (parity OmniRoute quota/limits — versi sederhana:
//! unit requests/tokens/usd, window hourly/daily/weekly/monthly, policy
//! hard/soft. Tanpa pool/fair-share/saturation yang ada di asli).
use crate::models::Quota;
use anyhow::Result;
use rusqlite::{Connection, params};

pub fn get_all(conn: &Connection) -> Result<Vec<Quota>> {
    let mut stmt = conn.prepare(
        "SELECT id, api_key_id, unit, quota_limit, window, policy, created_at FROM quotas ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Quota {
            id: r.get(0)?,
            api_key_id: r.get(1)?,
            unit: r.get(2)?,
            limit: r.get(3)?,
            window: r.get(4)?,
            policy: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_for_key(conn: &Connection, key_id: &str) -> Result<Vec<Quota>> {
    let mut stmt = conn.prepare(
        "SELECT id, api_key_id, unit, quota_limit, window, policy, created_at FROM quotas WHERE api_key_id = ?1",
    )?;
    let rows = stmt.query_map([key_id], |r| {
        Ok(Quota {
            id: r.get(0)?,
            api_key_id: r.get(1)?,
            unit: r.get(2)?,
            limit: r.get(3)?,
            window: r.get(4)?,
            policy: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn create(
    conn: &Connection,
    id: &str,
    api_key_id: &str,
    unit: &str,
    limit: f64,
    window: &str,
    policy: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO quotas (id, api_key_id, unit, quota_limit, window, policy)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, api_key_id, unit, limit, window, policy],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM quotas WHERE id = ?1", [id])?)
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
    fn quota_crud() {
        let conn = test_conn();
        create(&conn, "q1", "k1", "tokens", 100_000.0, "daily", "hard").unwrap();
        let all = get_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].unit, "tokens");
        assert_eq!(get_for_key(&conn, "k1").unwrap().len(), 1);
        assert!(get_for_key(&conn, "nope").unwrap().is_empty());
        delete(&conn, "q1").unwrap();
        assert!(get_all(&conn).unwrap().is_empty());
    }
}
