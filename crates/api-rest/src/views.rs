//! Content-type view HTTP handlers. Authentication is supplied by AdminCtx;
//! ownership and content-type RBAC are enforced by the views service.

use crate::{auth::AdminCtx, error::AppError, AppState, QsQuery};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use services::views;
use std::sync::Arc;

pub async fn list(admin: AdminCtx, Path(uid): Path<String>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        serde_json::json!({ "data": views::list_views(&admin.0, &uid).await? }),
    ))
}

pub async fn get(
    admin: AdminCtx,
    Path((uid, id)): Path<(String, i64)>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        serde_json::json!({ "data": views::get_view(&admin.0, &uid, id).await? }),
    ))
}

pub async fn create(
    admin: AdminCtx,
    Path(uid): Path<String>,
    Json(req): Json<api_types::admin::CreateContentTypeViewRequest>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        serde_json::json!({ "data": views::create_view(&admin.0, &uid, &req).await? }),
    ))
}

pub async fn update(
    admin: AdminCtx,
    Path((uid, id)): Path<(String, i64)>,
    Json(req): Json<api_types::admin::UpdateContentTypeViewRequest>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        serde_json::json!({ "data": views::update_view(&admin.0, &uid, id, &req).await? }),
    ))
}

pub async fn delete(
    admin: AdminCtx,
    Path((uid, id)): Path<(String, i64)>,
) -> Result<impl IntoResponse, AppError> {
    views::delete_view(&admin.0, &uid, id).await?;
    Ok(Json(serde_json::json!({ "data": null })))
}

pub async fn duplicate(
    admin: AdminCtx,
    Path((uid, id)): Path<(String, i64)>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        serde_json::json!({ "data": views::duplicate_view(&admin.0, &uid, id).await? }),
    ))
}

pub async fn set_default(
    admin: AdminCtx,
    Path((uid, id)): Path<(String, i64)>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        serde_json::json!({ "data": views::set_default_view(&admin.0, &uid, id).await? }),
    ))
}

pub async fn reorder(
    admin: AdminCtx,
    Path(uid): Path<String>,
    Json(req): Json<api_types::admin::ReorderContentTypeViewsRequest>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        serde_json::json!({ "data": views::reorder_views(&admin.0, &uid, &req).await? }),
    ))
}

pub async fn records(
    admin: AdminCtx,
    Path((uid, id)): Path<(String, i64)>,
    QsQuery(params): QsQuery,
) -> Result<impl IntoResponse, AppError> {
    let view = views::get_view(&admin.0, &uid, id).await?;
    let effective = params.effective_pagination();
    let query = views::query_params_for_view(&view, effective.page, effective.page_size);
    Ok(Json(services::cm_list(&admin.0, &uid, &query).await?))
}

// Keep State imported in this module's public surface consistent with the
// other route modules; it is useful to consumers adding tenant middleware.
#[allow(dead_code)]
fn _state_type(_: State<Arc<AppState>>) {}
