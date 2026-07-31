use crate::chat::{ToolCall, ToolDef};
use crate::executor::ApiFormat;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Translates tool definitions and tool calls between provider formats.
///
/// OpenAI and Claude share a similar "function" model but differ in field
/// names (`parameters` vs `input_schema`, `function_call` vs `tool_use`).
/// Gemini uses `functionDeclarations` at the request level and
/// `functionCall` content parts in responses.
pub struct ToolTranslator;

impl ToolTranslator {
    /// Convert OpenAI-style tools into the target provider's tool format.
    /// Returns `None` when the target format needs no conversion (OpenAI).
    pub fn translate_tools(target: ApiFormat, tools: &[ToolDef]) -> Option<Value> {
        match target {
            ApiFormat::OpenAi => None,
            ApiFormat::Claude => {
                Some(json!(
                tools.iter().map(|t| {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters.clone().unwrap_or_else(|| json!({
                            "type": "object",
                            "properties": {},
                        })),
                    })
                }).collect::<Vec<_>>()
            ))
            }
            ApiFormat::Gemini => Some(json!({
                "functionDeclarations": tools.iter().map(|t| {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "parameters": t.function.parameters.clone().unwrap_or_else(|| json!({
                            "type": "object",
                            "properties": {},
                        })),
                    })
                }).collect::<Vec<_>>()
            })),
        }
    }

    /// Parse tool calls out of a Claude Messages response body.
    /// Returns a list of OpenAI-compatible tool calls.
    pub fn parse_claude_tool_calls(body: &Value) -> Vec<ToolCall> {
        body.get("content")
            .and_then(Value::as_array)
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                            return None;
                        }
                        let name = b
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let input = b.get("input").cloned().unwrap_or_else(|| json!({}));
                        let id = b
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("toolu_unknown")
                            .to_string();
                        Some(ToolCall {
                            id,
                            r#type: "function".into(),
                            function: crate::chat::ToolCallFunction {
                                name,
                                arguments: input.to_string(),
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Parse tool calls out of a Gemini response body.
    pub fn parse_gemini_tool_calls(body: &Value) -> Vec<ToolCall> {
        body.get("candidates")
            .and_then(Value::as_array)
            .map(|candidates| {
                candidates
                    .iter()
                    .filter_map(|c| c.get("content")?.get("parts")?.as_array())
                    .flatten()
                    .filter_map(|part| {
                        let call = part.get("functionCall")?;
                        let name = call
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                        Some(ToolCall {
                            id: format!("fcall_{}", uuid::Uuid::new_v4()),
                            r#type: "function".into(),
                            function: crate::chat::ToolCallFunction {
                                name,
                                arguments: args.to_string(),
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Build a normalized registry of known translation pairs.
    /// Currently a placeholder for future source→target translators.
    pub fn registry() -> HashMap<String, &'static str> {
        let mut m = HashMap::new();
        m.insert("openai:claude".into(), "openai-claude-tools");
        m.insert("openai:gemini".into(), "openai-gemini-tools");
        m.insert("claude:openai".into(), "claude-openai-tools");
        m.insert("gemini:openai".into(), "gemini-openai-tools");
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tools() -> Vec<ToolDef> {
        vec![ToolDef {
            r#type: "function".into(),
            function: crate::chat::ToolFunction {
                name: "get_weather".into(),
                description: Some("Get weather for a city".into()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string", "description": "City name"}
                    },
                    "required": ["city"]
                })),
            },
        }]
    }

    #[test]
    fn test_openai_tools_no_conversion() {
        assert!(ToolTranslator::translate_tools(ApiFormat::OpenAi, &sample_tools()).is_none());
    }

    #[test]
    fn test_claude_tools_format() {
        let tools = ToolTranslator::translate_tools(ApiFormat::Claude, &sample_tools()).unwrap();
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "Get weather for a city");
        assert!(tools[0].get("input_schema").is_some());
        assert!(tools[0].get("parameters").is_none());
    }

    #[test]
    fn test_gemini_tools_format() {
        let tools = ToolTranslator::translate_tools(ApiFormat::Gemini, &sample_tools()).unwrap();
        assert_eq!(tools["functionDeclarations"][0]["name"], "get_weather");
        assert!(tools["functionDeclarations"][0].get("parameters").is_some());
    }

    #[test]
    fn test_parse_claude_tool_use() {
        let body = json!({
            "content": [
                {"type": "text", "text": "Let me check"},
                {
                    "type": "tool_use",
                    "id": "toolu_01",
                    "name": "get_weather",
                    "input": {"city": "Jakarta"}
                }
            ]
        });
        let calls = ToolTranslator::parse_claude_tool_calls(&body);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "get_weather");
        assert_eq!(calls[0].id, "toolu_01");
        assert!(calls[0].function.arguments.contains("Jakarta"));
    }

    #[test]
    fn test_parse_claude_no_tools() {
        let body = json!({"content": [{"type": "text", "text": "hi"}]});
        assert!(ToolTranslator::parse_claude_tool_calls(&body).is_empty());
    }

    #[test]
    fn test_parse_gemini_function_call() {
        let body = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"functionCall": {"name": "get_weather", "args": {"city": "Bandung"}}}
                    ]
                }
            }]
        });
        let calls = ToolTranslator::parse_gemini_tool_calls(&body);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "get_weather");
        assert!(calls[0].function.arguments.contains("Bandung"));
    }

    #[test]
    fn test_registry_pairs() {
        let reg = ToolTranslator::registry();
        assert!(reg.contains_key("openai:claude"));
        assert!(reg.contains_key("gemini:openai"));
        assert_eq!(reg.len(), 4);
    }
}
