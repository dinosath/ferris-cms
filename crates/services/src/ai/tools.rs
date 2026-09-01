//! RBAC-aware tool registry for the assistant.
//!
//! The model only ever returns *typed tool requests*. FerrisCMS resolves the
//! user's authorization (via the existing content services, which enforce RBAC
//! internally) and executes the tool. The LLM never touches the database,
//! filesystem, shell, secrets, or internal endpoints directly.

use ai::{AiTool, AiToolCall, AiToolResult};
use serde_json::{json, Value};

use crate::ai::content::translate_text;
use crate::ai::usage::log_usage;
use crate::content::{cm_publish, cm_unpublish};
use crate::content_type_builder::{ctb_apply, ctb_get, ctb_list};
use crate::workflow::{
    workflow_create, workflow_delete, workflow_get, workflow_list, workflow_save,
    workflow_set_active,
};
use crate::{AppContext, ServiceError};
use ::workflow::model::OwsDocument;

/// The set of tools the assistant may request.
pub fn definitions() -> Vec<AiTool> {
    vec![
        AiTool {
            name: "content_list_types".into(),
            description: "List all content types and their fields in the CMS.".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        AiTool {
            name: "content_list".into(),
            description: "List entries of a content type. Returns id, documentId and the data fields.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "uid": { "type": "string", "description": "Content type uid, e.g. api::product.product" },
                    "page": { "type": "integer" },
                    "pageSize": { "type": "integer" }
                },
                "required": ["uid"]
            }),
        },
        AiTool {
            name: "content_get".into(),
            description: "Get a single entry by documentId.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "uid": { "type": "string" },
                    "documentId": { "type": "string" }
                },
                "required": ["uid", "documentId"]
            }),
        },
        AiTool {
            name: "content_create".into(),
            description: "Create a new entry in a content type. Only fields defined on the content type are accepted.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "uid": { "type": "string" },
                    "data": { "type": "object", "description": "Field values to set" }
                },
                "required": ["uid", "data"]
            }),
        },
        AiTool {
            name: "content_update".into(),
            description: "Update an existing entry by documentId. Only fields defined on the content type are accepted.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "uid": { "type": "string" },
                    "documentId": { "type": "string" },
                    "data": { "type": "object", "description": "Field values to set" }
                },
                "required": ["uid", "documentId", "data"]
            }),
        },
        AiTool {
            name: "content_delete".into(),
            description: "Delete an entry by documentId.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "uid": { "type": "string" },
                    "documentId": { "type": "string" }
                },
                "required": ["uid", "documentId"]
            }),
        },
        AiTool {
            name: "content_translate".into(),
            description: "Translate a piece of text to a target locale (e.g. 'en', 'fr').".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" },
                    "targetLocale": { "type": "string" }
                },
                "required": ["text", "targetLocale"]
            }),
        },
        AiTool {
            name: "content_publish".into(),
            description: "Publish a draft entry of a content type.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "uid": { "type": "string" },
                    "documentId": { "type": "string" }
                },
                "required": ["uid", "documentId"]
            }),
        },
        AiTool {
            name: "content_unpublish".into(),
            description: "Unpublish a published entry (back to draft).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "uid": { "type": "string" },
                    "documentId": { "type": "string" }
                },
                "required": ["uid", "documentId"]
            }),
        },
        AiTool {
            name: "content_type_list".into(),
            description: "List all content types and components and their fields in the CMS.".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        AiTool {
            name: "content_type_get".into(),
            description: "Get a single content type schema by uid.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "uid": { "type": "string" } },
                "required": ["uid"]
            }),
        },
        AiTool {
            name: "content_type_save".into(),
            description: "Create or update a content type. Provide the full schema JSON (Strapi-style with uid, kind, info, attributes).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "schema": { "type": "object", "description": "The content type schema to create or update" }
                },
                "required": ["schema"]
            }),
        },
        AiTool {
            name: "content_type_delete".into(),
            description: "Delete a content type by uid.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "uid": { "type": "string" } },
                "required": ["uid"]
            }),
        },
        AiTool {
            name: "workflow_list".into(),
            description: "List workflows (optionally filter by name or active).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "activeOnly": { "type": "boolean" }
                }
            }),
        },
        AiTool {
            name: "workflow_get".into(),
            description: "Get a workflow definition by id.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }),
        },
        AiTool {
            name: "workflow_create".into(),
            description: "Create a workflow. Provide a name and an optional workflow definition JSON (nodes/connections/settings).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "definition": { "type": "object" }
                },
                "required": ["name"]
            }),
        },
        AiTool {
            name: "workflow_update".into(),
            description: "Update a workflow by id. Provide the full workflow definition JSON (nodes/connections/settings).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "definition": { "type": "object" }
                },
                "required": ["id"]
            }),
        },
        AiTool {
            name: "workflow_delete".into(),
            description: "Delete a workflow by id.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }),
        },
        AiTool {
            name: "workflow_activate".into(),
            description: "Activate a workflow by id.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }),
        },
        AiTool {
            name: "workflow_deactivate".into(),
            description: "Deactivate a workflow by id.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"]
            }),
        },
    ]
}

/// Execute a single tool call under the current user's RBAC context.
pub async fn execute_tool(
    ctx: &AppContext,
    call: &AiToolCall,
    fallback_model: Option<&str>,
) -> Result<AiToolResult, ServiceError> {
    let result = match call.name.as_str() {
        "content_list_types" => execute_list_types(ctx).await,
        "content_list" => execute_list(ctx, &call.arguments).await,
        "content_get" => execute_get(ctx, &call.arguments).await,
        "content_create" => execute_create(ctx, &call.arguments).await,
        "content_update" => execute_update(ctx, &call.arguments).await,
        "content_delete" => execute_delete(ctx, &call.arguments).await,
        "content_translate" => execute_translate(ctx, &call.arguments, fallback_model).await,
        "content_publish" => execute_content_publish(ctx, &call.arguments).await,
        "content_unpublish" => execute_content_unpublish(ctx, &call.arguments).await,
        "content_type_list" => execute_content_type_list(ctx).await,
        "content_type_get" => execute_content_type_get(ctx, &call.arguments).await,
        "content_type_save" => execute_content_type_save(ctx, &call.arguments).await,
        "content_type_delete" => execute_content_type_delete(ctx, &call.arguments).await,
        "workflow_list" => execute_workflow_list(ctx, &call.arguments).await,
        "workflow_get" => execute_workflow_get(ctx, &call.arguments).await,
        "workflow_create" => execute_workflow_create(ctx, &call.arguments).await,
        "workflow_update" => execute_workflow_update(ctx, &call.arguments).await,
        "workflow_delete" => execute_workflow_delete(ctx, &call.arguments).await,
        "workflow_activate" => execute_workflow_set_active(ctx, &call.arguments, true).await,
        "workflow_deactivate" => execute_workflow_set_active(ctx, &call.arguments, false).await,
        other => Err(ServiceError::internal(format!(
            "unknown AI tool '{other}' (not in the allowed tool registry)"
        ))),
    };

    match result {
        Ok(content) => Ok(AiToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
        }),
        Err(e) => {
            // Return the error to the model so it can self-correct, but never
            // leak internal details beyond a safe message.
            let safe = match &e {
                ServiceError::Forbidden => "forbidden: the current user lacks permission".to_string(),
                ServiceError::NotFound(m) => format!("not found: {m}"),
                other => other.to_string(),
            };
            Ok(AiToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: json!({ "error": safe, "ok": false }).to_string(),
            })
        }
    }
}

async fn execute_list_types(ctx: &AppContext) -> Result<String, ServiceError> {
    let types = crate::content::cm_content_types(ctx).await;
    let arr: Vec<Value> = types
        .into_iter()
        .map(|t| json!({ "uid": t.uid, "displayName": t.display_name }))
        .collect();
    Ok(json!({ "ok": true, "contentTypes": arr }).to_string())
}

async fn execute_list(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let page = args.get("page").and_then(|v| v.as_i64()).unwrap_or(1);
    let page_size = args.get("pageSize").and_then(|v| v.as_i64()).unwrap_or(20);
    let params = api_types::QueryParams {
        pagination: Some(api_types::PaginationParams::Page {
            page,
            page_size,
            with_count: Some(true),
        }),
        ..Default::default()
    };
    let resp = crate::content::cm_list(ctx, &uid, &params).await?;
    Ok(json!({ "ok": true, "total": resp.meta.pagination.map(|p| p.total).unwrap_or(0), "data": resp.data }).to_string())
}

async fn execute_get(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let doc = arg_str(args, "documentId").ok_or_else(|| ServiceError::validation("documentId required", vec![]))?;
    let resp = crate::content::cm_get(ctx, &uid, &doc, None).await?;
    Ok(json!({ "ok": true, "data": resp.data }).to_string())
}

async fn execute_create(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let data = args.get("data").cloned().unwrap_or(Value::Object(Default::default()));
    let resp = crate::content::cm_create(ctx, &uid, &data).await?;
    Ok(json!({ "ok": true, "created": true, "documentId": resp.data.get("documentId") }).to_string())
}

async fn execute_update(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let doc = arg_str(args, "documentId").ok_or_else(|| ServiceError::validation("documentId required", vec![]))?;
    let data = args.get("data").cloned().unwrap_or(Value::Object(Default::default()));
    let resp = crate::content::cm_update(ctx, &uid, &doc, &data).await?;
    Ok(json!({ "ok": true, "updated": true, "documentId": resp.data.get("documentId") }).to_string())
}

async fn execute_delete(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let doc = arg_str(args, "documentId").ok_or_else(|| ServiceError::validation("documentId required", vec![]))?;
    crate::content::cm_delete(ctx, &uid, &doc).await?;
    Ok(json!({ "ok": true, "deleted": true }).to_string())
}

async fn execute_translate(
    ctx: &AppContext,
    args: &Value,
    fallback_model: Option<&str>,
) -> Result<String, ServiceError> {
    let text = arg_str(args, "text").ok_or_else(|| ServiceError::validation("text required", vec![]))?;
    let target = arg_str(args, "targetLocale").unwrap_or_else(|| "en".to_string());
    let (translated, usage) = translate_text(ctx, &text, &target, fallback_model).await?;
    if let Some((uid, _)) = ctx.current_user.as_ref().map(|u| (u.id, ())) {
        let _ = log_usage(ctx, uid, None, fallback_model, Some("tool.translate"), usage, Some("ok")).await;
    }
    Ok(json!({ "ok": true, "translated": translated }).to_string())
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String, ServiceError> {
    serde_json::to_string(v).map_err(|e| ServiceError::internal(e.to_string()))
}

// ---------------------------------------------------------------------------
// Content publish/unpublish
// ---------------------------------------------------------------------------

async fn execute_content_publish(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let doc = arg_str(args, "documentId").ok_or_else(|| ServiceError::validation("documentId required", vec![]))?;
    cm_publish(ctx, &uid, &doc).await?;
    Ok(json!({ "ok": true, "published": true }).to_string())
}

async fn execute_content_unpublish(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let doc = arg_str(args, "documentId").ok_or_else(|| ServiceError::validation("documentId required", vec![]))?;
    cm_unpublish(ctx, &uid, &doc).await?;
    Ok(json!({ "ok": true, "unpublished": true }).to_string())
}

// ---------------------------------------------------------------------------
// Content-type CRUD
// ---------------------------------------------------------------------------

async fn execute_content_type_list(ctx: &AppContext) -> Result<String, ServiceError> {
    let types = ctb_list(ctx).await;
    to_json(&types)
}

async fn execute_content_type_get(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let schema = ctb_get(ctx, &uid).await?;
    to_json(&schema)
}

async fn execute_content_type_save(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let schema_json = args
        .get("schema")
        .ok_or_else(|| ServiceError::validation("schema required", vec![]))?;
    let schema: core_schema::Schema = serde_json::from_value(schema_json.clone())
        .map_err(|e| ServiceError::internal(format!("invalid schema: {e}")))?;
    let uid = schema.uid.as_str().to_string();

    let mut all = ctb_list(ctx).await;
    if let Some(pos) = all.iter().position(|s| s.uid == schema.uid) {
        all[pos] = schema;
    } else {
        all.push(schema);
    }
    let _applied = ctb_apply(ctx, all, Vec::new()).await?;
    Ok(json!({ "ok": true, "uid": uid }).to_string())
}

async fn execute_content_type_delete(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let uid = arg_str(args, "uid").ok_or_else(|| ServiceError::validation("uid required", vec![]))?;
    let all = ctb_list(ctx).await;
    if !all.iter().any(|s| s.uid.as_str() == uid) {
        return Ok(json!({ "ok": false, "error": format!("content type {uid} not found") }).to_string());
    }
    ctb_apply(ctx, all, vec![core_domain::Uid::new(&uid)]).await?;
    Ok(json!({ "ok": true, "deleted": uid }).to_string())
}

// ---------------------------------------------------------------------------
// Workflow actions
// ---------------------------------------------------------------------------

async fn execute_workflow_list(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let name = args.get("name").and_then(|v| v.as_str());
    let active_only = args.get("activeOnly").and_then(|v| v.as_bool());
    let rows = workflow_list(ctx, name, active_only).await?;
    to_json(&rows)
}

async fn execute_workflow_get(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let id = arg_i64(args, "id").ok_or_else(|| ServiceError::validation("id required", vec![]))?;
    let wf = workflow_get(ctx, id).await?;
    to_json(&wf)
}

async fn execute_workflow_create(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let name = arg_str(args, "name").ok_or_else(|| ServiceError::validation("name required", vec![]))?;
    let description = args.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
    let def = args
        .get("definition")
        .map(|d| {
            serde_json::from_value::<OwsDocument>(d.clone())
                .map_err(|e| ServiceError::internal(format!("invalid workflow definition: {e}")))
        })
        .transpose()?;
    let created = workflow_create(ctx, &name, description.as_deref(), def.as_ref()).await?;
    to_json(&created)
}

async fn execute_workflow_update(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let id = arg_i64(args, "id").ok_or_else(|| ServiceError::validation("id required", vec![]))?;
    let mut wf = workflow_get(ctx, id).await?;
    if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        wf.definition.document.name = name.to_string();
    }
    if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
        wf.definition.document.summary = Some(desc.to_string());
    }
    if let Some(def) = args.get("definition") {
        wf = serde_json::from_value::<OwsDocument>(def.clone())
            .map_err(|e| ServiceError::internal(format!("invalid workflow definition: {e}")))?;
        wf.id = id;
    }
    let saved = workflow_save(ctx, Some(id), &wf).await?;
    to_json(&saved)
}

async fn execute_workflow_delete(ctx: &AppContext, args: &Value) -> Result<String, ServiceError> {
    let id = arg_i64(args, "id").ok_or_else(|| ServiceError::validation("id required", vec![]))?;
    workflow_delete(ctx, id).await?;
    Ok(json!({ "ok": true, "deleted": id }).to_string())
}

async fn execute_workflow_set_active(
    ctx: &AppContext,
    args: &Value,
    active: bool,
) -> Result<String, ServiceError> {
    let id = arg_i64(args, "id").ok_or_else(|| ServiceError::validation("id required", vec![]))?;
    let wf = workflow_set_active(ctx, id, active).await?;
    Ok(json!({ "ok": true, "id": wf.id, "active": wf.active }).to_string())
}
