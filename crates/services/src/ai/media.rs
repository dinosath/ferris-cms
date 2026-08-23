//! AI media metadata.
//!
//! `analyze_media` asks the model to suggest descriptive metadata (title, alt
//! text, description, tags) for a media asset based on its filename and MIME
//! type. It returns a proposal only — it never mutates the media record, so
//! there is no risk of the model writing arbitrary data.

use ai::{AiMessage, AiRequest};

use crate::ai::content::extract_json_proposal;
use crate::ai::providers::{build_provider, first_enabled_model};
use crate::ai::usage::log_usage;
use crate::{AppContext, ServiceError};

/// Generate suggested metadata for a media asset.
pub async fn analyze_media(
    ctx: &AppContext,
    filename: &str,
    mime: Option<&str>,
    extra_context: Option<&str>,
    provider_id: Option<i64>,
    model: Option<&str>,
) -> Result<serde_json::Value, ServiceError> {
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let (pid, name) = match (provider_id, model) {
        (Some(pid), Some(name)) => (pid, name.to_string()),
        _ => first_enabled_model(ctx).await?,
    };
    let (_prow, provider) = build_provider(ctx, pid).await?;

    let system = "You generate SEO-friendly media metadata. Given a filename and MIME type, \
return a JSON object with keys: \"title\", \"alt\", \"description\", \"tags\" (array of strings). \
Return ONLY JSON.";
    let user = format!(
        "filename: {filename}\nmime: {}\ncontext: {}",
        mime.unwrap_or("unknown"),
        extra_context.unwrap_or("")
    );
    let request = AiRequest {
        model: name.clone(),
        messages: vec![AiMessage::user(&user)],
        system: Some(system.to_string()),
        temperature: Some(0.4),
        max_tokens: Some(500),
        tools: None,
    };
    let resp = provider.chat(&request).await.map_err(|e| ServiceError::internal(e.to_string()))?;
    let suggested = extract_json_proposal(&resp.content).ok_or_else(|| {
        ServiceError::internal("AI returned no parseable metadata — try again")
    })?;
    if let Some(uid) = user_id {
        let _ = log_usage(ctx, uid, Some(pid), Some(&name), Some("media.analyze"), resp.usage, Some("ok")).await;
    }
    Ok(serde_json::json!({
        "filename": filename,
        "suggested": suggested,
        "applied": false,
    }))
}
