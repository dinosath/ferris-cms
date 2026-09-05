//! api-rest — Axum routers + middleware (design Part II §2, Part V).
//!
//! Route groups: `/api/**` (public content), `/admin/**` (management),
//! `/content-type-builder/**` (schema CRUD).
//!
//! Every handler talks to `services` through `AppContext` stored as
//! an Axum extension/state.

pub mod auth;
pub mod content;
pub mod ctb;
pub mod error;
pub mod import_export;
pub mod workflow;
pub mod ai;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use rust_embed::RustEmbed;
use std::collections::HashMap;
use services::{
    api_token_create, api_token_delete, api_token_list, auth_login, auth_register,
    cm_content_types, cm_create, cm_delete, cm_discard_draft, cm_get, cm_get_configuration,
    cm_list, cm_publish, cm_unpublish, cm_update, cm_update_configuration, ctb_apply, ctb_get,
    ctb_list, ctb_reserved_names, i18n_create as svc_i18n_create, i18n_delete as svc_i18n_delete,
    i18n_list as svc_i18n_list, init_info, media_list as svc_media_list,
    media_upload as svc_media_upload, rbac_create_user, rbac_get_role, rbac_list_roles,
    rbac_list_users, rbac_update_permissions, AppConfig, AppContext, ServiceError,
};
use std::sync::Arc;

/// Shared application state held by Axum.
pub struct AppState {
    pub ctx: Arc<AppContext>,
}

impl AppState {
    pub fn new(db: sea_orm::DatabaseConnection, config: AppConfig) -> Self {
        Self {
            ctx: Arc::new(AppContext::new(db, config)),
        }
    }
}

/// Build the full Axum router.
pub fn build_router(state: Arc<AppState>) -> Router {
    let public_api = Router::new()
        .route("/api/{uid}", get(public_list).post(public_create))
        .route(
            "/api/{uid}/{document_id}",
            get(public_get).put(public_update).delete(public_delete),
        );

    let admin = Router::new()
        // Auth
        .route("/admin/init", get(admin_init))
        .route("/admin/login", post(admin_login))
        .route("/admin/register-admin", post(admin_register))
        // OpenID Connect SSO
        .route("/admin/oidc/status", get(oidc_status_handler))
        .route("/admin/oidc/authorize", get(oidc_authorize_handler))
        .route("/admin/oidc/callback", get(oidc_callback_handler))
        // Content-Type Builder
        .route(
            "/content-type-builder/content-types",
            get(ctb_list_handler).post(ctb_apply_handler),
        )
        .route(
            "/content-type-builder/content-types/{uid}",
            get(ctb_get_handler),
        )
        .route("/content-type-builder/schema", post(ctb_apply_handler))
        .route(
            "/content-type-builder/reserved-names",
            get(ctb_reserved_names_handler),
        )
        .route("/content-type-builder/components", get(ctb_list_handler))
        .route(
            "/content-type-builder/components/{uid}",
            get(ctb_get_handler),
        )
        // Content Manager
        .route(
            "/admin/content-manager/content-types",
            get(cm_ct_list_handler),
        )
        .route(
            "/admin/content-manager/collection-types/{uid}",
            get(cm_list_handler).post(cm_create_handler),
        )
        .route(
            "/admin/content-manager/collection-types/{uid}/{document_id}",
            get(cm_get_handler)
                .put(cm_update_handler)
                .delete(cm_delete_handler),
        )
        .route(
            "/admin/content-manager/collection-types/{uid}/{document_id}/actions/publish",
            post(cm_publish_handler),
        )
        .route(
            "/admin/content-manager/collection-types/{uid}/{document_id}/actions/unpublish",
            post(cm_unpublish_handler),
        )
        .route(
            "/admin/content-manager/collection-types/{uid}/{document_id}/actions/discard",
            post(cm_discard_handler),
        )
        .route(
            "/admin/content-manager/single-types/{uid}",
            get(cm_single_get_handler).put(cm_single_update_handler),
        )
        .route(
            "/admin/content-manager/content-types/{uid}/configuration",
            get(cm_config_handler).put(cm_config_update_handler),
        )
        // i18n
        .route(
            "/admin/i18n/locales",
            get(i18n_list_handler).post(i18n_create_handler),
        )
        .route("/admin/i18n/locales/{id}", delete(i18n_delete_handler))
        // RBAC
        .route("/admin/roles", get(rbac_roles_handler))
        .route("/admin/roles/{id}", get(rbac_role_handler))
        .route(
            "/admin/roles/{id}/permissions",
            put(rbac_permissions_handler),
        )
        .route(
            "/admin/users",
            get(rbac_users_handler).post(rbac_create_user_handler),
        )
        // API tokens
        .route(
            "/admin/api-tokens",
            get(api_tokens_list_handler).post(api_tokens_create_handler),
        )
        .route("/admin/api-tokens/{id}", delete(api_tokens_delete_handler))
        // Media
        .route(
            "/admin/upload/files",
            get(media_list_handler).post(media_upload_handler),
        )
        // Workflow automation
        .route(
            "/admin/workflows",
            get(workflow::list_workflows).post(workflow::create_workflow),
        )
        .route("/admin/workflows/import", post(workflow::import_workflow))
        .route(
            "/admin/workflows/{id}",
            get(workflow::get_workflow)
                .put(workflow::save_workflow)
                .delete(workflow::delete_workflow),
        )
        .route(
            "/admin/workflows/{id}/activate",
            post(workflow::activate_workflow),
        )
        .route(
            "/admin/workflows/{id}/deactivate",
            post(workflow::deactivate_workflow),
        )
        .route(
            "/admin/workflows/{id}/duplicate",
            post(workflow::duplicate_workflow),
        )
        .route(
            "/admin/workflows/{id}/validate",
            post(workflow::validate_workflow),
        )
        .route(
            "/admin/workflows/{id}/export",
            get(workflow::export_workflow),
        )
        .route(
            "/admin/workflows/{id}/execute",
            post(workflow::execute_workflow_handler),
        )
        // Executions
        .route("/admin/executions", get(workflow::list_executions))
        .route("/admin/executions/{id}", get(workflow::get_execution))
        .route(
            "/admin/executions/{id}/cancel",
            post(workflow::cancel_execution),
        )
        .route(
            "/admin/executions/{id}/retry",
            post(workflow::retry_execution),
        )
        // Credentials
        .route(
            "/admin/workflow-credentials",
            get(workflow::list_credentials).post(workflow::create_credential),
        )
        .route(
            "/admin/workflow-credentials/types",
            get(workflow::list_credential_types),
        )
        .route(
            "/admin/workflow-credentials/{id}",
            get(workflow::get_credential)
                .put(workflow::update_credential)
                .delete(workflow::delete_credential),
        )
        // Node library + content types + permission actions
        .route("/admin/workflow-node-library", get(workflow::node_library))
        .route(
            "/admin/workflow-content-types",
            get(workflow::workflow_content_types),
        )
        .route(
            "/admin/workflow-permission-actions",
            get(workflow::workflow_permission_actions),
        )
        .merge(import_export::router())
        .merge(ai::router());

    // Public webhook triggers for active workflows.
    let webhook_router = Router::new().route(
        "/workflow-hooks/{path}",
        axum::routing::any(workflow::webhook_trigger),
    );

    // Serve uploaded media files from the storage directory.
    let media_serve = Router::new().nest_service(
        "/uploads",
        tower_http::services::ServeDir::new(&state.ctx.config.media_storage_dir),
    );

    // Serve the Dioxus WASM admin UI at the site root. The UI is embedded in the
    // binary at compile time (rust-embed), so the deployment is a single
    // self-contained executable. Setting `FERRISCMS_UI_DIR` serves from that
    // directory instead (useful for dev/hot-reload with `dx serve`).
    let router = Router::new()
        .merge(public_api)
        .merge(admin)
        .merge(webhook_router)
        .merge(media_serve);

    let router = match std::env::var("FERRISCMS_UI_DIR") {
        Ok(dir) => router.fallback_service(
            tower_http::services::ServeDir::new(&dir).append_index_html_on_directories(true),
        ),
        Err(_) => router.fallback(get(embedded_ui)),
    };

    router.with_state(state)
}

/// The Dioxus WASM admin UI, embedded into the binary from `crates/api-rest/ui/`.
///
/// In release builds (the Docker image) the files are baked into the executable;
/// in debug builds they are read from disk. `#[allow_missing]` lets the crate
/// compile before the UI has been generated.
#[derive(RustEmbed)]
#[folder = "ui/"]
#[allow_missing = true]
struct UiAssets;

const UI_INDEX: &str = "index.html";

/// Serve the embedded admin UI. `/` and unknown extension-less paths return the
/// SPA `index.html`; real files are served directly; unmatched API routes 404.
async fn embedded_ui(uri: Uri) -> Response {
    let mut path = uri
        .path()
        .trim_start_matches('/')
        .trim_start_matches("./")
        .to_string();

    if path.is_empty() {
        path = UI_INDEX.to_string();
    }

    // Unmatched API routes must 404, not fall through to the SPA.
    if path.starts_with("api/")
        || path.starts_with("admin/")
        || path.starts_with("content-type-builder/")
        || path.starts_with("uploads/")
    {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    if let Some(content) = UiAssets::get(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();
        let mut resp = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            content.data.into_owned(),
        )
            .into_response();
        // The SPA entry must always be revalidated so browsers pick up the
        // current (hash-named) bundle after a redeploy instead of a stale one.
        if path == UI_INDEX {
            resp.headers_mut().insert(
                header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-cache"),
            );
        }
        return resp;
    }

    // SPA fallback: serve index.html for extension-less paths.
    if !path.contains('.') {
        if let Some(index) = UiAssets::get(UI_INDEX) {
            let mime = mime_guess::from_path(UI_INDEX).first_or_octet_stream();
            let mut resp = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                index.data.into_owned(),
            )
                .into_response();
            resp.headers_mut().insert(
                header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-cache"),
            );
            return resp;
        }
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

async fn admin_init(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, error::AppError> {
    let info = init_info(&state.ctx).await?;
    Ok(Json(info))
}

async fn admin_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<api_types::admin::LoginRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = auth_login(&state.ctx, &req).await?;
    Ok(Json(resp))
}

async fn admin_register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<api_types::admin::RegisterAdminRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = auth_register(&state.ctx, &req).await?;
    Ok(Json(resp))
}

// ---------------------------------------------------------------------------
// OpenID Connect (SSO) handlers
// ---------------------------------------------------------------------------

/// Whether OIDC SSO is enabled, plus the configured IdP issuer. Unauthenticated
/// so the login screen can decide whether to show a "Continue with SSO" button.
async fn oidc_status_handler(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, error::AppError> {
    let oidc = state.ctx.oidc.as_ref();
    Ok(Json(serde_json::json!({
        "data": {
            "enabled": oidc.is_some(),
            "issuer": oidc.map(|c| c.issuer.clone()),
        }
    })))
}

/// Kick off SSO: discover the IdP and redirect the browser to its authorization
/// endpoint (with `state` + PKCE). Fails when OIDC is not configured.
async fn oidc_authorize_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Redirect, error::AppError> {
    let url = services::oidc_authorize_url(&state.ctx).await?;
    Ok(Redirect::temporary(&url))
}

/// Complete SSO: exchange `code`, verify the ID token, map to an admin and then
/// redirect the browser back to the SPA, passing the session token in the URL
/// fragment (`/#oidc_token=...`) so the admin UI can log the user in without
/// exposing the token to the server again.
async fn oidc_callback_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Redirect, error::AppError> {
    let code = params
        .get("code")
        .cloned()
        .ok_or_else(|| error::AppError(services::ServiceError::Unauthorized))?;
    let state_param = params
        .get("state")
        .cloned()
        .ok_or_else(|| error::AppError(services::ServiceError::Unauthorized))?;

    let resp = services::oidc_login(&state.ctx, &code, &state_param).await?;
    let token = resp.data.token;
    Ok(Redirect::temporary(&format!("/#oidc_token={token}")))
}

// ---------------------------------------------------------------------------
// Content-Type Builder handlers
// ---------------------------------------------------------------------------

async fn ctb_list_handler(admin: auth::AdminCtx) -> Result<impl IntoResponse, error::AppError> {
    let schemas = ctb_list(&admin.0).await;
    Ok(Json(serde_json::json!({ "data": schemas })))
}

async fn ctb_get_handler(
    admin: auth::AdminCtx,
    Path(uid): Path<String>,
) -> Result<impl IntoResponse, error::AppError> {
    let schema = ctb_get(&admin.0, &uid).await?;
    Ok(Json(serde_json::json!({ "data": schema })))
}

async fn ctb_apply_handler(
    admin: auth::AdminCtx,
    Json(req): Json<api_types::admin::CtbApplyRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let removed = req.removed.iter().map(|s| core_domain::Uid::new(s)).collect();
    let schemas = ctb_apply(&admin.0, req.schemas, removed).await?;
    Ok(Json(api_types::admin::CtbApplyResponse {
        data: api_types::admin::CtbApplyData {
            schemas,
            applied_at: chrono::Utc::now(),
        },
    }))
}

async fn ctb_reserved_names_handler(
    _admin: auth::AdminCtx,
) -> Result<impl IntoResponse, error::AppError> {
    let names = ctb_reserved_names();
    Ok(Json(serde_json::json!({ "data": names })))
}

// ---------------------------------------------------------------------------
// Public content API
// ---------------------------------------------------------------------------

async fn public_list(
    State(state): State<Arc<AppState>>,
    Path(uid): Path<String>,
    Query(params): Query<api_types::QueryParams>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_list(&state.ctx, &uid, &params).await?;
    Ok(Json(resp))
}

async fn public_get(
    State(state): State<Arc<AppState>>,
    Path((uid, document_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_get(&state.ctx, &uid, &document_id, None).await?;
    Ok(Json(resp))
}

async fn public_create(
    State(state): State<Arc<AppState>>,
    Path(uid): Path<String>,
    Json(req): Json<api_types::admin::WriteEntryRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_create(&state.ctx, &uid, &req.data).await?;
    Ok(Json(resp))
}

async fn public_update(
    State(state): State<Arc<AppState>>,
    Path((uid, document_id)): Path<(String, String)>,
    Json(req): Json<api_types::admin::WriteEntryRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_update(&state.ctx, &uid, &document_id, &req.data).await?;
    Ok(Json(resp))
}

async fn public_delete(
    State(state): State<Arc<AppState>>,
    Path((uid, document_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, error::AppError> {
    cm_delete(&state.ctx, &uid, &document_id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

// ---------------------------------------------------------------------------
// Content Manager handlers
// ---------------------------------------------------------------------------

async fn cm_ct_list_handler(admin: auth::AdminCtx) -> Result<impl IntoResponse, error::AppError> {
    let types = cm_content_types(&admin.0).await;
    Ok(Json(serde_json::json!({ "data": types })))
}

async fn cm_list_handler(
    admin: auth::AdminCtx,
    Path(uid): Path<String>,
    Query(params): Query<api_types::QueryParams>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_list(&admin.0, &uid, &params).await?;
    Ok(Json(resp))
}

async fn cm_get_handler(
    admin: auth::AdminCtx,
    Path((uid, document_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_get(&admin.0, &uid, &document_id, None).await?;
    Ok(Json(resp))
}

async fn cm_create_handler(
    admin: auth::AdminCtx,
    Path(uid): Path<String>,
    Json(req): Json<api_types::admin::WriteEntryRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_create(&admin.0, &uid, &req.data).await?;
    Ok(Json(resp))
}

async fn cm_update_handler(
    admin: auth::AdminCtx,
    Path((uid, document_id)): Path<(String, String)>,
    Json(req): Json<api_types::admin::WriteEntryRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_update(&admin.0, &uid, &document_id, &req.data).await?;
    Ok(Json(resp))
}

async fn cm_delete_handler(
    admin: auth::AdminCtx,
    Path((uid, document_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, error::AppError> {
    cm_delete(&admin.0, &uid, &document_id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

async fn cm_publish_handler(
    admin: auth::AdminCtx,
    Path((uid, document_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_publish(&admin.0, &uid, &document_id).await?;
    Ok(Json(resp))
}

async fn cm_unpublish_handler(
    admin: auth::AdminCtx,
    Path((uid, document_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_unpublish(&admin.0, &uid, &document_id).await?;
    Ok(Json(resp))
}

async fn cm_discard_handler(
    admin: auth::AdminCtx,
    Path((uid, document_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, error::AppError> {
    cm_discard_draft(&admin.0, &uid, &document_id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

async fn cm_single_get_handler(
    admin: auth::AdminCtx,
    Path(uid): Path<String>,
) -> Result<impl IntoResponse, error::AppError> {
    // Single types: return the one entry or create it
    let resp = cm_get(&admin.0, &uid, "default", None).await;
    match resp {
        Ok(r) => Ok(Json(r)),
        Err(ServiceError::NotFound(_)) => {
            // Return empty template
            Ok(Json(api_types::EntryResponse {
                data: serde_json::json!({}),
                meta: None,
            }))
        }
        Err(e) => Err(error::AppError(e)),
    }
}

async fn cm_single_update_handler(
    admin: auth::AdminCtx,
    Path(uid): Path<String>,
    Json(req): Json<api_types::admin::WriteEntryRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let resp = cm_update(&admin.0, &uid, "default", &req.data).await?;
    Ok(Json(resp))
}

async fn cm_config_handler(
    admin: auth::AdminCtx,
    Path(uid): Path<String>,
) -> Result<impl IntoResponse, error::AppError> {
    let config = cm_get_configuration(&admin.0, &uid).await?;
    Ok(Json(serde_json::json!({ "data": config })))
}

async fn cm_config_update_handler(
    admin: auth::AdminCtx,
    Path(uid): Path<String>,
    Json(config): Json<api_types::admin::ViewConfiguration>,
) -> Result<impl IntoResponse, error::AppError> {
    let config = cm_update_configuration(&admin.0, &uid, &config).await?;
    Ok(Json(serde_json::json!({ "data": config })))
}

// ---------------------------------------------------------------------------
// i18n handlers
// ---------------------------------------------------------------------------

async fn i18n_list_handler(admin: auth::AdminCtx) -> Result<impl IntoResponse, error::AppError> {
    let locales = svc_i18n_list(&admin.0).await?;
    Ok(Json(serde_json::json!({ "data": locales })))
}

async fn i18n_create_handler(
    admin: auth::AdminCtx,
    Json(req): Json<api_types::admin::CreateLocaleRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let locale = svc_i18n_create(&admin.0, &req).await?;
    Ok(Json(serde_json::json!({ "data": locale })))
}

async fn i18n_delete_handler(
    admin: auth::AdminCtx,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, error::AppError> {
    svc_i18n_delete(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

// ---------------------------------------------------------------------------
// RBAC handlers
// ---------------------------------------------------------------------------

async fn rbac_roles_handler(admin: auth::AdminCtx) -> Result<impl IntoResponse, error::AppError> {
    let roles = rbac_list_roles(&admin.0).await?;
    Ok(Json(serde_json::json!({ "data": roles })))
}

async fn rbac_role_handler(
    admin: auth::AdminCtx,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, error::AppError> {
    let role = rbac_get_role(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": role })))
}

async fn rbac_permissions_handler(
    admin: auth::AdminCtx,
    Path(id): Path<i64>,
    Json(req): Json<api_types::admin::UpdateRolePermissionsRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let perms = rbac_update_permissions(&admin.0, id, &req).await?;
    Ok(Json(serde_json::json!({ "data": perms })))
}

async fn rbac_users_handler(admin: auth::AdminCtx) -> Result<impl IntoResponse, error::AppError> {
    let users = rbac_list_users(&admin.0).await?;
    Ok(Json(serde_json::json!({ "data": users })))
}

async fn rbac_create_user_handler(
    admin: auth::AdminCtx,
    Json(req): Json<api_types::admin::CreateAdminUserRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let user = rbac_create_user(&admin.0, &req).await?;
    Ok(Json(serde_json::json!({ "data": user })))
}

// ---------------------------------------------------------------------------
// API tokens handlers
// ---------------------------------------------------------------------------

async fn api_tokens_list_handler(
    admin: auth::AdminCtx,
) -> Result<impl IntoResponse, error::AppError> {
    let tokens = api_token_list(&admin.0).await?;
    Ok(Json(serde_json::json!({ "data": tokens })))
}

async fn api_tokens_create_handler(
    admin: auth::AdminCtx,
    Json(req): Json<api_types::admin::CreateApiTokenRequest>,
) -> Result<impl IntoResponse, error::AppError> {
    let token = api_token_create(&admin.0, &req).await?;
    Ok(Json(serde_json::json!({ "data": token })))
}

async fn api_tokens_delete_handler(
    admin: auth::AdminCtx,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, error::AppError> {
    api_token_delete(&admin.0, id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

// ---------------------------------------------------------------------------
// Media handlers
// ---------------------------------------------------------------------------

async fn media_list_handler(admin: auth::AdminCtx) -> Result<impl IntoResponse, error::AppError> {
    let files = svc_media_list(&admin.0).await?;
    Ok(Json(serde_json::json!({ "data": files })))
}

async fn media_upload_handler(
    admin: auth::AdminCtx,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, error::AppError> {
    // Collect the first file field from the multipart body.
    let mut uploaded = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ServiceError::internal(format!("multipart field: {e}")))?
    {
        let filename = field.file_name().unwrap_or("file").to_string();
        let mime = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let data = field
            .bytes()
            .await
            .map_err(|e| ServiceError::internal(format!("multipart bytes: {e}")))?;
        let file = svc_media_upload(&admin.0, &filename, &mime, &data).await?;
        uploaded.push(file);
    }
    if uploaded.is_empty() {
        return Err(ServiceError::validation(
            "upload",
            vec![services::ValidationErrorItem::new(
                vec!["files".into()],
                "no file provided in multipart body",
                "ValidationError",
            )],
        )
        .into());
    }
    Ok(Json(serde_json::json!({ "data": uploaded })))
}
