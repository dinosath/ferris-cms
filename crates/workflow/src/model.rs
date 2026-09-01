//! OWS — the Open Workflow Specification (canonical workflow model).
//!
//! This module defines the sole, canonical representation of a workflow in
//! FerrisCMS. It is a wrapper around the official
//! [`serverless_workflow_core`](https://github.com/open-workflow-specification/sdk-rust)
//! [`WorkflowDefinition`], which implements the CNCF **Open Workflow DSL** (the
//! Serverless Workflow specification).
//!
//! The legacy "custom workflow" model (flat `nodes`/`connections` with ad-hoc
//! trigger nodes) has been removed. In OWS:
//!
//! - **Tasks** are the executable units, declared as a named, ordered map in
//!   `definition.do` (`call`, `set`, `switch`, `for`, `fork`, `do`, `wait`,
//!   `try`, `emit`, `listen`, `raise`, `run`).
//! - **Flow** is expressed with `then` transitions instead of visual edges.
//! - **Triggers / event routing** are OWS events (`schedule.on`, `listen`).
//! - **Functions** (reusable callables) live in `definition.use.functions`.
//! - **Credentials / secrets** are declared in `definition.use.secrets` and
//!   referenced by functions via OWS `authentication` policies.
//! - **Error handling** is `try`/`catch` tasks and `errors`/`retries`.
//!
//! Static task/function *metadata* (definitions for the editor) lives in
//! `crate::node`; the runtime execution lives in `services`. This file only
//! describes *state* and wraps the SDK definition with app-level metadata.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serverless_workflow_core::models::workflow::WorkflowDefinition;

/// The OWS event types understood by the CMS trigger dispatcher
/// (values of `EventFilterDefinition::with.type` / `data.type`).
pub const OWS_TRIGGER_EVENTS: &[&str] = &[
    "content.created",
    "content.updated",
    "content.published",
    "content.deleted",
    "media.uploaded",
    "user.created",
    "webhook",
    "manual",
    "schedule",
    "workflow",
];

/// Whether an OWS event `type` is a known CMS trigger event.
pub fn is_trigger_event(event_type: &str) -> bool {
    OWS_TRIGGER_EVENTS.contains(&event_type)
}

/// A complete, persisted OWS workflow: app-level metadata plus the canonical
/// Open Workflow DSL definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwsDocument {
    pub id: i64,
    pub active: bool,
    pub version: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// The canonical OWS definition (Open Workflow DSL).
    pub definition: WorkflowDefinition,
}

impl OwsDocument {
    pub fn name(&self) -> &str {
        &self.definition.document.name
    }

    pub fn title(&self) -> Option<&str> {
        self.definition.document.title.as_deref()
    }

    pub fn summary(&self) -> Option<&str> {
        self.definition.document.summary.as_deref()
    }

    pub fn description(&self) -> Option<String> {
        self.definition
            .document
            .summary
            .clone()
            .or_else(|| self.definition.document.title.clone())
    }

    pub fn tags(&self) -> Vec<String> {
        self.definition
            .document
            .tags
            .as_ref()
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// The number of top-level tasks in the workflow.
    pub fn task_count(&self) -> usize {
        self.definition.do_.entries.len()
    }

    /// Names of the top-level tasks, in declaration order.
    pub fn task_names(&self) -> Vec<String> {
        self.definition
            .do_
            .entries
            .iter()
            .filter_map(|e| e.keys().next().cloned())
            .collect()
    }

    /// Whether the workflow is scheduled (`schedule.every`/`cron`/`after`).
    pub fn is_scheduled(&self) -> bool {
        self.definition
            .schedule
            .as_ref()
            .map(|s| s.every.is_some() || s.cron.is_some() || s.after.is_some())
            .unwrap_or(false)
    }

    /// Cron expression, if any.
    pub fn cron(&self) -> Option<String> {
        self.definition.schedule.as_ref().and_then(|s| s.cron.clone())
    }
}

// ---------------------------------------------------------------------------
// Execution status & logs (OWS runtime)
// ---------------------------------------------------------------------------

/// Execution status (persisted + shown in the UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OwsExecutionStatus {
    Running,
    Success,
    Failed,
    Waiting,
    Cancelled,
}

impl OwsExecutionStatus {
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

/// Per-task run status (overlaid on the inspector).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OwsTaskRunStatus {
    NotExecuted,
    Running,
    Success,
    Failed,
    Skipped,
    Waiting,
}

impl OwsTaskRunStatus {
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

/// The public execution view. Stored as a row + task-run rows in `services`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwsExecution {
    pub id: i64,
    pub workflow_id: i64,
    pub status: OwsExecutionStatus,
    /// manual | trigger | schedule | webhook
    pub mode: String,
    /// The trigger that started it (event name / task name).
    pub trigger: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One task run within an execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwsTaskRun {
    pub id: i64,
    pub execution_id: i64,
    pub task_name: String,
    pub task_type: String,
    pub status: OwsTaskRunStatus,
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
    /// Deterministic execution order index.
    pub order: i64,
}

/// A saved credential (an encrypted blob is stored in `services`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwsCredential {
    pub id: i64,
    pub name: String,
    /// Credential type key, e.g. `httpBasicAuth`, `httpHeaderAuth`, `postgres`.
    pub credential_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Validation result for an OWS document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwsValidation {
    pub valid: bool,
    #[serde(default)]
    pub errors: Vec<OwsValidationIssue>,
}

/// A single validation issue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwsValidationIssue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
    /// e.g. `missing_trigger`, `unknown_task_reference`, `unknown_function`,
    /// `missing_required_config`, `unknown_event`.
    pub code: String,
    pub message: String,
}

/// OWS task type names (for the executor + editor).
pub mod task_type {
    pub const CALL: &str = "call";
    pub const DO: &str = "do";
    pub const EMIT: &str = "emit";
    pub const FOR: &str = "for";
    pub const FORK: &str = "fork";
    pub const LISTEN: &str = "listen";
    pub const RAISE: &str = "raise";
    pub const RUN: &str = "run";
    pub const SET: &str = "set";
    pub const SWITCH: &str = "switch";
    pub const TRY: &str = "try";
    pub const WAIT: &str = "wait";
}

/// OWS function names (values of `CallTaskDefinition::call`) understood by the
/// FerrisCMS runtime. These map to the executor implementations in `services`.
pub mod function {
    // CMS content / media functions.
    pub const GET_CONTENT: &str = "cms.getContent";
    pub const FIND_CONTENT: &str = "cms.findContent";
    pub const QUERY_CONTENT: &str = "cms.queryContent";
    pub const CREATE_CONTENT: &str = "cms.createContent";
    pub const UPDATE_CONTENT: &str = "cms.updateContent";
    pub const DELETE_CONTENT: &str = "cms.deleteContent";
    pub const PUBLISH_CONTENT: &str = "cms.publishContent";
    pub const UNPUBLISH_CONTENT: &str = "cms.unpublishContent";
    pub const GET_MEDIA: &str = "cms.getMedia";
    pub const UPLOAD_MEDIA: &str = "cms.uploadMedia";

    // Data transformation functions.
    pub const TRANSFORM_DATA: &str = "cms.transformData";
    pub const JSON: &str = "data.json";
    pub const CSV: &str = "data.csv";
    pub const TRANSFORM: &str = "core.transform";
    pub const CODE: &str = "core.code";
    pub const EDIT_FIELDS: &str = "core.editFields";

    // Integration functions.
    pub const HTTP_REQUEST: &str = "http.request";
    pub const WEBHOOK: &str = "http.webhook";
    pub const GRAPHQL: &str = "http.graphql";
    pub const REST_API: &str = "http.rest";
    pub const DB_QUERY: &str = "db.query";
    pub const POSTGRES: &str = "db.postgres";
    pub const REDIS: &str = "db.redis";

    /// All functions the FerrisCMS runtime can execute.
    pub const ALL: &[&str] = &[
        GET_CONTENT,
        FIND_CONTENT,
        QUERY_CONTENT,
        CREATE_CONTENT,
        UPDATE_CONTENT,
        DELETE_CONTENT,
        PUBLISH_CONTENT,
        UNPUBLISH_CONTENT,
        GET_MEDIA,
        UPLOAD_MEDIA,
        TRANSFORM_DATA,
        JSON,
        CSV,
        TRANSFORM,
        CODE,
        EDIT_FIELDS,
        HTTP_REQUEST,
        WEBHOOK,
        GRAPHQL,
        REST_API,
        DB_QUERY,
        POSTGRES,
        REDIS,
    ];

    pub fn is_known(name: &str) -> bool {
        ALL.contains(&name)
    }
}

/// Workflow-level variables used to seed the runtime context (`$context`).
pub fn default_context(definition: &WorkflowDefinition) -> serde_json::Value {
    serde_json::Value::Object(
        definition
            .metadata
            .clone()
            .map(|m| m.into_iter().collect())
            .unwrap_or_default(),
    )
}

/// An ordered view of the workflow's named tasks.
pub fn task_entries(
    definition: &WorkflowDefinition,
) -> Vec<(String, &serverless_workflow_core::models::task::TaskDefinition)> {
    definition
        .do_
        .entries
        .iter()
        .filter_map(|e| {
            e.iter()
                .next()
                .map(|(name, task)| (name.clone(), task))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serverless_workflow_core::models::workflow::WorkflowDefinitionMetadata;

    #[test]
    fn wraps_definition_and_helpers() {
        let metadata = WorkflowDefinitionMetadata::new(
            "default",
            "demo",
            "1.0.0",
            Some("Demo".to_string()),
            Some("A demo".to_string()),
            None,
        );
        let mut definition = WorkflowDefinition::new(metadata);
        definition.do_.add(
            "first".to_string(),
            serverless_workflow_core::models::task::TaskDefinition::Set(
                serverless_workflow_core::models::task::SetTaskDefinition::new(),
            ),
        );

        let doc = OwsDocument {
            id: 1,
            active: false,
            version: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            definition,
        };
        assert_eq!(doc.name(), "demo");
        assert_eq!(doc.title(), Some("Demo"));
        assert_eq!(doc.task_count(), 1);
        assert_eq!(doc.task_names(), vec!["first".to_string()]);
        assert!(!doc.is_scheduled());
        assert!(is_trigger_event("content.created"));
        assert!(!is_trigger_event("core.set"));
        assert!(function::is_known(function::HTTP_REQUEST));
        assert!(!function::is_known("nope"));

        // JSON round-trip preserves the OWS document.
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(v["definition"]["document"]["name"], "demo");
        let back: OwsDocument = serde_json::from_value(v).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn status_helpers() {
        assert_eq!(OwsExecutionStatus::Running.as_str(), "running");
        assert!(OwsExecutionStatus::Success.is_terminal());
        assert!(OwsExecutionStatus::Failed.is_terminal());
        assert!(OwsExecutionStatus::Cancelled.is_terminal());
        assert!(!OwsExecutionStatus::Waiting.is_terminal());
        assert_eq!(OwsTaskRunStatus::Skipped.as_str(), "skipped");
    }
}
