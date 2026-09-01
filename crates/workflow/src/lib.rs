//! workflow — the Open Workflow Specification (OWS) domain + engine logic.
//!
//! This crate contains everything about workflows that is **pure** (no
//! database, no HTTP, no I/O):
//!
//! - `model` — the canonical OWS document (wrapper around the official
//!   `serverless_workflow_core` [`WorkflowDefinition`]) + execution/credential
//!   domain.
//! - `node` — the function/task catalog used by the editor (metadata only).
//! - `expression` — the n8n-style, safely-evaluated expression engine.
//! - `validation` — structural validation of an OWS document.
//! - `graph` — task ordering helpers used by the executor.
//!
//! Runtime *execution* (running OWS tasks against the CMS database, sending
//! HTTP requests, persisting executions) lives in `services` under the
//! `workflow` submodule.

pub mod expression;
pub mod graph;
pub mod model;
pub mod node;
pub mod validation;

pub use expression::{evaluate as eval_template, Context, ExpressionError};
pub use model::*;
pub use node::{NodeCategory, NodeDefinition, NodeField, NodeRegistry, REGISTRY};
pub use validation::validate;

/// Default function/task catalog (cached).
pub fn registry() -> &'static NodeRegistry {
    &REGISTRY
}

/// Validate an OWS document against the built-in catalog.
pub fn validate_workflow(doc: &OwsDocument) -> OwsValidation {
    validation::validate(doc, registry())
}
