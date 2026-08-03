//! Strapi-compatible envelopes (design Part V §2).

use serde::{Deserialize, Serialize};

/// List envelope: `{ data: [...], meta: { pagination } }`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListResponse<T> {
    pub data: Vec<T>,
    pub meta: ListMeta,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<Pagination>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pagination {
    pub page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
    #[serde(rename = "pageCount")]
    pub page_count: i64,
    pub total: i64,
}

/// Single-entry envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryResponse<T> {
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// Strapi error envelope: `{ data: null, error: {...} }`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub data: serde_json::Value, // always null
    pub error: ErrorBody,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub status: u16,
    pub name: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ErrorDetails>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorDetails {
    pub errors: Vec<ErrorDetailItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorDetailItem {
    pub path: Vec<String>,
    pub message: String,
    pub name: String,
}

impl ErrorResponse {
    pub fn new(status: u16, name: &str, message: impl Into<String>) -> Self {
        Self {
            data: serde_json::Value::Null,
            error: ErrorBody {
                status,
                name: name.to_string(),
                message: message.into(),
                details: None,
            },
        }
    }

    pub fn validation(message: impl Into<String>, errors: Vec<ErrorDetailItem>) -> Self {
        Self {
            data: serde_json::Value::Null,
            error: ErrorBody {
                status: 400,
                name: "ValidationError".to_string(),
                message: message.into(),
                details: Some(ErrorDetails { errors }),
            },
        }
    }
}

impl<T: Serialize> ListResponse<T> {
    pub fn new(data: Vec<T>, pagination: Option<Pagination>) -> Self {
        Self {
            data,
            meta: ListMeta { pagination },
        }
    }
}
