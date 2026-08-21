//! Import & Export admin handlers.

use std::sync::Arc;

use axum::extract::Path;
use axum::routing::{get, post};
use axum::{Json, Router};

use api_types::{AnalyzeRequest, ExportRequest, ImportRequest, MappingPresetUpsert};

use crate::auth::AdminCtx;
use crate::error::AppError;
use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/import-export/analyze", post(analyze))
        .route("/admin/import-export/import", post(import))
        .route("/admin/import-export/export", post(export))
        .route(
            "/admin/import-export/mappings",
            get(list_mappings).post(save_mapping),
        )
        .route(
            "/admin/import-export/mappings/{id}",
            axum::routing::delete(delete_mapping),
        )
}

async fn analyze(
    admin: AdminCtx,
    Json(req): Json<AnalyzeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = services::import_export::analyze(&admin.0, &req).await?;
    Ok(Json(serde_json::json!({ "data": resp })))
}

async fn import(
    admin: AdminCtx,
    Json(req): Json<ImportRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = services::import_export::run_import(&admin.0, &req).await?;
    Ok(Json(serde_json::json!({ "data": resp })))
}

async fn export(
    admin: AdminCtx,
    Json(req): Json<ExportRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = services::import_export::run_export(&admin.0, &req).await?;
    Ok(Json(serde_json::json!({ "data": resp })))
}

async fn list_mappings(_admin: AdminCtx) -> Result<Json<serde_json::Value>, AppError> {
    let presets = services::import_export::list_presets();
    Ok(Json(serde_json::json!({ "data": presets })))
}

async fn save_mapping(
    _admin: AdminCtx,
    Json(req): Json<MappingPresetUpsert>,
) -> Result<Json<serde_json::Value>, AppError> {
    let preset = services::import_export::upsert_preset(&req);
    Ok(Json(serde_json::json!({ "data": preset })))
}

async fn delete_mapping(
    _admin: AdminCtx,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ok = services::import_export::delete_preset(id);
    Ok(Json(serde_json::json!({ "deleted": ok })))
}
