//! client-core — transport-abstract SDK (design Part II §2).
//!
//! The UI calls `client-core` only, never services/db/dynamic-store directly.
//! Two transport impls give offline/online parity.

use async_trait::async_trait;
use std::sync::Arc;

// Re-export for convenience.
pub use api_types;

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
    async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, ClientError>;
    async fn put_json(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, ClientError>;
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
        Self {
            base_url: base_url.into(),
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
        let mut req = self.client.get(&url);
        if let Some(tok) = self.token.read().as_ref() {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        Ok(resp.json().await?)
    }

    async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url).json(body);
        if let Some(tok) = self.token.read().as_ref() {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        Ok(resp.json().await?)
    }

    async fn put_json(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.put(&url).json(body);
        if let Some(tok) = self.token.read().as_ref() {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        Ok(resp.json().await?)
    }

    async fn delete_json(&self, path: &str) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.delete(&url);
        if let Some(tok) = self.token.read().as_ref() {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        Ok(resp.json().await?)
    }

    fn set_token(&self, token: Option<String>) {
        *self.token.write() = token;
    }
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

    // -- Auth --
    pub async fn init_info(&self) -> Result<api_types::admin::InitInfo, ClientError> {
        let v = self.transport.get_json("/admin/init").await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn auth_login(
        &self,
        req: &api_types::admin::LoginRequest,
    ) -> Result<api_types::admin::LoginResponse, ClientError> {
        let v = self.transport.post_json("/admin/login", &serde_json::to_value(req)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn auth_register(
        &self,
        req: &api_types::admin::RegisterAdminRequest,
    ) -> Result<api_types::admin::LoginResponse, ClientError> {
        let v = self.transport.post_json("/admin/register-admin", &serde_json::to_value(req)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    // -- Content-Type Builder --
    pub async fn ctb_list(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/content-type-builder/content-types").await
    }

    pub async fn ctb_apply(
        &self,
        schemas: Vec<core_schema::Schema>,
    ) -> Result<api_types::admin::CtbApplyResponse, ClientError> {
        let req = api_types::admin::CtbApplyRequest { schemas };
        let v = self.transport.post_json("/content-type-builder/schema", &serde_json::to_value(req)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn ctb_reserved_names(&self) -> Result<serde_json::Value, ClientError> {
        self.transport.get_json("/content-type-builder/reserved-names").await
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
        let v = self.transport
            .get_json(&format!("/admin/content-manager/collection-types/{uid}/{document_id}"))
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn cm_create(
        &self,
        uid: &str,
        data: &serde_json::Value,
    ) -> Result<api_types::EntryResponse<serde_json::Value>, ClientError> {
        let req = api_types::admin::WriteEntryRequest { data: data.clone() };
        let v = self.transport
            .post_json(&format!("/admin/content-manager/collection-types/{uid}"), &serde_json::to_value(req)?)
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
        let v = self.transport
            .put_json(&format!("/admin/content-manager/collection-types/{uid}/{document_id}"), &serde_json::to_value(req)?)
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn cm_delete(&self, uid: &str, document_id: &str) -> Result<serde_json::Value, ClientError> {
        self.transport
            .delete_json(&format!("/admin/content-manager/collection-types/{uid}/{document_id}"))
            .await
    }

    pub async fn cm_publish(
        &self,
        uid: &str,
        document_id: &str,
    ) -> Result<api_types::EntryResponse<serde_json::Value>, ClientError> {
        let v = self.transport
            .post_json(
                &format!("/admin/content-manager/collection-types/{uid}/{document_id}/actions/publish"),
                &serde_json::json!({}),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
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
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        Ok(resp.json().await?)
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
            api_types::PaginationParams::Page { page, page_size, with_count } => {
                parts.push(format!("pagination[page]={page}"));
                parts.push(format!("pagination[pageSize]={page_size}"));
                if let Some(wc) = with_count {
                    parts.push(format!("pagination[withCount]={wc}"));
                }
            }
            api_types::PaginationParams::Offset { start, limit, with_count } => {
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
