use crate::chat::{ChatResponse, Choice, Content, Message, Usage};
use crate::executor::{ApiFormat, ExecutorError};
use serde_json::Value;

/// Normalize a provider's native response back into an OpenAI-compatible
/// `ChatResponse`.
pub fn parse_upstream_response(
    format: ApiFormat,
    model: &str,
    body: &[u8],
) -> Result<ChatResponse, ExecutorError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| ExecutorError::InvalidResponse(format!("bad JSON: {}", e)))?;
    match format {
        ApiFormat::OpenAi => parse_openai(model, &value),
        ApiFormat::Claude => parse_claude(model, &value),
        ApiFormat::Gemini => parse_gemini(model, &value),
    }
}

fn parse_openai(model: &str, v: &Value) -> Result<ChatResponse, ExecutorError> {
    let choices = v
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| ExecutorError::InvalidResponse("missing choices".into()))?
        .iter()
        .enumerate()
        .map(|(idx, c)| {
            let message = c.get("message").unwrap_or(&Value::Null);
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .map(|s| Content::Text(s.to_string()))
                .or_else(|| {
                    // content can be an array of parts (some providers)
                    message.get("content").and_then(Value::as_array).map(|arr| {
                        Content::Parts(
                            arr.iter()
                                .filter_map(|p| {
                                    p.get("text").and_then(Value::as_str).map(|t| {
                                        crate::chat::ContentPart {
                                            text: Some(t.to_string()),
                                            r#type: Some("text".into()),
                                            image_url: None,
                                        }
                                    })
                                })
                                .collect(),
                        )
                    })
                });
            let tool_calls = message.get("tool_calls").cloned();
            let finish_reason = c.get("finish_reason").and_then(Value::as_str);
            Choice {
                index: idx,
                message: Message {
                    role: message
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("assistant")
                        .to_string(),
                    content,
                    name: None,
                    tool_calls: tool_calls.and_then(|tc| serde_json::from_value(tc).ok()),
                    tool_call_id: None,
                },
                finish_reason: finish_reason.map(String::from),
            }
        })
        .collect::<Vec<_>>();

    let usage = v.get("usage").map(|u| Usage {
        prompt_tokens: u.get("prompt_tokens").and_then(Value::as_i64).unwrap_or(0),
        completion_tokens: u
            .get("completion_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        total_tokens: u.get("total_tokens").and_then(Value::as_i64).unwrap_or(0),
    });

    Ok(ChatResponse {
        id: v
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("chatcmpl-unknown")
            .to_string(),
        object: "chat.completion".into(),
        created: v.get("created").and_then(Value::as_i64).unwrap_or(0),
        model: model.to_string(),
        choices,
        usage,
    })
}

fn parse_claude(model: &str, v: &Value) -> Result<ChatResponse, ExecutorError> {
    let content_blocks = v
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ExecutorError::InvalidResponse("missing content".into()))?;

    let text = content_blocks
        .iter()
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");

    let usage = v.get("usage").map(|u| Usage {
        prompt_tokens: u.get("input_tokens").and_then(Value::as_i64).unwrap_or(0),
        completion_tokens: u.get("output_tokens").and_then(Value::as_i64).unwrap_or(0),
        total_tokens: u.get("input_tokens").and_then(Value::as_i64).unwrap_or(0)
            + u.get("output_tokens").and_then(Value::as_i64).unwrap_or(0),
    });

    Ok(ChatResponse {
        id: v
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("msg_unknown")
            .to_string(),
        object: "chat.completion".into(),
        created: chrono::Utc::now().timestamp(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: "assistant".into(),
                content: if text.is_empty() {
                    None
                } else {
                    Some(Content::Text(text))
                },
                name: None,
                tool_calls: {
                    let calls = crate::translator::ToolTranslator::parse_claude_tool_calls(v);
                    if calls.is_empty() { None } else { Some(calls) }
                },
                tool_call_id: None,
            },
            finish_reason: Some(
                v.get("stop_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("stop")
                    .to_string(),
            ),
        }],
        usage,
    })
}

fn parse_gemini(model: &str, v: &Value) -> Result<ChatResponse, ExecutorError> {
    let candidates = v
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or_else(|| ExecutorError::InvalidResponse("missing candidates".into()))?;

    let text = candidates
        .iter()
        .filter_map(|c| {
            let parts = c.get("content")?.get("parts")?.as_array()?;
            let texts: Vec<&str> = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect();
            Some(texts)
        })
        .flatten()
        .collect::<Vec<_>>()
        .join("");

    let usage = v.get("usageMetadata").map(|u| Usage {
        prompt_tokens: u
            .get("promptTokenCount")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        completion_tokens: u
            .get("candidatesTokenCount")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        total_tokens: u
            .get("totalTokenCount")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    });

    Ok(ChatResponse {
        id: format!("gen-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".into(),
        created: chrono::Utc::now().timestamp(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: "assistant".into(),
                content: if text.is_empty() {
                    None
                } else {
                    Some(Content::Text(text))
                },
                name: None,
                tool_calls: {
                    let calls = crate::translator::ToolTranslator::parse_gemini_tool_calls(v);
                    if calls.is_empty() { None } else { Some(calls) }
                },
                tool_call_id: None,
            },
            finish_reason: Some(
                candidates[0]
                    .get("finishReason")
                    .and_then(Value::as_str)
                    .unwrap_or("stop")
                    .to_string(),
            ),
        }],
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openai() {
        let body = br#"{
            "id": "chatcmpl-1",
            "created": 123,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}
        }"#;
        let resp = parse_upstream_response(ApiFormat::OpenAi, "gpt-4o", body).unwrap();
        assert_eq!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "Hello!"
        );
        assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 7);
    }

    #[test]
    fn test_parse_claude() {
        let body = br#"{
            "id": "msg_1",
            "content": [{"type": "text", "text": "Hi from Claude"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 4, "output_tokens": 3}
        }"#;
        let resp = parse_upstream_response(ApiFormat::Claude, "claude-x", body).unwrap();
        assert_eq!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "Hi from Claude"
        );
        assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 7);
    }

    #[test]
    fn test_parse_gemini() {
        let body = br#"{
            "candidates": [{
                "content": {"parts": [{"text": "Hi from Gemini"}], "role": "model"},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5}
        }"#;
        let resp = parse_upstream_response(ApiFormat::Gemini, "gemini-x", body).unwrap();
        assert_eq!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "Hi from Gemini"
        );
        assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 5);
    }

    #[test]
    fn test_parse_invalid_json() {
        let err = parse_upstream_response(ApiFormat::OpenAi, "m", b"not json").unwrap_err();
        assert!(matches!(err, ExecutorError::InvalidResponse(_)));
    }
}
