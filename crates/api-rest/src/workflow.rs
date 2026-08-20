//! Workflow automation API routes (`/admin/workflows/**`, `/admin/executions/**`,
//! `/admin/workflow-credentials/**`, and public `/workflow-hooks/{path}`).
//!
//! These extend the existing admin API, respect the same JWT auth + RBAC
//! conventions, and expose the workflow engine through a stable HTTP surface so
//! the UI never embeds business logic.

use crate::auth::AdminCtx;
use crate::error::AppError;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use services::workflow::engine::{RunOptions, execution_cancel, execution_get, execution_list, execution_retry};
use services::workflow::{
    action, credential_create, credential_delete, credential_get, credential_list, credential_types,
    credential_update, workflow_create, workflow_delete, workflow_duplicate, workflow_export,
    workflow_get, workflow_import, workflow_list, workflow_save, workflow_set_active,
    workflow_validate_definition,
};
use std::sync::Arc;

use crate::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowListParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkflowBody {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub definition: Option<::workflow::model::Workflow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteBody {
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialBody {
    pub name: String,
    #[serde(rename = "credentialType")]
    pub credential_type: String,
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Workflow CRUD + management
// ---------------------------------------------------------------------------

pub async fn list_workflows(
    admin: AdminCtx,
    Query(params): Query<WorkflowListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let items = workflow_list(&admin.0, params.name.as_deref(), params.active).await?;
    Ok(Json(serde_json::json!({ "data": items })))
}

pub async fn get_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wf = workflow_get(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": wf })))
}

pub async fn create_workflow(
    admin: AdminCtx,
    Json(body): Json<CreateWorkflowBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wf = workflow_create(&admin.0, &body.name, body.description.as_deref(), body.definition.as_ref()).await?;
    Ok(Json(serde_json::json!({ "data": wf })))
}

pub async fn save_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
    Json(wf): Json<::workflow::model::Workflow>,
) -> Result<Json<serde_json::Value>, AppError> {
    let saved = workflow_save(&admin.0, Some(id), &wf).await?;
    Ok(Json(serde_json::json!({ "data": saved })))
}

pub async fn delete_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    workflow_delete(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

pub async fn activate_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wf = workflow_set_active(&admin.0, id, true).await?;
    Ok(Json(serde_json::json!({ "data": wf })))
}

pub async fn deactivate_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wf = workflow_set_active(&admin.0, id, false).await?;
    Ok(Json(serde_json::json!({ "data": wf })))
}

pub async fn duplicate_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wf = workflow_duplicate(&admin.0, id, None).await?;
    Ok(Json(serde_json::json!({ "data": wf })))
}

pub async fn validate_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let v = workflow_validate_definition(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": v })))
}

pub async fn export_workflow(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let v = workflow_export(&admin.0, id).await?;
    Ok(Json(v))
}

pub async fn import_workflow(
    admin: AdminCtx,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wf = workflow_import(&admin.0, &value).await?;
    Ok(Json(serde_json::json!({ "data": wf })))
}

pub async fn execute_workflow_handler(
    admin: AdminCtx,
    Path(id): Path<i64>,
    Json(body): Json<ExecuteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let opts = RunOptions {
        mode: "manual".into(),
        trigger: "manual".into(),
        input: body.data,
        max_attempts: 3,
    };
    let exec_id = services::workflow::engine::execute_workflow(&admin.0, id, opts).await?;
    Ok(Json(serde_json::json!({ "data": { "executionId": exec_id } })))
}

// ---------------------------------------------------------------------------
// Executions
// ---------------------------------------------------------------------------

pub async fn list_executions(
    admin: AdminCtx,
    Query(params): Query<ExecutionListParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let items = execution_list(&admin.0, params.workflow_id, params.status.as_deref()).await?;
    Ok(Json(serde_json::json!({ "data": items })))
}

pub async fn get_execution(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (execution, node_runs) = execution_get(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": execution, "nodeRuns": node_runs })))
}

pub async fn cancel_execution(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    execution_cancel(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

pub async fn retry_execution(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let new_id = execution_retry(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": { "executionId": new_id } })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionListParams {
    #[serde(default)]
    pub workflow_id: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

pub async fn list_credentials(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let items = credential_list(&admin.0).await?;
    Ok(Json(serde_json::json!({ "data": items })))
}

pub async fn get_credential(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let c = credential_get(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": c })))
}

pub async fn create_credential(
    admin: AdminCtx,
    Json(body): Json<CredentialBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let c = credential_create(&admin.0, &body.name, &body.credential_type, &body.data).await?;
    Ok(Json(serde_json::json!({ "data": c })))
}

pub async fn update_credential(
    admin: AdminCtx,
    Path(id): Path<i64>,
    Json(body): Json<CredentialBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let c = credential_update(&admin.0, id, Some(&body.name), Some(&body.credential_type), Some(&body.data)).await?;
    Ok(Json(serde_json::json!({ "data": c })))
}

pub async fn delete_credential(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    credential_delete(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

pub async fn list_credential_types(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let types = credential_types();
    Ok(Json(serde_json::json!({ "data": types })))
}

// ---------------------------------------------------------------------------
// Node library + content types
// ---------------------------------------------------------------------------

pub async fn node_library(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let defs: Vec<_> = ::workflow::registry()
        .all()
        .into_iter()
        .cloned()
        .collect();
    Ok(Json(serde_json::json!({ "data": defs })))
}

pub async fn workflow_content_types(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let types: Vec<_> = admin
        .0
        .schema_cache
        .get_all()
        .into_iter()
        .filter(|s| {
            s.kind != ::core_domain::ContentTypeKind::Component
                && !s.kind.as_db_str().is_empty()
        })
        .map(|s| {
            serde_json::json!({
                "uid": s.uid.as_str(),
                "kind": s.kind.as_db_str(),
                "displayName": s.info.display_name,
                "attributes": s.attributes.keys().collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "data": types })))
}

// ---------------------------------------------------------------------------
// Public webhook trigger
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct WebhookQueryParams {
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// Public webhook endpoint: dispatches to the matching active workflow.
pub async fn webhook_trigger(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    body: axum::extract::Request,
) -> Result<Json<serde_json::Value>, AppError> {
    let method = body.method().to_string();
    // Extract the JSON body if present.
    let data = axum::body::to_bytes(body.into_body(), 2 * 1024 * 1024)
        .await
        .map_err(|e| AppError(::services::ServiceError::internal(format!("webhook body: {e}"))))?;
    let payload: serde_json::Value = if data.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&data).unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&data).to_string()))
    };

    match ::services::workflow::triggers::workflow_for_webhook(&state.ctx, &method, &path).await {
        Some((workflow_id, _node_id)) => {
            let exec_id =
                ::services::workflow::triggers::execute_webhook(&state.ctx, workflow_id, &method, &path, payload).await?;
            Ok(Json(serde_json::json!({ "data": { "executionId": exec_id } })))
        }
        None => Err(AppError(::services::ServiceError::not_found(
            "no active workflow matches this webhook path",
        ))),
    }
}

/// Expose the workflow permission actions to the settings UI.
pub async fn workflow_permission_actions(
    _admin: AdminCtx,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(serde_json::json!({ "data": action::ALL })))
}
