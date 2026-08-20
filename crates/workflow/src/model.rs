//! Workflow domain model (workflow domain layer).
//!
//! Pure, serializable types describing workflows, their nodes, connections,
//! executions, node runs and credentials. This is the stable wire + storage
//! format: the frontend canvas is *not* the source of truth — these types are.
//!
//! Node *metadata* (definitions) lives in `crate::node`; the actual runtime
//! execution (running node logic against the database) lives in `services`.
//! This file only describes *state*.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// How a node should react when it fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    /// Abort the whole execution immediately.
    #[default]
    Stop,
    /// Log the error but continue executing downstream nodes.
    Continue,
    /// Send the error to a dedicated error-output port/connection.
    Route,
}

/// A position on the infinite canvas.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// A reference from a node to a saved credential.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCredentialRef {
    /// The credential input name this node expects (e.g. `httpApi`).
    pub name: String,
    /// The saved credential entity id.
    pub credential_id: i64,
}

/// A node in a workflow graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    /// Unique id within the workflow (uuid).
    pub id: String,
    /// Node type key, e.g. `manualTrigger`, `httpRequest`, `if`, `getContent`.
    pub node_type: String,
    /// Display name shown on the canvas.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub position: Position,
    /// Node configuration (values for the node's config schema).
    #[serde(default)]
    pub parameters: IndexMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<NodeCredentialRef>,
    #[serde(default)]
    pub on_error: OnError,
    /// Output port name to route errors to when `on_error == Route`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_output: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl WorkflowNode {
    /// Read a string parameter with an optional default.
    pub fn param_str(&self, key: &str) -> Option<String> {
        self.parameters
            .get(key)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                serde_json::Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
    }

    pub fn param_bool(&self, key: &str) -> Option<bool> {
        self.parameters.get(key).and_then(|v| v.as_bool())
    }

    pub fn param_i64(&self, key: &str) -> Option<i64> {
        self.parameters
            .get(key)
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
    }
}

/// A directed edge between two node ports.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    /// Unique id within the workflow.
    pub id: String,
    /// Source node id.
    pub from: String,
    /// Source node output port name (`main`, `true`, `false`, ...).
    pub from_output: String,
    /// Target node id.
    pub to: String,
    /// Target node input port name (`main`, `error`, ...).
    pub to_input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Connection {
    /// The canonical key used for graph adjacency.
    pub fn source_key(&self) -> (String, String) {
        (self.from.clone(), self.from_output.clone())
    }
}

/// Execution ordering policy (n8n `v1`/`v2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionOrder {
    #[default]
    V1,
    V2,
}

/// Workflow-level settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSettings {
    #[serde(default)]
    pub execution_order: ExecutionOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub save_execution_progress: bool,
}

impl Default for WorkflowSettings {
    fn default() -> Self {
        Self {
            execution_order: ExecutionOrder::default(),
            timeout_secs: None,
            save_execution_progress: true,
        }
    }
}

/// A complete, persisted workflow definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub id: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: i64,
    pub active: bool,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub settings: WorkflowSettings,
    /// Workflow-level variables available to expressions (`$workflow.variables.x`).
    #[serde(default)]
    pub variables: IndexMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Workflow {
    pub fn node(&self, id: &str) -> Option<&WorkflowNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn trigger_nodes(&self) -> Vec<&WorkflowNode> {
        self.nodes.iter().filter(|n| is_trigger_type(&n.node_type)).collect()
    }

    /// All connections leaving a node's output port.
    pub fn outgoing(&self, node_id: &str, port: &str) -> Vec<&Connection> {
        self.connections
            .iter()
            .filter(|c| c.from == node_id && c.from_output == port)
            .collect()
    }

    /// All connections entering a node's input port.
    pub fn incoming(&self, node_id: &str) -> Vec<&Connection> {
        self.connections
            .iter()
            .filter(|c| c.to == node_id)
            .collect()
    }
}

/// Execution status (persisted + shown in the UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    Running,
    Success,
    Failed,
    Waiting,
    Cancelled,
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Waiting => "waiting",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Cancelled)
    }
}

/// Per-node run status (overlaid on the canvas).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeRunStatus {
    NotExecuted,
    Running,
    Success,
    Failed,
    Skipped,
    Waiting,
}

impl NodeRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotExecuted => "notExecuted",
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Waiting => "waiting",
        }
    }
}

/// The public execution view. Stored as a row + node-run rows in `services`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub id: i64,
    pub workflow_id: i64,
    pub status: ExecutionStatus,
    /// manual | trigger | schedule | webhook
    pub mode: String,
    /// The trigger that started it (event name / node name).
    pub trigger: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One node run within an execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRun {
    pub id: i64,
    pub execution_id: i64,
    pub node_id: String,
    pub node_name: String,
    pub node_type: String,
    pub status: NodeRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Input data captured at execution time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    /// Output data captured at execution time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub attempts: i64,
    /// Deterministic topological execution order index.
    pub order: i64,
}

/// A saved credential (an encrypted blob is stored in `services`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: i64,
    pub name: String,
    /// Credential type key, e.g. `httpBasicAuth`, `httpHeaderAuth`, `postgres`.
    pub credential_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Validation result for a workflow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowValidation {
    pub valid: bool,
    #[serde(default)]
    pub errors: Vec<ValidationIssue>,
}

/// A single validation issue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    /// e.g. `missing_trigger`, `unconnected_node`, `unknown_node_type`,
    /// `missing_required_param`, `unknown_connection_target`.
    pub code: String,
    pub message: String,
}

/// Check whether a node type is a trigger (has no `main` input).
pub fn is_trigger_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "manualTrigger"
            | "scheduleTrigger"
            | "webhookTrigger"
            | "httpTrigger"
            | "contentCreated"
            | "contentUpdated"
            | "contentPublished"
            | "contentDeleted"
            | "mediaUploaded"
            | "userCreated"
            | "workflowTrigger"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_helpers() {
        let now = chrono::Utc::now();
        let mut wf = Workflow {
            id: 1,
            name: "Demo".into(),
            description: None,
            version: 1,
            active: false,
            nodes: vec![
                WorkflowNode {
                    id: "a".into(),
                    node_type: "manualTrigger".into(),
                    name: "Manual".into(),
                    description: None,
                    position: Position { x: 0.0, y: 0.0 },
                    parameters: IndexMap::new(),
                    disabled: false,
                    notes: None,
                    credentials: vec![],
                    on_error: OnError::default(),
                    error_output: None,
                },
                WorkflowNode {
                    id: "b".into(),
                    node_type: "noop".into(),
                    name: "Noop".into(),
                    description: None,
                    position: Position { x: 200.0, y: 0.0 },
                    parameters: IndexMap::new(),
                    disabled: false,
                    notes: None,
                    credentials: vec![],
                    on_error: OnError::default(),
                    error_output: None,
                },
            ],
            connections: vec![Connection {
                id: "c1".into(),
                from: "a".into(),
                from_output: "main".into(),
                to: "b".into(),
                to_input: "main".into(),
                label: None,
            }],
            settings: WorkflowSettings::default(),
            variables: IndexMap::new(),
            tags: vec![],
            created_at: now,
            updated_at: now,
        };

        assert_eq!(wf.trigger_nodes().len(), 1);
        assert_eq!(wf.node("b").unwrap().name, "Noop");
        assert_eq!(wf.outgoing("a", "main").len(), 1);
        assert_eq!(wf.incoming("b").len(), 1);
        assert!(is_trigger_type("manualTrigger"));
        assert!(!is_trigger_type("noop"));

        // JSON round-trip preserves structure (stable format).
        let v = serde_json::to_value(&wf).unwrap();
        let back: Workflow = serde_json::from_value(v).unwrap();
        assert_eq!(wf, back);
    }

    #[test]
    fn status_helpers() {
        assert_eq!(ExecutionStatus::Running.as_str(), "running");
        assert!(ExecutionStatus::Success.is_terminal());
        assert!(ExecutionStatus::Failed.is_terminal());
        assert!(ExecutionStatus::Cancelled.is_terminal());
        assert!(!ExecutionStatus::Waiting.is_terminal());
        assert_eq!(NodeRunStatus::Skipped.as_str(), "skipped");
    }
}
