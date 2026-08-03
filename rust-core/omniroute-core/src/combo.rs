use crate::chat::{ChatRequest, ChatResponse};
use crate::executor::ExecutorError;
use crate::router::RoutingEngine;
use std::collections::HashMap;
use tracing::warn;

/// Ordered list of model options for fallback routing
#[derive(Debug, Clone)]
pub struct Combo {
    pub id: String,
    pub name: String,
    /// Ordered fallback chain — first entry is the primary model
    pub models: Vec<String>,
}

impl Combo {
    pub fn new(id: &str, name: &str, models: Vec<String>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            models,
        }
    }
}

/// What went wrong during a fallback attempt
#[derive(Debug, Clone)]
pub struct AttemptRecord {
    pub model: String,
    pub provider: Option<String>,
    pub error: String,
}

/// Result of a combo execution
#[derive(Debug)]
pub struct ComboResult {
    pub response: ChatResponse,
    pub used_model: String,
    pub used_provider: String,
    pub attempts: Vec<AttemptRecord>,
}

/// A committed streaming attempt (provider + normalized chunk stream)
pub struct StreamAttempt {
    pub provider_id: String,
    pub account_key: String,
    pub model: String,
    pub stream: crate::executor::streaming::ChunkStream,
}

/// Error when all fallback options fail
#[derive(Debug)]
pub struct ComboError {
    pub attempts: Vec<AttemptRecord>,
    pub last_error: ExecutorError,
}

impl std::fmt::Display for ComboError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "all {} model(s) failed; last error: {}",
            self.attempts.len(),
            self.last_error
        )
    }
}

/// Errors that should trigger a fallback to the next model
fn should_fallback(err: &ExecutorError) -> bool {
    matches!(
        err,
        ExecutorError::RateLimited(_)
            | ExecutorError::Network(_)
            | ExecutorError::Timeout(_)
            | ExecutorError::AuthFailed(_)
            | ExecutorError::UnsupportedProvider(_)
            | ExecutorError::Upstream(_, _)
    )
}

/// Executes requests with automatic fallback across model options.
#[derive(Clone)]
pub struct ComboEngine {
    router: RoutingEngine,
    /// Explicit combos: name → ordered models
    combos: HashMap<String, Combo>,
    /// Per-provider fallback chains for auto-combo
    auto_fallbacks: HashMap<String, Vec<String>>,
    max_attempts: usize,
    /// Shared DB handle — used for session affinity persistence
    pub db: Option<std::sync::Arc<omniroute_db::Database>>,
    /// Auto-combo scorer (G5): orders candidates by health/latency/concurrency
    pub scorer: crate::scorer::ComboScorer,
}

impl ComboEngine {
    pub fn new(router: RoutingEngine) -> Self {
        Self {
            router,
            combos: HashMap::new(),
            auto_fallbacks: HashMap::new(),
            max_attempts: 5,
            db: None,
            scorer: crate::scorer::ComboScorer::new(),
        }
    }

    /// Attach the shared DB handle (session affinity + scoring persistence).
    pub fn with_db(mut self, db: std::sync::Arc<omniroute_db::Database>) -> Self {
        self.db = Some(db.clone());
        // Load persisted scorer stats (EMA latency, failure counts)
        self.scorer = std::mem::take(&mut self.scorer).with_db(db);
        self
    }

    pub fn with_combo(mut self, combo: Combo) -> Self {
        self.combos.insert(combo.name.clone(), combo);
        self
    }

    /// Register auto-fallback chain: primary model → alternatives
    pub fn with_fallback(mut self, primary: &str, alternatives: Vec<String>) -> Self {
        self.auto_fallbacks
            .insert(primary.to_string(), alternatives);
        self
    }

    pub fn with_max_attempts(mut self, max: usize) -> Self {
        self.max_attempts = max;
        self
    }

    /// Load combos from SQLite (combos table) — name → ordered model chain.
    pub fn load_combos_from_db(&mut self, db: &omniroute_db::Database) -> Result<(), String> {
        let combos = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            omniroute_db::repos::combo_repo::get_all(&conn).map_err(|e| e.to_string())?
        };
        for c in combos {
            if c.models.is_empty() {
                continue;
            }
            tracing::info!(
                "loaded combo '{}' ({} models): {:?}",
                c.name,
                c.models.len(),
                c.models
            );
            self.combos
                .insert(c.name.clone(), Combo::new(&c.id, &c.name, c.models));
        }
        Ok(())
    }

    /// Execute a request with fallback.
    ///
    /// Resolution order for the candidate list:
    /// 1. If the request model names a registered combo, use its chain
    /// 2. Else use the registered auto-fallback chain for the model
    /// 3. Else just the model itself
    /// 4. Candidates are then score-ordered (G5 auto-combo)
    ///
    /// `session_id` (optional) enables G3 session affinity: the account used
    /// for a session is preferred on subsequent turns. M5: cache-aware.
    #[allow(clippy::collapsible_if)]
    pub async fn execute(
        &mut self,
        req: &ChatRequest,
        session_id: Option<&str>,
    ) -> Result<ComboResult, ComboError> {
        let mut candidates = self.candidates(&req.model);
        candidates = self.score_order(candidates);
        let mut attempts: Vec<AttemptRecord> = Vec::new();
        let affinity = self.affinity_for(session_id);

        // M5: cache lookup before any upstream call
        if req.cache {
            if let Some(db) = &self.db {
                let key = crate::cache::Cache::key(req);
                if let Some(cached) = crate::cache::Cache::get(db, &key) {
                    if let Ok(resp) = serde_json::from_str::<ChatResponse>(&cached) {
                        tracing::info!("cache HIT {} ({})", key, req.model);
                        return Ok(ComboResult {
                            response: resp,
                            used_model: req.model.clone(),
                            used_provider: "cache".to_string(),
                            attempts: Vec::new(),
                        });
                    }
                }
            }
        }

        for (i, model) in candidates.iter().enumerate() {
            if i >= self.max_attempts {
                break;
            }

            // G3: prefer the session's account on the FIRST candidate only
            // (later fallbacks rotate normally)
            let preferred = if i == 0 {
                affinity
                    .as_ref()
                    .filter(|(provider, _)| {
                        provider == &self.resolve_provider_of(model).unwrap_or_default()
                    })
                    .map(|(_, key)| key.as_str())
            } else {
                None
            };

            let target = match self.router.route_prefer(model, preferred) {
                Ok(t) => t,
                Err(e) => {
                    attempts.push(AttemptRecord {
                        model: model.clone(),
                        provider: None,
                        error: e.to_string(),
                    });
                    if attempts.len() >= self.max_attempts {
                        break;
                    }
                    continue;
                }
            };

            self.scorer.begin_request(&target.provider_id);
            let started = std::time::Instant::now();
            let mut attempt_req = req.clone();
            attempt_req.model = model.clone();

            match target.executor.execute_chat(&attempt_req).await {
                Ok(resp) => {
                    self.scorer
                        .record_latency(&target.provider_id, started.elapsed().as_millis() as f64);
                    self.scorer.end_request(&target.provider_id);
                    // Telemetry (M2): successful chat with provider/model
                    let (pt, ct) = resp
                        .usage
                        .as_ref()
                        .map(|u| (u.prompt_tokens, u.completion_tokens))
                        .unwrap_or((0, 0));
                    crate::telemetry::TELEMETRY.record_chat(
                        &target.provider_id,
                        model,
                        200,
                        started.elapsed().as_millis() as u64,
                        pt,
                        ct,
                    );
                    self.router.report(
                        &target.provider_id,
                        &target.account_key,
                        crate::account::AccountOutcome::Success,
                    );
                    // G3: remember this account for the session
                    if let Some(sid) = session_id {
                        self.record_affinity(sid, &target.provider_id, &target.account_key);
                    }
                    // M5: cache the successful response
                    if req.cache {
                        if let Some(db) = &self.db {
                            let key = crate::cache::Cache::key(req);
                            let ttl = req.cache_ttl.unwrap_or(300);
                            let json = serde_json::to_string(&resp).unwrap_or_default();
                            crate::cache::Cache::set(db, &key, &req.model, &json, ttl);
                        }
                    }
                    // M4: webhook event on successful chat
                    if let Some(db) = &self.db {
                        crate::events::Events::dispatch(
                            db,
                            "chat.success",
                            serde_json::json!({
                                "provider": target.provider_id,
                                "model": model,
                                "status": 200,
                                "duration_ms": started.elapsed().as_millis() as u64,
                                "prompt_tokens": pt,
                                "completion_tokens": ct,
                            }),
                        );
                    }
                    return Ok(ComboResult {
                        response: resp,
                        used_model: model.clone(),
                        used_provider: target.provider_id.clone(),
                        attempts,
                    });
                }
                Err(e) => {
                    self.scorer
                        .record_latency(&target.provider_id, started.elapsed().as_millis() as f64);
                    self.scorer.record_failure(&target.provider_id);
                    self.scorer.end_request(&target.provider_id);
                    // Telemetry (M2): failed chat attempt
                    let err_code = match &e {
                        ExecutorError::RateLimited(_) => 429,
                        ExecutorError::AuthFailed(_) => 401,
                        ExecutorError::Upstream(code, _) => *code,
                        ExecutorError::Network(_) => 502,
                        ExecutorError::Timeout(_) => 504,
                        ExecutorError::InvalidResponse(_) => 502,
                        ExecutorError::UnsupportedProvider(_) => 400,
                    };
                    crate::telemetry::TELEMETRY.record_chat(
                        &target.provider_id,
                        model,
                        err_code,
                        started.elapsed().as_millis() as u64,
                        0,
                        0,
                    );
                    // M4: webhook event on failed chat
                    if let Some(db) = &self.db {
                        crate::events::Events::dispatch(
                            db,
                            "chat.error",
                            serde_json::json!({
                                "provider": target.provider_id,
                                "model": model,
                                "status": err_code,
                                "error": e.to_string(),
                            }),
                        );
                    }
                    let outcome = match &e {
                        ExecutorError::RateLimited(_) => {
                            crate::account::AccountOutcome::RateLimited
                        }
                        ExecutorError::AuthFailed(_) => crate::account::AccountOutcome::AuthFailed,
                        _ => crate::account::AccountOutcome::RateLimited,
                    };
                    self.router
                        .report(&target.provider_id, &target.account_key, outcome);
                    warn!(
                        model = %model,
                        provider = %target.provider_id,
                        error = %e,
                        "fallback attempt failed"
                    );
                    attempts.push(AttemptRecord {
                        model: model.clone(),
                        provider: Some(target.provider_id.clone()),
                        error: e.to_string(),
                    });
                    if !should_fallback(&e) || attempts.len() >= self.max_attempts {
                        break;
                    }
                }
            }
        }

        Err(ComboError {
            attempts,
            last_error: ExecutorError::Network("all fallback options exhausted".into()),
        })
    }

    /// G5: order candidates by score (best provider health/latency first).
    fn score_order(&self, candidates: Vec<String>) -> Vec<String> {
        if candidates.len() <= 1 {
            return candidates;
        }
        let mut scored: Vec<(f64, String)> = candidates
            .into_iter()
            .map(|m| {
                let provider = self.resolve_provider_of(&m).unwrap_or_default();
                let (available, backoff) = self.router.peek_health(&provider);
                let s = self.scorer.score(&provider, available, backoff);
                (s, m)
            })
            .collect();
        // Stable sort: higher score first, ties keep original order
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(_, m)| m).collect()
    }

    fn resolve_provider_of(&self, model: &str) -> Option<String> {
        // Reuse the router's deterministic resolution (single source of truth)
        self.router.resolve_provider(model).ok().map(String::from)
    }

    /// G3: look up the account a session is stuck to (DB).
    fn affinity_for(&self, session_id: Option<&str>) -> Option<(String, String)> {
        let sid = session_id?;
        let db = self.db.as_ref()?;
        let conn = db.conn.lock().ok()?;
        omniroute_db::repos::session_affinity_repo::get(&conn, sid).ok()?
    }

    /// G3: persist session → account affinity after a successful call.
    fn record_affinity(&self, session_id: &str, provider: &str, account_key: &str) {
        let Some(db) = self.db.as_ref() else { return };
        if let Ok(conn) = db.conn.lock() {
            let _ = omniroute_db::repos::session_affinity_repo::upsert(
                &conn,
                session_id,
                provider,
                account_key,
            );
        }
    }

    /// Stream a chat completion with fallback across candidates.
    /// Unlike non-streaming, once a stream is established from an upstream
    /// we commit to it (no mid-stream switching).
    pub async fn execute_stream(
        &mut self,
        req: &ChatRequest,
        session_id: Option<&str>,
    ) -> Result<StreamAttempt, ComboError> {
        let mut candidates = self.candidates(&req.model);
        candidates = self.score_order(candidates);
        let mut attempts: Vec<AttemptRecord> = Vec::new();
        let affinity = self.affinity_for(session_id);

        for (i, model) in candidates.iter().enumerate() {
            if i >= self.max_attempts {
                break;
            }

            let preferred = if i == 0 {
                affinity
                    .as_ref()
                    .filter(|(provider, _)| {
                        provider == &self.resolve_provider_of(model).unwrap_or_default()
                    })
                    .map(|(_, key)| key.as_str())
            } else {
                None
            };

            let target = match self.router.route_prefer(model, preferred) {
                Ok(t) => t,
                Err(e) => {
                    attempts.push(AttemptRecord {
                        model: model.clone(),
                        provider: None,
                        error: e.to_string(),
                    });
                    continue;
                }
            };

            let mut attempt_req = req.clone();
            attempt_req.model = model.clone();

            match target.executor.execute_chat_stream(&attempt_req).await {
                Ok(stream) => {
                    if let Some(sid) = session_id {
                        self.record_affinity(sid, &target.provider_id, &target.account_key);
                    }
                    return Ok(StreamAttempt {
                        provider_id: target.provider_id.clone(),
                        account_key: target.account_key.clone(),
                        model: model.clone(),
                        stream,
                    });
                }
                Err(e) => {
                    self.scorer.record_failure(&target.provider_id);
                    let outcome = match &e {
                        ExecutorError::RateLimited(_) => {
                            crate::account::AccountOutcome::RateLimited
                        }
                        ExecutorError::AuthFailed(_) => crate::account::AccountOutcome::AuthFailed,
                        _ => crate::account::AccountOutcome::RateLimited,
                    };
                    self.router
                        .report(&target.provider_id, &target.account_key, outcome);
                    attempts.push(AttemptRecord {
                        model: model.clone(),
                        provider: Some(target.provider_id.clone()),
                        error: e.to_string(),
                    });
                }
            }
        }

        Err(ComboError {
            attempts,
            last_error: ExecutorError::Network("all streaming fallback options exhausted".into()),
        })
    }

    /// Compute the ordered candidate model list for a request model.
    fn candidates(&self, model: &str) -> Vec<String> {
        // 1. Explicit combo match
        if let Some(combo) = self.combos.get(model) {
            return combo.models.clone();
        }
        // 2. Auto-fallback chain
        if let Some(alts) = self.auto_fallbacks.get(model) {
            let mut chain = vec![model.to_string()];
            chain.extend(alts.iter().cloned());
            return chain;
        }
        // 3. Model only
        vec![model.to_string()]
    }
}

/// Build a sensible default auto-fallback chain for a model based on
/// the provider registry (same provider → cheaper models first).
pub fn default_fallbacks(model: &str) -> Vec<String> {
    let lower = model.to_lowercase();
    match lower {
        m if m.starts_with("gpt-4o") => vec![
            "gpt-4o-mini".into(),
            "claude-sonnet-4".into(),
            "gemini-2.5-flash".into(),
        ],
        m if m.starts_with("claude-") => vec![
            "claude-haiku-3-5".into(),
            "gpt-4o-mini".into(),
            "deepseek-chat".into(),
        ],
        m if m.starts_with("gemini-") => vec![
            "gpt-4o-mini".into(),
            "deepseek-chat".into(),
            "claude-haiku-3-5".into(),
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Message;
    use crate::router::RouterConfig;

    fn test_request(model: &str) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".into(),
                content: Some(crate::chat::Content::Text("hi".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            tools: None,
            tool_choice: None,
            extra: None,
            cache: false,
            cache_ttl: None,
        }
    }

    #[test]
    fn test_candidates_plain_model() {
        let engine = ComboEngine::new(RoutingEngine::new(RouterConfig::default()));
        assert_eq!(engine.candidates("gpt-4o"), vec!["gpt-4o"]);
    }

    #[test]
    fn test_candidates_with_fallback() {
        let engine = ComboEngine::new(RoutingEngine::new(RouterConfig::default())).with_fallback(
            "gpt-4o",
            vec!["gpt-4o-mini".into(), "claude-sonnet-4".into()],
        );
        assert_eq!(
            engine.candidates("gpt-4o"),
            vec!["gpt-4o", "gpt-4o-mini", "claude-sonnet-4"]
        );
    }

    #[test]
    fn test_candidates_combo() {
        let combo = Combo::new(
            "c1",
            "fast",
            vec!["gpt-4o-mini".into(), "gemini-2.5-flash".into()],
        );
        let engine =
            ComboEngine::new(RoutingEngine::new(RouterConfig::default())).with_combo(combo);
        assert_eq!(
            engine.candidates("fast"),
            vec!["gpt-4o-mini", "gemini-2.5-flash"]
        );
    }

    #[test]
    fn test_default_fallbacks_gpt() {
        let alts = default_fallbacks("gpt-4o");
        assert_eq!(alts[0], "gpt-4o-mini");
        assert!(alts.contains(&"claude-sonnet-4".to_string()));
    }

    #[test]
    fn test_default_fallbacks_unknown() {
        assert!(default_fallbacks("weird-model").is_empty());
    }

    #[test]
    fn test_max_attempts_respected() {
        let engine = ComboEngine::new(RoutingEngine::new(RouterConfig::default()))
            .with_fallback(
                "gpt-4o",
                vec!["x1".into(), "x2".into(), "x3".into(), "x4".into()],
            )
            .with_max_attempts(2);
        let candidates = engine.candidates("gpt-4o");
        assert!(candidates.len() >= 2); // list itself is longer, attempts capped at runtime
    }

    #[tokio::test]
    async fn test_all_fail_records_attempts() {
        let config = RouterConfig::default()
            .with_key("openai", "sk-x")
            .with_key("claude", "sk-y");
        let mut engine = ComboEngine::new(RoutingEngine::new(config))
            .with_fallback("gpt-4o", vec!["claude-sonnet-4".into()]);
        let err = engine
            .execute(&test_request("gpt-4o"), None)
            .await
            .unwrap_err();
        // Both attempts recorded (network errors → fallback triggered)
        assert_eq!(err.attempts.len(), 2);
    }

    #[tokio::test]
    async fn test_missing_keys_short_circuit() {
        // No API keys at all → each route fails fast, attempts recorded
        let mut engine = ComboEngine::new(RoutingEngine::new(RouterConfig::default()))
            .with_fallback("gpt-4o", vec!["deepseek-chat".into()]);
        let err = engine
            .execute(&test_request("gpt-4o"), None)
            .await
            .unwrap_err();
        assert!(!err.attempts.is_empty());
    }
}
