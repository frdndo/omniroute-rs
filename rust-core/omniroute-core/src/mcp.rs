use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

/// M6: Model Context Protocol server (JSON-RPC 2.0 over HTTP POST).
///
/// Exposes the router to MCP clients (Claude Desktop, Cursor, etc.) via
/// three tools: `chat`, `list_models`, `server_status`.
/// Transport: MCP "Streamable HTTP" non-streaming responses.
pub struct Mcp;

const PROTOCOL_VERSION: &str = "2025-03-26";

fn rpc_ok(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_err(id: Option<Value>, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

pub fn tools_list() -> Value {
    json!([
        tool(
            "chat",
            "Route a chat completion through the smart provider router (fallback, scoring, session affinity)",
            json!({
                "type": "object",
                "properties": {
                    "model": { "type": "string", "description": "Model id or combo name (e.g. gpt-4o)" },
                    "messages": { "type": "array", "description": "Chat messages [{role, content}]" },
                    "temperature": { "type": "number" },
                    "session_id": { "type": "string", "description": "Optional session id for affinity" }
                },
                "required": ["model", "messages"]
            }),
        ),
        tool(
            "list_models",
            "List all models the router can serve",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "server_status",
            "Router health: version, uptime, active providers, request totals",
            json!({ "type": "object", "properties": {} }),
        ),
    ])
}

impl Mcp {
    /// Handle a single JSON-RPC request envelope (or a batch).
    pub async fn handle(req: &Value, state: &crate::proxy::AppState) -> Value {
        if let Some(batch) = req.as_array() {
            let mut out = Vec::with_capacity(batch.len());
            for m in batch {
                out.push(Self::handle_single(m, state).await);
            }
            return json!(out);
        }
        Self::handle_single(req, state).await
    }

    async fn handle_single(req: &Value, state: &crate::proxy::AppState) -> Value {
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

        match method {
            "initialize" => {
                let client = params
                    .get("clientInfo")
                    .and_then(|c| c.get("name"))
                    .cloned();
                tracing::info!("MCP client initialized: {:?}", client);
                rpc_ok(
                    id,
                    json!({
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": "omniroute-rs", "version": env!("CARGO_PKG_VERSION") },
                    }),
                )
            }
            "notifications/initialized" | "notifications/cancelled" => rpc_ok(id, json!(null)),
            "ping" => rpc_ok(id, json!({})),
            "tools/list" => rpc_ok(
                id,
                json!({ "tools": Self::tools_list_as_value(), "nextCursor": null }),
            ),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match Self::call_tool(name, &args, state).await {
                    Ok(result) => rpc_ok(
                        id,
                        json!({ "content": [{ "type": "text", "text": result }] }),
                    ),
                    Err(e) => rpc_err(id, -32602, e),
                }
            }
            _ => rpc_err(id, -32601, format!("method not found: {method}")),
        }
    }

    fn tools_list_as_value() -> Value {
        // tools_list() is a module-level helper returning a Value array
        tools_list().as_array().cloned().unwrap_or_default().into()
    }

    async fn call_tool(
        name: &str,
        args: &Value,
        state: &crate::proxy::AppState,
    ) -> Result<String, String> {
        match name {
            "list_models" => {
                let providers = omniroute_providers::list_providers();
                let models: Vec<String> = providers
                    .iter()
                    .flat_map(|p| p.models.iter().map(|m| m.id.clone()))
                    .collect();
                let total = models.len();
                Ok(format!("Total models: {total}\n{}", models.join("\n")))
            }
            "server_status" => {
                let stats = crate::telemetry::TELEMETRY.stats();
                Ok(serde_json::to_string_pretty(&stats).unwrap_or_default())
            }
            "chat" => {
                let model = args
                    .get("model")
                    .and_then(|m| m.as_str())
                    .ok_or("model required")?;
                let msgs = args
                    .get("messages")
                    .and_then(|m| m.as_array())
                    .ok_or("messages required")?;
                let messages: Vec<crate::chat::Message> = msgs
                    .iter()
                    .filter_map(|m| {
                        let role = m.get("role").and_then(|r| r.as_str())?;
                        let content = m.get("content").and_then(|c| c.as_str())?;
                        Some(crate::chat::Message {
                            role: role.to_string(),
                            content: Some(crate::chat::Content::Text(content.to_string())),
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                        })
                    })
                    .collect();
                if messages.is_empty() {
                    return Err("messages must have role+content".into());
                }
                let req = crate::chat::ChatRequest {
                    model: model.to_string(),
                    messages,
                    stream: Some(false),
                    max_tokens: args.get("max_tokens").and_then(|v| v.as_i64()),
                    temperature: args.get("temperature").and_then(|v| v.as_f64()),
                    top_p: None,
                    stop: None,
                    tools: None,
                    tool_choice: None,
                    extra: None,
                    cache: false,
                    cache_ttl: None,
                    compress: false,
                    max_context_tokens: None,
                };
                let session_id = args.get("session_id").and_then(|s| s.as_str());

                let mut combo = state.combo.write().await;
                let result = combo.execute(&req, session_id, None).await;
                drop(combo);
                match result {
                    Ok(r) => {
                        let content = r
                            .response
                            .choices
                            .first()
                            .and_then(|c| c.message.content.clone())
                            .map(|c| c.to_string())
                            .unwrap_or_default();
                        let usage = r
                            .response
                            .usage
                            .map(|u| {
                                format!(
                                    " [prompt={} completion={}]",
                                    u.prompt_tokens, u.completion_tokens
                                )
                            })
                            .unwrap_or_default();
                        Ok(format!(
                            "({} via {}) {}{}",
                            r.used_model, r.used_provider, content, usage
                        ))
                    }
                    Err(e) => Err(format!("routing failed: {e}")),
                }
            }
            _ => Err(format!("tool not found: {name}")),
        }
    }
}

/// axum handler for POST /mcp.
pub async fn handle_mcp(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    Ok(Json(Mcp::handle(&body, &state).await))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_state() -> crate::proxy::AppState {
        // Minimal state for protocol-level tests (chat tool needs runtime+combo,
        // so those are covered by e2e instead).
        let combo = crate::combo::ComboEngine::new(crate::router::RoutingEngine::new(
            crate::router::RouterConfig::default(),
        ));
        let combo = std::sync::Arc::new(tokio::sync::RwLock::new(combo));
        crate::proxy::AppState {
            started_at: chrono::Utc::now(),
            version: "test".into(),
            combo,
            gateway_keys: std::sync::Arc::new(std::sync::RwLock::new(
                crate::auth::GatewayKeys::new(vec!["sk-test".into()]),
            )),
            allowed_hosts: crate::auth::AllowedHosts::new(vec!["localhost".into()]),
            admin_keys: crate::admin::AdminKeys::new(vec!["sk-admin".into()]),
            db_path: ":memory:".into(),
            db: None,
        }
    }

    #[tokio::test]
    async fn test_initialize() {
        let state = dummy_state();
        let resp = Mcp::handle(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "test" } } }),
            &state,
        )
        .await;
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(resp["result"]["serverInfo"]["name"], "omniroute-rs");
        assert_eq!(resp["id"], 1);
    }

    #[tokio::test]
    async fn test_tools_list() {
        let state = dummy_state();
        let resp = Mcp::handle(
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
            &state,
        )
        .await;
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "chat"));
        assert!(tools.iter().any(|t| t["name"] == "list_models"));
        assert!(tools.iter().any(|t| t["name"] == "server_status"));
    }

    #[tokio::test]
    async fn test_unknown_method_and_batch() {
        let state = dummy_state();
        let resp = Mcp::handle(
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" }),
            &state,
        )
        .await;
        assert_eq!(resp["error"]["code"], -32601);

        let batch = Mcp::handle(
            &json!([
                { "jsonrpc": "2.0", "id": 4, "method": "ping" },
                { "jsonrpc": "2.0", "id": 5, "method": "tools/list", "params": {} }
            ]),
            &state,
        )
        .await;
        assert_eq!(batch.as_array().unwrap().len(), 2);
        assert_eq!(batch[1]["result"]["tools"].as_array().unwrap().len(), 3);
    }
}
