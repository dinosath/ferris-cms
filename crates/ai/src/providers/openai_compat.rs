//! OpenAI / OpenAI-compatible provider (chat completions).
//!
//! Works against OpenAI and any compatible endpoint (OpenRouter, Together,
//! vLLM, LM Studio, ...) — the base URL is configurable.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::provider::AiProvider;
use crate::types::{
    AiError, AiMessage, AiMessageRole, AiProviderConfig, AiRequest, AiResponse, AiToolCall,
    AiUsage,
};

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    config: AiProviderConfig,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: AiProviderConfig) -> Self {
        let client = reqwest::Client::new();
        Self { client, config }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'))
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
                            "function": { "name": c.name, "arguments": c.arguments.to_string() }
                        }))
                        .collect::<Vec<_>>());
                }
                o
            }
            AiMessageRole::Tool => json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.as_deref().unwrap_or(""),
                "content": m.content
            }),
        }
    }
}

#[async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    async fn chat(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        let mut body = json!({
            "model": request.model,
            "messages": request.messages.iter().map(Self::wire_message).collect::<Vec<_>>(),
        });
        if let Some(t) = request.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = request.max_tokens {
            body["max_tokens"] = json!(m);
        }
        if let Some(tools) = &request.tools {
            body["tools"] = json!(tools
                .iter()
                .map(|t| json!({ "type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.parameters
                }}))
                .collect::<Vec<_>>());
        }

        let mut req = self
            .client
            .post(self.endpoint())
            .json(&body);
        if let Some(key) = &self.config.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        if let Some(org) = &self.config.organization {
            req = req.header("OpenAI-Organization", org);
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

        let message = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .ok_or_else(|| AiError::InvalidResponse("missing choices[0].message".into()))?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let tool_calls = message.get("tool_calls").and_then(|t| t.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    let id = tc.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|x| x.as_str())
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(Value::Null);
                    if name.is_empty() {
                        None
                    } else {
                        Some(AiToolCall { id, name, arguments })
                    }
                })
                .collect()
        });

        let usage = v
            .get("usage")
            .map(|u| {
                AiUsage::new(
                    u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                    u.get("completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                )
            })
            .unwrap_or_default();

        Ok(AiResponse {
            content,
            tool_calls,
            usage,
            model: request.model.clone(),
        })
    }
}
