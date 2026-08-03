use crate::chat::ChatRequest;
use sha2::{Digest, Sha256};

/// M5 response cache: keyed by (model + normalized request), stored in SQLite.
pub struct Cache;

impl Cache {
    /// Deterministic key from request content (ignores cache flags).
    pub fn key(req: &ChatRequest) -> String {
        let mut hasher = Sha256::new();
        hasher.update(req.model.as_bytes());
        for m in &req.messages {
            hasher.update(m.role.as_bytes());
            hasher.update([0u8]);
            if let Some(c) = &m.content {
                hasher.update(c.to_string().as_bytes());
            }
            hasher.update([0u8]);
        }
        if let Some(t) = req.temperature {
            hasher.update(t.to_le_bytes());
        }
        if let Some(t) = req.max_tokens {
            hasher.update(t.to_le_bytes());
        }
        if let Some(v) = &req.tools {
            hasher.update(v.len().to_le_bytes());
        }
        let digest = hasher.finalize();
        format!("{:x}", digest)[..40].to_string()
    }

    /// Try a cache lookup; returns Some(response_json) on hit.
    pub fn get(db: &omniroute_db::Database, key: &str) -> Option<String> {
        let conn = db.conn.lock().ok()?;
        let entry = omniroute_db::repos::cache_repo::get(&conn, key).ok()??;
        if entry.expires_at < chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string() {
            let _ = omniroute_db::repos::cache_repo::delete(&conn, key);
            return None;
        }
        let _ = omniroute_db::repos::cache_repo::record_hit(&conn, key);
        Some(entry.response)
    }

    /// Store a response (JSON string) with TTL seconds.
    pub fn set(db: &omniroute_db::Database, key: &str, model: &str, response: &str, ttl_secs: u64) {
        let Ok(conn) = db.conn.lock() else { return };
        let expires = (chrono::Utc::now() + chrono::Duration::seconds(ttl_secs as i64))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let _ = omniroute_db::repos::cache_repo::set(&conn, key, model, response, &expires);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ChatRequest, Message};

    fn req(content: &str) -> ChatRequest {
        ChatRequest {
            model: "gpt-4o".into(),
            messages: vec![Message {
                role: "user".into(),
                content: Some(crate::chat::Content::Text(content.into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            tools: None,
            tool_choice: None,
            extra: None,
            cache: false,
            cache_ttl: None,
            compress: false,
            max_context_tokens: None,
        }
    }

    #[test]
    fn test_key_deterministic_and_sensitive_to_content() {
        let a = Cache::key(&req("hello"));
        let b = Cache::key(&req("hello"));
        let c = Cache::key(&req("world"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 40);
    }

    #[test]
    fn test_cache_roundtrip() {
        let db = omniroute_db::Database::open_in_memory().unwrap();
        let k = Cache::key(&req("hi"));
        assert!(Cache::get(&db, &k).is_none());
        Cache::set(
            &db,
            &k,
            "gpt-4o",
            r#"{"choices":[{"message":{"content":"cached"}}]}"#,
            300,
        );
        let v = Cache::get(&db, &k).unwrap();
        assert!(v.contains("cached"));
        // second get = hit
        assert!(Cache::get(&db, &k).is_some());
    }
}
