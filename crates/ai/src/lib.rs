//! `ai` — provider-agnostic AI core for FerrisCMS.
//!
//! The CMS never depends on a specific LLM provider. This crate defines the
//! neutral types (`AiRequest`, `AiResponse`, `AiMessage`, tools, usage) and the
//! `AiProvider` trait, plus concrete providers (OpenAI-compatible, Ollama,
//! Anthropic, Gemini) selected by a small registry.
//!
//! The LLM is **never the security boundary**: the CMS resolves authorization,
//! executes tools, validates, and persists — this crate only talks to the model.

pub mod provider;
pub mod providers;
pub mod types;

pub use provider::{from_config, AiProvider};
pub use providers::rig::{list_provider_models, RigProvider};
pub use providers::registry;
pub use types::*;
