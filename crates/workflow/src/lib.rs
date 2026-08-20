//! workflow — the workflow automation domain + engine logic.
//!
//! This crate contains everything about workflows that is **pure** (no
//! database, no HTTP, no I/O):
//!
//! - `model` — the stable, serializable workflow/execution/credential domain.
//! - `node` — the node registry + static node definitions (metadata only).
//! - `expression` — the n8n-style, safely-evaluated expression engine.
//! - `validation` — structural validation of a workflow graph.
//! - `graph` — topological ordering + reachability helpers used by the executor.
//!
//! Runtime *execution* (running node logic against the CMS database, sending
//! HTTP requests, persisting executions) lives in `services` under the
//! `workflow` submodule. Keeping the logic here means the domain can be tested
//! and versioned independently, and new node types only need a definition
//! (here) plus an executor (in `services`) — never an edit to the editor.

pub mod expression;
pub mod graph;
pub mod model;
pub mod node;
pub mod validation;

pub use expression::{evaluate as eval_template, Context, ExpressionError};
pub use model::*;
pub use node::{NodeCategory, NodeDefinition, NodeField, NodeRegistry, REGISTRY};
pub use validation::validate;

/// Default node registry (cached).
pub fn registry() -> &'static NodeRegistry {
    &REGISTRY
}

/// Validate a workflow against the built-in registry.
pub fn validate_workflow(workflow: &Workflow) -> WorkflowValidation {
    validation::validate(workflow, registry())
}
