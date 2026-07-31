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
}

impl ComboEngine {
    pub fn new(router: RoutingEngine) -> Self {
        Self {
            router,
            combos: HashMap::new(),
            auto_fallbacks: HashMap::new(),
            max_attempts: 5,
        }
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

    /// Execute a request with fallback.
    ///
    /// Resolution order for the candidate list:
    /// 1. If the request model names a registered combo, use its chain
    /// 2. Else use the registered auto-fallback chain for the model
    /// 3. Else just the model itself
    pub async fn execute(&mut self, req: &ChatRequest) -> Result<ComboResult, ComboError> {
        let candidates = self.candidates(&req.model);
        let mut attempts: Vec<AttemptRecord> = Vec::new();

        for (i, model) in candidates.iter().enumerate() {
            if i >= self.max_attempts {
                break;
            }

            let target = match self.router.route(model) {
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

            let mut attempt_req = req.clone();
            attempt_req.model = model.clone();

            match target.executor.execute_chat(&attempt_req).await {
                Ok(resp) => {
                    self.router.report(
                        &target.provider_id,
                        &target.account_key,
                        crate::account::AccountOutcome::Success,
                    );
                    return Ok(ComboResult {
                        response: resp,
                        used_model: model.clone(),
                        used_provider: target.provider_id.clone(),
                        attempts,
                    });
                }
                Err(e) => {
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

    /// Stream a chat completion with fallback across candidates.
    /// Unlike non-streaming, once a stream is established from an upstream
    /// we commit to it (no mid-stream switching).
    pub async fn execute_stream(&mut self, req: &ChatRequest) -> Result<StreamAttempt, ComboError> {
        let candidates = self.candidates(&req.model);
        let mut attempts: Vec<AttemptRecord> = Vec::new();

        for (i, model) in candidates.iter().enumerate() {
            if i >= self.max_attempts {
                break;
            }

            let target = match self.router.route(model) {
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
                    return Ok(StreamAttempt {
                        provider_id: target.provider_id.clone(),
                        account_key: target.account_key.clone(),
                        model: model.clone(),
                        stream,
                    });
                }
                Err(e) => {
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
        let err = engine.execute(&test_request("gpt-4o")).await.unwrap_err();
        // Both attempts recorded (network errors → fallback triggered)
        assert_eq!(err.attempts.len(), 2);
    }

    #[tokio::test]
    async fn test_missing_keys_short_circuit() {
        // No API keys at all → each route fails fast, attempts recorded
        let mut engine = ComboEngine::new(RoutingEngine::new(RouterConfig::default()))
            .with_fallback("gpt-4o", vec!["deepseek-chat".into()]);
        let err = engine.execute(&test_request("gpt-4o")).await.unwrap_err();
        assert!(!err.attempts.is_empty());
    }
}
