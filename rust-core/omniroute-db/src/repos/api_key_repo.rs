use crate::models::ApiKey;
use anyhow::Result;
use rusqlite::{Connection, params};

pub fn get_all(conn: &Connection) -> Result<Vec<ApiKey>> {
    let mut stmt = conn.prepare(
        "SELECT id, key, name, machine_id, is_active, created_at FROM apiKeys ORDER BY created_at DESC"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ApiKey {
            id: row.get(0)?,
            key: row.get(1)?,
            name: row.get(2)?,
            machine_id: row.get(3)?,
            is_active: row.get::<_, i32>(4)? != 0,
            created_at: row.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn insert(conn: &Connection, key: &ApiKey) -> Result<()> {
    conn.execute(
        "INSERT INTO apiKeys (id, key, name, machine_id, is_active) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            key.id,
            key.key,
            key.name,
            key.machine_id,
            key.is_active as i32
        ],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM apiKeys WHERE id=?1", params![id])?;
    Ok(())
}

pub fn update(
    conn: &Connection,
    id: &str,
    name: Option<&str>,
    is_active: Option<bool>,
) -> Result<()> {
    conn.execute(
        "UPDATE apiKeys SET name=COALESCE(?2, name), is_active=COALESCE(?3, is_active) WHERE id=?1",
        params![id, name, is_active],
    )?;
    Ok(())
}
