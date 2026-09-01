//! Credential service for workflow integration nodes.
//!
//! Credentials are stored **encrypted** at rest (ChaCha20-Poly1305 AEAD keyed
//! by a SHA-256 hash of the server's JWT secret). Decrypted values are only
//! handed to node executors at run time and are never persisted into workflow
//! JSON, execution records, node-run output, or returned by the API.

use crate::{AppContext, ServiceError};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use db::entities::workflow_credential;
use sea_orm::{
    ActiveModelTrait, EntityTrait, PaginatorTrait, QueryOrder, Set,
};
use sha2::{Digest, Sha256};
use ::workflow::model::OwsCredential;

/// Credential type keys recognized by the engine.
pub const CRED_HTTP_API: &str = "httpApi";
pub const CRED_HTTP_HEADER: &str = "httpHeaderAuth";
pub const CRED_HTTP_BASIC: &str = "httpBasicAuth";
pub const CRED_POSTGRES: &str = "postgres";
pub const CRED_REDIS: &str = "redis";

/// All supported credential types (used by the credentials UI).
pub fn credential_types() -> Vec<(&'static str, &'static str)> {
    vec![
        (CRED_HTTP_API, "HTTP Request"),
        (CRED_HTTP_HEADER, "HTTP Header Auth"),
        (CRED_HTTP_BASIC, "HTTP Basic Auth"),
        (CRED_POSTGRES, "PostgreSQL"),
        (CRED_REDIS, "Redis"),
    ]
}

/// Derive a fixed 32-byte AEAD key from the JWT secret.
fn derive_key(jwt_secret: &str) -> chacha20poly1305::Key {
    let mut hasher = Sha256::new();
    hasher.update(jwt_secret.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    chacha20poly1305::Key::from(key)
}

/// Encrypt a plaintext JSON value into a base64 string.
pub fn encrypt_value(secret: &str, plaintext: &serde_json::Value) -> Result<String, ServiceError> {
    let cipher = ChaCha20Poly1305::new(&derive_key(secret));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let plain = serde_json::to_vec(plaintext)
        .map_err(|e| ServiceError::internal(format!("credential serialization: {e}")))?;
    let ct = cipher
        .encrypt(&nonce, plain.as_ref())
        .map_err(|_| ServiceError::internal("credential encryption failed"))?;
    let mut blob = Vec::with_capacity(nonce.len() + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &blob))
}

/// Decrypt a base64 credential blob back into a JSON value.
pub fn decrypt_value(secret: &str, blob: &str) -> Result<serde_json::Value, ServiceError> {
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, blob)
        .map_err(|_| ServiceError::internal("credential decode failed"))?;
    if decoded.len() < 12 {
        return Err(ServiceError::internal("invalid credential blob"));
    }
    let (nonce_bytes, ct) = decoded.split_at(12);
    let cipher = ChaCha20Poly1305::new(&derive_key(secret));
    let nonce = Nonce::from_slice(nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ct)
        .map_err(|_| ServiceError::internal("credential decryption failed"))?;
    serde_json::from_slice(&plain)
        .map_err(|e| ServiceError::internal(format!("credential deserialization: {e}")))
}

fn to_dto(m: &workflow_credential::Model) -> OwsCredential {
    OwsCredential {
        id: m.id,
        name: m.name.clone(),
        credential_type: m.credential_type.clone(),
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

/// List credentials (metadata only, never decrypted data).
pub async fn credential_list(ctx: &AppContext) -> Result<Vec<OwsCredential>, ServiceError> {
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::workflow::action::VIEW_CREDENTIALS,
        crate::workflow::action::SUBJECT_WORKFLOW,
    )
    .await?;
    let rows = workflow_credential::Entity::find()
        .order_by_asc(workflow_credential::Column::Name)
        .all(&ctx.db)
        .await?;
    Ok(rows.iter().map(to_dto).collect())
}

/// Get a credential's decrypted data (executor path; not exposed via API).
pub async fn credential_get_data(
    ctx: &AppContext,
    id: i64,
) -> Result<serde_json::Value, ServiceError> {
    let row = workflow_credential::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("credential not found"))?;
    decrypt_value(&ctx.config.jwt_secret, &row.data_encrypted)
}

/// Get one credential's metadata.
pub async fn credential_get(ctx: &AppContext, id: i64) -> Result<OwsCredential, ServiceError> {
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::workflow::action::VIEW_CREDENTIALS,
        crate::workflow::action::SUBJECT_WORKFLOW,
    )
    .await?;
    let row = workflow_credential::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("credential not found"))?;
    Ok(to_dto(&row))
}

/// Create a credential. `data` is the plaintext JSON credential (e.g.
/// `{ "headerName": "Authorization", "headerValue": "Bearer ..." }`).
pub async fn credential_create(
    ctx: &AppContext,
    name: &str,
    credential_type: &str,
    data: &serde_json::Value,
) -> Result<OwsCredential, ServiceError> {
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::workflow::action::MANAGE_CREDENTIALS,
        crate::workflow::action::SUBJECT_WORKFLOW,
    )
    .await?;
    let now = chrono::Utc::now();
    let encrypted = encrypt_value(&ctx.config.jwt_secret, data)?;
    let row = workflow_credential::ActiveModel {
        name: Set(name.to_string()),
        credential_type: Set(credential_type.to_string()),
        data_encrypted: Set(encrypted),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(to_dto(&row))
}

/// Update a credential (replaces the encrypted data).
pub async fn credential_update(
    ctx: &AppContext,
    id: i64,
    name: Option<&str>,
    credential_type: Option<&str>,
    data: Option<&serde_json::Value>,
) -> Result<OwsCredential, ServiceError> {
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::workflow::action::MANAGE_CREDENTIALS,
        crate::workflow::action::SUBJECT_WORKFLOW,
    )
    .await?;
    let existing = workflow_credential::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("credential not found"))?;
    let now = chrono::Utc::now();
    let mut am: workflow_credential::ActiveModel = existing.into();
    if let Some(n) = name {
        am.name = Set(n.to_string());
    }
    if let Some(t) = credential_type {
        am.credential_type = Set(t.to_string());
    }
    if let Some(d) = data {
        am.data_encrypted = Set(encrypt_value(&ctx.config.jwt_secret, d)?);
    }
    am.updated_at = Set(now);
    let row = am.update(&ctx.db).await?;
    Ok(to_dto(&row))
}

/// Delete a credential.
pub async fn credential_delete(ctx: &AppContext, id: i64) -> Result<(), ServiceError> {
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::workflow::action::MANAGE_CREDENTIALS,
        crate::workflow::action::SUBJECT_WORKFLOW,
    )
    .await?;
    let row = workflow_credential::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("credential not found"))?;
    let am: workflow_credential::ActiveModel = row.into();
    am.delete(&ctx.db).await?;
    Ok(())
}

/// Count of credentials (metadata helper).
pub async fn credential_count(ctx: &AppContext) -> Result<u64, ServiceError> {
    Ok(workflow_credential::Entity::find()
        .count(&ctx.db)
        .await?)
}

/// Redact sensitive values from a credential data object before it is ever
/// placed in a log or node-run record. (The engine already avoids persisting
/// credential data; this is a belt-and-suspenders helper.)
pub fn redact(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                let k_lower = k.to_lowercase();
                if k_lower.contains("password")
                    || k_lower.contains("secret")
                    || k_lower.contains("token")
                    || k_lower.contains("apikey")
                    || k_lower.contains("api_key")
                    || k_lower == "value"
                    || k_lower == "headervalue"
                {
                    *val = serde_json::Value::String("***redacted***".into());
                } else {
                    redact(val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_roundtrip() {
        let data = serde_json::json!({ "headerName": "Authorization", "headerValue": "Bearer abc" });
        let enc = encrypt_value("test-secret", &data).unwrap();
        assert!(!enc.contains("Authorization"));
        let dec = decrypt_value("test-secret", &enc).unwrap();
        assert_eq!(dec, data);
        // Wrong secret fails.
        assert!(decrypt_value("other-secret", &enc).is_err());
    }

    #[test]
    fn redact_removes_secrets() {
        let mut v = serde_json::json!({
            "headerName": "Authorization",
            "headerValue": "Bearer xyz",
            "nested": { "apiKey": "secret-value", "safe": "ok" }
        });
        redact(&mut v);
        assert_eq!(v["headerValue"], "***redacted***");
        assert_eq!(v["nested"]["apiKey"], "***redacted***");
        assert_eq!(v["nested"]["safe"], "ok");
        assert_eq!(v["headerName"], "Authorization");
    }
}
