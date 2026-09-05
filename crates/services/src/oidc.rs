//! OpenID Connect (SSO) support for the admin panel (design extension).
//!
//! Lets administrators sign in through an external OpenID Connect identity
//! provider (Keycloak, Okta, Google, ...) using the Authorization Code flow
//! with PKCE. Configuration comes from environment variables (see
//! [`OidcConfig::from_env`]) so it can be supplied through the Helm chart, the
//! same way as the local-admin bootstrap credentials.
//!
//! Flow:
//!  1. `oidc_authorize_url` discovers the provider, builds an authorization URL
//!     (with `state` + PKCE challenge) and remembers the verifier/nonce keyed by
//!     `state`.
//!  2. The user authenticates at the IdP and is redirected back with `code`.
//!  3. `oidc_exchange_and_login` exchanges the code at the token endpoint,
//!     verifies the returned ID token (signature, issuer, audience, expiry,
//!     nonce) and maps the authenticated identity to an admin account (by
//!     email), provisioning one when `auto_provision` is enabled. It then issues
//!     the same JWT the rest of the admin API expects.
//!
//! Provider discovery/token exchange/ID-token verification are delegated to the
//! `openidconnect` crate. Because that crate performs blocking HTTP for JWKS
//! verification, all provider calls run on a blocking thread via
//! `tokio::task::spawn_blocking` so they never stall the async workers.

use crate::auth::{self, load_user_roles};
use crate::{AppContext, ServiceError};
use chrono::Utc;
use db::entities::{admin_role, admin_user, admin_user_role};
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::reqwest::http_client;
use openidconnect::{
    AccessTokenHash, AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    Scope, TokenResponse,
};
use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

/// TTL for an in-flight authorization (`state`), after which it is rejected.
const AUTH_TTL: Duration = Duration::from_secs(600);

/// Env var names for OIDC configuration.
const ENV_ISSUER: &str = "OIDC_ISSUER";
const ENV_CLIENT_ID: &str = "OIDC_CLIENT_ID";
const ENV_CLIENT_SECRET: &str = "OIDC_CLIENT_SECRET";
const ENV_REDIRECT_URI: &str = "OIDC_REDIRECT_URI";
const ENV_SCOPES: &str = "OIDC_SCOPES";
const ENV_AUTO_PROVISION: &str = "OIDC_AUTO_PROVISION";

/// OpenID Connect configuration loaded from environment variables.
#[derive(Clone, Debug)]
pub struct OidcConfig {
    /// IdP issuer URL, e.g. `https://accounts.google.com`.
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    /// Callback URL registered with the IdP.
    pub redirect_uri: String,
    /// Requested scopes (always includes `openid`).
    pub scopes: Vec<String>,
    /// Whether a first-time SSO user is auto-provisioned as a Super Admin.
    pub auto_provision: bool,
}

impl OidcConfig {
    /// Load OIDC config from the environment. Returns `None` when not enabled
    /// (i.e. any of issuer/client id/client secret is missing).
    pub fn from_env() -> Option<Self> {
        let issuer = std::env::var(ENV_ISSUER).ok().filter(|s| !s.is_empty())?;
        let client_id = std::env::var(ENV_CLIENT_ID).ok().filter(|s| !s.is_empty())?;
        let client_secret = std::env::var(ENV_CLIENT_SECRET)
            .ok()
            .filter(|s| !s.is_empty())?;
        let redirect_uri = std::env::var(ENV_REDIRECT_URI)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(default_redirect_uri);

        let scopes = std::env::var(ENV_SCOPES)
            .ok()
            .map(|s| {
                s.split_whitespace()
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["openid".into(), "profile".into(), "email".into()]);

        let auto_provision = std::env::var(ENV_AUTO_PROVISION)
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        Some(Self {
            issuer,
            client_id,
            client_secret,
            redirect_uri,
            scopes,
            auto_provision,
        })
    }
}

/// Default redirect URI is the standard callback route under the admin panel.
fn default_redirect_uri() -> String {
    // Best-effort default; operators should set OIDC_REDIRECT_URI explicitly.
    "http://localhost:1337/admin/oidc/callback".to_string()
}

/// A normalized, verified identity returned by the IdP.
#[derive(Clone, Debug)]
pub struct OidcIdentity {
    pub subject: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub preferred_username: Option<String>,
}

/// Outbound descriptor exposed to the frontend (so it can render SSO UI).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OidcDescriptor {
    pub enabled: bool,
    pub issuer: Option<String>,
    pub authorize_url: Option<String>,
}

/// A pending authorization: verifier/nonce we must match on callback.
struct PendingAuth {
    verifier: PkceCodeVerifier,
    nonce: Nonce,
    created_at: Instant,
}

impl PendingAuth {
    fn expired(&self) -> bool {
        self.created_at.elapsed() > AUTH_TTL
    }
}

// Shared (single-process) store of in-flight authorization challenges keyed by
// `state`. Not shared across replicas; document that SSO should run on a single
// replica, or move the store to the DB for multi-replica deployments.
static PENDING: Mutex<Option<HashMap<String, PendingAuth>>> = Mutex::new(None);

fn pending_lock() -> &'static Mutex<Option<HashMap<String, PendingAuth>>> {
    &PENDING
}

fn is_enabled(ctx: &AppContext) -> bool {
    ctx.oidc.is_some()
}

/// Return a descriptor describing whether OIDC SSO is enabled and, when it is,
/// the authorization URL the admin should be redirected to.
pub async fn oidc_descriptor(ctx: &AppContext) -> Result<OidcDescriptor, ServiceError> {
    if !is_enabled(ctx) {
        return Ok(OidcDescriptor {
            enabled: false,
            issuer: None,
            authorize_url: None,
        });
    }
    let cfg = ctx.oidc.as_ref().expect("checked");
    let authorize_url = oidc_authorize_url(ctx).await?;
    Ok(OidcDescriptor {
        enabled: true,
        issuer: Some(cfg.issuer.clone()),
        authorize_url: Some(authorize_url),
    })
}

/// Build the provider authorization URL for a new login attempt.
///
/// Discovers the provider, generates `state` + PKCE + nonce, stores them so the
/// callback can be matched, and returns the URL to redirect the browser to.
pub async fn oidc_authorize_url(ctx: &AppContext) -> Result<String, ServiceError> {
    let cfg = ctx.oidc.clone()
        .ok_or_else(|| ServiceError::internal("OIDC is not configured"))?;

    let (url, state, verifier, nonce) =
        tokio::task::spawn_blocking(move || build_authorize_url(&cfg))
            .await
            .map_err(|e| ServiceError::internal(format!("oidc task: {e}")))??;

    let key = state.secret().to_string();
    let mut guard = pending_lock().lock().map_err(|_| {
        ServiceError::internal("pending authorization lock poisoned".to_string())
    })?;
    guard.get_or_insert_with(HashMap::new).insert(
        key,
        PendingAuth {
            verifier,
            nonce,
            created_at: Instant::now(),
        },
    );

    Ok(url)
}

/// Perform discovery + construct the authorization URL (blocking).
fn build_authorize_url(cfg: &OidcConfig) -> Result<(String, CsrfToken, PkceCodeVerifier, Nonce), ServiceError> {
    let client = discovered_client(cfg)?;

    // Generate a PKCE challenge (and keep the verifier for the callback).
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut auth = client
        .authorize_url(
            AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .set_pkce_challenge(pkce_challenge);

    for scope in &cfg.scopes {
        if scope != "openid" {
            auth = auth.add_scope(Scope::new(scope.clone()));
        }
    }

    let (auth_url, csrf_state, nonce) = auth.url();
    Ok((auth_url.to_string(), csrf_state, pkce_verifier, nonce))
}

/// Complete the callback: exchange `code` for tokens, verify the ID token, map
/// the identity to an admin, and issue the ferriscms admin JWT.
pub async fn oidc_login(
    ctx: &AppContext,
    code: &str,
    state: &str,
) -> Result<api_types::admin::LoginResponse, ServiceError> {
    let cfg = ctx.oidc.clone()
        .ok_or_else(|| ServiceError::internal("OIDC is not configured"))?;

    // Consume + validate the pending state.
    let pending = {
        let mut guard = pending_lock().lock().map_err(|_| {
            ServiceError::internal("pending authorization lock poisoned".to_string())
        })?;
        let store = guard.as_mut().ok_or(ServiceError::Unauthorized)?;
        let p = store.remove(state).ok_or(ServiceError::Unauthorized)?;
        if p.expired() {
            return Err(ServiceError::Unauthorized);
        }
        p
    };

    // Exchange the code + verify the ID token on a blocking thread.
    let (verifier, nonce) = (pending.verifier, pending.nonce);
    let code_owned = code.to_string();
    let identity = tokio::task::spawn_blocking(move || {
        exchange_and_verify(&cfg, code_owned, verifier, nonce)
    })
    .await
    .map_err(|e| ServiceError::internal(format!("oidc task: {e}")))??;

    resolve_admin_for_identity(ctx, &identity).await
}

/// Exchange the authorization code and verify the ID token (blocking).
fn exchange_and_verify(
    cfg: &OidcConfig,
    code: String,
    verifier: PkceCodeVerifier,
    nonce: Nonce,
) -> Result<OidcIdentity, ServiceError> {
    let client = discovered_client(cfg)?;

    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request(http_client)
        .map_err(|e| ServiceError::internal(format!("oidc token exchange: {e}")))?;

    let id_token = token
        .id_token()
        .ok_or_else(|| ServiceError::Unauthorized)?;

    // Verify the signature, issuer, audience, expiry and nonce.
    let verifier = client.id_token_verifier();
    let claims = id_token
        .claims(&verifier, &nonce)
        .map_err(|_| ServiceError::Unauthorized)?;

    // Reject if the access token was swapped for another subject's.
    if let Some(expected_hash) = claims.access_token_hash() {
        let actual_hash = AccessTokenHash::from_token(
            token.access_token(),
            &id_token.signing_alg().map_err(|e| {
                ServiceError::internal(format!("oidc signing alg: {e}"))
            })?,
        )
        .map_err(|e| ServiceError::internal(format!("oidc at_hash: {e}")))?;
        if actual_hash != *expected_hash {
            return Err(ServiceError::Unauthorized);
        }
    }

    let email = claims.email().map(|e| e.as_str().to_string());
    // `name` is a possibly-localized claim; fall back to the default value.
    let name = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|n| n.as_str().to_string());
    let preferred_username = claims.preferred_username().map(|u| u.as_str().to_string());

    Ok(OidcIdentity {
        subject: claims.subject().as_str().to_string(),
        email,
        name,
        preferred_username,
    })
}

/// Discover provider metadata and build a configured `CoreClient` (blocking).
fn discovered_client(cfg: &OidcConfig) -> Result<CoreClient, ServiceError> {
    let issuer = IssuerUrl::new(cfg.issuer.clone())
        .map_err(|e| ServiceError::internal(format!("invalid issuer: {e}")))?;
    let provider_metadata = CoreProviderMetadata::discover(&issuer, http_client)
        .map_err(|e| ServiceError::internal(format!("oidc discovery: {e}")))?;
    let redirect_uri = RedirectUrl::new(cfg.redirect_uri.clone())
        .map_err(|e| ServiceError::internal(format!("invalid redirect uri: {e}")))?;

    let client = CoreClient::from_provider_metadata(
        provider_metadata,
        ClientId::new(cfg.client_id.clone()),
        Some(ClientSecret::new(cfg.client_secret.clone())),
    )
    .set_redirect_uri(redirect_uri);

    Ok(client)
}

/// Map a verified OIDC identity to an admin account and issue a session token.
///
/// Looks the user up by email. When no admin matches and auto-provisioning is
/// enabled, a Super Admin is created for the first-time SSO user; otherwise the
/// login is rejected so that SSO cannot escalate access on its own.
pub async fn resolve_admin_for_identity(
    ctx: &AppContext,
    identity: &OidcIdentity,
) -> Result<api_types::admin::LoginResponse, ServiceError> {
    let email = identity
        .email
        .clone()
        .ok_or(ServiceError::Unauthorized)?;
    let email_lc = email.trim().to_lowercase();
    if email_lc.is_empty() {
        return Err(ServiceError::Unauthorized);
    }

    let user = admin_user::Entity::find()
        .filter(admin_user::COLUMN.email.eq(&email_lc))
        .one(&ctx.db)
        .await?;

    let user = match user {
        Some(u) => u,
        None => {
            let auto = ctx.oidc.as_ref().map(|c| c.auto_provision).unwrap_or(false);
            if !auto {
                return Err(ServiceError::Forbidden);
            }
            provision_oidc_admin(ctx, identity).await?
        }
    };

    if !user.is_active || user.blocked {
        return Err(ServiceError::Unauthorized);
    }

    let token = auth::sign_admin_token(user.id, &ctx.config)?;
    let roles = load_user_roles(&ctx.db, user.id).await?;

    Ok(api_types::admin::LoginResponse {
        data: api_types::admin::LoginData {
            token,
            user: api_types::admin::AdminUserDto {
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

/// Create an admin account for a first-time SSO user, assigned the Super Admin
/// role (app-level + SeaORM RBAC). The local password is a random value so SSO
/// remains the only way in.
async fn provision_oidc_admin(
    ctx: &AppContext,
    identity: &OidcIdentity,
) -> Result<db::entities::admin_user::Model, ServiceError> {
    let email = identity.email.clone().unwrap_or_default().trim().to_lowercase();
    if email.is_empty() {
        return Err(ServiceError::Unauthorized);
    }

    let username = identity
        .preferred_username
        .clone()
        .filter(|u| !u.is_empty())
        .or_else(|| email.split('@').next().map(|s| s.to_string()))
        .unwrap_or_else(|| "admin".to_string());

    // A random unguessable local hash (users cannot sign in with a password).
    let random_password = uuid::Uuid::new_v4().to_string();
    let password_hash = crate::auth::hash_password(&random_password)?;
    let now = Utc::now();

    let user = admin_user::ActiveModel {
        email: Set(email),
        first_name: Set(identity.name.clone().or_else(|| Some(username.clone()))),
        last_name: Set(None),
        username: Set(Some(username)),
        password_hash: Set(password_hash),
        is_active: Set(true),
        blocked: Set(false),
        prefered_locale: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let user = user.insert(&ctx.db).await?;

    // Assign Super Admin role (app-level + SeaORM RBAC).
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

    Ok(user)
}

/// Parse an IdP callback URL and extract `code` + `state`.
pub fn parse_callback_url(url: &Url) -> Result<(String, String), ServiceError> {
    let mut code = None;
    let mut state = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => {
                return Err(ServiceError::Unauthorized);
            }
            _ => {}
        }
    }
    match (code, state) {
        (Some(code), Some(state)) => Ok((code, state)),
        _ => Err(ServiceError::Unauthorized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;
    use api_types::admin::LoginRequest;
    use sea_orm_migration::MigratorTrait;

    fn test_config() -> AppConfig {
        AppConfig {
            db_driver: "sqlite".into(),
            ..Default::default()
        }
    }

    fn oidc_config(auto_provision: bool) -> OidcConfig {
        OidcConfig {
            issuer: "https://issuer.example.test".into(),
            client_id: "client".into(),
            client_secret: "secret".into(),
            redirect_uri: "http://localhost:1337/admin/oidc/callback".into(),
            scopes: vec!["openid".into(), "profile".into(), "email".into()],
            auto_provision,
        }
    }

    async fn setup(auto_provision: bool) -> AppContext {
        let db = db::connect_sqlite_memory().await.unwrap();
        db::migration::Migrator::up(&db, None).await.unwrap();
        db::seed::seed(&db).await.unwrap();
        AppContext::new_with_oidc(db, test_config(), Some(oidc_config(auto_provision)))
    }

    fn identity(email: &str) -> OidcIdentity {
        OidcIdentity {
            subject: "sub-123".into(),
            email: Some(email.to_string()),
            name: Some("Oidc User".into()),
            preferred_username: Some(email.split('@').next().unwrap_or("u").to_string()),
        }
    }

    #[test]
    fn config_disabled_without_all_env() {
        std::env::remove_var("OIDC_ISSUER");
        std::env::remove_var("OIDC_CLIENT_ID");
        std::env::remove_var("OIDC_CLIENT_SECRET");
        assert!(OidcConfig::from_env().is_none());
    }

    /// Without auto-provisioning, an unknown SSO user is rejected (no access
    /// escalation), and a known admin can sign in.
    #[tokio::test]
    async fn known_admin_login_and_unknown_rejected() {
        let ctx = setup(false).await;
        // Provision a known admin.
        crate::auth::provision_admin(&ctx, "admin", "admin@corp.test", "LocalPass!123")
            .await
            .unwrap();

        // Unknown email -> forbidden (no auto-provision).
        let err = resolve_admin_for_identity(&ctx, &identity("new@corp.test"))
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Forbidden));

        // Known email logs in.
        let resp = resolve_admin_for_identity(&ctx, &identity("admin@corp.test"))
            .await
            .expect("known admin logs in");
        assert!(!resp.data.token.is_empty());

        // The issued token works for a subsequent /admin login-style request by
        // verifying we can also still log in via the admin login (password).
        let pw = crate::auth::auth_login(
            &ctx,
            &LoginRequest {
                email: "admin".into(),
                password: "LocalPass!123".into(),
            },
        )
        .await
        .expect("password login still works");
        assert_eq!(pw.data.user.email, "admin@corp.test");
    }

    /// With auto-provisioning on, an unknown SSO user gets a Super Admin account.
    #[tokio::test]
    async fn auto_provision_creates_super_admin() {
        let ctx = setup(true).await;
        assert!(!db::seed::has_admin(&ctx.db).await.unwrap());

        let resp = resolve_admin_for_identity(&ctx, &identity("sso@corp.test"))
            .await
            .expect("auto-provisioned and logged in");
        assert_eq!(resp.data.user.email, "sso@corp.test");
        assert!(db::seed::has_admin(&ctx.db).await.unwrap());
        assert!(resp
            .data
            .user
            .roles
            .iter()
            .any(|r| r.code == db::seed::ROLE_SUPER_ADMIN));

        // Idempotent: a second SSO login maps to the same account (no dupes).
        let again = resolve_admin_for_identity(&ctx, &identity("sso@corp.test"))
            .await
            .expect("second login maps to same admin");
        assert_eq!(again.data.user.id, resp.data.user.id);
    }

    #[test]
    fn callback_url_parsing() {
        let url = Url::parse(
            "http://localhost/cb?code=abc&state=xyz&session_state=1",
        )
        .unwrap();
        assert_eq!(parse_callback_url(&url).unwrap(), ("abc".into(), "xyz".into()));

        let err_url = Url::parse("http://localhost/cb?error=access_denied").unwrap();
        assert!(parse_callback_url(&err_url).is_err());
    }
}
