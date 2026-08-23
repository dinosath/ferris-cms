//! AI provider + model CRUD.
//!
//! Provider API keys are encrypted at rest (ChaCha20-Poly1305 keyed by the
//! server JWT secret) and are never returned by the API. `build_provider`
//! decrypts the key at call time and constructs a provider instance.

use crate::ai::kind_from_str;
use crate::workflow::credentials::{decrypt_value, encrypt_value};
use crate::{AppContext, ServiceError};
use ai::AiProviderConfig;
use db::entities::{ai_model, ai_provider};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set,
};

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

/// List providers (metadata only — never the encrypted key).
pub async fn list_providers(ctx: &AppContext) -> Result<Vec<serde_json::Value>, ServiceError> {
    let rows = ai_provider::Entity::find()
        .order_by_asc(ai_provider::Column::SortOrder)
        .order_by_asc(ai_provider::Column::Id)
        .all(&ctx.db)
        .await?;
    Ok(rows.into_iter().map(provider_dto).collect())
}

/// Get a single provider (metadata only).
pub async fn get_provider(ctx: &AppContext, id: i64) -> Result<serde_json::Value, ServiceError> {
    let row = ai_provider::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai provider {id}")))?;
    Ok(provider_dto(row))
}

/// Create a provider. `api_key` may be empty for local providers (Ollama).
pub async fn create_provider(
    ctx: &AppContext,
    name: String,
    kind: String,
    base_url: Option<String>,
    api_key: Option<String>,
    organization: Option<String>,
    enabled: bool,
    sort_order: Option<i64>,
) -> Result<serde_json::Value, ServiceError> {
    kind_from_str(&kind)?;
    let api_key_encrypted = api_key
        .filter(|k| !k.trim().is_empty())
        .map(|k| encrypt_value(&ctx.config.jwt_secret, &serde_json::json!(k)))
        .transpose()?;

    let now = chrono::Utc::now();
    let row = ai_provider::ActiveModel {
        name: Set(name),
        kind: Set(kind),
        base_url: Set(base_url),
        api_key_encrypted: Set(api_key_encrypted),
        organization: Set(organization),
        enabled: Set(enabled),
        sort_order: Set(sort_order),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(provider_dto(row))
}

/// Update a provider. `api_key` is optional: when supplied it replaces the
/// stored key; when omitted (None) the existing key is retained.
pub async fn update_provider(
    ctx: &AppContext,
    id: i64,
    name: String,
    kind: String,
    base_url: Option<String>,
    api_key: Option<String>,
    organization: Option<String>,
    enabled: bool,
    sort_order: Option<i64>,
) -> Result<serde_json::Value, ServiceError> {
    kind_from_str(&kind)?;
    let existing = ai_provider::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai provider {id}")))?;
    let mut am: ai_provider::ActiveModel = existing.clone().into();
    am.name = Set(name);
    am.kind = Set(kind);
    am.base_url = Set(base_url);
    am.organization = Set(organization);
    am.enabled = Set(enabled);
    am.sort_order = Set(sort_order);
    am.updated_at = Set(chrono::Utc::now());
    if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
        am.api_key_encrypted = Set(Some(encrypt_value(
            &ctx.config.jwt_secret,
            &serde_json::json!(key),
        )?));
    }
    let row = am.update(&ctx.db).await?;
    Ok(provider_dto(row))
}

/// Delete a provider and its models.
pub async fn delete_provider(ctx: &AppContext, id: i64) -> Result<(), ServiceError> {
    let row = ai_provider::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai provider {id}")))?;
    ai_model::Entity::delete_many()
        .filter(ai_model::Column::ProviderId.eq(id))
        .exec(&ctx.db)
        .await?;
    let am: ai_provider::ActiveModel = row.into();
    am.delete(&ctx.db).await?;
    Ok(())
}

fn provider_dto(row: ai_provider::Model) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "name": row.name,
        "kind": row.kind,
        "baseUrl": row.base_url,
        "organization": row.organization,
        "enabled": row.enabled,
        "sortOrder": row.sort_order,
        "createdAt": row.created_at,
        "updatedAt": row.updated_at,
        // Never expose the key.
    })
}

/// Build a configured provider instance from a provider row (decrypting the key).
pub async fn build_provider(
    ctx: &AppContext,
    provider_id: i64,
) -> Result<(ai_provider::Model, Box<dyn ai::AiProvider>), ServiceError> {
    let row = ai_provider::Entity::find_by_id(provider_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai provider {provider_id}")))?;
    if !row.enabled {
        return Err(ServiceError::internal(format!("ai provider '{}' is disabled", row.name)));
    }
    let kind = kind_from_str(&row.kind)?;
    let api_key = match &row.api_key_encrypted {
        Some(blob) => Some(
            decrypt_value(&ctx.config.jwt_secret, blob)?
                .as_str()
                .unwrap_or_default()
                .to_string(),
        ),
        None => None,
    };
    let config = AiProviderConfig {
        kind,
        base_url: row.base_url.clone().unwrap_or_default(),
        api_key,
        organization: row.organization.clone(),
    };
    let provider = ai::from_config(&config)
        .map_err(|e| ServiceError::internal(e.to_string()))?;
    Ok((row, provider))
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/// List models for a provider (or all when `provider_id` is None).
pub async fn list_models(
    ctx: &AppContext,
    provider_id: Option<i64>,
) -> Result<Vec<serde_json::Value>, ServiceError> {
    let mut q = ai_model::Entity::find().order_by_asc(ai_model::Column::Id);
    if let Some(pid) = provider_id {
        q = q.filter(ai_model::Column::ProviderId.eq(pid));
    }
    let rows = q.all(&ctx.db).await?;
    Ok(rows.into_iter().map(model_dto).collect())
}

/// Create a model for a provider.
#[allow(clippy::too_many_arguments)]
pub async fn create_model(
    ctx: &AppContext,
    provider_id: i64,
    name: String,
    display_name: Option<String>,
    description: Option<String>,
    supports_chat: bool,
    supports_tools: bool,
    supports_streaming: bool,
    max_input_tokens: Option<i64>,
    max_output_tokens: Option<i64>,
    enabled: bool,
) -> Result<serde_json::Value, ServiceError> {
    ai_provider::Entity::find_by_id(provider_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai provider {provider_id}")))?;
    let now = chrono::Utc::now();
    let row = ai_model::ActiveModel {
        provider_id: Set(provider_id),
        name: Set(name),
        display_name: Set(display_name),
        description: Set(description),
        supports_chat: Set(supports_chat),
        supports_tools: Set(supports_tools),
        supports_streaming: Set(supports_streaming),
        max_input_tokens: Set(max_input_tokens),
        max_output_tokens: Set(max_output_tokens),
        enabled: Set(enabled),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(model_dto(row))
}

/// Update a model.
#[allow(clippy::too_many_arguments)]
pub async fn update_model(
    ctx: &AppContext,
    id: i64,
    name: String,
    display_name: Option<String>,
    description: Option<String>,
    supports_chat: bool,
    supports_tools: bool,
    supports_streaming: bool,
    max_input_tokens: Option<i64>,
    max_output_tokens: Option<i64>,
    enabled: bool,
) -> Result<serde_json::Value, ServiceError> {
    let existing = ai_model::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai model {id}")))?;
    let mut am: ai_model::ActiveModel = existing.into();
    am.name = Set(name);
    am.display_name = Set(display_name);
    am.description = Set(description);
    am.supports_chat = Set(supports_chat);
    am.supports_tools = Set(supports_tools);
    am.supports_streaming = Set(supports_streaming);
    am.max_input_tokens = Set(max_input_tokens);
    am.max_output_tokens = Set(max_output_tokens);
    am.enabled = Set(enabled);
    am.updated_at = Set(chrono::Utc::now());
    let row = am.update(&ctx.db).await?;
    Ok(model_dto(row))
}

/// Delete a model.
pub async fn delete_model(ctx: &AppContext, id: i64) -> Result<(), ServiceError> {
    let row = ai_model::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai model {id}")))?;
    let am: ai_model::ActiveModel = row.into();
    am.delete(&ctx.db).await?;
    Ok(())
}

fn model_dto(row: ai_model::Model) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "providerId": row.provider_id,
        "name": row.name,
        "displayName": row.display_name,
        "description": row.description,
        "supportsChat": row.supports_chat,
        "supportsTools": row.supports_tools,
        "supportsStreaming": row.supports_streaming,
        "maxInputTokens": row.max_input_tokens,
        "maxOutputTokens": row.max_output_tokens,
        "enabled": row.enabled,
        "createdAt": row.created_at,
        "updatedAt": row.updated_at,
    })
}

/// Resolve the first enabled provider + model that supports chat, so feature
/// endpoints (generate/edit/translate) can run without the caller pinning a
/// specific model. Returns `(provider_id, model_name)`.
pub async fn first_enabled_model(ctx: &AppContext) -> Result<(i64, String), ServiceError> {
    let provider = ai_provider::Entity::find()
        .filter(ai_provider::Column::Enabled.eq(true))
        .order_by_asc(ai_provider::Column::SortOrder)
        .order_by_asc(ai_provider::Column::Id)
        .one(&ctx.db)
        .await?;
    let pid = match provider {
        Some(p) => p.id,
        None => {
            return Err(ServiceError::internal(
                "no AI provider is enabled — configure one in AI Settings first",
            ))
        }
    };
    let model = ai_model::Entity::find()
        .filter(ai_model::Column::ProviderId.eq(pid))
        .filter(ai_model::Column::Enabled.eq(true))
        .order_by_asc(ai_model::Column::Id)
        .one(&ctx.db)
        .await?;
    let name = match model {
        Some(m) => m.name,
        None => {
            return Err(ServiceError::internal(
                "the enabled AI provider has no enabled model — add one in AI Settings first",
            ))
        }
    };
    Ok((pid, name))
}
