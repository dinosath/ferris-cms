//! `ai` — provider-agnostic AI core for FerrisCMS.
//!
//! The CMS never depends on a specific LLM provider. This crate defines the
//! neutral types (`AiRequest`, `AiResponse`, `AiMessage`, tools, usage) and the
//! `AiProvider` trait, plus concrete providers (OpenAI-compatible, Ollama,
//! Anthropic, Gemini) selected by a small registry.
//!
//! The LLM is **never the security boundary**: the CMS resolves authorization,
//! executes tools, validates, and persists — this crate only talks to the model.

use std::sync::Once;

/// Install a process-wide rustls crypto provider (ring) exactly once.
///
/// rig's HTTP client uses reqwest with `rustls-no-provider`, so a provider must
/// be installed before the first TLS connection. `ring` is used instead of the
/// otherwise-default `aws-lc-rs` to keep the binary small.
pub fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub mod provider;
pub mod providers;
pub mod types;

pub use provider::{from_config, AiProvider};
pub use providers::registry;
pub use providers::rig::{list_provider_models, RigProvider};
pub use types::*;
