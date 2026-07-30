use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Chat Request ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", flatten)]
    pub extra: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl std::fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Content::Text(t) => write!(f, "{}", t),
            Content::Parts(parts) => {
                for p in parts {
                    if let Some(text) = &p.text {
                        write!(f, "{}", text)?;
                    }
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub r#type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

// ── Chat Response ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: usize,
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

// ── SSE Chunk ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: usize,
    pub delta: Delta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaToolCall {
    pub index: usize,
    pub id: String,
    pub r#type: String,
    pub function: DeltaFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaFunction {
    pub name: String,
    pub arguments: String,
}

impl ChatRequest {
    pub fn is_streaming(&self) -> bool {
        self.stream.unwrap_or(false)
    }

    pub fn last_user_message(&self) -> Option<&Message> {
        self.messages.iter().rev().find(|m| m.role == "user")
    }

    pub fn system_message(&self) -> Option<&Message> {
        self.messages.iter().find(|m| m.role == "system")
    }
}

impl ChatResponse {
    pub fn new(model: &str, content: &str, usage: Option<Usage>) -> Self {
        let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
        Self {
            id,
            object: "chat.completion".into(),
            created: chrono::Utc::now().timestamp(),
            model: model.into(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".into(),
                    content: Some(Content::Text(content.into())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage,
        }
    }
}

impl SseChunk {
    pub fn delta(model: &str, content: &str, index: usize, finish: Option<&str>) -> Self {
        Self {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            object: "chat.completion.chunk".into(),
            created: chrono::Utc::now().timestamp(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index,
                delta: Delta {
                    role: None,
                    content: Some(content.into()),
                    tool_calls: None,
                },
                finish_reason: finish.map(String::from),
            }],
        }
    }

    pub fn to_sse_line(&self) -> String {
        format!(
            "data: {}\n\n",
            serde_json::to_string(self).unwrap_or_default()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_request_deserialize() {
        let json = r#"{
            "model": "gpt-4o",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true,
            "temperature": 0.7
        }"#;
        let req: ChatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert!(req.is_streaming());
    }

    #[test]
    fn test_chat_request_system_message() {
        let json = r#"{
            "model": "claude-sonnet-4",
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hi"}
            ]
        }"#;
        let req: ChatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.system_message()
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "You are helpful"
        );
    }

    #[test]
    fn test_chat_response_new() {
        let resp = ChatResponse::new(
            "gpt-4o",
            "Hello!",
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        );
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 15);
    }

    #[test]
    fn test_sse_chunk_to_line() {
        let chunk = SseChunk::delta("gpt-4o", "Hello", 0, Some("stop"));
        let line = chunk.to_sse_line();
        assert!(line.starts_with("data: "));
        assert!(line.contains("Hello"));
    }

    #[test]
    fn test_tool_calls() {
        let json = r#"{
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"loc\": \"Jakarta\"}"
                        }
                    }]
                }
            ]
        }"#;
        let req: ChatRequest = serde_json::from_str(json).unwrap();
        let tool_calls = req.messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls[0].function.name, "get_weather");
    }

    #[test]
    fn test_system_message_not_found() {
        let req: ChatRequest =
            serde_json::from_str(r#"{"model":"x","messages":[{"role":"user","content":"Hi"}]}"#)
                .unwrap();
        assert!(req.system_message().is_none());
    }

    #[test]
    fn test_last_user_message() {
        let req: ChatRequest = serde_json::from_str(
            r#"{"model":"x","messages":[
            {"role":"system","content":"S"},
            {"role":"user","content":"U1"},
            {"role":"assistant","content":"A"},
            {"role":"user","content":"U2"}
        ]}"#,
        )
        .unwrap();
        assert_eq!(
            req.last_user_message()
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .to_string(),
            "U2"
        );
    }
}
