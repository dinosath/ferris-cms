//! Workflow services (OWS persistence, activation, import/export, execution).
//!
//! This is the `services` layer for the OWS workflow automation engine. It
//! depends on the pure `workflow` crate for the canonical OWS domain model
//! (the Open Workflow DSL), the function catalog, expression engine and
//! validation, and wires them to the database, the CMS (`dynamic-store` /
//! content services) and external HTTP integrations.

pub mod credentials;
pub mod engine;
pub mod executors;
pub mod runtime_fn;
pub mod triggers;

pub use credentials::*;
pub use engine::*;
pub use executors::*;
pub use runtime_fn::*;
pub use triggers::*;

use crate::{AppContext, ServiceError};
use db::entities::{workflow, workflow_execution};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::Serialize;
use ::workflow::model::OwsDocument;

/// Workflow permission actions (integrated into the existing RBAC matrix).
pub mod action {
    pub const VIEW: &str = "plugin::workflow.workflow.read";
    pub const CREATE: &str = "plugin::workflow.workflow.create";
    pub const UPDATE: &str = "plugin::workflow.workflow.update";
    pub const DELETE: &str = "plugin::workflow.workflow.delete";
    pub const EXECUTE: &str = "plugin::workflow.workflow.execute";
    pub const ACTIVATE: &str = "plugin::workflow.workflow.activate";
    pub const VIEW_EXECUTIONS: &str = "plugin::workflow.execution.read";
    pub const VIEW_CREDENTIALS: &str = "plugin::workflow.credential.read";
    pub const MANAGE_CREDENTIALS: &str = "plugin::workflow.credential.manage";

    pub const SUBJECT_WORKFLOW: &str = "plugin::workflow.workflow";

    pub const ALL: &[&str] = &[
        VIEW,
        CREATE,
        UPDATE,
        DELETE,
        EXECUTE,
        ACTIVATE,
        VIEW_EXECUTIONS,
        VIEW_CREDENTIALS,
        MANAGE_CREDENTIALS,
    ];
}

/// A lightweight execution summary used on the workflow list screen.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub id: i64,
    pub status: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<&workflow_execution::Model> for ExecutionSummary {
    fn from(e: &workflow_execution::Model) -> Self {
        Self {
            id: e.id,
            status: e.status.clone(),
            started_at: e.started_at,
            finished_at: e.finished_at,
            duration_ms: e.duration_ms,
            error: e.error.clone(),
        }
    }
}

/// Summary row for the Workflows list screen.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummary {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: i64,
    pub active: bool,
    pub task_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    pub execution_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_execution: Option<ExecutionSummary>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn model_to_workflow(m: &workflow::Model) -> Result<OwsDocument, ServiceError> {
    serde_json::from_value(m.definition_json.clone())
        .map_err(|e| ServiceError::internal(format!("corrupt OWS definition: {e}")))
}

/// Build a fresh, empty OWS document for a new workflow.
pub fn new_empty_document(
    name: &str,
    description: Option<&str>,
    version: i64,
    active: bool,
) -> OwsDocument {
    use serverless_workflow_core::models::workflow::{WorkflowDefinition, WorkflowDefinitionMetadata};
    let now = chrono::Utc::now();
    let metadata = WorkflowDefinitionMetadata::new(
        "default",
        name,
        &version.to_string(),
        None,
        description.map(|s| s.to_string()),
        None,
    );
    OwsDocument {
        id: 0,
        active,
        version,
        created_at: now,
        updated_at: now,
        definition: WorkflowDefinition::new(metadata),
    }
}

/// A human-readable trigger label for a workflow (first schedule event type,
/// else "manual").
pub fn trigger_label(doc: &OwsDocument) -> Option<String> {
    use ::workflow::model::is_trigger_event;
    let mut labels = Vec::new();
    if let Some(schedule) = &doc.definition.schedule {
        if let Some(on) = &schedule.on {
            for f in on
                .all
                .iter()
                .flat_map(|v| v.iter())
                .chain(on.any.iter().flat_map(|v| v.iter()))
                .chain(on.one.iter())
            {
                if let Some(ty) = f.with.as_ref().and_then(|w| w.get("type")).and_then(|v| v.as_str()) {
                    if is_trigger_event(ty) {
                        labels.push(ty.to_string());
                    }
                }
            }
        }
    }
    if labels.is_empty() {
        labels.push("manual".to_string());
    }
    Some(labels.join(", "))
}

/// Enforce a workflow permission for the current user.
pub async fn enforce(ctx: &AppContext, perm: &str) -> Result<(), ServiceError> {
    crate::rbac::enforce_action(&ctx.db, ctx.current_user.as_ref(), perm, action::SUBJECT_WORKFLOW)
        .await
}

/// List workflows (optionally filtered by name/status).
pub async fn workflow_list(
    ctx: &AppContext,
    name_filter: Option<&str>,
    active_only: Option<bool>,
) -> Result<Vec<WorkflowSummary>, ServiceError> {
    enforce(ctx, action::VIEW).await?;
    let mut query = workflow::Entity::find().order_by_desc(workflow::Column::UpdatedAt);
    if let Some(name) = name_filter {
        if !name.trim().is_empty() {
            query = query.filter(workflow::Column::Name.contains(name.trim()));
        }
    }
    if let Some(active) = active_only {
        query = query.filter(workflow::Column::Active.eq(active));
    }
    let rows = query.all(&ctx.db).await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let def = model_to_workflow(row)?;
        let execution_count = workflow_execution::Entity::find()
            .filter(workflow_execution::Column::WorkflowId.eq(row.id))
            .count(&ctx.db)
            .await?;
        let last = workflow_execution::Entity::find()
            .filter(workflow_execution::Column::WorkflowId.eq(row.id))
            .order_by_desc(workflow_execution::Column::StartedAt)
            .one(&ctx.db)
            .await?;
        result.push(WorkflowSummary {
            id: row.id,
            name: def.name().to_string(),
            description: def.description(),
            version: row.version,
            active: row.active,
            task_count: def.task_count(),
            trigger: trigger_label(&def),
            execution_count,
            last_execution: last.as_ref().map(ExecutionSummary::from),
            created_at: row.created_at,
            updated_at: row.updated_at,
        });
    }
    Ok(result)
}

/// Get a full OWS workflow definition by id.
pub async fn workflow_get(ctx: &AppContext, id: i64) -> Result<OwsDocument, ServiceError> {
    enforce(ctx, action::VIEW).await?;
    let row = workflow::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("workflow not found"))?;
    model_to_workflow(&row)
}

/// Load a workflow definition without permission checks (executor path).
pub async fn workflow_load(ctx: &AppContext, id: i64) -> Result<OwsDocument, ServiceError> {
    let row = workflow::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("workflow not found"))?;
    model_to_workflow(&row)
}

/// Create a new workflow from a definition.
pub async fn workflow_create(
    ctx: &AppContext,
    name: &str,
    description: Option<&str>,
    def: Option<&OwsDocument>,
) -> Result<OwsDocument, ServiceError> {
    enforce(ctx, action::CREATE).await?;
    let now = chrono::Utc::now();
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let mut workflow = def
        .cloned()
        .unwrap_or_else(|| new_empty_document(name, description, 1, false));
    workflow.id = 0;
    workflow.version = 1;
    workflow.active = false;
    workflow.created_at = now;
    workflow.updated_at = now;
    // Ensure the OWS document name matches.
    workflow.definition.document.name = name.to_string();
    if description.is_some() {
        workflow.definition.document.summary = description.map(|s| s.to_string());
    }

    let row = workflow::ActiveModel {
        name: Set(name.to_string()),
        description: Set(workflow.definition.document.summary.clone()),
        version: Set(workflow.version),
        active: Set(workflow.active),
        definition_json: Set(
            serde_json::to_value(&workflow)
                .map_err(|e| ServiceError::internal(format!("workflow serialize: {e}")))?,
        ),
        created_at: Set(now),
        updated_at: Set(now),
        created_by: Set(user_id),
        updated_by: Set(user_id),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    workflow.id = row.id;
    Ok(workflow)
}

/// Save (create or update) an OWS workflow definition, bumping its version.
pub async fn workflow_save(
    ctx: &AppContext,
    id: Option<i64>,
    def: &OwsDocument,
) -> Result<OwsDocument, ServiceError> {
    let now = chrono::Utc::now();
    let user_id = ctx.current_user.as_ref().map(|u| u.id);

    let row = match id {
        Some(id) => {
            enforce(ctx, action::UPDATE).await?;
            let existing = workflow::Entity::find_by_id(id)
                .one(&ctx.db)
                .await?
                .ok_or_else(|| ServiceError::not_found("workflow not found"))?;
            let version = existing.version + 1;
            let mut am: workflow::ActiveModel = existing.into();
            let mut new_def = def.clone();
            new_def.id = id;
            new_def.version = version;
            new_def.updated_at = now;
            am.name = Set(new_def.name().to_string());
            am.description = Set(new_def.definition.document.summary.clone());
            am.version = Set(version);
            am.active = Set(new_def.active);
            am.definition_json = Set(
                serde_json::to_value(&new_def)
                    .map_err(|e| ServiceError::internal(format!("workflow serialize: {e}")))?,
            );
            am.updated_at = Set(now);
            am.updated_by = Set(user_id);
            am.update(&ctx.db).await?
        }
        None => {
            enforce(ctx, action::CREATE).await?;
            let mut new_def = def.clone();
            new_def.id = 0;
            new_def.version = 1;
            new_def.created_at = now;
            new_def.updated_at = now;
            workflow::ActiveModel {
                name: Set(new_def.name().to_string()),
                description: Set(new_def.definition.document.summary.clone()),
                version: Set(new_def.version),
                active: Set(new_def.active),
                definition_json: Set(
                    serde_json::to_value(&new_def)
                        .map_err(|e| ServiceError::internal(format!("workflow serialize: {e}")))?,
                ),
                created_at: Set(now),
                updated_at: Set(now),
                created_by: Set(user_id),
                updated_by: Set(user_id),
                ..Default::default()
            }
            .insert(&ctx.db)
            .await?
        }
    };
    let mut saved = model_to_workflow(&row)?;
    saved.id = row.id;
    Ok(saved)
}

/// Delete a workflow (and its executions).
pub async fn workflow_delete(ctx: &AppContext, id: i64) -> Result<(), ServiceError> {
    enforce(ctx, action::DELETE).await?;
    let row = workflow::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("workflow not found"))?;
    let am: workflow::ActiveModel = row.into();
    am.delete(&ctx.db).await?;
    workflow_execution::Entity::delete_many()
        .filter(workflow_execution::Column::WorkflowId.eq(id))
        .exec(&ctx.db)
        .await?;
    Ok(())
}

/// Activate or deactivate a workflow.
pub async fn workflow_set_active(
    ctx: &AppContext,
    id: i64,
    active: bool,
) -> Result<OwsDocument, ServiceError> {
    enforce(ctx, action::ACTIVATE).await?;
    let row = workflow::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("workflow not found"))?;

    let mut def = model_to_workflow(&row)?;
    def.active = active;
    if active {
        let validation = ::workflow::validate_workflow(&def);
        if !validation.valid {
            return Err(ServiceError::Validation(
                validation
                    .errors
                    .into_iter()
                    .map(|e| crate::ValidationErrorItem::new(vec![], e.message, e.code))
                    .collect(),
            ));
        }
    }

    let mut am: workflow::ActiveModel = row.into();
    am.active = Set(active);
    am.definition_json = Set(
        serde_json::to_value(&def)
            .map_err(|e| ServiceError::internal(format!("workflow serialize: {e}")))?,
    );
    am.updated_at = Set(chrono::Utc::now());
    let updated = am.update(&ctx.db).await?;
    model_to_workflow(&updated)
}

/// Duplicate a workflow with a new name.
pub async fn workflow_duplicate(
    ctx: &AppContext,
    id: i64,
    new_name: Option<&str>,
) -> Result<OwsDocument, ServiceError> {
    enforce(ctx, action::CREATE).await?;
    let def = workflow_get(ctx, id).await?;
    let mut copy = def.clone();
    copy.id = 0;
    copy.definition.document.name = new_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{} (copy)", def.name()));
    copy.active = false;
    copy.version = 1;
    let now = chrono::Utc::now();
    copy.created_at = now;
    copy.updated_at = now;
    workflow_save(ctx, None, &copy).await
}

/// Validate a stored workflow against the catalog.
pub async fn workflow_validate_definition(
    ctx: &AppContext,
    id: i64,
) -> Result<::workflow::model::OwsValidation, ServiceError> {
    let def = workflow_load(ctx, id).await?;
    Ok(::workflow::validate_workflow(&def))
}

/// Export a workflow to a stable OWS JSON document.
pub async fn workflow_export(ctx: &AppContext, id: i64) -> Result<serde_json::Value, ServiceError> {
    let def = workflow_get(ctx, id).await?;
    Ok(serde_json::to_value(&def)
        .map_err(|e| ServiceError::internal(format!("workflow serialize: {e}")))?)
}

/// Export a workflow to a stable OWS YAML document (Open Workflow DSL uses
/// YAML natively).
pub async fn workflow_export_yaml(ctx: &AppContext, id: i64) -> Result<String, ServiceError> {
    let def = workflow_get(ctx, id).await?;
    serde_yaml::to_string(&def)
        .map_err(|e| ServiceError::internal(format!("workflow yaml serialize: {e}")))
}

/// Parse an OWS document from a JSON value or a raw YAML/JSON text string.
pub fn parse_ows_document(value: &serde_json::Value) -> Result<OwsDocument, ServiceError> {
    // Direct JSON object.
    if let Ok(def) = serde_json::from_value::<OwsDocument>(value.clone()) {
        return Ok(def);
    }
    // A JSON/YAML text string.
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if trimmed.starts_with('{') {
            if let Ok(def) = serde_json::from_str::<OwsDocument>(trimmed) {
                return Ok(def);
            }
        }
        if let Ok(def) = serde_yaml::from_str::<OwsDocument>(trimmed) {
            return Ok(def);
        }
    }
    // YAML-parse the JSON value directly (JSON is a subset of YAML).
    if let Ok(text) = serde_json::to_string(value) {
        if let Ok(def) = serde_yaml::from_str::<OwsDocument>(&text) {
            return Ok(def);
        }
    }
    Err(ServiceError::validation(
        "workflow",
        vec![crate::ValidationErrorItem::new(
            vec!["workflow".into()],
            "invalid OWS workflow: expected a JSON or YAML definition".to_string(),
            "ValidationError",
        )],
    ))
}

/// Import a workflow from a JSON/YAML document (validate before persisting).
pub async fn workflow_import(
    ctx: &AppContext,
    value: &serde_json::Value,
) -> Result<OwsDocument, ServiceError> {
    let mut def = parse_ows_document(value)?;
    let validation = ::workflow::validate_workflow(&def);
    if !validation.valid {
        return Err(ServiceError::Validation(
            validation
                .errors
                .into_iter()
                .map(|e| crate::ValidationErrorItem::new(vec![], e.message, e.code))
                .collect(),
        ));
    }
    def.active = false;
    workflow_save(ctx, None, &def).await
}

/// Seed demo workflows on first boot (idempotent — no-op when workflows exist).
pub async fn seed_demo_workflows(ctx: &AppContext) -> Result<usize, ServiceError> {
    use sea_orm::PaginatorTrait;
    let count = workflow::Entity::find().count(&ctx.db).await?;
    if count > 0 {
        return Ok(0);
    }
    let now = chrono::Utc::now();

    fn build(
        name: &str,
        tasks: serde_json::Value,
        now: chrono::DateTime<chrono::Utc>,
    ) -> OwsDocument {
        use serverless_workflow_core::models::workflow::{WorkflowDefinition, WorkflowDefinitionMetadata};
        let metadata = WorkflowDefinitionMetadata::new(
            "default",
            name,
            "1.0.0",
            None,
            Some("Demo".to_string()),
            None,
        );
        let mut def = WorkflowDefinition::new(metadata);
        if let Ok(m) = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(tasks) {
            for (k, v) in m {
                if let Ok(t) = serde_json::from_value(v) {
                    def.do_.add(k, t);
                }
            }
        }
        OwsDocument {
            id: 0,
            active: false,
            version: 1,
            created_at: now,
            updated_at: now,
            definition: def,
        }
    }

    // Manual → Set (context) → no-op HTTP-less path.
    let demo1 = build(
        "Manual → Set → JSON",
        serde_json::json!({
            "setGreeting": { "set": { "greeting": "Hello from Ferris!" } },
            "output": { "call": "data.json", "with": { "json": { "greeting": "${ .greeting }" } } }
        }),
        now,
    );

    // Webhook → Transform → HTTP Request.
    let demo2 = build(
        "Webhook → Transform → HTTP Request",
        serde_json::json!({
            "transform": { "call": "core.transform", "with": { "transformExpression": "${ . }" } },
            "request": { "call": "http.request", "with": { "method": "GET", "url": "https://httpbin.org/get", "authentication": "none" } }
        }),
        now,
    );

    // Content Created → Switch (featured?) → Create Draft.
    let demo3 = build(
        "Content Created → Switch → Create",
        serde_json::json!({
            "branch": {
                "switch": {
                    "featured": { "when": "${ .featured }", "then": "createDraft" },
                    "default": { "then": "noop" }
                }
            },
            "createDraft": { "call": "cms.createContent", "with": { "contentType": "api::article.article", "data": { "title": "Derived draft" } } },
            "noop": { "call": "data.json", "with": { "json": {} } }
        }),
        now,
    );

    // Content Created trigger via schedule.on event.
    let mut demos: Vec<OwsDocument> = vec![demo1, demo2, demo3];
    let mut with = std::collections::HashMap::new();
    with.insert(
        "type".to_string(),
        serde_json::json!("content.created"),
    );
    with.insert("contentType".to_string(), serde_json::json!("api::article.article"));
    demos[2].definition.schedule = Some(serverless_workflow_core::models::workflow::WorkflowScheduleDefinition {
        every: None,
        cron: None,
        after: None,
        on: Some(serverless_workflow_core::models::event::EventConsumptionStrategyDefinition {
            all: None,
            any: None,
            one: Some(serverless_workflow_core::models::event::EventFilterDefinition {
                with: Some(with),
                correlate: None,
            }),
            until: None,
        }),
    });

    let mut created = 0;
    for def in demos {
        let row = workflow::ActiveModel {
            name: Set(def.name().to_string()),
            description: Set(def.definition.document.summary.clone()),
            version: Set(1),
            active: Set(false),
            definition_json: Set(serde_json::to_value(&def).map_err(|e| ServiceError::internal(e.to_string()))?),
            created_at: Set(now),
            updated_at: Set(now),
            created_by: Set(None),
            updated_by: Set(None),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
        let _ = row;
        created += 1;
    }
    Ok(created)
}
