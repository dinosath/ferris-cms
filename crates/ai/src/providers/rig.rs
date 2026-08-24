//! Rig-backed provider — runs all LLM calls through [Rig](https://rig.rs).
//!
//! This replaces the hand-rolled HTTP providers with Rig's provider clients
//! and completion models. FerrisCMS still exposes the neutral `AiProvider`
//! trait; this implementation adapts our `AiRequest`/`AiResponse` to Rig's
//! `CompletionRequest`/`CompletionResponse` and dispatches each provider kind
//! to the matching Rig client.

use async_trait::async_trait;
use rig::client::{CompletionClient, ModelListingClient};
use rig::completion::message::{
    AssistantContent, Message, ProviderCallId, Text as RigText, ToolCall as RigToolCall,
    ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use rig::completion::{CompletionError, CompletionModel, CompletionRequest, CompletionResponse, ToolDefinition};
use rig::providers::{anthropic, gemini, ollama, openai};

use crate::provider::AiProvider;
use crate::types::{
    AiError, AiMessage, AiMessageRole, AiProviderConfig, AiProviderKind, AiRequest, AiResponse,
    AiTool, AiToolCall, AiUsage,
};

/// A Rig provider client. Each variant holds a concrete provider client; the
/// completion model is constructed per-request because Rig's completion models
/// are concrete (not object-safe) types.
pub enum RigClient {
    OpenAi(openai::client::CompletionsClient),
    Ollama(ollama::Client),
    Anthropic(anthropic::Client),
    Gemini(gemini::Client),
}

/// Build a Rig provider client for a neutral config.
fn build_rig_client(config: &AiProviderConfig) -> Result<RigClient, AiError> {
    let api_key = || config.api_key.clone().unwrap_or_default();
    match config.kind {
        AiProviderKind::OpenAiCompatible => {
            let c = openai::client::CompletionsClient::builder()
                .api_key(rig::client::BearerAuth::from(api_key()))
                .base_url(&config.base_url)
                .build()
                .map_err(|e| AiError::Request(format!("rig openai client: {e}")))?;
            Ok(RigClient::OpenAi(c))
        }
        AiProviderKind::Ollama => {
            let c = ollama::Client::builder()
                .api_key(ollama::OllamaApiKey::from(api_key()))
                .base_url(&config.base_url)
                .build()
                .map_err(|e| AiError::Request(format!("rig ollama client: {e}")))?;
            Ok(RigClient::Ollama(c))
        }
        AiProviderKind::Anthropic => {
            let c = anthropic::Client::builder()
                .api_key(anthropic::client::AnthropicKey::from(api_key()))
                .base_url(&config.base_url)
                .build()
                .map_err(|e| AiError::Request(format!("rig anthropic client: {e}")))?;
            Ok(RigClient::Anthropic(c))
        }
        AiProviderKind::Gemini => {
            let c = gemini::Client::builder()
                .api_key(gemini::client::GeminiApiKey::from(api_key()))
                .base_url(&config.base_url)
                .build()
                .map_err(|e| AiError::Request(format!("rig gemini client: {e}")))?;
            Ok(RigClient::Gemini(c))
        }
    }
}

/// A provider that runs completions through Rig.
pub struct RigProvider {
    client: RigClient,
}

impl RigProvider {
    /// Build a Rig-backed provider for a neutral config.
    pub fn new(config: &AiProviderConfig) -> Result<Self, AiError> {
        Ok(Self {
            client: build_rig_client(config)?,
        })
    }

    /// Test connectivity and list the models a provider advertises.
    ///
    /// This both verifies the provider is reachable (returns `Err` on failure)
    /// and, when it can, returns the model identifiers so the CMS can
    /// auto-populate models instead of requiring manual entry.
    pub async fn list_models(config: &AiProviderConfig) -> Result<Vec<String>, AiError> {
        let client = build_rig_client(config)?;
        let models = match &client {
            RigClient::OpenAi(c) => c.list_models().await,
            RigClient::Ollama(c) => c.list_models().await,
            RigClient::Anthropic(c) => c.list_models().await,
            RigClient::Gemini(c) => c.list_models().await,
        }
        .map_err(|e| AiError::Request(format!("failed to connect / list models: {e}")))?;
        Ok(models.iter().map(|m| m.id.clone()).collect())
    }
}

/// Test connectivity and list the models a provider advertises.
pub async fn list_provider_models(config: &AiProviderConfig) -> Result<Vec<String>, AiError> {
    RigProvider::list_models(config).await
}

#[async_trait]
impl AiProvider for RigProvider {
    async fn chat(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        let rig_request = build_request(request)?;
        let response = match &self.client {
            RigClient::OpenAi(c) => c
                .completion_model(request.model.clone())
                .completion(rig_request)
                .await,
            RigClient::Ollama(c) => c
                .completion_model(request.model.clone())
                .completion(rig_request)
                .await,
            RigClient::Anthropic(c) => c
                .completion_model(request.model.clone())
                .completion(rig_request)
                .await,
            RigClient::Gemini(c) => c
                .completion_model(request.model.clone())
                .completion(rig_request)
                .await,
        }
        .map_err(map_error)?;
        Ok(map_response(response, &request.model))
    }
}

/// Convert a neutral `AiRequest` into a Rig `CompletionRequest`.
fn build_request(request: &AiRequest) -> Result<CompletionRequest, AiError> {
    let chat_history = request
        .messages
        .iter()
        .map(map_message)
        .collect::<Result<Vec<_>, _>>()?;
    let tools = request
        .tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(map_tool)
        .collect();
    Ok(CompletionRequest {
        model: Some(request.model.clone()),
        preamble: request.system.clone(),
        chat_history,
        documents: Vec::new(),
        tools,
        temperature: request.temperature.map(|t| t as f64),
        max_tokens: request.max_tokens.map(|m| m as u64),
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    })
}

/// Convert a neutral `AiMessage` into a Rig `Message`.
fn map_message(m: &AiMessage) -> Result<Message, AiError> {
    match m.role {
        AiMessageRole::System => Ok(Message::System {
            content: m.content.clone(),
        }),
        AiMessageRole::User => Ok(Message::User {
            content: vec![UserContent::Text(RigText::new(m.content.clone()))],
        }),
        AiMessageRole::Assistant => {
            let mut content: Vec<AssistantContent> = Vec::new();
            if !m.content.is_empty() {
                content.push(AssistantContent::Text(RigText::new(m.content.clone())));
            }
            if let Some(calls) = &m.tool_calls {
                for c in calls {
                    let id = ToolCallId::new(c.id.clone()).unwrap_or_else(ToolCallId::mint);
                    let function = ToolFunction::new(c.name.clone(), c.arguments.clone());
                    content.push(AssistantContent::ToolCall(RigToolCall::new(id, function)));
                }
            }
            Ok(Message::Assistant { id: None, content })
        }
        AiMessageRole::Tool => {
            let call_id = m.tool_call_id.clone().ok_or_else(|| {
                AiError::Request("tool message is missing a tool_call_id".to_string())
            })?;
            let id = ToolCallId::new(call_id.clone()).unwrap_or_else(ToolCallId::mint);
            let result = ToolResult {
                call: id,
                provider: ProviderCallId::new(call_id),
                name: m.name.clone().unwrap_or_default(),
                content: vec![ToolResultContent::Text(RigText::new(m.content.clone()))],
            };
            Ok(Message::User {
                content: vec![UserContent::ToolResult(result)],
            })
        }
    }
}

/// Convert a neutral `AiTool` into a Rig `ToolDefinition`.
fn map_tool(t: &AiTool) -> ToolDefinition {
    ToolDefinition {
        name: t.name.clone(),
        description: t.description.clone(),
        parameters: t.parameters.clone(),
    }
}

/// Convert a Rig `CompletionResponse` into a neutral `AiResponse`.
fn map_response(r: CompletionResponse, fallback_model: &str) -> AiResponse {
    let mut content = String::new();
    let mut tool_calls: Vec<AiToolCall> = Vec::new();
    for item in r.choice {
        match item {
            AssistantContent::Text(t) => {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(&t.text);
            }
            AssistantContent::ToolCall(tc) => {
                tool_calls.push(AiToolCall {
                    id: tc.id.as_str().to_string(),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                });
            }
            AssistantContent::Reasoning(_) | AssistantContent::Image(_) => {}
        }
    }
    AiResponse {
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        usage: AiUsage::new(r.usage.input_tokens, r.usage.output_tokens),
        model: r.model.unwrap_or_else(|| fallback_model.to_string()),
    }
}

/// Map a Rig completion error into the neutral `AiError`.
fn map_error(e: CompletionError) -> AiError {
    AiError::Request(format!("rig completion failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AiMessage, AiRequest, AiTool};

    #[test]
    fn builds_request_with_system_and_tools() {
        let mut req = AiRequest::new("gpt-4o-mini");
        req.system = Some("Be terse.".into());
        req.messages.push(AiMessage::user("hello"));
        let built = build_request(&req).unwrap();
        assert_eq!(built.preamble.as_deref(), Some("Be terse."));
        assert_eq!(built.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(built.chat_history.len(), 1);
    }

    #[test]
    fn maps_tool_definitions() {
        let mut req = AiRequest::new("x");
        req.tools = Some(vec![AiTool {
            name: "content.list".into(),
            description: "list".into(),
            parameters: serde_json::json!({"type":"object"}),
        }]);
        let built = build_request(&req).unwrap();
        assert_eq!(built.tools.len(), 1);
        assert_eq!(built.tools[0].name, "content.list");
    }

    #[test]
    fn maps_text_response() {
        let resp = CompletionResponse::new(
            vec![AssistantContent::Text(RigText::new("hi"))],
            rig::completion::Usage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                ..Default::default()
            },
            "openai",
        );
        let out = map_response(resp, "fallback");
        assert_eq!(out.content, "hi");
        assert_eq!(out.usage.total_tokens, 3);
        assert_eq!(out.model, "fallback");
    }
}
