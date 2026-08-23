//! AI subsystem DTOs — the wire contract between api-rest and client-core.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderCreate {
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub organization: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    pub sort_order: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderUpdate {
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub organization: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    pub sort_order: Option<i64>,
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelCreate {
    pub provider_id: i64,
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub supports_chat: bool,
    #[serde(default)]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_streaming: bool,
    pub max_input_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelUpdate {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub supports_chat: bool,
    #[serde(default)]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_streaming: bool,
    pub max_input_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Chat / assistant
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConversationCreate {
    pub title: String,
    pub system_prompt: Option<String>,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
    /// Privacy mode: when true, conversation history is not sent to the
    /// provider (only the current message + system prompt are).
    #[serde(default)]
    pub privacy_mode: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSendMessage {
    pub text: String,
}

/// A tool call to confirm (for mutating actions). Mirrors `ai::AiToolCall`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfirmToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfirmBody {
    pub calls: Vec<AiConfirmToolCall>,
}

// ---------------------------------------------------------------------------
// Content generation / editing / translation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerateBody {
    pub uid: String,
    pub prompt: String,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub apply: bool,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEditBody {
    pub uid: String,
    pub document_id: String,
    pub instruction: String,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTranslateBody {
    pub text: String,
    pub target_locale: String,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

// ---------------------------------------------------------------------------
// Schema generation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSchemaGenerateBody {
    pub description: String,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSchemaApplyBody {
    pub schema: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Media metadata
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMediaAnalyzeBody {
    pub filename: String,
    pub mime: Option<String>,
    pub context: Option<String>,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}
