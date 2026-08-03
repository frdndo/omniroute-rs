use std::sync::Mutex;

/// Persistent request telemetry (M2): every proxied request lands in the
/// `request_logs` table, powering the Analytics dashboard and the
/// auto-combo scorer's cost/usage factors later (TD-10).
pub struct Telemetry {
    /// Shared DB handle (set at startup via `attach`).
    db: Mutex<Option<std::sync::Arc<omniroute_db::Database>>>,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            db: Mutex::new(None),
        }
    }

    pub fn attach(&self, db: std::sync::Arc<omniroute_db::Database>) {
        if let Ok(mut slot) = self.db.lock() {
            *slot = Some(db);
        }
    }
    /// Insert one request row (non-chat: models/health/admin).
    pub fn record(&self, method: &str, uri: &str, status: u16, duration_ms: u64) {
        let slot = match self.db.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let Some(db) = slot.as_ref() else { return };
        let Ok(conn) = db.conn.lock() else { return };
        let _ = omniroute_db::repos::request_log_repo::insert(
            &conn,
            method,
            uri,
            status as i64,
            duration_ms as i64,
            None,
            None,
            0,
            0,
        );
    }

    /// Record a chat call with provider/model + token usage (called by the
    /// combo engine). `status`: 200 success, else mapped error code.
    pub fn record_chat(
        &self,
        provider: &str,
        model: &str,
        status: u16,
        duration_ms: u64,
        prompt_tokens: i64,
        completion_tokens: i64,
    ) {
        let slot = match self.db.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let Some(db) = slot.as_ref() else { return };
        let Ok(conn) = db.conn.lock() else { return };
        let _ = omniroute_db::repos::request_log_repo::insert(
            &conn,
            "POST",
            "/v1/chat/completions",
            status as i64,
            duration_ms as i64,
            Some(provider),
            Some(model),
            prompt_tokens,
            completion_tokens,
        );
    }

    /// Aggregates for the Analytics dashboard.
    /// `hours` buckets of request counts (last N hours), status breakdown,
    /// per-provider request counts + avg latency, overall totals.
    pub fn stats(&self) -> serde_json::Value {
        let slot = match self.db.lock() {
            Ok(s) => s,
            Err(_) => return serde_json::json!({"error": "telemetry not attached"}),
        };
        let Some(db) = slot.as_ref() else {
            return serde_json::json!({"error": "telemetry not attached"});
        };
        let Ok(conn) = db.conn.lock() else {
            return serde_json::json!({"error": "db locked"});
        };
        use omniroute_db::repos::request_log_repo as rlog;
        serde_json::json!({
            "total_requests": rlog::count_total(&conn),
            "total_errors": rlog::count_status_class(&conn, 4) + rlog::count_status_class(&conn, 5),
            "avg_duration_ms": rlog::avg_duration(&conn),
            "by_status": {
                "2xx": rlog::count_status_class(&conn, 2),
                "3xx": rlog::count_status_class(&conn, 3),
                "4xx": rlog::count_status_class(&conn, 4),
                "5xx": rlog::count_status_class(&conn, 5),
            },
            "by_provider": rlog::by_provider(&conn),
            "hourly": rlog::hourly_counts(&conn, 24),
        })
    }
}

/// Global telemetry instance (attached at server start).
pub static TELEMETRY: std::sync::LazyLock<Telemetry> = std::sync::LazyLock::new(Telemetry::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_stats() {
        let t = Telemetry::new();
        let db = std::sync::Arc::new(omniroute_db::Database::open_in_memory().expect("temp db"));
        t.attach(db.clone());
        t.record("POST", "/v1/chat/completions", 200, 42);
        t.record("POST", "/v1/chat/completions", 429, 5);
        t.record("GET", "/health", 200, 1);
        t.record("POST", "/v1/chat/completions", 500, 100);

        let s = t.stats();
        assert_eq!(s["total_requests"], 4);
        assert_eq!(s["by_status"]["2xx"], 2);
        assert_eq!(s["by_status"]["4xx"], 1);
        assert_eq!(s["by_status"]["5xx"], 1);
        assert!(s["avg_duration_ms"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn test_stats_empty() {
        // Local instance: the global TELEMETRY static is shared across
        // parallel tests and would be polluted by test_record_and_stats.
        let t = Telemetry::new();
        let db = std::sync::Arc::new(omniroute_db::Database::open_in_memory().expect("temp db"));
        t.attach(db);
        let s = t.stats();
        assert_eq!(s["total_requests"], 0);
        assert_eq!(s["by_status"]["2xx"], 0);
    }
}
