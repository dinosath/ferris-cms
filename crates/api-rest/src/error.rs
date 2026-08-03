//! Error mapping: `ServiceError` → Strapi-compatible HTTP responses (design Part V §2).

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use services::ServiceError;

/// Wrapper that implements `IntoResponse` for `ServiceError`.
pub struct AppError(pub ServiceError);

impl From<ServiceError> for AppError {
    fn from(e: ServiceError) -> Self {
        Self(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, name, message) = match &self.0 {
            ServiceError::Validation(errors) => {
                let body = api_types::ErrorResponse {
                    data: serde_json::Value::Null,
                    error: api_types::ErrorBody {
                        status: 400,
                        name: "ValidationError".into(),
                        message: "Validation failed".into(),
                        details: Some(api_types::ErrorDetails {
                            errors: errors
                                .iter()
                                .map(|e| api_types::ErrorDetailItem {
                                    path: e.path.clone(),
                                    message: e.message.clone(),
                                    name: e.name.clone(),
                                })
                                .collect(),
                        }),
                    },
                };
                return (StatusCode::BAD_REQUEST, Json(body)).into_response();
            }
            ServiceError::NotFound(msg) => (StatusCode::NOT_FOUND, "NotFound", msg.clone()),
            ServiceError::Conflict(msg) => (StatusCode::CONFLICT, "Conflict", msg.clone()),
            ServiceError::Forbidden => (StatusCode::FORBIDDEN, "Forbidden", "Forbidden".into()),
            ServiceError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "Unauthorized", "Unauthorized".into())
            }
            ServiceError::Db(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DatabaseError",
                e.to_string(),
            ),
            ServiceError::Store(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "StoreError",
                e.to_string(),
            ),
            ServiceError::Internal(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "InternalError", msg.clone())
            }
            ServiceError::Rbac(msg) => {
                (StatusCode::FORBIDDEN, "RbacError", msg.clone())
            }
        };

        let body = api_types::ErrorResponse::new(status.as_u16(), name, message);
        (status, Json(body)).into_response()
    }
}
