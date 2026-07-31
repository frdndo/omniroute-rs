use anyhow::Result;
use rusqlite::{Connection, params};

/// Session → account affinity: keeps multi-turn conversations on the same
/// upstream account (matches OmniRoute `session_account_affinity` table).
pub fn get(conn: &Connection, session_id: &str) -> Result<Option<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT provider, account_key FROM session_account_affinity WHERE session_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        _ => Ok(None),
    }
}

pub fn upsert(
    conn: &Connection,
    session_id: &str,
    provider: &str,
    account_key: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO session_account_affinity (session_id, provider, account_key, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(session_id) DO UPDATE SET
           provider=?2, account_key=?3, updated_at=datetime('now')",
        params![session_id, provider, account_key],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM session_account_affinity WHERE session_id=?1",
        params![session_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;

    #[test]
    fn test_affinity_crud() {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run_migrations(&conn).unwrap();

        assert!(get(&conn, "s1").unwrap().is_none());
        upsert(&conn, "s1", "openai", "sk-a").unwrap();
        assert_eq!(
            get(&conn, "s1").unwrap(),
            Some(("openai".into(), "sk-a".into()))
        );

        // Upsert moves affinity to a new account
        upsert(&conn, "s1", "openai", "sk-b").unwrap();
        assert_eq!(
            get(&conn, "s1").unwrap(),
            Some(("openai".into(), "sk-b".into()))
        );

        delete(&conn, "s1").unwrap();
        assert!(get(&conn, "s1").unwrap().is_none());
    }
}
