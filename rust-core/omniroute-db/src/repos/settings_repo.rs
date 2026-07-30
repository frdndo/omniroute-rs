use crate::models::Settings;
use anyhow::Result;
use rusqlite::{Connection, params};

pub fn get(conn: &Connection) -> Result<Settings> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        let k: String = row.get(0)?;
        let v: String = row.get(1)?;
        Ok((k, v))
    })?;
    let mut settings = Settings {
        password: None,
        jwt_secret: None,
    };
    for row in rows {
        let (k, v) = row?;
        match k.as_str() {
            "password" => settings.password = Some(v),
            "jwt_secret" => settings.jwt_secret = Some(v),
            _ => {}
        }
    }
    Ok(settings)
}

pub fn upsert(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
