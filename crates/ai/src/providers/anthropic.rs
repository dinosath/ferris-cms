//! Anthropic Messages API provider (`/v1/messages`).

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::provider::AiProvider;
use crate::types::{
    AiError, AiMessage, AiMessageRole, AiProviderConfig, AiRequest, AiResponse, AiTool,
    AiToolCall, AiUsage,
};

pub struct AnthropicProvider {
    client: reqwest::Client,
    config: AiProviderConfig,
}

impl AnthropicProvider {
    pub fn new(config: AiProviderConfig) -> Self {
        let client = reqwest::Client::new();
        Self { client, config }
    }

    fn endpoint(&self) -> String {
        format!("{}/messages", self.config.base_url.trim_end_matches('/'))
    }

    fn wire_message(m: &AiMessage) -> Value {
        match m.role {
            AiMessageRole::User => json!({ "role": "user", "content": m.content }),
            AiMessageRole::Assistant => {
                let mut o = json!({ "role": "assistant", "content": m.content });
                if let Some(calls) = &m.tool_calls {
                    o["content"] = json!(calls
                        .iter()
                        .map(|c| json!({
                            "type": "tool_use",
                            "id": c.id,
                            "name": c.name,
                            "input": c.arguments
                        }))
                        .collect::<Vec<_>>());
                }
                o
            }
            AiMessageRole::Tool => json!({
                "role": "user",
                "content": vec![json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.as_deref().unwrap_or(""),
                    "content": m.content
                })]
            }),
            AiMessageRole::System => json!({ "role": "user", "content": m.content }),
        }
    }

    fn wire_tool(t: &AiTool) -> Value {
        json!({
            "name": t.name,
            "description": t.description,
            "input_schema": t.parameters
        })
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    async fn chat(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        // Anthropic wants system as a top-level field, not a message.
        let system = request.system.clone().unwrap_or_default();
        let messages: Vec<Value> = request
            .messages
            .iter()
            .filter(|m| m.role != AiMessageRole::System)
            .map(Self::wire_message)
            .collect();

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(1024),
            "messages": messages,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if let Some(t) = request.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(tools) = &request.tools {
            body["tools"] = json!(tools.iter().map(Self::wire_tool).collect::<Vec<_>>());
        }

        let mut req = self.client.post(self.endpoint()).json(&body).header("anthropic-version", "2023-06-01");
        if let Some(key) = &self.config.api_key {
            req = req.header("x-api-key", key);
        }

        let resp = req.send().await.map_err(AiError::Network)?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(AiError::Network)?;
        if status >= 400 {
            return Err(AiError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        let v: Value = serde_json::from_slice(&bytes).map_err(AiError::Serde)?;

        // Extract text blocks + tool_use blocks from `content`.
        let mut content = String::new();
        let mut tool_calls: Vec<AiToolCall> = Vec::new();
        if let Some(blocks) = v.get("content").and_then(|c| c.as_array()) {
            for b in blocks {
                match b.get("type").and_then(|x| x.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                            content.push_str(t);
                        }
                    }
                    Some("tool_use") => {
                        let id = b.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let arguments = b.get("input").cloned().unwrap_or(Value::Null);
                        if !name.is_empty() {
                            tool_calls.push(AiToolCall { id, name, arguments });
                        }
                    }
                    _ => {}
                }
            }
        }

        let usage = v.get("usage").map(|u| {
            AiUsage::new(
                u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            )
        }).unwrap_or_default();

        Ok(AiResponse {
            content,
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            usage,
            model: request.model.clone(),
        })
    }
}
