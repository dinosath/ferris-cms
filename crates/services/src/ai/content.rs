//! AI content generation, editing, and translation.
//!
//! These run as single-shot feature endpoints (not assistant tools). They build
//! a schema-aware prompt, call the configured provider, parse the structured
//! reply, validate it, and — only when `apply` is requested and the user holds
//! the RBAC permission — persist via the existing content service.

use ai::{AiMessage, AiRequest, AiUsage};
use serde_json::{json, Value};

use crate::ai::providers::{build_provider, first_enabled_model};
use crate::ai::security::guard_prompt;
use crate::ai::usage::log_usage;
use crate::{AppContext, ServiceError};

/// Resolve provider + model, preferring an explicit `(provider_id, model)`
/// when given, otherwise the first enabled provider + model.
async fn resolve(
    ctx: &AppContext,
    provider_id: Option<i64>,
    model: Option<&str>,
) -> Result<(i64, String, Box<dyn ai::AiProvider>), ServiceError> {
    let (pid, name) = match (provider_id, model) {
        (Some(pid), Some(name)) => (pid, name.to_string()),
        _ => first_enabled_model(ctx).await?,
    };
    let (_row, provider) = build_provider(ctx, pid).await?;
    Ok((pid, name, provider))
}

/// Describe the fields of a content type so the model can produce valid data.
fn schema_prompt(uid: &str, schema: &core_schema::Schema, fields: &[String]) -> String {
    let mut attrs: Vec<Value> = Vec::new();
    for (name, attr) in &schema.attributes {
        if !fields.is_empty() && !fields.iter().any(|f| f == name) {
            continue;
        }
        let ty = format!("{:?}", attr.attr_type).to_lowercase();
        let required = attr.required;
        attrs.push(json!({ "field": name, "type": ty, "required": required }));
    }
    json!({
        "contentTypeUid": uid,
        "contentType": schema.info.display_name,
        "fields": attrs,
    })
    .to_string()
}

/// Strip markdown code fences / prose so we can parse a JSON object.
fn extract_json(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    if let Some(open) = trimmed.find('{') {
        let mut depth = 0i32;
        for (i, b) in trimmed[open..].bytes().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let slice = &trimmed[open..open + i + 1];
                        if let Ok(v) = serde_json::from_str::<Value>(slice) {
                            return Some(v);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Public wrapper so other modules (e.g. schema generation) can parse a model's
/// structured reply.
pub fn extract_json_proposal(content: &str) -> Option<Value> {
    extract_json(content)
}

/// Record usage + audit for a feature call.
async fn audit(
    ctx: &AppContext,
    user_id: Option<i64>,
    provider_id: i64,
    model: &str,
    feature: &str,
    usage: AiUsage,
    status: &str,
) {
    if let Some(uid) = user_id {
        let _ = log_usage(ctx, uid, Some(provider_id), Some(model), Some(feature), usage, Some(status)).await;
    }
}

/// Generate content for a content type from a natural-language prompt.
///
/// Returns the proposed field values. When `apply` is true the entry is
/// created through the RBAC-enforcing content service.
#[allow(clippy::too_many_arguments)]
pub async fn generate_content(
    ctx: &AppContext,
    uid: &str,
    prompt: &str,
    fields: Vec<String>,
    apply: bool,
    provider_id: Option<i64>,
    model: Option<&str>,
) -> Result<serde_json::Value, ServiceError> {
    guard_prompt(prompt)?;
    let schema = crate::content::load_schema(ctx, uid)?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let (pid, name, provider) = resolve(ctx, provider_id, model).await?;

    let system = format!(
        "You are FerrisCMS's content writer. Produce a JSON object with values only for the fields described below. \
Do not include system metadata (id, documentId, locale, publicationState, timestamps). Return ONLY JSON.\n{}",
        schema_prompt(uid, &schema, &fields)
    );
    let request = AiRequest {
        model: name.clone(),
        messages: vec![AiMessage::user(prompt)],
        system: Some(system),
        temperature: Some(0.7),
        max_tokens: Some(1200),
        tools: None,
    };
    let resp = provider.chat(&request).await.map_err(|e| ServiceError::internal(e.to_string()))?;
    let data = extract_json(&resp.content).ok_or_else(|| {
        ServiceError::internal("AI returned no parseable JSON content — try again")
    })?;
    audit(ctx, user_id, pid, &name, "content.generate", resp.usage, "ok").await;

    if apply {
        let created = crate::content::cm_create(ctx, uid, &data).await?;
        audit(ctx, user_id, pid, &name, "content.generate.apply", resp.usage, "ok").await;
        return Ok(json!({ "proposed": data, "applied": true, "documentId": created.data.get("documentId") }));
    }
    Ok(json!({ "proposed": data, "applied": false }))
}

/// Edit an existing entry by natural-language instruction.
pub async fn edit_content(
    ctx: &AppContext,
    uid: &str,
    document_id: &str,
    instruction: &str,
    provider_id: Option<i64>,
    model: Option<&str>,
) -> Result<serde_json::Value, ServiceError> {
    guard_prompt(instruction)?;
    let current = crate::content::cm_get(ctx, uid, document_id, None).await?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let (pid, name, provider) = resolve(ctx, provider_id, model).await?;

    let system = format!(
        "You are FerrisCMS's editor. Given the current entry JSON and an editing instruction, \
return the FULL updated entry JSON (keep existing fields you don't change). Return ONLY JSON.\nCurrent entry:\n{}",
        current.data
    );
    let request = AiRequest {
        model: name.clone(),
        messages: vec![AiMessage::user(instruction)],
        system: Some(system),
        temperature: Some(0.4),
        max_tokens: Some(1500),
        tools: None,
    };
    let resp = provider.chat(&request).await.map_err(|e| ServiceError::internal(e.to_string()))?;
    let updated = extract_json(&resp.content).ok_or_else(|| {
        ServiceError::internal("AI returned no parseable JSON — try again")
    })?;
    let applied = crate::content::cm_update(ctx, uid, document_id, &updated).await?;
    audit(ctx, user_id, pid, &name, "content.edit", resp.usage, "ok").await;
    Ok(json!({ "updated": true, "data": applied.data }))
}

/// Translate a text string to a target locale.
pub async fn translate_text(
    ctx: &AppContext,
    text: &str,
    target_locale: &str,
    model: Option<&str>,
) -> Result<(String, AiUsage), ServiceError> {
    guard_prompt(text)?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let (pid, name, provider) = resolve(ctx, None, model).await?;
    let system = format!("You are a translation engine. Translate the user's text to '{target_locale}'. Return ONLY the translation, no commentary.");
    let request = AiRequest {
        model: name.clone(),
        messages: vec![AiMessage::user(text)],
        system: Some(system),
        temperature: Some(0.2),
        max_tokens: Some(800),
        tools: None,
    };
    let resp = provider.chat(&request).await.map_err(|e| ServiceError::internal(e.to_string()))?;
    let translated = resp.content.trim().to_string();
    audit(ctx, user_id, pid, &name, "content.translate", resp.usage, "ok").await;
    Ok((translated, resp.usage))
}
