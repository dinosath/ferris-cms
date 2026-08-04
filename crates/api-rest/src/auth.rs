//! Auth middleware for api-rest — JWT extraction and admin identity resolution.
//!
//! Phase 2 (this pass): JWT auth is fully wired. Every `/admin/**` handler
//! resolves the `Authorization: Bearer <jwt>` token, loads the admin user,
//! and builds a per-request `AppContext` carrying that identity. The
//! content-service RBAC layer then gates actions per content-type. Public
//! `/api/**` routes stay unauthenticated (they are not governed by the admin
//! permission matrix).
//!
//! Phase 3+ (deferred): API-token authentication for the public API.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use services::{load_current_user, AppContext, ServiceError};
use std::sync::Arc;

use crate::error::AppError;
use crate::AppState;

/// Per-request authenticated admin context extractor.
///
/// Decodes the `Authorization: Bearer <jwt>` token, loads the admin user, and
/// yields an `AppContext` scoped to that identity (sharing `db` + `schema_cache`).
/// Rejects with `401 Unauthorized` when the token is missing or invalid.
pub struct AdminCtx(pub AppContext);

impl FromRequestParts<Arc<AppState>> for AdminCtx {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let user = resolve_admin(&state.ctx, &parts.headers).await?;
        Ok(AdminCtx(state.ctx.with_user(user)))
    }
}

/// Resolve the authenticated admin from the request headers.
///
/// Returns `Ok(Some(user))` when a valid `Bearer` token is present,
/// `Ok(None)` when no token is supplied (unauthenticated), and `Err` when an
/// invalid/expired token is supplied. Callers choose whether to require auth.
pub async fn resolve_admin(
    ctx: &AppContext,
    headers: &HeaderMap,
) -> Result<Option<services::CurrentUser>, ServiceError> {
    let token = extract_bearer(headers).ok_or(ServiceError::Unauthorized)?;
    let user_id = services::decode_admin_token(&token, &ctx.config)?;
    let user = load_current_user(&ctx.db, user_id).await?;
    if !user.is_active {
        return Err(ServiceError::Unauthorized);
    }
    Ok(Some(user))
}

/// Require a valid admin token, returning a per-request `AppContext` with the
/// authenticated identity attached. Returns `Unauthorized` without a token.
pub async fn require_admin_ctx(
    ctx: &AppContext,
    headers: &HeaderMap,
) -> Result<AppContext, ServiceError> {
    let user = resolve_admin(ctx, headers).await?;
    match user {
        Some(u) => Ok(ctx.with_user(Some(u))),
        None => Err(ServiceError::Unauthorized),
    }
}

/// Build an `AppContext` scoped to the authenticated user when a token is
/// present, or to the anonymous (public) identity when none is supplied.
/// Unlike `require_admin_ctx`, this never fails on a missing token — it is
/// used for routes that are reachable both publicly and by admins.
pub async fn scoped_admin_ctx(
    ctx: &AppContext,
    headers: &HeaderMap,
) -> Result<AppContext, ServiceError> {
    match resolve_admin(ctx, headers).await {
        Ok(Some(u)) => Ok(ctx.with_user(Some(u))),
        Ok(None) => Ok(ctx.with_user(None)),
        Err(ServiceError::Unauthorized) => Ok(ctx.with_user(None)),
        Err(e) => Err(e),
    }
}

/// Pull the `Bearer <token>` from the `Authorization` header.
fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
