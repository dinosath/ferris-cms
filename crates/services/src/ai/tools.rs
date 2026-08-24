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
use crate::{AppContext, ServiceError};

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
