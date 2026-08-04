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

mod auth;
mod content;
mod content_type_builder;
mod i18n;
mod media;
mod rbac;
mod schema_cache;

pub use auth::*;
pub use content::*;
pub use content_type_builder::*;
pub use i18n::*;
pub use media::*;
pub use rbac::*;
pub use schema_cache::*;

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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            db_driver: "sqlite".into(),
            jwt_secret: "change-me-in-production".into(),
            jwt_expiry_secs: 30 * 24 * 3600,
            admin_registration_open: true,
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
        self.current_user
            .as_ref()
            .ok_or(ServiceError::Unauthorized)
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
    pub fn restricted_connection(
        &self,
    ) -> Result<sea_orm::RestrictedConnection, ServiceError> {
        let user_id = self
            .current_user
            .as_ref()
            .map(|u| u.id)
            .unwrap_or(0); // 0 = unauthenticated/public

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
