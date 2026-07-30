use crate::models::ProviderConnection;
use anyhow::Result;
use rusqlite::{Connection, params};

pub fn get_all(conn: &Connection) -> Result<Vec<ProviderConnection>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider, auth_type, name, email, api_key, is_active, priority, data, created_at, updated_at
         FROM providerConnections ORDER BY priority ASC"
    )?;
    let rows = stmt.query_map([], |row| {
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
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<ProviderConnection>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider, auth_type, name, email, api_key, is_active, priority, data, created_at, updated_at
         FROM providerConnections WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
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
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;
    match rows.next() {
        Some(Ok(c)) => Ok(Some(c)),
        _ => Ok(None),
    }
}

pub fn insert(conn: &Connection, c: &ProviderConnection) -> Result<()> {
    conn.execute(
        "INSERT INTO providerConnections (id, provider, auth_type, name, email, api_key, is_active, priority, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            c.id, c.provider, c.auth_type, c.name, c.email, c.api_key,
            c.is_active as i32, c.priority, serde_json::to_string(&c.data)?
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

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM providerConnections WHERE id=?1", params![id])?;
    Ok(())
}
