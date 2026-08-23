//! Google Gemini provider (`generateContent`).

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::provider::AiProvider;
use crate::types::{
    AiError, AiMessage, AiMessageRole, AiProviderConfig, AiRequest, AiResponse, AiTool,
    AiToolCall, AiUsage,
};

pub struct GeminiProvider {
    client: reqwest::Client,
    config: AiProviderConfig,
}

impl GeminiProvider {
    pub fn new(mut config: AiProviderConfig) -> Self {
        if config.base_url.trim().is_empty() {
            config.base_url = "https://generativelanguage.googleapis.com".into();
        }
        let client = reqwest::Client::new();
        Self { client, config }
    }

    fn wire_part(m: &AiMessage) -> Value {
        // Map assistant/system -> "model", user/tool -> "user".
        let role = match m.role {
            AiMessageRole::User | AiMessageRole::Tool => "user",
            _ => "model",
        };
        if let Some(calls) = &m.tool_calls {
            json!({
                "role": role,
                "parts": calls.iter().map(|c| json!({
                    "functionCall": { "name": c.name, "args": c.arguments }
                })).collect::<Vec<_>>()
            })
        } else {
            json!({ "role": role, "parts": [ { "text": m.content } ] })
        }
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    async fn chat(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        let mut body = json!({
            "contents": request.messages
                .iter()
                .filter(|m| m.role != AiMessageRole::System)
                .map(Self::wire_part)
                .collect::<Vec<_>>(),
        });
        if let Some(s) = &request.system {
            if !s.is_empty() {
                body["systemInstruction"] = json!({ "parts": [ { "text": s } ] });
            }
        }
        let mut gen = serde_json::Map::new();
        if let Some(t) = request.temperature {
            gen.insert("temperature".into(), json!(t));
        }
        if let Some(m) = request.max_tokens {
            gen.insert("maxOutputTokens".into(), json!(m));
        }
        if !gen.is_empty() {
            body["generationConfig"] = Value::Object(gen);
        }
        if let Some(tools) = &request.tools {
            body["tools"] = json!(tools
                .iter()
                .map(|t: &AiTool| json!({ "functionDeclarations": [ {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                }] }))
                .collect::<Vec<_>>());
        }

        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.config.base_url.trim_end_matches('/'),
            request.model
        );
        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.config.api_key {
            req = req.query(&[("key", key)]);
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

        let mut content = String::new();
        let mut tool_calls: Vec<AiToolCall> = Vec::new();
        if let Some(parts) = v
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            for part in parts {
                if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                    content.push_str(t);
                }
                if let Some(fc) = part.get("functionCall") {
                    let name = fc.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let args = fc.get("args").cloned().unwrap_or(Value::Null);
                    if !name.is_empty() {
                        tool_calls.push(AiToolCall {
                            id: format!("gemini-{name}"),
                            name,
                            arguments: args,
                        });
                    }
                }
            }
        }

        let usage = v.get("usageMetadata").map(|u| {
            AiUsage::new(
                u.get("promptTokenCount").and_then(|x| x.as_u64()).unwrap_or(0),
                u.get("candidatesTokenCount").and_then(|x| x.as_u64()).unwrap_or(0),
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
