use crate::chat::ChatRequest;
use crate::executor::{ApiFormat, ExecutorError, ProviderExecutor};
use futures::Stream;
use futures::stream::StreamExt;
use reqwest::StatusCode;
use serde_json::Value;
use std::pin::Pin;

/// A normalized streaming chunk (OpenAI-compatible shape)
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// OpenAI-compatible chunk JSON (serialized)
    Data(String),
    /// End of stream marker
    Done,
}

/// Stream of normalized chunks from an upstream SSE response
pub type ChunkStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, ExecutorError>> + Send>>;

impl ProviderExecutor {
    /// Execute a chat request with streaming enabled; returns a normalized
    /// chunk stream. Each provider's native SSE format is translated to
    /// OpenAI-compatible chunk JSON on the fly.
    pub async fn execute_chat_stream(
        &self,
        req: &ChatRequest,
    ) -> Result<ChunkStream, ExecutorError> {
        let url = match self.api_format {
            ApiFormat::OpenAi => format!("{}/chat/completions", self.base_url),
            ApiFormat::Claude => format!("{}/messages", self.base_url),
            ApiFormat::Gemini => format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                self.base_url, req.model
            ),
        };

        let body = crate::executor::request_builder::build_upstream_request(self.api_format, req)?;
        let mut builder = self.client.post(&url).json(&body);

        match self.api_format {
            ApiFormat::OpenAi => {
                builder = builder.bearer_auth(&self.api_key);
            }
            ApiFormat::Claude => {
                builder = builder
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", "2023-06-01");
            }
            ApiFormat::Gemini => {
                builder = builder.query(&[("key", &self.api_key)]);
            }
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| ExecutorError::Network(e.to_string()))?;

        match resp.status() {
            StatusCode::OK => {}
            StatusCode::TOO_MANY_REQUESTS => return Err(ExecutorError::RateLimited(429)),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(ExecutorError::AuthFailed(resp.status().as_u16()));
            }
            other => {
                return Err(ExecutorError::Upstream(
                    other.as_u16(),
                    "stream setup failed".into(),
                ));
            }
        }

        let format = self.api_format;
        let byte_stream = resp.bytes_stream();
        let model_owned = req.model.clone();
        let stream = parse_sse_stream(byte_stream, format, model_owned).await;
        Ok(Box::pin(stream))
    }
}

/// Parse raw SSE bytes from reqwest into normalized chunks.
async fn parse_sse_stream<S>(
    byte_stream: S,
    format: ApiFormat,
    model: String,
) -> impl Stream<Item = Result<StreamChunk, ExecutorError>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send,
{
    let buffer: Vec<u8> = Vec::new();
    let events: Vec<String> = Vec::new();
    let byte_stream = Box::pin(byte_stream);
    let model_for_normalize = model.clone();

    futures::stream::unfold(
        (byte_stream, buffer, events, format, model),
        |(mut byte_stream, mut buffer, mut events, format, model)| async move {
            loop {
                match byte_stream.next().await {
                    Some(Ok(chunk)) => {
                        buffer.extend_from_slice(&chunk);
                        let mut start = 0;
                        for i in 0..buffer.len().saturating_sub(1) {
                            if buffer[i] == b'\n' && buffer[i + 1] == b'\n' {
                                if i > start {
                                    let evt =
                                        String::from_utf8_lossy(&buffer[start..i]).to_string();
                                    if let Some(data) = extract_sse_data(&evt) {
                                        events.push(data);
                                    }
                                }
                                start = i + 2;
                            }
                        }
                        if start > 0 {
                            buffer.drain(..start);
                        }
                        if !events.is_empty() {
                            let drained: Vec<String> = std::mem::take(&mut events);
                            return Some((
                                Ok(drained),
                                (byte_stream, buffer, events, format, model),
                            ));
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(ExecutorError::Network(e.to_string())),
                            (byte_stream, buffer, events, format, model),
                        ));
                    }
                    None => {
                        // Flush any trailing event without double-newline
                        if !events.is_empty() {
                            let drained: Vec<String> = std::mem::take(&mut events);
                            return Some((
                                Ok(drained),
                                (byte_stream, buffer, events, format, model),
                            ));
                        }
                        return None;
                    }
                }
            }
        },
    )
    .flat_map(move |evs: Result<Vec<String>, ExecutorError>| {
        let mut out: Vec<Result<StreamChunk, ExecutorError>> = Vec::new();
        match evs {
            Ok(events) => {
                for evt in events {
                    match normalize_chunk(&evt, format, &model_for_normalize) {
                        Some(Ok(StreamChunk::Done)) => {
                            out.push(Ok(StreamChunk::Done));
                        }
                        Some(Ok(StreamChunk::Data(d))) => {
                            out.push(Ok(StreamChunk::Data(d)));
                        }
                        Some(Err(e)) => out.push(Err(e)),
                        None => {}
                    }
                }
            }
            Err(e) => out.push(Err(e)),
        }
        futures::stream::iter(out)
    })
}

/// Extract the `data:` payload from an SSE event block.
fn extract_sse_data(event: &str) -> Option<String> {
    for line in event.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("data:") {
            let payload = rest.trim_start().trim_end_matches('\r').to_string();
            if !payload.is_empty() {
                return Some(payload);
            }
        }
    }
    None
}

/// Normalize one provider SSE payload into an OpenAI-compatible chunk.
fn normalize_chunk(
    payload: &str,
    format: ApiFormat,
    model: &str,
) -> Option<Result<StreamChunk, ExecutorError>> {
    if payload == "[DONE]" {
        return Some(Ok(StreamChunk::Done));
    }

    let v: Value = serde_json::from_str(payload).ok()?;

    match format {
        ApiFormat::OpenAi => {
            // Already OpenAI-compatible → pass through
            Some(Ok(StreamChunk::Data(payload.to_string())))
        }
        ApiFormat::Claude => {
            // Claude streams events like:
            //   {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}
            //   {"type":"message_stop"}
            match v.get("type").and_then(Value::as_str) {
                Some("content_block_delta") => {
                    let text = v
                        .pointer("/delta/text")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let chunk = serde_json::json!({
                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": model,
                        "choices": [{
                            "index": 0,
                            "delta": {"content": text},
                            "finish_reason": null,
                        }]
                    });
                    Some(Ok(StreamChunk::Data(chunk.to_string())))
                }
                Some("message_stop") => Some(Ok(StreamChunk::Done)),
                _ => None,
            }
        }
        ApiFormat::Gemini => {
            // Gemini streams {"candidates":[{"content":{"parts":[{"text":...}]}}]}
            let text = v
                .pointer("/candidates/0/content/parts")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            if text.is_empty() {
                return Some(Ok(StreamChunk::Done));
            }
            let chunk = serde_json::json!({
                "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {"content": text},
                    "finish_reason": null,
                }]
            });
            Some(Ok(StreamChunk::Data(chunk.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sse_data_simple() {
        let evt = "data: {\"hello\":\"world\"}\n";
        assert_eq!(extract_sse_data(evt).unwrap(), "{\"hello\":\"world\"}");
    }

    #[test]
    fn test_extract_sse_data_comment() {
        let evt = ": keepalive\n\n";
        assert!(extract_sse_data(evt).is_none());
    }

    #[test]
    fn test_extract_sse_done() {
        let evt = "data: [DONE]\n";
        assert_eq!(extract_sse_data(evt).unwrap(), "[DONE]");
    }

    #[test]
    fn test_normalize_openai_passthrough() {
        let chunk = normalize_chunk(
            r#"{"id":"x","object":"chat.completion.chunk","choices":[{"delta":{"content":"hi"}}]}"#,
            ApiFormat::OpenAi,
            "gpt-4o",
        )
        .unwrap()
        .unwrap();
        match chunk {
            StreamChunk::Data(d) => assert!(d.contains("hi")),
            _ => panic!("expected data"),
        }
    }

    #[test]
    fn test_normalize_claude_delta() {
        let chunk = normalize_chunk(
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}"#,
            ApiFormat::Claude,
            "claude-x",
        )
        .unwrap()
        .unwrap();
        match chunk {
            StreamChunk::Data(d) => {
                let v: Value = serde_json::from_str(&d).unwrap();
                assert_eq!(v["choices"][0]["delta"]["content"], "Hello");
            }
            _ => panic!("expected data"),
        }
    }

    #[test]
    fn test_normalize_claude_stop() {
        let chunk = normalize_chunk(r#"{"type":"message_stop"}"#, ApiFormat::Claude, "claude-x")
            .unwrap()
            .unwrap();
        assert!(matches!(chunk, StreamChunk::Done));
    }

    #[test]
    fn test_normalize_gemini() {
        let chunk = normalize_chunk(
            r#"{"candidates":[{"content":{"parts":[{"text":"World"}]}}]}"#,
            ApiFormat::Gemini,
            "gemini-x",
        )
        .unwrap()
        .unwrap();
        match chunk {
            StreamChunk::Data(d) => {
                let v: Value = serde_json::from_str(&d).unwrap();
                assert_eq!(v["choices"][0]["delta"]["content"], "World");
            }
            _ => panic!("expected data"),
        }
    }

    #[test]
    fn test_normalize_done_marker() {
        let chunk = normalize_chunk("[DONE]", ApiFormat::OpenAi, "gpt-4o")
            .unwrap()
            .unwrap();
        assert!(matches!(chunk, StreamChunk::Done));
    }
}
