use crate::chat::{ChatRequest, Content};
use crate::executor::{ApiFormat, ExecutorError};
use serde_json::{Value, json};

/// Convert a normalized `ChatRequest` into the provider's native wire format.
pub fn build_upstream_request(
    format: ApiFormat,
    req: &ChatRequest,
) -> Result<Value, ExecutorError> {
    match format {
        ApiFormat::OpenAi => build_openai(req),
        ApiFormat::Claude => build_claude(req),
        ApiFormat::Gemini => build_gemini(req),
    }
}

fn messages_to_openai(req: &ChatRequest) -> Vec<Value> {
    req.messages
        .iter()
        .map(|m| {
            let mut msg = json!({ "role": m.role });
            match &m.content {
                Some(Content::Text(t)) => {
                    msg["content"] = Value::String(t.clone());
                }
                Some(Content::Parts(parts)) => {
                    let arr: Vec<Value> = parts
                        .iter()
                        .map(|p| {
                            if let Some(url) = &p.image_url {
                                json!({
                                    "type": "image_url",
                                    "image_url": { "url": url.url }
                                })
                            } else if let Some(text) = &p.text {
                                json!({ "type": "text", "text": text })
                            } else {
                                Value::Null
                            }
                        })
                        .collect();
                    msg["content"] = Value::Array(arr);
                }
                None => {
                    msg["content"] = Value::Null;
                }
            }
            if let Some(tc) = &m.tool_calls {
                msg["tool_calls"] = serde_json::to_value(tc).unwrap_or_default();
            }
            if let Some(id) = &m.tool_call_id {
                msg["tool_call_id"] = Value::String(id.clone());
            }
            msg
        })
        .collect()
}

fn build_openai(req: &ChatRequest) -> Result<Value, ExecutorError> {
    let mut body = json!({
        "model": req.model,
        "messages": messages_to_openai(req),
    });
    if let Some(stream) = req.stream {
        body["stream"] = json!(stream);
    }
    if let Some(max_tokens) = req.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(top_p) = req.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(stop) = &req.stop {
        body["stop"] = json!(stop);
    }
    if let Some(tools) = &req.tools {
        body["tools"] = serde_json::to_value(tools).unwrap_or_default();
    }
    if let Some(tc) = &req.tool_choice {
        body["tool_choice"] = tc.clone();
    }
    Ok(body)
}

/// Claude Messages API wants system prompt separate from messages.
fn build_claude(req: &ChatRequest) -> Result<Value, ExecutorError> {
    let mut messages: Vec<Value> = Vec::new();
    let mut system: Option<String> = None;

    for m in &req.messages {
        if m.role == "system" {
            if let Some(Content::Text(t)) = &m.content {
                system = Some(t.clone());
            }
            continue;
        }
        let content = match &m.content {
            Some(Content::Text(t)) => Value::String(t.clone()),
            Some(Content::Parts(parts)) => Value::Array(
                parts
                    .iter()
                    .map(|p| {
                        if let Some(text) = &p.text {
                            json!({ "type": "text", "text": text })
                        } else {
                            Value::Null
                        }
                    })
                    .collect(),
            ),
            None => Value::Null,
        };
        messages.push(json!({ "role": m.role, "content": content }));
    }

    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens.unwrap_or(4096),
    });
    if let Some(s) = system {
        body["system"] = json!(s);
    }
    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(top_p) = req.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(stop) = &req.stop {
        body["stop_sequences"] = json!(stop);
    }
    if let Some(tools) = &req.tools {
        body["tools"] = serde_json::to_value(tools).unwrap_or_default();
    }
    Ok(body)
}

/// Gemini generateContent format.
fn build_gemini(req: &ChatRequest) -> Result<Value, ExecutorError> {
    let contents: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| {
            let text = match &m.content {
                Some(Content::Text(t)) => t.clone(),
                Some(Content::Parts(parts)) => parts
                    .iter()
                    .filter_map(|p| p.text.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
                None => String::new(),
            };
            json!({
                "role": if m.role == "assistant" { "model" } else { "user" },
                "parts": [{"text": text}]
            })
        })
        .collect();

    let mut body = json!({ "contents": contents });
    let mut gen_config = serde_json::Map::new();
    if let Some(temp) = req.temperature {
        gen_config.insert("temperature".into(), json!(temp));
    }
    if let Some(top_p) = req.top_p {
        gen_config.insert("topP".into(), json!(top_p));
    }
    if let Some(max_tokens) = req.max_tokens {
        gen_config.insert("maxOutputTokens".into(), json!(max_tokens));
    }
    if !gen_config.is_empty() {
        body["generationConfig"] = Value::Object(gen_config);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Message;

    fn req_with(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: "test-model".into(),
            messages,
            stream: Some(true),
            max_tokens: Some(100),
            temperature: Some(0.5),
            top_p: None,
            stop: Some(vec!["END".into()]),
            tools: None,
            tool_choice: None,
            extra: None,
        }
    }

    #[test]
    fn test_openai_builder_includes_params() {
        let body = build_upstream_request(
            ApiFormat::OpenAi,
            &req_with(vec![Message {
                role: "user".into(),
                content: Some(Content::Text("Hi".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }]),
        )
        .unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["stop"][0], "END");
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn test_claude_builder_moves_system_out() {
        let body = build_upstream_request(
            ApiFormat::Claude,
            &req_with(vec![
                Message {
                    role: "system".into(),
                    content: Some(Content::Text("Be concise".into())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: "user".into(),
                    content: Some(Content::Text("Hi".into())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ]),
        )
        .unwrap();
        assert_eq!(body["system"], "Be concise");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["max_tokens"], 100);
    }

    #[test]
    fn test_gemini_builder_roles() {
        let body = build_upstream_request(
            ApiFormat::Gemini,
            &req_with(vec![
                Message {
                    role: "system".into(),
                    content: Some(Content::Text("S".into())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: "user".into(),
                    content: Some(Content::Text("Hi".into())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ]),
        )
        .unwrap();
        // system filtered out
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
    }
}
