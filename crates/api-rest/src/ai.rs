//! AI subsystem admin routes (`/admin/ai/**`).
//!
//! These extend the existing admin API, share the same JWT auth + RBAC
//! conventions, and expose the AI services through a stable HTTP surface so the
//! UI never embeds business logic.

use std::sync::Arc;

use axum::extract::Path;
use axum::routing::{get, post, put};
use axum::{Json, Router};

use api_types::{
    AiConfirmBody, AiEditBody, AiGenerateBody, AiMediaAnalyzeBody, AiModelCreate, AiModelUpdate,
    AiProviderCreate, AiProviderUpdate, AiSchemaApplyBody, AiSchemaGenerateBody, AiSendMessage,
    AiTranslateBody, AiConversationCreate,
};
use services::ai;

use crate::auth::AdminCtx;
use crate::error::AppError;
use crate::AppState;
use services::ServiceError;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Providers
        .route("/admin/ai/providers", get(list_providers).post(create_provider))
        .route(
            "/admin/ai/providers/{id}",
            get(get_provider).put(update_provider).delete(delete_provider),
        )
        .route("/admin/ai/providers/test-connection", post(test_connection))
        .route("/admin/ai/providers/{id}/models", get(list_provider_models))
        // Models
        .route("/admin/ai/models", get(list_models).post(create_model))
        .route(
            "/admin/ai/models/{id}",
            put(update_model).delete(delete_model),
        )
        // Conversations + assistant
        .route(
            "/admin/ai/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/admin/ai/conversations/{id}",
            get(get_conversation).delete(delete_conversation),
        )
        .route("/admin/ai/conversations/{id}/messages", get(list_messages))
        .route("/admin/ai/conversations/{id}/messages", post(send_message))
        .route("/admin/ai/conversations/{id}/confirm", post(confirm_tool_calls))
        // Features
        .route("/admin/ai/generate", post(generate_content))
        .route("/admin/ai/edit", post(edit_content))
        .route("/admin/ai/translate", post(translate))
        .route("/admin/ai/schema/generate", post(schema_generate))
        .route("/admin/ai/schema/apply", post(schema_apply))
        .route("/admin/ai/media/analyze", post(media_analyze))
        // Tools + usage
        .route("/admin/ai/tools", get(tools))
        .route("/admin/ai/usage", get(usage_list))
        .route("/admin/ai/usage/summary", get(usage_summary))
}

fn wrap<T: serde::Serialize>(
    r: Result<T, ServiceError>,
) -> Result<Json<serde_json::Value>, AppError> {
    match r {
        Ok(v) => Ok(Json(serde_json::json!({ "data": v }))),
        Err(e) => Err(AppError::from(e)),
    }
}

async fn list_providers(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::list_providers(&admin.0).await;
    wrap(r)
}

async fn create_provider(
    admin: AdminCtx,
    Json(req): Json<AiProviderCreate>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::create_provider(
        &admin.0,
        req.name,
        req.kind,
        req.base_url,
        req.api_key,
        req.organization,
        req.enabled,
        req.sort_order,
    )
    .await;
    wrap(r)
}

async fn get_provider(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::get_provider(&admin.0, id).await;
    wrap(r)
}

async fn update_provider(
    admin: AdminCtx,
    Path(id): Path<i64>,
    Json(req): Json<AiProviderUpdate>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::update_provider(
        &admin.0,
        id,
        req.name,
        req.kind,
        req.base_url,
        req.api_key,
        req.organization,
        req.enabled,
        req.sort_order,
    )
    .await;
    wrap(r)
}

async fn delete_provider(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::delete_provider(&admin.0, id).await.map(|_| serde_json::json!({ "deleted": true }));
    wrap(r)
}

/// Probe a provider configuration (connectivity + discovered models) without
/// saving it. Used by the UI to verify a provider before adding/editing.
async fn test_connection(
    admin: AdminCtx,
    Json(req): Json<AiProviderCreate>,
) -> Result<Json<serde_json::Value>, AppError> {
    match ai::test_provider_connection(
        &req.kind,
        req.base_url.as_deref(),
        req.api_key.as_deref(),
        req.organization.as_deref(),
    )
    .await
    {
        Ok(models) => Ok(Json(serde_json::json!({ "data": { "ok": true, "models": models } }))),
        Err(e) => Err(AppError::from(e)),
    }
}

async fn list_provider_models(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::list_models(&admin.0, Some(id)).await;
    wrap(r)
}

async fn list_models(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::list_models(&admin.0, None).await;
    wrap(r)
}

async fn create_model(
    admin: AdminCtx,
    Json(req): Json<AiModelCreate>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::create_model(
        &admin.0,
        req.provider_id,
        req.name,
        req.display_name,
        req.description,
        req.supports_chat,
        req.supports_tools,
        req.supports_streaming,
        req.max_input_tokens,
        req.max_output_tokens,
        req.enabled,
    )
    .await;
    wrap(r)
}

async fn update_model(
    admin: AdminCtx,
    Path(id): Path<i64>,
    Json(req): Json<AiModelUpdate>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::update_model(
        &admin.0,
        id,
        req.name,
        req.display_name,
        req.description,
        req.supports_chat,
        req.supports_tools,
        req.supports_streaming,
        req.max_input_tokens,
        req.max_output_tokens,
        req.enabled,
    )
    .await;
    wrap(r)
}

async fn delete_model(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::delete_model(&admin.0, id).await.map(|_| serde_json::json!({ "deleted": true }));
    wrap(r)
}

// ---------------------------------------------------------------------------
// Conversations / assistant
// ---------------------------------------------------------------------------

async fn list_conversations(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::list_conversations(&admin.0).await;
    wrap(r)
}

async fn create_conversation(
    admin: AdminCtx,
    Json(req): Json<AiConversationCreate>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::create_conversation(&admin.0, req.title, req.system_prompt, req.provider_id, req.model, req.privacy_mode).await;
    wrap(r)
}

async fn get_conversation(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::get_conversation(&admin.0, id).await;
    wrap(r)
}

async fn delete_conversation(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::delete_conversation(&admin.0, id).await.map(|_| serde_json::json!({ "deleted": true }));
    wrap(r)
}

async fn list_messages(
    admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::list_messages(&admin.0, id).await;
    wrap(r)
}

async fn send_message(
    admin: AdminCtx,
    Path(id): Path<i64>,
    Json(req): Json<AiSendMessage>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::send_message(&admin.0, id, &req.text).await;
    wrap(r)
}

async fn confirm_tool_calls(
    admin: AdminCtx,
    Path(id): Path<i64>,
    Json(req): Json<AiConfirmBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let calls = req.calls.into_iter().map(|c| ::ai::AiToolCall {
        id: c.id,
        name: c.name,
        arguments: c.arguments,
    }).collect();
    let r = ai::confirm_tool_calls(&admin.0, id, calls).await;
    wrap(r)
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

async fn generate_content(
    admin: AdminCtx,
    Json(req): Json<AiGenerateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::generate_content(&admin.0, &req.uid, &req.prompt, req.fields, req.apply, req.provider_id, req.model.as_deref()).await;
    wrap(r)
}

async fn edit_content(
    admin: AdminCtx,
    Json(req): Json<AiEditBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::edit_content(&admin.0, &req.uid, &req.document_id, &req.instruction, req.provider_id, req.model.as_deref()).await;
    wrap(r)
}

async fn translate(
    admin: AdminCtx,
    Json(req): Json<AiTranslateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::translate_text(&admin.0, &req.text, &req.target_locale, req.model.as_deref())
        .await
        .map(|(translated, usage)| serde_json::json!({ "translated": translated, "usage": usage }));
    wrap(r)
}

async fn schema_generate(
    admin: AdminCtx,
    Json(req): Json<AiSchemaGenerateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::generate_schema(&admin.0, &req.description, req.provider_id, req.model.as_deref()).await;
    wrap(r)
}

async fn schema_apply(
    admin: AdminCtx,
    Json(req): Json<AiSchemaApplyBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::apply_generated_schema(&admin.0, req.schema).await;
    wrap(r)
}

async fn media_analyze(
    admin: AdminCtx,
    Json(req): Json<AiMediaAnalyzeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let r = ai::analyze_media(&admin.0, &req.filename, req.mime.as_deref(), req.context.as_deref(), req.provider_id, req.model.as_deref()).await;
    wrap(r)
}

async fn tools(_admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let defs = ai::tools::definitions();
    let confirmed = ai::tools::definitions()
        .iter()
        .map(|t| (t.name.clone(), ai::requires_confirmation(&t.name)))
        .collect::<Vec<_>>();
    Ok(Json(serde_json::json!({
        "data": { "tools": defs, "requiresConfirmation": confirmed }
    })))
}

async fn usage_list(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let uid = admin.0.current_user.as_ref().map(|u| u.id);
    let r = ai::list_usage(&admin.0, uid, 200).await;
    wrap(r)
}

async fn usage_summary(admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let uid = admin.0.current_user.as_ref().map(|u| u.id);
    let r = ai::usage_summary(&admin.0, uid).await;
    wrap(r)
}
