//! Local Ollama provider (`POST /api/chat`).
//!
//! No API key is required; the base URL defaults to `http://localhost:11434`.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::provider::AiProvider;
use crate::types::{
    AiError, AiMessage, AiMessageRole, AiProviderConfig, AiRequest, AiResponse, AiToolCall,
    AiUsage,
};

pub struct OllamaProvider {
    client: reqwest::Client,
    config: AiProviderConfig,
}

impl OllamaProvider {
    pub fn new(mut config: AiProviderConfig) -> Self {
        if config.base_url.trim().is_empty() {
            config.base_url = "http://localhost:11434".into();
        }
        let client = reqwest::Client::new();
        Self { client, config }
    }

    fn endpoint(&self) -> String {
        format!("{}/api/chat", self.config.base_url.trim_end_matches('/'))
    }

    fn wire_message(m: &AiMessage) -> Value {
        match m.role {
            AiMessageRole::System => json!({ "role": "system", "content": m.content }),
            AiMessageRole::User => json!({ "role": "user", "content": m.content }),
            AiMessageRole::Assistant => {
                let mut o = json!({ "role": "assistant", "content": m.content });
                if let Some(calls) = &m.tool_calls {
                    o["tool_calls"] = json!(calls
                        .iter()
                        .map(|c| json!({
                            "id": c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments }
                        }))
                        .collect::<Vec<_>>());
                }
                o
            }
            AiMessageRole::Tool => json!({
                "role": "tool",
                "content": m.content
            }),
        }
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    async fn chat(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        let mut body = json!({
            "model": request.model,
            "messages": request.messages.iter().map(Self::wire_message).collect::<Vec<_>>(),
            "stream": false,
        });
        if let Some(t) = request.temperature {
            body["options"] = json!({ "temperature": t });
        }
        if let Some(tools) = &request.tools {
            body["tools"] = json!(tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                }))
                .collect::<Vec<_>>());
        }

        let resp = self.client.post(self.endpoint()).json(&body).send().await.map_err(AiError::Network)?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(AiError::Network)?;
        if status >= 400 {
            return Err(AiError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        let v: Value = serde_json::from_slice(&bytes).map_err(AiError::Serde)?;

        let content = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str()).unwrap_or("").to_string();
        let tool_calls = v
            .get("message")
            .and_then(|m| m.get("tool_calls"))
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let id = tc.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let name = tc.get("function").and_then(|f| f.get("name")).and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let arguments = tc.get("function").and_then(|f| f.get("arguments")).cloned().unwrap_or(Value::Null);
                        if name.is_empty() {
                            None
                        } else {
                            Some(AiToolCall { id, name, arguments })
                        }
                    })
                    .collect()
            });

        let usage = AiUsage::new(
            v.get("prompt_eval_count").and_then(|x| x.as_u64()).unwrap_or(0),
            v.get("eval_count").and_then(|x| x.as_u64()).unwrap_or(0),
        );

        Ok(AiResponse {
            content,
            tool_calls,
            usage,
            model: request.model.clone(),
        })
    }
}
