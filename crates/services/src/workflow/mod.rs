//! Workflow services (persistence, activation, import/export, execution).
//!
//! This is the `services` layer for the workflow automation engine. It depends
//! on the pure `workflow` crate for the domain model, node definitions,
//! expression engine and validation, and wires them to the database, the CMS
//! (`dynamic-store`/content services) and external HTTP integrations.

pub mod credentials;
pub mod engine;
pub mod executors;
pub mod triggers;

pub use credentials::*;
pub use engine::*;
pub use executors::*;
pub use triggers::*;

use crate::{AppContext, ServiceError};
use db::entities::{workflow, workflow_execution};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::Serialize;
use ::workflow::model::Workflow as WorkflowModel;

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

    /// All workflow actions (used by the settings UI + seeding).
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
    pub node_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    pub execution_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_execution: Option<ExecutionSummary>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn model_to_workflow(m: &workflow::Model) -> Result<WorkflowModel, ServiceError> {
    serde_json::from_value(m.definition_json.clone())
        .map_err(|e| ServiceError::internal(format!("corrupt workflow definition: {e}")))
}

/// Enforce a workflow permission for the current user.
pub async fn enforce(
    ctx: &AppContext,
    perm: &str,
) -> Result<(), ServiceError> {
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
            name: row.name.clone(),
            description: row.description.clone(),
            version: row.version,
            active: row.active,
            node_count: def.nodes.len(),
            trigger: def
                .trigger_nodes()
                .first()
                .map(|t| t.name.clone()),
            execution_count,
            last_execution: last.as_ref().map(ExecutionSummary::from),
            created_at: row.created_at,
            updated_at: row.updated_at,
        });
    }
    Ok(result)
}

/// Get a full workflow definition by id.
pub async fn workflow_get(ctx: &AppContext, id: i64) -> Result<WorkflowModel, ServiceError> {
    enforce(ctx, action::VIEW).await?;
    let row = workflow::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("workflow not found"))?;
    model_to_workflow(&row)
}

/// Load a workflow definition without permission checks (executor path).
pub async fn workflow_load(ctx: &AppContext, id: i64) -> Result<WorkflowModel, ServiceError> {
    let row = workflow::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("workflow not found"))?;
    model_to_workflow(&row)
}

/// Create a new workflow from a definition. `id`/timestamps in the definition
/// are normalized to the created row.
pub async fn workflow_create(
    ctx: &AppContext,
    name: &str,
    description: Option<&str>,
    def: Option<&WorkflowModel>,
) -> Result<WorkflowModel, ServiceError> {
    enforce(ctx, action::CREATE).await?;
    let now = chrono::Utc::now();
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let mut workflow = def
        .cloned()
        .unwrap_or_else(|| WorkflowModel {
            id: 0,
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            version: 1,
            active: false,
            nodes: vec![],
            connections: vec![],
            settings: Default::default(),
            variables: Default::default(),
            tags: vec![],
            created_at: now,
            updated_at: now,
        });
    workflow.id = 0;
    workflow.name = name.to_string();
    if description.is_some() {
        workflow.description = description.map(|s| s.to_string());
    }
    workflow.created_at = now;
    workflow.updated_at = now;

    let row = workflow::ActiveModel {
        name: Set(workflow.name.clone()),
        description: Set(workflow.description.clone()),
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

/// Save (create or update) a workflow definition, bumping its version.
pub async fn workflow_save(
    ctx: &AppContext,
    id: Option<i64>,
    def: &WorkflowModel,
) -> Result<WorkflowModel, ServiceError> {
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
            am.name = Set(new_def.name.clone());
            am.description = Set(new_def.description.clone());
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
                name: Set(new_def.name.clone()),
                description: Set(new_def.description.clone()),
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
    // Remove executions of this workflow.
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
) -> Result<WorkflowModel, ServiceError> {
    enforce(ctx, action::ACTIVATE).await?;
    let row = workflow::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("workflow not found"))?;

    // Keep the embedded definition's `active` in sync with the column.
    let mut def = model_to_workflow(&row)?;
    def.active = active;
    if active {
        let validation = ::workflow::validate(&def, ::workflow::registry());
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
) -> Result<WorkflowModel, ServiceError> {
    enforce(ctx, action::CREATE).await?;
    let def = workflow_get(ctx, id).await?;
    let mut copy = def.clone();
    copy.id = 0;
    copy.name = new_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{} (copy)", def.name));
    copy.active = false;
    copy.version = 1;
    let now = chrono::Utc::now();
    copy.created_at = now;
    copy.updated_at = now;
    // Re-assign unique node ids for the copy.
    for node in copy.nodes.iter_mut() {
        node.id = uuid::Uuid::new_v4().to_string();
    }
    for conn in copy.connections.iter_mut() {
        conn.id = uuid::Uuid::new_v4().to_string();
    }
    workflow_save(ctx, None, &copy).await
}

/// Validate a stored workflow against the registry.
pub async fn workflow_validate_definition(
    ctx: &AppContext,
    id: i64,
) -> Result<::workflow::model::WorkflowValidation, ServiceError> {
    let def = workflow_load(ctx, id).await?;
    Ok(::workflow::validate(&def, ::workflow::registry()))
}

/// Export a workflow to a stable JSON document.
pub async fn workflow_export(ctx: &AppContext, id: i64) -> Result<serde_json::Value, ServiceError> {
    let def = workflow_get(ctx, id).await?;
    Ok(serde_json::to_value(&def)
        .map_err(|e| ServiceError::internal(format!("workflow serialize: {e}")))?)
}

/// Import a workflow from a JSON document (validate before persisting).
pub async fn workflow_import(
    ctx: &AppContext,
    value: &serde_json::Value,
) -> Result<WorkflowModel, ServiceError> {
    let mut def: WorkflowModel = serde_json::from_value(value.clone())
        .map_err(|e| ServiceError::validation("workflow", vec![crate::ValidationErrorItem::new(
            vec!["workflow".into()],
            format!("invalid workflow JSON: {e}"),
            "ValidationError",
        )]))?;
    let validation = ::workflow::validate(&def, ::workflow::registry());
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
/// Gives the Workflows screen immediate, runnable examples.
pub async fn seed_demo_workflows(ctx: &AppContext) -> Result<usize, ServiceError> {
    use sea_orm::PaginatorTrait;
    let count = workflow::Entity::find().count(&ctx.db).await?;
    if count > 0 {
        return Ok(0);
    }
    let now = chrono::Utc::now();

    fn node(id: &str, node_type: &str, name: &str, x: f64, y: f64, params: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": id, "nodeType": node_type, "name": name,
            "position": { "x": x, "y": y }, "parameters": params,
            "disabled": false
        })
    }
    fn conn(id: &str, from: &str, out: &str, to: &str) -> serde_json::Value {
        serde_json::json!({ "id": id, "from": from, "fromOutput": out, "to": to, "toInput": "main" })
    }

    let demos: Vec<serde_json::Value> = vec![
        serde_json::json!({
            "id": 0, "name": "Manual → Set → No-op",
            "description": "A simple starter workflow triggered manually.",
            "version": 1, "active": false,
            "nodes": [
                node("t", "manualTrigger", "Manual Trigger", 40.0, 60.0, serde_json::json!({})),
                node("s", "set", "Set Field", 300.0, 60.0, serde_json::json!({ "field": "greeting", "value": "Hello from Ferris!" })),
                node("n", "noop", "No-op", 560.0, 60.0, serde_json::json!({})),
            ],
            "connections": [ conn("c1", "t", "main", "s"), conn("c2", "s", "main", "n") ],
            "settings": {}, "variables": {}, "tags": ["demo"],
            "createdAt": now.to_rfc3339(), "updatedAt": now.to_rfc3339(),
        }),
        serde_json::json!({
            "id": 0, "name": "Webhook → Transform → HTTP Request",
            "description": "Triggered by a webhook at /workflow-hooks/notify.",
            "version": 1, "active": false,
            "nodes": [
                node("w", "webhookTrigger", "Webhook", 40.0, 60.0, serde_json::json!({ "path": "notify", "method": "POST" })),
                node("tf", "transform", "Transform", 300.0, 60.0, serde_json::json!({ "transformExpression": "{{ $json }}" })),
                node("h", "httpRequest", "HTTP Request", 560.0, 60.0, serde_json::json!({ "method": "GET", "url": "https://httpbin.org/get", "authentication": "none", "headers": {} })),
            ],
            "connections": [ conn("c1", "w", "main", "tf"), conn("c2", "tf", "main", "h") ],
            "settings": {}, "variables": {}, "tags": ["demo", "webhook"],
            "createdAt": now.to_rfc3339(), "updatedAt": now.to_rfc3339(),
        }),
        serde_json::json!({
            "id": 0, "name": "Content Created → If → Create",
            "description": "On content creation, branch on a field and create related content.",
            "version": 1, "active": false,
            "nodes": [
                node("cc", "contentCreated", "Content Created", 40.0, 60.0, serde_json::json!({ "contentType": "api::article.article" })),
                node("if", "if", "If featured", 300.0, 40.0, serde_json::json!({ "operator": "true", "value1": "{{ $json.featured }}", "condition": "" })),
                node("no", "noop", "No-op (false)", 560.0, 140.0, serde_json::json!({})),
                node("cr", "createContent", "Create Draft", 560.0, -40.0, serde_json::json!({ "contentType": "api::article.article", "data": { "title": "Derived draft" } })),
            ],
            "connections": [
                conn("c1", "cc", "main", "if"),
                serde_json::json!({ "id": "c2", "from": "if", "fromOutput": "true", "to": "cr", "toInput": "main" }),
                serde_json::json!({ "id": "c3", "from": "if", "fromOutput": "false", "to": "no", "toInput": "main" }),
            ],
            "settings": {}, "variables": {}, "tags": ["demo", "content"],
            "createdAt": now.to_rfc3339(), "updatedAt": now.to_rfc3339(),
        }),
    ];

    let mut created = 0;
    for demo in demos {
        let def: WorkflowModel = serde_json::from_value(demo)
            .map_err(|e| ServiceError::internal(format!("demo workflow parse: {e}")))?;
        let row = workflow::ActiveModel {
            name: Set(def.name.clone()),
            description: Set(def.description.clone()),
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
