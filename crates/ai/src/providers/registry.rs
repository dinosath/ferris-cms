//! Provider registry — builds a concrete `AiProvider` from neutral config.
//!
//! All provider families are dispatched through the Rig-backed provider
//! (`super::rig::RigProvider`); CMS functionality does not change.

use crate::provider::AiProvider;
use crate::types::{AiError, AiProviderConfig, AiProviderKind};

use super::rig::RigProvider;

/// Build a provider from neutral configuration.
pub fn build(config: &AiProviderConfig) -> Result<Box<dyn AiProvider>, AiError> {
    Ok(Box::new(RigProvider::new(config)?))
}

/// Human-readable label for a provider kind (used by the UI).
pub fn kind_label(kind: AiProviderKind) -> &'static str {
    match kind {
        AiProviderKind::OpenAiCompatible => "OpenAI-compatible",
        AiProviderKind::Ollama => "Ollama (local)",
        AiProviderKind::Anthropic => "Anthropic",
        AiProviderKind::Gemini => "Google Gemini",
    }
}
