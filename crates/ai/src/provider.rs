//! The `AiProvider` trait — the single seam between the CMS and any LLM.

use async_trait::async_trait;

use crate::types::{AiError, AiProviderConfig, AiRequest, AiResponse};

/// A concrete LLM provider.
///
/// Implementations are stateless over the request: they translate an
/// `AiRequest` into the provider's wire format, call the HTTP endpoint, and
/// normalize the response back into an `AiResponse`. Trait is `Send` so it can
/// be used inside axum handlers (server-side only).
#[async_trait]
pub trait AiProvider: Send {
    /// Perform a (non-streaming) chat completion.
    async fn chat(&self, request: &AiRequest) -> Result<AiResponse, AiError>;
}

/// Build a provider from neutral configuration.
pub fn from_config(config: &AiProviderConfig) -> Result<Box<dyn AiProvider>, AiError> {
    crate::providers::registry::build(config)
}
