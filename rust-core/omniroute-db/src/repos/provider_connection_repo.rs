use crate::models::ProviderConnection;
use anyhow::Result;
use rusqlite::{Connection, params};

const SELECT_COLS: &str = "id, provider, auth_type, name, email, api_key, is_active, priority, data, rate_limited_until, backoff_level, created_at, updated_at";

fn row_to_conn(row: &rusqlite::Row) -> rusqlite::Result<ProviderConnection> {
    let data_str: String = row.get(8)?;
    Ok(ProviderConnection {
        id: row.get(0)?,
        provider: row.get(1)?,
        auth_type: row.get(2)?,
        name: row.get(3)?,
        email: row.get(4)?,
        api_key: row.get(5)?,
        is_active: row.get::<_, i32>(6)? != 0,
        priority: row.get(7)?,
        data: serde_json::from_str(&data_str).unwrap_or_default(),
        rate_limited_until: row.get(9)?,
        backoff_level: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

pub fn get_all(conn: &Connection) -> Result<Vec<ProviderConnection>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM providerConnections ORDER BY priority ASC, updated_at DESC"
    ))?;
    let rows = stmt.query_map([], row_to_conn)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Active connections only, ordered by priority — the routing pool source.
pub fn get_active(conn: &Connection) -> Result<Vec<ProviderConnection>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM providerConnections
         WHERE is_active = 1 ORDER BY priority ASC, updated_at DESC"
    ))?;
    let rows = stmt.query_map([], row_to_conn)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<ProviderConnection>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM providerConnections WHERE id = ?1"
    ))?;
    let mut rows = stmt.query_map(params![id], row_to_conn)?;
    match rows.next() {
        Some(Ok(c)) => Ok(Some(c)),
        _ => Ok(None),
    }
}

pub fn insert(conn: &Connection, c: &ProviderConnection) -> Result<()> {
    conn.execute(
        "INSERT INTO providerConnections (id, provider, auth_type, name, email, api_key, is_active, priority, data, rate_limited_until, backoff_level)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            c.id, c.provider, c.auth_type, c.name, c.email, c.api_key,
            c.is_active as i32, c.priority, serde_json::to_string(&c.data)?,
            c.rate_limited_until, c.backoff_level.unwrap_or(0)
        ],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, c: &ProviderConnection) -> Result<()> {
    conn.execute(
        "UPDATE providerConnections SET provider=?2, auth_type=?3, name=?4, email=?5, api_key=?6,
         is_active=?7, priority=?8, data=?9, updated_at=datetime('now')
         WHERE id=?1",
        params![
            c.id,
            c.provider,
            c.auth_type,
            c.name,
            c.email,
            c.api_key,
            c.is_active as i32,
            c.priority,
            serde_json::to_string(&c.data)?
        ],
    )?;
    Ok(())
}

/// Persist health state after a request outcome (matches OmniRoute flow).
/// - rate_limited_until: RFC3339 timestamp or NULL when cooled down
/// - backoff_level: current exponential backoff level
pub fn update_health(
    conn: &Connection,
    id: &str,
    rate_limited_until: Option<&str>,
    backoff_level: i32,
) -> Result<()> {
    conn.execute(
        "UPDATE providerConnections SET rate_limited_until=?2, backoff_level=?3,
         updated_at=datetime('now') WHERE id=?1",
        params![id, rate_limited_until, backoff_level],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM providerConnections WHERE id=?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run_migrations(&conn).unwrap();
        conn
    }

    fn sample(id: &str, provider: &str, priority: i32, active: bool) -> ProviderConnection {
        ProviderConnection {
            id: id.into(),
            provider: provider.into(),
            auth_type: Some("apikey".into()),
            name: Some(id.into()),
            email: None,
            api_key: Some(format!("sk-{id}")),
            is_active: active,
            priority: Some(priority),
            data: serde_json::json!({}),
            rate_limited_until: None,
            backoff_level: Some(0),
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
        }
    }

    #[test]
    fn test_get_active_filters_and_orders() {
        let conn = test_conn();
        insert(&conn, &sample("a", "openai", 2, true)).unwrap();
        insert(&conn, &sample("b", "openai", 1, true)).unwrap();
        insert(&conn, &sample("c", "openai", 0, false)).unwrap(); // inactive → skipped

        let active = get_active(&conn).unwrap();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].id, "b", "priority 1 first");
        assert_eq!(active[1].id, "a", "priority 2 second");
    }

    #[test]
    fn test_update_health_persists() {
        let conn = test_conn();
        insert(&conn, &sample("a", "openai", 1, true)).unwrap();

        update_health(&conn, "a", Some("2026-01-02T00:00:00Z"), 3).unwrap();
        let c = get_by_id(&conn, "a").unwrap().unwrap();
        assert_eq!(
            c.rate_limited_until.as_deref(),
            Some("2026-01-02T00:00:00Z")
        );
        assert_eq!(c.backoff_level, Some(3));

        // Cool down → clear
        update_health(&conn, "a", None, 0).unwrap();
        let c = get_by_id(&conn, "a").unwrap().unwrap();
        assert!(c.rate_limited_until.is_none());
        assert_eq!(c.backoff_level, Some(0));
    }
}
