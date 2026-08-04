//! API tokens service (design Part V §6).
//!
//! CRUD for public API tokens. The raw access key is returned once on create;
//! only its SHA-256 hash is stored. Token type (`read-only` / `full-access` /
//! `custom`) and per-token permission actions are supported.

use crate::{AppContext, ServiceError};
use api_types::admin::{ApiTokenDto, CreateApiTokenRequest};
use core_domain::ApiTokenType;
use db::entities::{api_token, api_token_permission};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use sha2::{Digest, Sha256};

/// Generate a cryptographically-random access key with a readable prefix.
fn generate_access_key() -> String {
    use rand::Rng;
    let key: String = rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();
    format!("ferris_{key}")
}

/// SHA-256 hex of the access key (what we store).
fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn to_dto(token: api_token::Model, access_key: Option<String>) -> ApiTokenDto {
    ApiTokenDto {
        id: token.id,
        name: token.name,
        description: token.description,
        token_type: token.token_type,
        last_used_at: token.last_used_at,
        expires_at: token.expires_at,
        lifespan: token.lifespan,
        created_at: token.created_at,
        access_key,
        permissions: vec![],
    }
}

/// List all API tokens (most recent first).
pub async fn api_token_list(ctx: &AppContext) -> Result<Vec<ApiTokenDto>, ServiceError> {
    let tokens = api_token::Entity::find()
        .order_by_desc(api_token::COLUMN.created_at)
        .all(&ctx.db)
        .await?;
    Ok(tokens.into_iter().map(|t| to_dto(t, None)).collect())
}

/// Create an API token; returns the raw access key once.
pub async fn api_token_create(
    ctx: &AppContext,
    req: &CreateApiTokenRequest,
) -> Result<ApiTokenDto, ServiceError> {
    if req.name.trim().is_empty() {
        return Err(ServiceError::validation("create-api-token", vec![
            crate::ValidationErrorItem::new(vec!["name".into()], "name is required", "ValidationError"),
        ]));
    }

    let access_key = generate_access_key();
    let now = chrono::Utc::now();
    let expires_at = req.lifespan.map(|secs| now + chrono::Duration::seconds(secs));

    let model = api_token::ActiveModel {
        name: Set(req.name.clone()),
        description: Set(req.description.clone()),
        token_type: Set(req.token_type.as_db_str().to_string()),
        access_key_hash: Set(hash_key(&access_key)),
        last_used_at: Set(None),
        expires_at: Set(expires_at),
        lifespan: Set(req.lifespan),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let token = model.insert(&ctx.db).await?;

    // Persist permission actions for custom tokens.
    if req.token_type == ApiTokenType::Custom {
        for action in &req.permissions {
            let perm = api_token_permission::ActiveModel {
                token_id: Set(token.id),
                action: Set(action.clone()),
                ..Default::default()
            };
            perm.insert(&ctx.db).await?;
        }
    }

    let mut dto = to_dto(token, Some(access_key));
    dto.permissions = req.permissions.clone();
    Ok(dto)
}

/// Delete an API token by id.
pub async fn api_token_delete(ctx: &AppContext, id: i64) -> Result<(), ServiceError> {
    api_token_permission::Entity::delete_many()
        .filter(api_token_permission::COLUMN.token_id.eq(id))
        .exec(&ctx.db)
        .await?;
    api_token::Entity::delete_many()
        .filter(api_token::COLUMN.id.eq(id))
        .exec(&ctx.db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_hashing() {
        let k = "ferris_abc123";
        let h1 = hash_key(k);
        let h2 = hash_key(k);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert_ne!(h1, hash_key("ferris_other"));
    }
}
