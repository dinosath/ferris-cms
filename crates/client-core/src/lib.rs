//! client-core — transport-abstract SDK (design Part II §2).
//!
//! The UI calls `client-core` only, never services/db/dynamic-store directly.
//! Two transport impls give offline/online parity.

use async_trait::async_trait;
use std::sync::Arc;

// Re-export for convenience.
pub use api_types;

/// Resolve the current web origin (e.g. `http://localhost:1337`) so the HTTP
/// transport can build absolute URLs in the browser.
#[cfg(target_arch = "wasm32")]
fn web_origin() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default()
}

/// Errors the client can return.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("service error: {0}")]
    Service(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("not connected")]
    NotConnected,
    #[error("unauthorized")]
    Unauthorized,
}

/// Abstract transport — uses JSON serialization for dyn compatibility.
/// `?Send` futures so the same client compiles for web (wasm, single-threaded)
/// and native desktop.
#[async_trait(?Send)]
pub trait ApiTransport: Send + Sync {
    async fn get_json(&self, path: &str) -> Result<serde_json::Value, ClientError>;
    async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError>;
    async fn put_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError>;
    async fn delete_json(&self, path: &str) -> Result<serde_json::Value, ClientError>;
    fn set_token(&self, token: Option<String>);
    /// Downcast to the concrete HTTP transport, if this is one.
    /// Returns `None` for non-HTTP (e.g. future in-process) transports.
    fn as_http(&self) -> Option<&HttpTransport> {
        None
    }
}

// ---------------------------------------------------------------------------
// HTTP transport (web/WASM mode)
// ---------------------------------------------------------------------------

pub struct HttpTransport {
    base_url: String,
    token: parking_lot::RwLock<Option<String>>,
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut base_url = base_url.into();

        // On the web the UI is served same-origin and `api_base_url()` is empty.
        // reqwest requires absolute URLs, so resolve to the current origin here.
        #[cfg(target_arch = "wasm32")]
        if base_url.is_empty() {
            base_url = web_origin();
        }

        Self {
            base_url,
            token: parking_lot::RwLock::new(None),
            client: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn token(&self) -> Option<String> {
        self.token.read().clone()
    }
}

#[async_trait(?Send)]
impl ApiTransport for HttpTransport {
    fn as_http(&self) -> Option<&HttpTransport> {
        Some(self)
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let token = self.token.read().clone();
        #[cfg(target_arch = "wasm32")]
        {
            return wasm_fetch_json("GET", &url, token.as_deref(), None).await;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut req = self.client.get(&url);
            if let Some(tok) = &token {
                req = req.header("Authorization", format!("Bearer {tok}"));
            }
            let resp = req.send().await?;
            parse_response(resp).await
        }
    }

    async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let token = self.token.read().clone();
        #[cfg(target_arch = "wasm32")]
        {
            return wasm_fetch_json("POST", &url, token.as_deref(), Some(body)).await;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut req = self.client.post(&url).json(body);
            if let Some(tok) = &token {
                req = req.header("Authorization", format!("Bearer {tok}"));
            }
            let resp = req.send().await?;
            parse_response(resp).await
        }
    }

    async fn put_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let token = self.token.read().clone();
        #[cfg(target_arch = "wasm32")]
        {
            return wasm_fetch_json("PUT", &url, token.as_deref(), Some(body)).await;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut req = self.client.put(&url).json(body);
            if let Some(tok) = &token {
                req = req.header("Authorization", format!("Bearer {tok}"));
            }
            let resp = req.send().await?;
            parse_response(resp).await
        }
    }

    async fn delete_json(&self, path: &str) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let token = self.token.read().clone();
        #[cfg(target_arch = "wasm32")]
        {
            return wasm_fetch_json("DELETE", &url, token.as_deref(), None).await;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut req = self.client.delete(&url);
            if let Some(tok) = &token {
                req = req.header("Authorization", format!("Bearer {tok}"));
            }
            let resp = req.send().await?;
            parse_response(resp).await
        }
    }

    fn set_token(&self, token: Option<String>) {
        *self.token.write() = token;
    }
}

/// Turn an HTTP status + body text into a `serde_json::Value`, but check the
/// status first. On a non-2xx response the server returns a Strapi error
/// envelope `{ data: null, error: {...} }`; surfacing the real `error.message`
/// (plus validation details) is far more useful than letting downstream typed
/// deserializers fail with a misleading "invalid type: null" error.
async fn parse_response(resp: reqwest::Response) -> Result<serde_json::Value, ClientError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp.json().await?);
    }
    let text = resp.text().await.unwrap_or_default();
    parse_status_text(status.as_u16(), &text)
}

/// Shared status/text → value logic used by both the native (reqwest) and the
/// wasm (native `fetch`) transports.
fn parse_status_text(status: u16, text: &str) -> Result<serde_json::Value, ClientError> {
    if (200..300).contains(&status) {
        if let Ok(v) = serde_json::from_str(text) {
            return Ok(v);
        }
        return Err(ClientError::Service(format!(
            "HTTP {status}: invalid JSON response"
        )));
    }
    if let Ok(err) = serde_json::from_str::<api_types::ErrorResponse>(text) {
        let mut msg = err.error.message;
        if let Some(details) = err.error.details {
            let joined: Vec<String> = details.errors.iter().map(|e| e.message.clone()).collect();
            if !joined.is_empty() {
                msg = format!("{msg}: {}", joined.join("; "));
            }
        }
        return Err(ClientError::Service(msg));
    }
    Err(ClientError::Service(format!(
        "HTTP {status}: {}",
        text.chars().take(300).collect::<String>()
    )))
}

/// Browser `fetch`-based request for the wasm target. reqwest's wasm backend
/// does not reliably attach the Authorization header on the wire, so we drive
/// the browser's native `fetch` directly, which does.
#[cfg(target_arch = "wasm32")]
async fn wasm_fetch_json(
    method: &str,
    url: &str,
    token: Option<&str>,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, ClientError> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit};

    let mut init = RequestInit::new();
    init.method(method);

    let headers = web_sys::Headers::new().map_err(|_| ClientError::NotConnected)?;
    headers
        .set("Content-Type", "application/json")
        .map_err(|_| ClientError::NotConnected)?;
    if let Some(tok) = token {
        headers
            .set("Authorization", &format!("Bearer {tok}"))
            .map_err(|_| ClientError::NotConnected)?;
    }
    init.headers(&headers);

    if let Some(b) = body {
        let body_str = serde_json::to_string(b).map_err(|e| ClientError::Service(e.to_string()))?;
        init.body(Some(&JsValue::from_str(&body_str)));
    }

    let request =
        Request::new_with_str_and_init(url, &init).map_err(|_| ClientError::NotConnected)?;
    let window = web_sys::window().ok_or(ClientError::NotConnected)?;
    let promise = window.fetch_with_request(&request);
    let resp = JsFuture::from(promise)
        .await
        .map_err(|_| ClientError::NotConnected)?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| ClientError::NotConnected)?;
    let status = resp.status();
    let text_promise = resp.text().map_err(|_| ClientError::NotConnected)?;
    let text = JsFuture::from(text_promise)
        .await
        .map_err(|_| ClientError::NotConnected)?
        .as_string()
        .ok_or(ClientError::NotConnected)?;
    parse_status_text(status, &text)
}

// ---------------------------------------------------------------------------
// Client — public API surface
// ---------------------------------------------------------------------------

pub struct Client {
    transport: Arc<dyn ApiTransport>,
}

impl Client {
    pub fn new(transport: Arc<dyn ApiTransport>) -> Self {
        Self { transport }
    }

    pub fn set_token(&self, token: Option<String>) {
        self.transport.set_token(token);
    }

    /// Read the current transport bearer token (useful for diagnostics/tests).
    pub fn token(&self) -> Option<String> {
        self.transport.as_http().and_then(|h| h.token())
    }

    // -- Auth --
    pub async fn init_info(&self) -> Result<api_types::admin::InitInfo, ClientError> {
        let v = self.transport.get_json("/admin/init").await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn auth_login(
        &self,
        req: &api_types::admin::LoginRequest,
    ) -> Result<api_types::admin::LoginResponse, ClientError> {
        let v = self
            .transport
            .post_json("/admin/login", &serde_json::to_value(req)?)
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn auth_register(
        &self,
        req: &api_types::admin::RegisterAdminRequest,
    ) -> Result<api_types::admin::LoginResponse, ClientError> {
        let v = self
            .transport
            .post_json("/admin/register-admin", &serde_json::to_value(req)?)
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    // -- Content-Type Builder --
    pub async fn ctb_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json("/content-type-builder/content-types")
            .await
    }

    pub async fn ctb_apply(
        &self,
        schemas: Vec<core_schema::Schema>,
        removed: Vec<String>,
    ) -> Result<api_types::admin::CtbApplyResponse, ClientError> {
        let req = api_types::admin::CtbApplyRequest { schemas, removed };
        let v = self
            .transport
            .post_json("/content-type-builder/schema", &serde_json::to_value(req)?)
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn ctb_reserved_names(&self) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json("/content-type-builder/reserved-names")
            .await
    }

    // -- Content Manager --
    pub async fn cm_list(
        &self,
        uid: &str,
        params: &api_types::QueryParams,
    ) -> Result<api_types::ListResponse<serde_json::Value>, ClientError> {
        let qs = build_query_string(params);
        let path = if qs.is_empty() {
            format!("/admin/content-manager/collection-types/{uid}")
        } else {
            format!("/admin/content-manager/collection-types/{uid}?{qs}")
        };
        let v = self.transport.get_json(&path).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn cm_get(
        &self,
        uid: &str,
        document_id: &str,
    ) -> Result<api_types::EntryResponse<serde_json::Value>, ClientError> {
        let v = self
            .transport
            .get_json(&format!(
                "/admin/content-manager/collection-types/{uid}/{document_id}"
            ))
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn cm_create(
        &self,
        uid: &str,
        data: &serde_json::Value,
    ) -> Result<api_types::EntryResponse<serde_json::Value>, ClientError> {
        let req = api_types::admin::WriteEntryRequest { data: data.clone() };
        let v = self
            .transport
            .post_json(
                &format!("/admin/content-manager/collection-types/{uid}"),
                &serde_json::to_value(req)?,
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn cm_update(
        &self,
        uid: &str,
        document_id: &str,
        data: &serde_json::Value,
    ) -> Result<api_types::EntryResponse<serde_json::Value>, ClientError> {
        let req = api_types::admin::WriteEntryRequest { data: data.clone() };
        let v = self
            .transport
            .put_json(
                &format!("/admin/content-manager/collection-types/{uid}/{document_id}"),
                &serde_json::to_value(req)?,
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn cm_delete(
        &self,
        uid: &str,
        document_id: &str,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .delete_json(&format!(
                "/admin/content-manager/collection-types/{uid}/{document_id}"
            ))
            .await
    }

    pub async fn cm_publish(
        &self,
        uid: &str,
        document_id: &str,
    ) -> Result<api_types::EntryResponse<serde_json::Value>, ClientError> {
        let v = self
            .transport
            .post_json(
                &format!(
                    "/admin/content-manager/collection-types/{uid}/{document_id}/actions/publish"
                ),
                &serde_json::json!({}),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    /// Discard draft changes (soft-delete the draft, keep published).
    pub async fn cm_discard(
        &self,
        uid: &str,
        document_id: &str,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json(
                &format!(
                    "/admin/content-manager/collection-types/{uid}/{document_id}/actions/discard"
                ),
                &serde_json::json!({}),
            )
            .await
    }

    /// Unpublish a published entry (go back to draft).
    pub async fn cm_unpublish(
        &self,
        uid: &str,
        document_id: &str,
    ) -> Result<api_types::EntryResponse<serde_json::Value>, ClientError> {
        let v = self
            .transport
            .post_json(
                &format!(
                    "/admin/content-manager/collection-types/{uid}/{document_id}/actions/unpublish"
                ),
                &serde_json::json!({}),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    /// Get the Content Manager list-view configuration for a content-type.
    pub async fn cm_get_configuration(&self, uid: &str) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json(&format!(
                "/admin/content-manager/content-types/{uid}/configuration"
            ))
            .await
    }

    /// Update the Content Manager list-view configuration for a content-type.
    pub async fn cm_update_configuration(
        &self,
        uid: &str,
        config: &api_types::admin::ViewConfiguration,
    ) -> Result<serde_json::Value, ClientError> {
        let body = serde_json::to_value(config)?;
        self.transport
            .put_json(
                &format!("/admin/content-manager/content-types/{uid}/configuration"),
                &body,
            )
            .await
    }

    // -- Admin RBAC / Settings --

    /// List admin roles.
    pub async fn roles_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/roles").await
    }

    /// Get one admin role by id.
    pub async fn role_get(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json(&format!("/admin/roles/{id}")).await
    }

    /// Replace a role's permissions.
    pub async fn role_update_permissions(
        &self,
        id: i64,
        permissions: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .put_json(
                &format!("/admin/roles/{id}/permissions"),
                &serde_json::json!({ "permissions": permissions }),
            )
            .await
    }

    /// List admin users.
    pub async fn users_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/users").await
    }

    /// Create an admin user.
    pub async fn user_create(
        &self,
        req: &api_types::admin::CreateAdminUserRequest,
    ) -> Result<serde_json::Value, ClientError> {
        let body = serde_json::to_value(req)?;
        self.transport.post_json("/admin/users", &body).await
    }

    // -- Media --

    /// List media files.
    pub async fn media_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/upload/files").await
    }

    // -- API tokens --

    /// List API tokens.
    pub async fn api_tokens_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/api-tokens").await
    }

    // -- Workflow automation --

    /// List workflows (optionally filtered).
    pub async fn workflow_list(
        &self,
        name: Option<&str>,
        active: Option<bool>,
    ) -> Result<serde_json::Value, ClientError> {
        let mut qs = Vec::new();
        if let Some(n) = name {
            if !n.is_empty() {
                qs.push(format!("name={}", n));
            }
        }
        if let Some(a) = active {
            qs.push(format!("active={a}"));
        }
        let path = if qs.is_empty() {
            "/admin/workflows".to_string()
        } else {
            format!("/admin/workflows?{}", qs.join("&"))
        };
        self.transport.get_json(&path).await
    }

    /// Get one workflow definition.
    pub async fn workflow_get(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json(&format!("/admin/workflows/{id}"))
            .await
    }

    /// Create a workflow.
    pub async fn workflow_create(&self, name: &str) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/workflows", &serde_json::json!({ "name": name }))
            .await
    }

    /// Save a workflow definition (full Workflow JSON).
    pub async fn workflow_save(
        &self,
        id: i64,
        definition: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .put_json(&format!("/admin/workflows/{id}"), definition)
            .await
    }

    /// Delete a workflow.
    pub async fn workflow_delete(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .delete_json(&format!("/admin/workflows/{id}"))
            .await
    }

    /// Activate/deactivate a workflow.
    pub async fn workflow_set_active(
        &self,
        id: i64,
        active: bool,
    ) -> Result<serde_json::Value, ClientError> {
        let path = if active {
            format!("/admin/workflows/{id}/activate")
        } else {
            format!("/admin/workflows/{id}/deactivate")
        };
        self.transport
            .post_json(&path, &serde_json::json!({}))
            .await
    }

    /// Duplicate a workflow.
    pub async fn workflow_duplicate(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json(
                &format!("/admin/workflows/{id}/duplicate"),
                &serde_json::json!({}),
            )
            .await
    }

    /// Validate a workflow.
    pub async fn workflow_validate(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json(
                &format!("/admin/workflows/{id}/validate"),
                &serde_json::json!({}),
            )
            .await
    }

    /// Execute a workflow manually with input data.
    pub async fn workflow_execute(
        &self,
        id: i64,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json(
                &format!("/admin/workflows/{id}/execute"),
                &serde_json::json!({ "data": data }),
            )
            .await
    }

    /// Export a workflow as JSON.
    pub async fn workflow_export(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json(&format!("/admin/workflows/{id}/export"))
            .await
    }

    /// Import a workflow from JSON.
    pub async fn workflow_import(
        &self,
        value: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/workflows/import", value)
            .await
    }

    /// List executions.
    pub async fn execution_list(
        &self,
        workflow_id: Option<i64>,
        status: Option<&str>,
    ) -> Result<serde_json::Value, ClientError> {
        let mut qs = Vec::new();
        if let Some(w) = workflow_id {
            qs.push(format!("workflow_id={w}"));
        }
        if let Some(s) = status {
            qs.push(format!("status={s}"));
        }
        let path = if qs.is_empty() {
            "/admin/executions".to_string()
        } else {
            format!("/admin/executions?{}", qs.join("&"))
        };
        self.transport.get_json(&path).await
    }

    /// Get one execution + its node runs.
    pub async fn execution_get(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json(&format!("/admin/executions/{id}"))
            .await
    }

    /// Cancel an execution.
    pub async fn execution_cancel(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json(
                &format!("/admin/executions/{id}/cancel"),
                &serde_json::json!({}),
            )
            .await
    }

    /// Retry a failed execution.
    pub async fn execution_retry(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json(
                &format!("/admin/executions/{id}/retry"),
                &serde_json::json!({}),
            )
            .await
    }

    // -- Credentials --

    pub async fn credential_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/workflow-credentials").await
    }
    pub async fn credential_types(&self) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json("/admin/workflow-credentials/types")
            .await
    }
    pub async fn credential_create(
        &self,
        name: &str,
        credential_type: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json(
                "/admin/workflow-credentials",
                &serde_json::json!({ "name": name, "credentialType": credential_type, "data": data }),
            )
            .await
    }
    pub async fn credential_delete(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .delete_json(&format!("/admin/workflow-credentials/{id}"))
            .await
    }

    // -- Node library / content types --

    pub async fn workflow_node_library(&self) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json("/admin/workflow-node-library")
            .await
    }
    pub async fn workflow_content_types(&self) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json("/admin/workflow-content-types")
            .await
    }

    // -- i18n locales --

    /// List locales.
    pub async fn i18n_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/i18n/locales").await
    }

    /// Create a locale.
    pub async fn i18n_create(
        &self,
        req: &api_types::admin::CreateLocaleRequest,
    ) -> Result<serde_json::Value, ClientError> {
        let body = serde_json::to_value(req)?;
        self.transport.post_json("/admin/i18n/locales", &body).await
    }

    /// Delete a locale by id.
    pub async fn i18n_delete(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .delete_json(&format!("/admin/i18n/locales/{id}"))
            .await
    }

    /// Create an API token; returns the raw access key once.
    pub async fn api_token_create(
        &self,
        req: &api_types::admin::CreateApiTokenRequest,
    ) -> Result<serde_json::Value, ClientError> {
        let body = serde_json::to_value(req)?;
        self.transport.post_json("/admin/api-tokens", &body).await
    }

    /// Delete an API token by id.
    pub async fn api_token_delete(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport
            .delete_json(&format!("/admin/api-tokens/{id}"))
            .await
    }

    /// Upload a file as multipart. Returns the JSON `{ "data": [...] }`.
    pub async fn media_upload(
        &self,
        filename: &str,
        mime: &str,
        data: &[u8],
    ) -> Result<serde_json::Value, ClientError> {
        use reqwest::multipart::{Form, Part};
        // HTTP-only path: the offline transport resolves to an HTTP client too.
        let http = self
            .transport
            .as_http()
            .ok_or_else(|| ClientError::NotConnected)?;
        let url = format!("{}/admin/upload/files", http.base_url());
        let part = Part::bytes(data.to_vec())
            .file_name(filename.to_string())
            .mime_str(mime)
            .map_err(|e| ClientError::Service(e.to_string()))?;
        let form = Form::new().part("files", part);
        let mut req = http.client().post(&url).multipart(form);
        if let Some(tok) = http.token().as_ref() {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }
        let resp = req.send().await?;
        Ok(resp.json().await?)
    }

    // -- Import / Export --

    /// Analyze uploaded files (parse, infer schemas, detect content types).
    pub async fn import_export_analyze(
        &self,
        req: &api_types::AnalyzeRequest,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/import-export/analyze", &serde_json::to_value(req)?)
            .await
    }

    /// Run an import.
    pub async fn import_export_import(
        &self,
        req: &api_types::ImportRequest,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/import-export/import", &serde_json::to_value(req)?)
            .await
    }

    /// Run an export.
    pub async fn import_export_export(
        &self,
        req: &api_types::ExportRequest,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/import-export/export", &serde_json::to_value(req)?)
            .await
    }

    /// List saved mapping presets.
    pub async fn import_export_mappings(&self) -> Result<serde_json::Value, ClientError> {
        self.transport
            .get_json("/admin/import-export/mappings")
            .await
    }

    /// Save a mapping preset.
    pub async fn import_export_mapping_save(
        &self,
        req: &api_types::MappingPresetUpsert,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/import-export/mappings", &serde_json::to_value(req)?)
            .await
    }

    /// Delete a mapping preset.
    pub async fn import_export_mapping_delete(
        &self,
        id: i64,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .delete_json(&format!("/admin/import-export/mappings/{id}"))
            .await
    }

    // -----------------------------------------------------------------------
    // AI subsystem
    // -----------------------------------------------------------------------

    pub async fn ai_providers(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/ai/providers").await
    }

    pub async fn ai_provider_create(
        &self,
        req: &api_types::AiProviderCreate,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/providers", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_provider_update(
        &self,
        id: i64,
        req: &api_types::AiProviderUpdate,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .put_json(&format!("/admin/ai/providers/{id}"), &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_provider_delete(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport.delete_json(&format!("/admin/ai/providers/{id}")).await
    }

    pub async fn ai_models(&self, provider_id: Option<i64>) -> Result<serde_json::Value, ClientError> {
        match provider_id {
            Some(pid) => self.transport.get_json(&format!("/admin/ai/providers/{pid}/models")).await,
            None => self.transport.get_json("/admin/ai/models").await,
        }
    }

    pub async fn ai_model_create(
        &self,
        req: &api_types::AiModelCreate,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/models", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_model_update(
        &self,
        id: i64,
        req: &api_types::AiModelUpdate,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .put_json(&format!("/admin/ai/models/{id}"), &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_model_delete(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport.delete_json(&format!("/admin/ai/models/{id}")).await
    }

    pub async fn ai_conversations(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/ai/conversations").await
    }

    pub async fn ai_conversation_create(
        &self,
        req: &api_types::AiConversationCreate,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/conversations", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_conversation_delete(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport.delete_json(&format!("/admin/ai/conversations/{id}")).await
    }

    pub async fn ai_messages(&self, id: i64) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json(&format!("/admin/ai/conversations/{id}/messages")).await
    }

    pub async fn ai_send_message(
        &self,
        id: i64,
        text: &str,
    ) -> Result<serde_json::Value, ClientError> {
        let body = api_types::AiSendMessage { text: text.to_string() };
        self.transport
            .post_json(&format!("/admin/ai/conversations/{id}/messages"), &serde_json::to_value(&body)?)
            .await
    }

    pub async fn ai_confirm_tool_calls(
        &self,
        id: i64,
        calls: Vec<api_types::AiConfirmToolCall>,
    ) -> Result<serde_json::Value, ClientError> {
        let body = api_types::AiConfirmBody { calls };
        self.transport
            .post_json(&format!("/admin/ai/conversations/{id}/confirm"), &serde_json::to_value(&body)?)
            .await
    }

    pub async fn ai_generate(
        &self,
        req: &api_types::AiGenerateBody,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/generate", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_edit(&self, req: &api_types::AiEditBody) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/edit", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_translate(
        &self,
        req: &api_types::AiTranslateBody,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/translate", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_schema_generate(
        &self,
        req: &api_types::AiSchemaGenerateBody,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/schema/generate", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_schema_apply(
        &self,
        schema: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let body = api_types::AiSchemaApplyBody { schema };
        self.transport
            .post_json("/admin/ai/schema/apply", &serde_json::to_value(&body)?)
            .await
    }

    pub async fn ai_media_analyze(
        &self,
        req: &api_types::AiMediaAnalyzeBody,
    ) -> Result<serde_json::Value, ClientError> {
        self.transport
            .post_json("/admin/ai/media/analyze", &serde_json::to_value(req)?)
            .await
    }

    pub async fn ai_tools(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/ai/tools").await
    }

    pub async fn ai_usage(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/ai/usage").await
    }

    pub async fn ai_usage_summary(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/admin/ai/usage/summary").await
    }
}

/// Simple query string builder for QueryParams.
fn build_query_string(params: &api_types::QueryParams) -> String {
    let mut parts = Vec::new();
    if let Some(fields) = &params.fields {
        for (i, f) in fields.iter().enumerate() {
            parts.push(format!("fields[{i}]={f}"));
        }
    }
    if let Some(populate) = &params.populate {
        match populate {
            api_types::PopulateSpec::Star => parts.push("populate=*".into()),
            api_types::PopulateSpec::List(list) => {
                for (i, item) in list.iter().enumerate() {
                    parts.push(format!("populate[{i}]={item}"));
                }
            }
            api_types::PopulateSpec::Map(_) => {}
        }
    }
    if let Some(locale) = &params.locale {
        parts.push(format!("locale={locale}"));
    }
    if let Some(status) = &params.status {
        parts.push(format!("status={}", status.as_db_str()));
    }
    if let Some(pagination) = &params.pagination {
        match pagination {
            api_types::PaginationParams::Page {
                page,
                page_size,
                with_count,
            } => {
                parts.push(format!("pagination[page]={page}"));
                parts.push(format!("pagination[pageSize]={page_size}"));
                if let Some(wc) = with_count {
                    parts.push(format!("pagination[withCount]={wc}"));
                }
            }
            api_types::PaginationParams::Offset {
                start,
                limit,
                with_count,
            } => {
                parts.push(format!("pagination[start]={start}"));
                parts.push(format!("pagination[limit]={limit}"));
                if let Some(wc) = with_count {
                    parts.push(format!("pagination[withCount]={wc}"));
                }
            }
        }
    }
    for s in &params.sort {
        let dir = if s.descending { "desc" } else { "asc" };
        parts.push(format!("sort[]={}:{}", s.field, dir));
    }
    parts.join("&")
}
