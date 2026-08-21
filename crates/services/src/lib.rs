//! services — business logic for the ferriscms headless CMS.
//!
//! Design Part II §2: the only crate that transports call.
//! Each service method receives an `AppContext` with db, current user, config.
//!
//! Module map:
//! - `auth` — admin JWT, login, registration, API tokens
//! - `content_type_builder` — validate → diff → DDL → registry → router rebuild
//! - `content` — dynamic CRUD, filters/sort/pagination/populate, draft/publish, i18n
//! - `media` — upload, thumbnails, folder CRUD, media picker
//! - `rbac` — role-based access control enforcement
//! - `i18n` — locale CRUD, localized content lookup
//! - `schema_cache` — lock-free schema cache (arc-swap), rebuilt on save

mod api_tokens;
mod auth;
mod content;
mod content_type_builder;
mod i18n;
pub mod import_export;
mod media;
mod rbac;
mod schema_cache;
pub mod workflow;

pub use api_tokens::*;
pub use auth::*;
pub use content::*;
pub use content_type_builder::*;
pub use i18n::*;
pub use import_export::*;
pub use media::*;
pub use rbac::*;
pub use schema_cache::*;
pub use workflow::*;

use sea_orm::DatabaseConnection;

// ---------------------------------------------------------------------------
// Error model (design Part II §8)
// ---------------------------------------------------------------------------

/// Service-level error, mapped by `api-rest` to Strapi-compatible bodies.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("validation error")]
    Validation(Vec<ValidationErrorItem>),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("forbidden")]
    Forbidden,

    #[error("unauthorized")]
    Unauthorized,

    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),

    #[error("store error: {0}")]
    Store(#[from] dynamic_store::StoreError),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("rbac error: {0}")]
    Rbac(String),
}

impl ServiceError {
    pub fn validation(_msg: impl Into<String>, errors: Vec<ValidationErrorItem>) -> Self {
        Self::Validation(errors)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

/// One validation error item, compatible with Strapi's error-details format.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ValidationErrorItem {
    pub path: Vec<String>,
    pub message: String,
    pub name: String,
}

impl ValidationErrorItem {
    pub fn new(path: Vec<String>, message: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            path,
            message: message.into(),
            name: name.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// AppContext — the shared context every service method receives
// ---------------------------------------------------------------------------

/// Current user identity extracted from auth middleware.
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i64,
    pub email: String,
    pub is_active: bool,
    pub roles: Vec<String>,
}

/// Shared application context passed through every service call.
///
/// Cloneable so each HTTP request can build its own context carrying the
/// authenticated identity. `db` and `schema_cache` are cheap clones sharing
/// the same underlying connections / snapshot; only `current_user` differs.
#[derive(Clone)]
pub struct AppContext {
    pub db: DatabaseConnection,
    pub current_user: Option<CurrentUser>,
    pub config: AppConfig,
    pub schema_cache: SchemaCache,
}

/// Server configuration (design Part II §6).
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Database driver hint: "sqlite" or "postgres".
    pub db_driver: String,
    /// JWT signing secret (HS256).
    pub jwt_secret: String,
    /// Token expiry in seconds (default 30 days).
    pub jwt_expiry_secs: i64,
    /// Whether to serve the admin registration endpoint.
    pub admin_registration_open: bool,
    /// Directory for storing uploaded media files.
    pub media_storage_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            db_driver: "sqlite".into(),
            jwt_secret: "change-me-in-production".into(),
            jwt_expiry_secs: 30 * 24 * 3600,
            admin_registration_open: true,
            media_storage_dir: "media".into(),
        }
    }
}

impl AppContext {
    /// Create a new context with a fresh schema cache.
    pub fn new(db: DatabaseConnection, config: AppConfig) -> Self {
        Self {
            db,
            current_user: None,
            config,
            schema_cache: SchemaCache::empty(),
        }
    }

    /// Returns true when a user is authenticated.
    pub fn is_authenticated(&self) -> bool {
        self.current_user.is_some()
    }

    /// Require an authenticated admin user or return `Unauthorized`.
    pub fn require_admin(&self) -> Result<&CurrentUser, ServiceError> {
        self.current_user.as_ref().ok_or(ServiceError::Unauthorized)
    }

    /// Return a per-request clone with the given authenticated identity.
    /// Sharing `db` and `schema_cache` while isolating `current_user`.
    pub fn with_user(&self, user: Option<CurrentUser>) -> Self {
        let mut c = self.clone();
        c.current_user = user;
        c
    }

    /// Get the sea-orm backend variant.
    pub fn db_backend(&self) -> sea_orm::DbBackend {
        if self.config.db_driver.contains("postgres") || self.config.db_driver.contains("postgre") {
            sea_orm::DbBackend::Postgres
        } else {
            sea_orm::DbBackend::Sqlite
        }
    }

    /// Get a SeaORM RBAC-restricted connection for the current user.
    /// Returns the raw database connection if no user is authenticated (public).
    /// Requires that `init_rbac` was called at boot.
    pub fn restricted_connection(&self) -> Result<sea_orm::RestrictedConnection, ServiceError> {
        let user_id = self.current_user.as_ref().map(|u| u.id).unwrap_or(0); // 0 = unauthenticated/public

        self.db
            .restricted_for(sea_orm::rbac::RbacUserId(user_id))
            .map_err(|e| ServiceError::Rbac(format!("failed to create restricted connection: {e}")))
    }

    /// Initialize SeaORM RBAC engine and standard roles/permissions.
    /// Idempotent — safe to call on every boot.
    pub async fn init_rbac(&self) -> Result<(), ServiceError> {
        crate::rbac::init_rbac(&self.db)
            .await
            .map_err(|e| ServiceError::Rbac(format!("rbac init failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_error_display_and_helpers() {
        assert_eq!(
            ServiceError::Validation(vec![]).to_string(),
            "validation error"
        );
        assert_eq!(
            ServiceError::NotFound("x".into()).to_string(),
            "not found: x"
        );
        assert_eq!(
            ServiceError::Conflict("c".into()).to_string(),
            "conflict: c"
        );
        assert_eq!(ServiceError::Forbidden.to_string(), "forbidden");
        assert_eq!(ServiceError::Unauthorized.to_string(), "unauthorized");
        assert_eq!(
            ServiceError::Internal("i".into()).to_string(),
            "internal error: i"
        );
        assert_eq!(ServiceError::Rbac("r".into()).to_string(), "rbac error: r");

        assert_eq!(ServiceError::not_found("n").to_string(), "not found: n");
        assert_eq!(ServiceError::conflict("f").to_string(), "conflict: f");
        assert_eq!(ServiceError::internal("x").to_string(), "internal error: x");

        let item = ValidationErrorItem::new(vec!["a".into()], "m", "ValidationError");
        assert_eq!(item.path, vec!["a"]);
        assert_eq!(item.message, "m");
        assert_eq!(item.name, "ValidationError");
    }

    #[tokio::test]
    async fn app_context_auth_and_backend() {
        let db = db::connect_sqlite_memory().await.unwrap();
        let config = AppConfig {
            db_driver: "sqlite".into(),
            ..Default::default()
        };
        let ctx = AppContext::new(db.clone(), config);

        assert!(!ctx.is_authenticated());
        assert!(ctx.require_admin().is_err());
        assert_eq!(ctx.db_backend(), sea_orm::DbBackend::Sqlite);

        let user = CurrentUser {
            id: 7,
            email: "a@b.dev".into(),
            is_active: true,
            roles: vec!["strapi-editor".into()],
        };
        let c2 = ctx.with_user(Some(user.clone()));
        assert!(c2.is_authenticated());
        let admin = c2.require_admin().expect("admin present");
        assert_eq!(admin.id, 7);
        assert_eq!(admin.email, "a@b.dev");

        // Postgres driver hint maps to the Postgres backend.
        let pg = AppConfig {
            db_driver: "postgres".into(),
            ..Default::default()
        };
        let ctx_pg = AppContext::new(db.clone(), pg);
        assert_eq!(ctx_pg.db_backend(), sea_orm::DbBackend::Postgres);
    }
}
