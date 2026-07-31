use crate::models::Combo;
use anyhow::Result;
use rusqlite::{Connection, params};

pub fn get_all(conn: &Connection) -> Result<Vec<Combo>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, models, created_at, updated_at FROM combos ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let models_str: String = row.get(3)?;
        Ok(Combo {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            models: serde_json::from_str(&models_str).unwrap_or_default(),
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn insert(conn: &Connection, c: &Combo) -> Result<()> {
    conn.execute(
        "INSERT INTO combos (id, name, kind, models) VALUES (?1, ?2, ?3, ?4)",
        params![c.id, c.name, c.kind, serde_json::to_string(&c.models)?],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM combos WHERE id=?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;

    #[test]
    fn test_combo_crud() {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run_migrations(&conn).unwrap();
        insert(
            &conn,
            &Combo {
                id: "c1".into(),
                name: "smart".into(),
                kind: "model".into(),
                models: vec!["gpt-4o".into(), "claude-sonnet-4".into()],
                created_at: "2026-01-01".into(),
                updated_at: "2026-01-01".into(),
            },
        )
        .unwrap();
        let all = get_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].models, vec!["gpt-4o", "claude-sonnet-4"]);
        delete(&conn, "c1").unwrap();
        assert!(get_all(&conn).unwrap().is_empty());
    }
}
