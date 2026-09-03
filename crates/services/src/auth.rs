//! Auth service — admin JWT, login, registration (design Part V §6).

use crate::{AppConfig, AppContext, CurrentUser, ServiceError};
use api_types::admin::{AdminUserDto, InitInfo, LoginRequest, LoginResponse, RegisterAdminRequest};
use chrono::Utc;
use db::entities::{admin_role, admin_user, admin_user_role};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT claims carried in admin tokens.
#[derive(Debug, Serialize, Deserialize)]
pub struct AdminClaims {
    pub sub: i64,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
}

/// Hash a plaintext password with argon2id.
pub fn hash_password(password: &str) -> Result<String, ServiceError> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::from_b64("c29tZXNhbHRzYWx0MTIzNA")
        .map_err(|e| ServiceError::internal(e.to_string()))?;
    use argon2::Argon2;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ServiceError::internal(format!("password hashing: {e}")))
}

/// Verify a password against an argon2 hash.
pub fn verify_password(hash: &str, password: &str) -> Result<bool, ServiceError> {
    use argon2::password_hash::PasswordVerifier;
    use argon2::Argon2;
    let parsed = argon2::PasswordHash::new(hash)
        .map_err(|e| ServiceError::internal(format!("bad password hash: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Sign an admin JWT.
pub fn sign_admin_token(user_id: i64, config: &AppConfig) -> Result<String, ServiceError> {
    let now = Utc::now();
    let claims = AdminClaims {
        sub: user_id,
        iat: now.timestamp(),
        exp: now.timestamp() + config.jwt_expiry_secs,
        jti: Uuid::new_v4().to_string(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| ServiceError::internal(format!("jwt encode: {e}")))
}

/// Decode and validate an admin JWT, returning the user id.
pub fn decode_admin_token(token: &str, config: &AppConfig) -> Result<i64, ServiceError> {
    let mut validation = Validation::default();
    validation.validate_exp = true;
    let data = decode::<AdminClaims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| ServiceError::Unauthorized)?;
    Ok(data.claims.sub)
}

// ---------------------------------------------------------------------------
// Public service functions
// ---------------------------------------------------------------------------

/// Check whether any admin user exists.
pub async fn init_info(ctx: &AppContext) -> Result<InitInfo, ServiceError> {
    let has_admin = db::seed::has_admin(&ctx.db).await?;
    Ok(InitInfo { has_admin })
}

/// Load user roles by user id.
async fn load_user_roles(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
) -> Result<Vec<api_types::admin::AdminRoleDto>, ServiceError> {
    let role_links = admin_user_role::Entity::find()
        .filter(admin_user_role::COLUMN.user_id.eq(user_id))
        .all(db)
        .await?;

    if role_links.is_empty() {
        return Ok(vec![]);
    }

    let role_ids: Vec<i64> = role_links.iter().map(|r| r.role_id).collect();
    let roles = admin_role::Entity::find()
        .filter(admin_role::COLUMN.id.is_in(role_ids))
        .all(db)
        .await?;

    Ok(roles
        .into_iter()
        .map(|r| api_types::admin::AdminRoleDto {
            id: r.id,
            name: r.name,
            code: r.code,
            description: r.description,
        })
        .collect())
}

/// Admin login.
///
/// The `email` field doubles as an identifier: it matches against an admin's
/// email *or* their username, so an operator provisioned from environment
/// configuration (see `bootstrap_admin`) can sign in with `admin`.
pub async fn auth_login(
    ctx: &AppContext,
    req: &LoginRequest,
) -> Result<LoginResponse, ServiceError> {
    // Match on email first, then fall back to username.
    let user = match admin_user::Entity::find()
        .filter(admin_user::COLUMN.email.eq(&req.email))
        .one(&ctx.db)
        .await?
    {
        Some(u) => u,
        None => admin_user::Entity::find()
            .filter(admin_user::COLUMN.username.eq(req.email.clone()))
            .one(&ctx.db)
            .await?
            .ok_or_else(|| ServiceError::Unauthorized)?,
    };

    if !user.is_active || user.blocked {
        return Err(ServiceError::Unauthorized);
    }

    if !verify_password(&user.password_hash, &req.password)? {
        return Err(ServiceError::Unauthorized);
    }

    let token = sign_admin_token(user.id, &ctx.config)?;
    let roles = load_user_roles(&ctx.db, user.id).await?;

    Ok(LoginResponse {
        data: api_types::admin::LoginData {
            token,
            user: AdminUserDto {
                id: user.id,
                email: user.email,
                firstname: user.first_name,
                lastname: user.last_name,
                username: user.username,
                prefered_locale: user.prefered_locale,
                is_active: user.is_active,
                blocked: user.blocked,
                roles,
            },
        },
    })
}

/// Register the first super admin.
pub async fn auth_register(
    ctx: &AppContext,
    req: &RegisterAdminRequest,
) -> Result<LoginResponse, ServiceError> {
    if db::seed::has_admin(&ctx.db).await? {
        return Err(ServiceError::Conflict(
            "An admin user already exists".into(),
        ));
    }

    let password_hash = hash_password(&req.password)?;
    let now = Utc::now();

    let user = admin_user::ActiveModel {
        email: Set(req.email.clone()),
        first_name: Set(req.firstname.clone()),
        last_name: Set(req.lastname.clone()),
        password_hash: Set(password_hash),
        is_active: Set(true),
        blocked: Set(false),
        prefered_locale: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let user = user.insert(&ctx.db).await?;

    // Assign Super Admin role
    let super_role = admin_role::Entity::find()
        .filter(admin_role::COLUMN.code.eq(db::seed::ROLE_SUPER_ADMIN))
        .one(&ctx.db)
        .await?;

    let assigned_role = super_role.clone();
    if let Some(role) = super_role {
        let link = admin_user_role::ActiveModel {
            user_id: Set(user.id),
            role_id: Set(role.id),
            ..Default::default()
        };
        link.insert(&ctx.db).await?;

        // Also assign to SeaORM RBAC role for table-level access control.
        let _ =
            crate::rbac::assign_user_role(&ctx.db, user.id, crate::rbac::ROLE_SUPER_ADMIN).await;
    }

    let token = sign_admin_token(user.id, &ctx.config)?;

    let roles = assigned_role
        .map(|r| api_types::admin::AdminRoleDto {
            id: r.id,
            name: r.name,
            code: r.code,
            description: r.description,
        })
        .into_iter()
        .collect();

    Ok(LoginResponse {
        data: api_types::admin::LoginData {
            token,
            user: AdminUserDto {
                id: user.id,
                email: user.email,
                firstname: user.first_name,
                lastname: user.last_name,
                username: user.username,
                prefered_locale: user.prefered_locale,
                is_active: user.is_active,
                blocked: user.blocked,
                roles,
            },
        },
    })
}

/// The credentials of an admin auto-provisioned from environment config.
#[derive(Clone, Debug)]
pub struct BootstrapAdmin {
    pub username: String,
    pub email: String,
}

/// Provision the initial Super Admin from environment variables at first boot.
///
/// Reads `ADMIN_USERNAME` (default `admin`), `ADMIN_EMAIL` (default
/// `{username}@ferriscms.local`) and `ADMIN_PASSWORD`. When `ADMIN_PASSWORD` is
/// set (non-empty) and no admin user exists yet, a Super Admin is created so the
/// operator never has to register through the UI. Idempotent: once an admin
/// exists, subsequent boots do nothing.
///
/// Returns `Ok(Some(..))` with the created credentials, or `Ok(None)` when
/// nothing was provisioned (no password configured, or an admin already exists).
pub async fn bootstrap_admin(ctx: &AppContext) -> Result<Option<BootstrapAdmin>, ServiceError> {
    let password = match std::env::var("ADMIN_PASSWORD") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => return Ok(None),
    };

    let username = std::env::var("ADMIN_USERNAME")
        .ok()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "admin".into());

    let email = std::env::var("ADMIN_EMAIL")
        .ok()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| format!("{username}@ferriscms.local"));

    // An admin already exists (e.g. registered through the UI previously) —
    // never override existing credentials.
    if db::seed::has_admin(&ctx.db).await? {
        return Ok(None);
    }

    let password_hash = hash_password(&password)?;
    let now = Utc::now();

    let user = admin_user::ActiveModel {
        email: Set(email.clone()),
        first_name: Set(Some("Admin".into())),
        last_name: Set(None),
        username: Set(Some(username.clone())),
        password_hash: Set(password_hash),
        is_active: Set(true),
        blocked: Set(false),
        prefered_locale: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let user = user.insert(&ctx.db).await?;

    // Assign the Super Admin role (app-level + SeaORM RBAC) so the new admin
    // has full access, mirroring `auth_register`.
    if let Some(role) = admin_role::Entity::find()
        .filter(admin_role::COLUMN.code.eq(db::seed::ROLE_SUPER_ADMIN))
        .one(&ctx.db)
        .await?
    {
        admin_user_role::ActiveModel {
            user_id: Set(user.id),
            role_id: Set(role.id),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    let _ =
        crate::rbac::assign_user_role(&ctx.db, user.id, crate::rbac::ROLE_SUPER_ADMIN).await;

    Ok(Some(BootstrapAdmin { username, email }))
}

/// Build a CurrentUser from a user id.
pub async fn load_current_user(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
) -> Result<CurrentUser, ServiceError> {
    let user = admin_user::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or(ServiceError::Unauthorized)?;

    let role_links = admin_user_role::Entity::find()
        .filter(admin_user_role::COLUMN.user_id.eq(user_id))
        .all(db)
        .await?;

    let role_ids: Vec<i64> = role_links.iter().map(|r| r.role_id).collect();
    let roles: Vec<String> = if role_ids.is_empty() {
        vec![]
    } else {
        admin_role::Entity::find()
            .filter(admin_role::COLUMN.id.is_in(role_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|r| r.code)
            .collect()
    };

    Ok(CurrentUser {
        id: user.id,
        email: user.email,
        is_active: user.is_active && !user.blocked,
        roles,
    })
}
