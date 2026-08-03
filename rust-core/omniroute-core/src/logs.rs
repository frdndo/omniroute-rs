use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;

/// One recorded request (kept in a bounded in-memory ring buffer).
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts: String,
    pub method: String,
    pub uri: String,
    pub status: u16,
    pub duration_ms: u64,
}

/// Bounded request-log ring buffer (survives per-request, not restarts —
/// persistent telemetry arrives with M2).
pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
    cap: usize,
}

impl LogBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(cap)),
            cap,
        }
    }

    pub fn push(&self, entry: LogEntry) {
        if let Ok(mut q) = self.entries.lock() {
            if q.len() >= self.cap {
                q.pop_front();
            }
            q.push_back(entry);
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        match self.entries.lock() {
            Ok(q) => q.iter().rev().take(limit).cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|q| q.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Global shared buffer (set once at server start).
pub static LOG_BUFFER: std::sync::LazyLock<LogBuffer> =
    std::sync::LazyLock::new(|| LogBuffer::new(200));

/// In-flight request entries keyed by tracing span id (filled on_response).
pub static PENDING_LOGS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<u64, LogEntry>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Stage a request entry at span creation (has method/uri).
pub fn stage_request(span_id: u64, method: &str, uri: &str) {
    if let Ok(mut m) = PENDING_LOGS.lock() {
        m.insert(
            span_id,
            LogEntry {
                ts: chrono::Utc::now().to_rfc3339(),
                method: method.to_string(),
                uri: uri.to_string(),
                status: 0,
                duration_ms: 0,
            },
        );
    }
}

/// Peek a staged entry (method/uri) without consuming it.
pub fn peek_request(span_id: u64) -> Option<LogEntry> {
    PENDING_LOGS
        .lock()
        .ok()
        .and_then(|m| m.get(&span_id).cloned())
}

/// Finalize a staged entry with status/duration and push to the buffer.
pub fn finalize_request(span_id: u64, status: u16, duration_ms: u64) {
    let entry = PENDING_LOGS
        .lock()
        .ok()
        .and_then(|mut m| m.remove(&span_id));
    if let Some(mut e) = entry {
        e.status = status;
        e.duration_ms = duration_ms;
        LOG_BUFFER.push(e);
    }
}

/// GET /admin/logs — recent request log (newest first).
pub async fn handle_logs(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let logs = LOG_BUFFER.recent(200);
    Ok(Json(serde_json::json!({
        "object": "list",
        "data": logs,
        "count": logs.len(),
        "uptime_seconds": chrono::Utc::now()
            .signed_duration_since(state.started_at)
            .num_seconds(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(status: u16) -> LogEntry {
        LogEntry {
            ts: "2026-07-31T00:00:00Z".into(),
            method: "POST".into(),
            uri: "/v1/chat/completions".into(),
            status,
            duration_ms: 5,
        }
    }

    #[test]
    fn test_buffer_bounded() {
        let buf = LogBuffer::new(3);
        for s in [200, 200, 200, 401, 503] {
            buf.push(entry(s));
        }
        assert_eq!(buf.len(), 3, "ring buffer caps at 3");
        // newest first
        let r = buf.recent(10);
        assert_eq!(r[0].status, 503);
        assert_eq!(r[2].status, 200);
    }

    #[test]
    fn test_recent_limit() {
        let buf = LogBuffer::new(10);
        for s in [200, 201, 400] {
            buf.push(entry(s));
        }
        let r = buf.recent(2);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].status, 400);
    }
}
