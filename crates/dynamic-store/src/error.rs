//! dynamic-store errors.

use sea_orm::DbErr;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] DbErr),
    #[error("invalid value for {field}: {reason}")]
    BadValue { field: String, reason: String },
    #[error("validation error")]
    Validation(Vec<core_schema::PayloadError>),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("not found: {0}")]
    NotFound(String),
}

impl StoreError {
    pub fn bad_value(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::BadValue {
            field: field.into(),
            reason: reason.into(),
        }
    }
}
