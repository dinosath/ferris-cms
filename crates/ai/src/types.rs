//! Provider-agnostic AI types (design: "AI Core").
//!
//! FerrisCMS never depends on a specific provider implementation. Everything
//! below is the neutral wire model that all providers speak. The CMS maps its
//! own domain types to these and back; provider HTTP details live in
//! `providers/`.

use serde::{Deserialize, Serialize};

/// Supported provider families. Additional providers are added by extending
/// this enum and registering a builder in `providers::registry` — the CMS
/// functionality itself never changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderKind {
    /// OpenAI-compatible chat-completions API (covers OpenAI + any compatible
    /// hosted/self-hosted endpoint).
    #[serde(rename = "openai")]
    OpenAiCompatible,
    /// Local Ollama server (`/api/chat`).
    Ollama,
    /// Anthropic Messages API (`/v1/messages`).
    Anthropic,
    /// Google Gemini generateContent API.
    Gemini,
}

/// Roles for chat messages. `System` is only used on the wire; the CMS keeps
/// system prompts out of user-visible conversation history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A single chat message. Tool/assistant messages may carry `tool_calls` and
/// `tool_call_id` for function calling.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: AiMessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl AiMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: AiMessageRole::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: AiMessageRole::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: AiMessageRole::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_tool_calls(calls: Vec<AiToolCall>) -> Self {
        Self {
            role: AiMessageRole::Assistant,
            content: String::new(),
            tool_calls: Some(calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool(name: impl Into<String>, call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: AiMessageRole::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
            name: Some(name.into()),
        }
    }
}

/// A tool the model may request. The CMS defines tools as typed JSON-schema
/// definitions and executes them server-side under RBAC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiTool {
    /// Unique tool name (e.g. `content.list`).
    pub name: String,
    /// Human-readable description for the model.
    pub description: String,
    /// JSON-schema object for the parameters.
    pub parameters: serde_json::Value,
}

/// A concrete tool invocation requested by the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// The result of executing a tool, returned to the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiToolResult {
    /// Matches the `id` of the corresponding `AiToolCall`.
    pub call_id: String,
    pub name: String,
    pub content: String,
}

/// Token/usage accounting returned by a provider.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

impl AiUsage {
    pub fn new(input: u64, output: u64) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
        }
    }
}

/// A request sent to a provider: the model name, a temperature, optional
/// system prompt, the message history, and the tools the model may call.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiRequest {
    pub model: String,
    pub messages: Vec<AiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AiTool>>,
}

impl AiRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: Vec::new(),
            system: None,
            temperature: None,
            max_tokens: None,
            tools: None,
        }
    }
}

/// A provider response: the assistant text plus any requested tool calls.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AiToolCall>>,
    pub usage: AiUsage,
    /// Provider-specific model that actually produced the response.
    pub model: String,
}

/// A single streaming event (used when `stream` is enabled). The provider
/// yields text deltas and, at the end, usage + any tool calls.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiStreamEvent {
    /// Text delta produced so far (empty for non-text events).
    pub delta: String,
    /// Set once at the end of the stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AiUsage>,
    /// Tool calls discovered during streaming (OpenAI-compatible only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AiToolCall>>,
    /// True when the stream is complete.
    pub done: bool,
}

/// Provider-agnostic errors surfaced by the `ai` crate.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("provider not configured: {0}")]
    NotConfigured(String),
    #[error("provider request failed: {0}")]
    Request(String),
    #[error("provider returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("provider returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("unsupported provider kind: {0}")]
    Unsupported(String),
    #[error("empty completion")]
    EmptyCompletion,
}

/// Provider configuration passed to a concrete provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiProviderConfig {
    /// Provider family.
    pub kind: AiProviderKind,
    /// Base URL (e.g. `https://api.openai.com/v1`, `http://localhost:11434`).
    pub base_url: String,
    /// API key (may be empty for local providers such as Ollama).
    pub api_key: Option<String>,
    /// Optional organization/extra header value (OpenAI-compatible).
    pub organization: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_roles_and_serde() {
        let u = AiMessage::user("hello");
        assert_eq!(u.role, AiMessageRole::User);
        let s = serde_json::to_string(&u).unwrap();
        assert!(s.contains("\"role\":\"user\""));
        let back: AiMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(back.content, "hello");
    }

    #[test]
    fn provider_kind_serde_lowercase() {
        assert_eq!(
            serde_json::to_string(&AiProviderKind::OpenAiCompatible).unwrap(),
            "\"openai\""
        );
        assert_eq!(
            serde_json::from_str::<AiProviderKind>("\"ollama\"").unwrap(),
            AiProviderKind::Ollama
        );
    }

    #[test]
    fn usage_sums_tokens() {
        let u = AiUsage::new(10, 20);
        assert_eq!(u.total_tokens, 30);
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 20);
    }

    #[test]
    fn tool_call_roundtrip() {
        let call = AiToolCall {
            id: "c1".into(),
            name: "content.list".into(),
            arguments: serde_json::json!({"uid": "api::x.x"}),
        };
        let s = serde_json::to_string(&call).unwrap();
        assert!(s.contains("\"content.list\""));
        let back: AiToolCall = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, "c1");
    }
}
