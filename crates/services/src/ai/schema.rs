//! AI content-type / schema generation.
//!
//! `generate_schema` asks the model to draft a content-type schema (no
//! mutation). `apply_generated_schema` merges a proposed schema into the
//! existing content-type registry through the standard `ctb_apply` path, which
//! performs validation + DDL. Only an authenticated admin may apply.

use ai::{AiMessage, AiRequest};
use serde_json::Value;

use crate::ai::providers::{build_provider, first_enabled_model};
use crate::ai::security::guard_prompt;
use crate::ai::usage::log_usage;
use crate::{AppContext, ServiceError};

/// Have the model draft a content-type schema from a description. Returns the
/// proposed schema as JSON (not yet persisted).
pub async fn generate_schema(
    ctx: &AppContext,
    description: &str,
    provider_id: Option<i64>,
    model: Option<&str>,
) -> Result<serde_json::Value, ServiceError> {
    guard_prompt(description)?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let (pid, name) = match (provider_id, model) {
        (Some(pid), Some(name)) => (pid, name.to_string()),
        _ => first_enabled_model(ctx).await?,
    };
    let (_prow, provider) = build_provider(ctx, pid).await?;

    let system = "You design content-type schemas for FerrisCMS (Strapi-style). \
Produce a single JSON object in this exact shape:\n\
{\"uid\":\"api::<kebab>.plural\",\"kind\":\"collectionType\",\"info\":{\"singularName\":\"...\",\"pluralName\":\"...\",\"displayName\":\"...\"},\"options\":{\"draftAndPublish\":true},\"attributes\":{\"fieldName\":{\"type\":\"string\"}}}\n\
Field types: string, text, integer, decimal, boolean, date, datetime, email, richtext, enumeration (with \"enum\":[]). \
Return ONLY JSON.";

    let request = AiRequest {
        model: name.clone(),
        messages: vec![AiMessage::user(description)],
        system: Some(system.to_string()),
        temperature: Some(0.5),
        max_tokens: Some(1200),
        tools: None,
    };
    let resp = provider.chat(&request).await.map_err(|e| ServiceError::internal(e.to_string()))?;
    let proposed = crate::ai::content::extract_json_proposal(&resp.content).ok_or_else(|| {
        ServiceError::internal("AI returned no parseable schema — try again")
    })?;
    if let Some(uid) = user_id {
        let _ = log_usage(ctx, uid, Some(pid), Some(&name), Some("schema.generate"), resp.usage, Some("ok")).await;
    }
    Ok(serde_json::json!({ "proposed": proposed, "applied": false }))
}

/// Validate a proposed schema and apply it by merging into the registry.
pub async fn apply_generated_schema(
    ctx: &AppContext,
    schema_json: Value,
) -> Result<serde_json::Value, ServiceError> {
    ctx.require_admin()?;
    let schema: core_schema::Schema =
        serde_json::from_value(schema_json).map_err(|e| ServiceError::internal(format!("invalid schema: {e}")))?;

    let mut all = crate::content_type_builder::ctb_list(ctx).await;
    let new_uid = schema.uid.as_str().to_string();
    if all.iter().any(|s| s.uid == schema.uid) {
        return Err(ServiceError::conflict(format!(
            "a content type with uid '{}' already exists",
            schema.uid
        )));
    }
    all.push(schema);
    let applied = crate::content_type_builder::ctb_apply(ctx, all).await?;
    let added = applied
        .iter()
        .find(|s| s.uid.as_str() == new_uid)
        .ok_or_else(|| ServiceError::internal("schema applied but not found in result"))?;
    Ok(serde_json::json!({
        "applied": true,
        "uid": added.uid.as_str(),
        "displayName": added.info.display_name,
    }))
}
