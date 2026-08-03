//! Admin / management DTOs (design Part V §4-§8).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Auth (Part V §6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub data: LoginData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginData {
    pub token: String,
    pub user: AdminUserDto,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserDto {
    pub id: i64,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firstname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lastname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefered_locale: Option<String>,
    pub is_active: bool,
    pub blocked: bool,
    #[serde(default)]
    pub roles: Vec<AdminRoleDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminRoleDto {
    pub id: i64,
    pub name: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAdminRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub firstname: Option<String>,
    #[serde(default)]
    pub lastname: Option<String>,
    #[serde(default)]
    pub registration_token: Option<String>,
}

/// First-run probe: does an admin exist yet?
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitInfo {
    pub has_admin: bool,
}

// ---------------------------------------------------------------------------
// API tokens (Part V §6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenDto {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifespan: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Only present on create/regenerate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiTokenRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub token_type: core_domain::ApiTokenType,
    #[serde(default)]
    pub lifespan: Option<i64>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

// ---------------------------------------------------------------------------
// Content-Type Builder (Part V §5)
// ---------------------------------------------------------------------------

/// `POST /content-type-builder/schema` body: the full desired schema set.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CtbApplyRequest {
    pub schemas: Vec<core_schema::Schema>,
}

/// Result of a batch apply.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CtbApplyResponse {
    pub data: CtbApplyData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CtbApplyData {
    pub schemas: Vec<core_schema::Schema>,
    pub applied_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Content Manager (Part V §4)
// ---------------------------------------------------------------------------

/// Create/update body: `{ "data": { ... } }`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteEntryRequest {
    pub data: serde_json::Value,
}

/// Per-CT view configuration (design Part III §8).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewConfiguration {
    pub settings: ViewSettings,
    pub layouts: ViewLayouts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSettings {
    pub default_sort_by: String,
    pub default_sort_order: String,
    pub page_size: i64,
    pub main_field: String,
    #[serde(default)]
    pub bulkable: bool,
    #[serde(default)]
    pub filterable: bool,
    #[serde(default)]
    pub searchable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewLayouts {
    pub list: Vec<String>,
    pub edit: Vec<Vec<EditLayoutRow>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditLayoutRow {
    pub name: String,
    pub size: u8,
}

impl ViewConfiguration {
    /// Sensible default derived from a schema (design Part III §8).
    pub fn default_for(schema: &core_schema::Schema) -> Self {
        let main_field = schema.main_field();
        let list: Vec<String> = schema
            .attributes
            .iter()
            .filter(|(_, a)| {
                a.attr_type.is_scalar_column()
                    && !matches!(
                        a.attr_type,
                        core_domain::FieldType::Password | core_domain::FieldType::Json
                    )
                    && !a.private
            })
            .take(4)
            .map(|(n, _)| n.clone())
            .collect();
        let edit = schema
            .attributes
            .keys()
            .map(|n| {
                vec![EditLayoutRow {
                    name: n.clone(),
                    size: 12,
                }]
            })
            .collect();
        Self {
            settings: ViewSettings {
                default_sort_by: "createdAt".into(),
                default_sort_order: "DESC".into(),
                page_size: 10,
                main_field,
                bulkable: true,
                filterable: true,
                searchable: true,
            },
            layouts: ViewLayouts { list, edit },
        }
    }
}

// ---------------------------------------------------------------------------
// i18n (Part V §8)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocaleDto {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub is_default: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLocaleRequest {
    pub code: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

// ---------------------------------------------------------------------------
// RBAC (Part V §6)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminPermissionDto {
    pub id: i64,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub conditions: serde_json::Value,
    #[serde(default)]
    pub role_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRolePermissionsRequest {
    pub permissions: Vec<NewPermission>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewPermission {
    pub action: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub conditions: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAdminUserRequest {
    pub email: String,
    #[serde(default)]
    pub firstname: Option<String>,
    #[serde(default)]
    pub lastname: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub roles: Vec<i64>,
    #[serde(default)]
    pub is_active: Option<bool>,
}
