pub mod request_builder;
pub mod response_parser;
pub mod streaming;

use crate::chat::{ChatRequest, ChatResponse};
use reqwest::StatusCode;
use std::time::Duration;
use thiserror::Error;

/// Errors that can occur while executing a request against a provider
#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("rate limited by upstream (HTTP {0})")]
    RateLimited(u16),
    #[error("authentication failed (HTTP {0})")]
    AuthFailed(u16),
    #[error("upstream error (HTTP {0}): {1}")]
    Upstream(u16, String),
    #[error("network error: {0}")]
    Network(String),
    #[error("timeout after {0}s")]
    Timeout(u64),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
}

/// Which upstream API format a provider speaks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFormat {
    /// OpenAI-compatible chat completions (OpenAI, DeepSeek, Groq, ...)
    OpenAi,
    /// Anthropic Messages API
    Claude,
    /// Google Gemini generateContent
    Gemini,
}

impl std::fmt::Display for ApiFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiFormat::OpenAi => write!(f, "openai"),
            ApiFormat::Claude => write!(f, "claude"),
            ApiFormat::Gemini => write!(f, "gemini"),
        }
    }
}

/// HTTP executor that talks to a real provider upstream
#[derive(Debug)]
pub struct ProviderExecutor {
    client: reqwest::Client,
    api_format: ApiFormat,
    base_url: String,
    api_key: String,
    timeout_secs: u64,
}

impl ProviderExecutor {
    pub fn new(api_format: ApiFormat, base_url: &str, api_key: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .user_agent("omniroute-rs/0.1.0")
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            api_format,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            timeout_secs: 120,
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Which upstream API format this executor speaks
    pub fn api_format(&self) -> ApiFormat {
        self.api_format
    }

    /// Execute a chat completion request against the configured provider.
    ///
    /// The request is translated to the provider's native wire format, sent
    /// upstream, and the response is normalized back to the OpenAI-compatible
    /// `ChatResponse` shape.
    pub async fn execute_chat(&self, req: &ChatRequest) -> Result<ChatResponse, ExecutorError> {
        let started = std::time::Instant::now();
        let result = self.execute_chat_inner(req).await;
        match &result {
            Ok(_) => tracing::info!(
                "upstream {} ok in {} ms",
                self.api_format,
                started.elapsed().as_millis()
            ),
            Err(e) => tracing::warn!(
                "upstream {} failed in {} ms: {}",
                self.api_format,
                started.elapsed().as_millis(),
                e
            ),
        }
        result
    }

    async fn execute_chat_inner(&self, req: &ChatRequest) -> Result<ChatResponse, ExecutorError> {
        // Curated registry base_url sudah berisi endpoint path lengkap
        // (https://api.openai.com/v1/chat/completions) — jangan append dobel.
        // Env override (OMNIROUTE_BASE_URL_<PROVIDER>) biasanya base-only
        // (https://api.openai.com/v1) → append path.
        let url = match self.api_format {
            ApiFormat::OpenAi if self.base_url.ends_with("/chat/completions") => {
                self.base_url.clone()
            }
            ApiFormat::Claude if self.base_url.ends_with("/messages") => self.base_url.clone(),
            ApiFormat::Gemini if self.base_url.ends_with(":generateContent") => {
                self.base_url.clone()
            }
            ApiFormat::OpenAi => format!("{}/chat/completions", self.base_url),
            ApiFormat::Claude => format!("{}/messages", self.base_url),
            ApiFormat::Gemini => format!("{}/models/{}:generateContent", self.base_url, req.model),
        };

        let body = request_builder::build_upstream_request(self.api_format, req)?;
        let mut builder = self.client.post(&url).json(&body);

        match self.api_format {
            // No-auth providers (e.g. opencode free) have an empty key —
            // skip the auth header entirely in that case.
            ApiFormat::OpenAi => {
                if !self.api_key.is_empty() {
                    builder = builder.bearer_auth(&self.api_key);
                }
            }
            ApiFormat::Claude => {
                if !self.api_key.is_empty() {
                    builder = builder.header("x-api-key", &self.api_key);
                }
                builder = builder.header("anthropic-version", "2023-06-01");
            }
            ApiFormat::Gemini => {
                if !self.api_key.is_empty() {
                    builder = builder.query(&[("key", &self.api_key)]);
                }
            }
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| ExecutorError::Network(e.to_string()))?;

        let status = resp.status();
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| ExecutorError::Network(e.to_string()))?;

        match status {
            StatusCode::OK => {
                response_parser::parse_upstream_response(self.api_format, &req.model, &body_bytes)
            }
            StatusCode::TOO_MANY_REQUESTS => Err(ExecutorError::RateLimited(status.as_u16())),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Err(ExecutorError::AuthFailed(status.as_u16()))
            }
            _ => Err(ExecutorError::Upstream(
                status.as_u16(),
                String::from_utf8_lossy(&body_bytes).into_owned(),
            )),
        }
    }

    /// Convenience: factory that picks the right executor for a provider id.
    /// `base_override` (optional) replaces the default upstream URL —
    /// used for tests and self-hosted mirrors.
    pub fn from_provider_id_with_base(
        provider_id: &str,
        api_key: &str,
        base_override: Option<&str>,
    ) -> Result<Self, ExecutorError> {
        // Prefer curated registry (base_url + format dari OmniRoute
        // registry — 220 provider) sebelum fallback ke hardcoded match.
        match omniroute_providers::get_provider(provider_id) {
            Some(p) if p.base_url.is_some() => {
                let base = p.base_url.as_deref().unwrap();
                let fmt = match p.format.as_deref() {
                    Some("claude" | "anthropic") => ApiFormat::Claude,
                    Some("gemini") => ApiFormat::Gemini,
                    _ => ApiFormat::OpenAi,
                };
                return Ok(Self::new(fmt, base_override.unwrap_or(base), api_key));
            }
            _ => {}
        }

        // Fallback: default base URLs — all OpenAI-compatible except...
        let (format, base) = match provider_id {
            "openai" => (ApiFormat::OpenAi, "https://api.openai.com/v1"),
            "deepseek" => (ApiFormat::OpenAi, "https://api.deepseek.com/v1"),
            "claude" => (ApiFormat::Claude, "https://api.anthropic.com/v1"),
            "gemini" | "google" => (
                ApiFormat::Gemini,
                "https://generativelanguage.googleapis.com/v1beta",
            ),
            "groq" => (ApiFormat::OpenAi, "https://api.groq.com/openai/v1"),
            "mistral" => (ApiFormat::OpenAi, "https://api.mistral.ai/v1"),
            "cerebras" => (ApiFormat::OpenAi, "https://api.cerebras.ai/v1"),
            "huggingface" => (ApiFormat::OpenAi, "https://router.huggingface.co/v1"),
            "openrouter" => (ApiFormat::OpenAi, "https://openrouter.ai/api/v1"),
            "together" => (ApiFormat::OpenAi, "https://api.together.xyz/v1"),
            "xai" => (ApiFormat::OpenAi, "https://api.x.ai/v1"),
            "cohere" => (ApiFormat::OpenAi, "https://api.cohere.com/v2"),
            "deepinfra" => (ApiFormat::OpenAi, "https://api.deepinfra.com/v1/openai"),
            "sambanova" => (ApiFormat::OpenAi, "https://api.sambanova.ai/v1"),
            "moonshot" => (ApiFormat::OpenAi, "https://api.moonshot.ai/v1"),
            "nvidia" => (ApiFormat::OpenAi, "https://integrate.api.nvidia.com/v1"),
            "fireworks" => (ApiFormat::OpenAi, "https://api.fireworks.ai/inference/v1"),
            "perplexity" => (ApiFormat::OpenAi, "https://api.perplexity.ai"),
            "zhipu" => (ApiFormat::OpenAi, "https://open.bigmodel.cn/api/paas/v4"),
            "qwen" => (
                ApiFormat::OpenAi,
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
            ),
            // public no-key endpoint (OpenCode free)
            "opencode" => (ApiFormat::OpenAi, "https://opencode.ai/zen/v1"),
            other => return Err(ExecutorError::UnsupportedProvider(other.to_string())),
        };
        Ok(Self::new(format, base_override.unwrap_or(base), api_key))
    }

    /// Convenience: factory that picks the right executor for a provider id.
    pub fn from_provider_id(provider_id: &str, api_key: &str) -> Result<Self, ExecutorError> {
        Self::from_provider_id_with_base(provider_id, api_key, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Message;
    use axum::{Json, Router, extract::State, http::StatusCode as AxumStatus, routing::post};
    use serde_json::Value;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    // ── Mock upstream server ──

    #[derive(Clone)]
    struct MockState {
        api_format: ApiFormat,
    }

    async fn mock_handler(
        State(state): State<Arc<MockState>>,
        Json(body): Json<Value>,
    ) -> (AxumStatus, Json<Value>) {
        match state.api_format {
            ApiFormat::OpenAi => (
                AxumStatus::OK,
                Json(serde_json::json!({
                    "id": "chatcmpl-mock",
                    "object": "chat.completion",
                    "created": 0,
                    "model": body["model"],
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "Mock OpenAI reply"},
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                })),
            ),
            ApiFormat::Claude => (
                AxumStatus::OK,
                Json(serde_json::json!({
                    "id": "msg_01mock",
                    "type": "message",
                    "role": "assistant",
                    "model": body["model"],
                    "content": [{"type": "text", "text": "Mock Claude reply"}],
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                })),
            ),
            ApiFormat::Gemini => (
                AxumStatus::OK,
                Json(serde_json::json!({
                    "candidates": [{
                        "content": {
                            "parts": [{"text": "Mock Gemini reply"}],
                            "role": "model"
                        },
                        "finishReason": "STOP"
                    }],
                    "usageMetadata": {
                        "promptTokenCount": 1,
                        "candidatesTokenCount": 1,
                        "totalTokenCount": 2
                    }
                })),
            ),
        }
    }

    async fn spawn_mock(format: ApiFormat) -> (String, tokio::task::JoinHandle<()>) {
        let state = Arc::new(MockState { api_format: format });
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_handler))
            .route("/v1/messages", post(mock_handler))
            .route("/v1beta/models/{*rest}", post(mock_handler))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Executor appends the resource path to base_url; give it the
        // matching API prefix so the mock routes line up.
        let base = match format {
            ApiFormat::OpenAi | ApiFormat::Claude => format!("http://{}/v1", addr),
            ApiFormat::Gemini => format!("http://{}/v1beta", addr),
        };
        (base, server)
    }

    fn sample_request(model: &str) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".into(),
                content: Some(crate::chat::Content::Text("Hello".into())),
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
            compress: false,
            max_context_tokens: None,
        }
    }

    #[tokio::test]
    async fn test_openai_executor() {
        let (base, server) = spawn_mock(ApiFormat::OpenAi).await;
        let executor = ProviderExecutor::new(ApiFormat::OpenAi, &base, "test-key");
        let resp = executor
            .execute_chat(&sample_request("gpt-4o"))
            .await
            .unwrap();
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "Mock OpenAI reply"
        );
        assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 2);
        server.abort();
    }

    #[tokio::test]
    async fn test_claude_executor() {
        let (base, server) = spawn_mock(ApiFormat::Claude).await;
        let executor = ProviderExecutor::new(ApiFormat::Claude, &base, "test-key");
        let resp = executor
            .execute_chat(&sample_request("claude-sonnet-4-20250514"))
            .await
            .unwrap();
        assert_eq!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "Mock Claude reply"
        );
        server.abort();
    }

    #[tokio::test]
    async fn test_gemini_executor() {
        let (base, server) = spawn_mock(ApiFormat::Gemini).await;
        let executor = ProviderExecutor::new(ApiFormat::Gemini, &base, "test-key");
        let resp = executor
            .execute_chat(&sample_request("gemini-2.5-flash"))
            .await
            .unwrap();
        assert_eq!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "Mock Gemini reply"
        );
        server.abort();
    }

    #[tokio::test]
    async fn test_unsupported_provider() {
        let err = ProviderExecutor::from_provider_id("nonexistent", "key").unwrap_err();
        assert!(matches!(err, ExecutorError::UnsupportedProvider(_)));
    }

    #[tokio::test]
    async fn test_factory_creates_executor() {
        let exec = ProviderExecutor::from_provider_id("openai", "key").unwrap();
        assert_eq!(exec.api_format, ApiFormat::OpenAi);
        // base_url dari curated registry = endpoint penuh
        assert!(exec.base_url.ends_with("/chat/completions"));
        // base_override (env) tetap menang
        let exec2 = ProviderExecutor::from_provider_id_with_base(
            "openai",
            "key",
            Some("http://127.0.0.1:9999/v1"),
        )
        .unwrap();
        assert_eq!(exec2.base_url, "http://127.0.0.1:9999/v1");
    }
}
