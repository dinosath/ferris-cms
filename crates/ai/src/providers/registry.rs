//! Provider registry — builds a concrete `AiProvider` from neutral config.
//!
//! New providers register here; CMS functionality does not change.

use crate::provider::AiProvider;
use crate::types::{AiError, AiProviderConfig, AiProviderKind};

use super::anthropic::AnthropicProvider;
use super::gemini::GeminiProvider;
use super::ollama::OllamaProvider;
use super::openai_compat::OpenAiCompatibleProvider;

/// Build a provider from neutral configuration.
pub fn build(config: &AiProviderConfig) -> Result<Box<dyn AiProvider>, AiError> {
    match config.kind {
        AiProviderKind::OpenAiCompatible => {
            Ok(Box::new(OpenAiCompatibleProvider::new(config.clone())))
        }
        AiProviderKind::Ollama => Ok(Box::new(OllamaProvider::new(config.clone()))),
        AiProviderKind::Anthropic => Ok(Box::new(AnthropicProvider::new(config.clone()))),
        AiProviderKind::Gemini => Ok(Box::new(GeminiProvider::new(config.clone()))),
    }
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
