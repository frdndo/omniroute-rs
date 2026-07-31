use crate::chat::{ChatResponse, Content};
use crate::executor::ExecutorError;
use axum::http::StatusCode;
use serde_json::json;

/// Normalize a ChatResponse before returning it to the client.
pub fn sanitize_response(mut resp: ChatResponse) -> ChatResponse {
    // Ensure object type is correct
    resp.object = "chat.completion".to_string();

    for choice in resp.choices.iter_mut() {
        // Empty text content → None (OpenAI-compatible)
        #[allow(clippy::collapsible_if)]
        if let Some(Content::Text(t)) = &choice.message.content {
            if t.is_empty() {
                choice.message.content = None;
            }
        }
        // Normalize finish_reason values from providers
        if let Some(fr) = &choice.finish_reason {
            let normalized = match fr.as_str() {
                "end_turn" | "STOP" | "MAX_TOKENS" | "SAFETY" | "RECITATION" => match fr.as_str() {
                    "end_turn" | "STOP" => "stop",
                    "MAX_TOKENS" => "length",
                    _ => "content_filter",
                },
                other => other,
            };
            choice.finish_reason = Some(normalized.to_string());
        }
    }

    // created = 0 shouldn't happen (means unparsed)
    if resp.created == 0 {
        resp.created = chrono::Utc::now().timestamp();
    }

    resp
}

/// Map an ExecutorError to (HTTP status, OpenAI-compatible error body).
pub fn error_to_response(
    e: &ExecutorError,
    provider: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let (status, err_type, code) = match e {
        ExecutorError::RateLimited(_) => {
            (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", Some(429))
        }
        ExecutorError::AuthFailed(_) => {
            (StatusCode::UNAUTHORIZED, "authentication_error", Some(401))
        }
        ExecutorError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, "upstream_timeout", Some(504)),
        ExecutorError::Network(_) => (StatusCode::BAD_GATEWAY, "upstream_error", Some(502)),
        ExecutorError::Upstream(status, _body) => {
            let t = if *status >= 500 {
                "upstream_error"
            } else {
                "invalid_request_error"
            };
            (StatusCode::BAD_GATEWAY, t, Some(*status as i64))
        }
        ExecutorError::InvalidResponse(_) => {
            (StatusCode::BAD_GATEWAY, "invalid_response", Some(502))
        }
        ExecutorError::UnsupportedProvider(_) => {
            (StatusCode::BAD_REQUEST, "invalid_request_error", Some(400))
        }
    };

    let mut error = json!({
        "message": e.to_string(),
        "type": err_type,
        "code": code,
    });
    if let Some(p) = provider {
        error["provider"] = json!(p);
    }

    (status, json!({ "error": error }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Choice, Message, Usage};

    fn resp_with(finish: &str, content: &str) -> ChatResponse {
        ChatResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "m".into(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".into(),
                    content: Some(Content::Text(content.into())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some(finish.into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
            }),
        }
    }

    #[test]
    fn test_empty_content_nulled() {
        let resp = sanitize_response(resp_with("stop", ""));
        assert!(resp.choices[0].message.content.is_none());
    }

    #[test]
    fn test_finish_reason_normalized() {
        let resp = sanitize_response(resp_with("end_turn", "hi"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));

        let resp = sanitize_response(resp_with("MAX_TOKENS", "hi"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("length"));

        let resp = sanitize_response(resp_with("SAFETY", "hi"));
        assert_eq!(
            resp.choices[0].finish_reason.as_deref(),
            Some("content_filter")
        );
    }

    #[test]
    fn test_created_filled() {
        let resp = sanitize_response(resp_with("stop", "hi"));
        assert!(resp.created > 0);
    }

    #[test]
    fn test_error_mapping_rate_limit() {
        let (status, body) = error_to_response(&ExecutorError::RateLimited(429), Some("openai"));
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"]["type"], "rate_limit_error");
        assert_eq!(body["error"]["provider"], "openai");
    }

    #[test]
    fn test_error_mapping_auth() {
        let (status, _) = error_to_response(&ExecutorError::AuthFailed(401), None);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_error_mapping_unsupported() {
        let (status, body) =
            error_to_response(&ExecutorError::UnsupportedProvider("x".into()), None);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["type"], "invalid_request_error");
    }
}
