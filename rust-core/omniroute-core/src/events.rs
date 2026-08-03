use serde_json::json;

/// M4 webhook dispatcher + audit log helper.
///
/// Webhooks fire-and-forget: spawn a tokio task per matching subscription so
/// the request path is never blocked by a slow endpoint.
pub struct Events;

impl Events {
    /// Fire all webhooks subscribed to `event` (non-blocking).
    pub fn dispatch(db: &omniroute_db::Database, event: &str, payload: serde_json::Value) {
        let Ok(conn) = db.conn.lock() else { return };
        let hooks = omniroute_db::repos::webhook_repo::for_event(&conn, event);
        drop(conn);
        for h in hooks {
            let url = h.url.clone();
            let body = json!({
                "event": event,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "data": payload,
            });
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                let _ = client
                    .post(&url)
                    .json(&body)
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await;
            });
        }
    }

    /// Append an audit log entry.
    pub fn audit(
        db: &omniroute_db::Database,
        action: &str,
        resource: &str,
        resource_id: Option<&str>,
        detail: Option<&str>,
    ) {
        if let Ok(conn) = db.conn.lock() {
            let _ = omniroute_db::repos::webhook_repo::audit_insert(
                &conn,
                action,
                resource,
                resource_id,
                detail,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_helpers() {
        let db = omniroute_db::Database::open_in_memory().unwrap();
        Events::audit(&db, "create", "provider", Some("p1"), Some("detail"));
        {
            let conn = db.conn.lock().unwrap();
            let rows = omniroute_db::repos::webhook_repo::audit_recent(&conn, 10);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].action, "create");
        }
    }
}
