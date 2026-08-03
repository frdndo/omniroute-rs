use anyhow::Result;
use rusqlite::{Connection, params};

/// Webhook subscription: fires on matching events.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebhookRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub events: String,
    pub is_active: bool,
}

pub fn get_all(conn: &Connection) -> Result<Vec<WebhookRow>> {
    let mut stmt =
        conn.prepare("SELECT id, name, url, events, is_active FROM webhooks ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        Ok(WebhookRow {
            id: row.get(0)?,
            name: row.get(1)?,
            url: row.get(2)?,
            events: row.get(3)?,
            is_active: row.get::<_, i64>(4)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Webhooks subscribed to a specific event and active.
pub fn for_event(conn: &Connection, event: &str) -> Vec<WebhookRow> {
    let all = match get_all(conn) {
        Ok(a) => a,
        Err(_) => return vec![],
    };
    all.into_iter()
        .filter(|w| w.is_active && w.events.split(',').any(|e| e.trim() == event))
        .collect()
}

pub fn insert(conn: &Connection, w: &WebhookRow) -> Result<()> {
    conn.execute(
        "INSERT INTO webhooks (id, name, url, events, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![w.id, w.name, w.url, w.events, w.is_active as i64],
    )?;
    Ok(())
}

pub fn update(conn: &Connection, id: &str, w: &WebhookRow) -> Result<()> {
    conn.execute(
        "UPDATE webhooks SET name=?2, url=?3, events=?4, is_active=?5, updated_at=datetime('now') WHERE id=?1",
        params![id, w.name, w.url, w.events, w.is_active as i64],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM webhooks WHERE id=?1", params![id])?;
    Ok(())
}

/// Audit log row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditRow {
    pub id: i64,
    pub ts: String,
    pub action: String,
    pub resource: String,
    pub resource_id: Option<String>,
    pub detail: Option<String>,
}

pub fn audit_insert(
    conn: &Connection,
    action: &str,
    resource: &str,
    resource_id: Option<&str>,
    detail: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO audit_logs (action, resource, resource_id, detail) VALUES (?1, ?2, ?3, ?4)",
        params![action, resource, resource_id, detail],
    )?;
    Ok(())
}

pub fn audit_recent(conn: &Connection, limit: i64) -> Vec<AuditRow> {
    let mut stmt = match conn.prepare(
        "SELECT id, ts, action, resource, resource_id, detail FROM audit_logs ORDER BY id DESC LIMIT ?1",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = match stmt.query_map(params![limit], |row| {
        Ok(AuditRow {
            id: row.get(0)?,
            ts: row.get(1)?,
            action: row.get(2)?,
            resource: row.get(3)?,
            resource_id: row.get(4)?,
            detail: row.get(5)?,
        })
    }) {
        Ok(rows) => rows,
        Err(_) => return vec![],
    };
    rows.filter_map(|r| r.ok()).collect()
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

    fn wh(id: &str, events: &str, active: bool) -> WebhookRow {
        WebhookRow {
            id: id.into(),
            name: id.into(),
            url: "http://localhost/h".into(),
            events: events.into(),
            is_active: active,
        }
    }

    #[test]
    fn test_webhook_filter() {
        let c = conn();
        insert(&c, &wh("a", "chat.success", true)).unwrap();
        insert(&c, &wh("b", "chat.error", true)).unwrap();
        insert(&c, &wh("c", "chat.success,rate_limited", true)).unwrap();
        insert(&c, &wh("d", "chat.success", false)).unwrap();

        let succ = for_event(&c, "chat.success");
        assert_eq!(succ.len(), 2, "a + c (d inactive)");
        assert!(succ.iter().any(|w| w.id == "a"));
        assert!(succ.iter().any(|w| w.id == "c"));

        let rate = for_event(&c, "rate_limited");
        assert_eq!(rate.len(), 1);
        assert_eq!(rate[0].id, "c");

        update(&c, "a", &wh("a", "chat.error", true)).unwrap();
        assert!(for_event(&c, "chat.success").iter().all(|w| w.id != "a"));
        delete(&c, "a").unwrap();
        assert_eq!(get_all(&c).unwrap().len(), 3);
    }

    #[test]
    fn test_audit_roundtrip() {
        let c = conn();
        audit_insert(&c, "create", "provider", Some("p1"), Some("openai acc")).unwrap();
        audit_insert(&c, "delete", "api-key", Some("k1"), None).unwrap();
        let recent = audit_recent(&c, 10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].action, "delete");
        assert_eq!(recent[1].resource_id.as_deref(), Some("p1"));
    }
}
