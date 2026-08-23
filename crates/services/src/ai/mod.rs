//! `services::ai` — the AI subsystem.
//!
//! Layers:
//! - `providers` — AI provider + model CRUD (persisted, keys encrypted).
//! - `chat` — assistant conversations + messages + the tool-calling loop.
//! - `tools` — RBAC-aware tool registry (the LLM is never the security boundary).
//! - `content` — AI content generation / editing / translation.
//! - `schema` — AI content-type / schema generation.
//! - `media` — AI media metadata.
//! - `usage` — AI usage + audit accounting.
//! - `security` — prompt-injection guard + mutation confirmation.
//!
//! Every operation runs under `AppContext` and enforces the same RBAC that the
//! rest of the CMS uses. The model only ever returns *typed tool requests*;
//! FerrisCMS authorizes and executes them.

pub mod chat;
pub mod content;
pub mod media;
pub mod providers;
pub mod schema;
pub mod security;
pub mod tools;
pub mod usage;

pub use chat::*;
pub use content::*;
pub use media::*;
pub use providers::*;
pub use schema::*;
pub use security::*;
pub use tools::*;
pub use usage::*;

use crate::ServiceError;

/// Resolve the `AiProviderKind` from the lowercase string stored on a provider row.
pub fn kind_from_str(kind: &str) -> Result<ai::AiProviderKind, ServiceError> {
    match kind {
        "openai" | "openai-compatible" | "openai_compatible" => Ok(ai::AiProviderKind::OpenAiCompatible),
        "ollama" => Ok(ai::AiProviderKind::Ollama),
        "anthropic" => Ok(ai::AiProviderKind::Anthropic),
        "gemini" => Ok(ai::AiProviderKind::Gemini),
        other => Err(ServiceError::internal(format!("unknown AI provider kind: {other}"))),
    }
}
