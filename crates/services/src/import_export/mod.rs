//! Import & Export service — parse, analyze, map, validate, import, export.
//!
//! Business logic lives here, never in the Dioxus UI. The UI talks to the
//! backend through `client-core` and the DTOs in `api-types`.

pub mod analyzer;
pub mod exporter;
pub mod importer;
pub mod mapper;
pub mod parser;
pub mod transformer;
pub mod validator;

use api_types::{
    AnalyzeFileResponse, AnalyzeRequest, AnalyzeResponse, ContentTypeSuggestion, DataFormat,
    FilePayload, MappingDto, MappingPreset, MappingPresetUpsert,
};
use db::entities::import_export_mapping_preset::{ActiveModel, Column, Entity};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use crate::AppContext;
use crate::ServiceError;

pub use exporter::run_export;
pub use importer::run_import;

/// Analyze uploaded files: parse into datasets, infer schemas, and detect the
/// most likely target content type for each dataset.
pub async fn analyze(
    ctx: &AppContext,
    req: &AnalyzeRequest,
) -> Result<AnalyzeResponse, ServiceError> {
    let schemas = crate::content_type_builder::ctb_list(ctx).await;
    let mut datasets = Vec::new();
    for file in &req.files {
        let parsed = parser::parse_content_with_csv(
            &file.filename,
            &file.content,
            file.csv_delimiter.as_deref(),
            file.csv_has_header,
        )
        .map_err(ServiceError::Internal)?;
        for ds in parsed {
            let inferred = analyzer::infer_schema(&ds.records);
            let candidates = analyzer::detect_content_types(&schemas, &inferred);
            let detected = analyzer::best_suggestion(&candidates, 0.4);
            // If the caller asked for a specific content type (e.g. launched
            // from the Content Manager), compute the suggested mapping against
            // it so the wizard pre-fills that target.
            let (detected, suggested_mapping) = match req.prefer_uid.as_deref() {
                Some(uid) => match schemas.iter().find(|s| s.uid.as_str() == uid) {
                    Some(schema) => (
                        Some(ContentTypeSuggestion {
                            uid: schema.uid.as_str().to_string(),
                            display_name: schema.info.display_name.clone(),
                            confidence: 1.0,
                            matched_fields: vec![],
                        }),
                        mapper::build_mappings(&inferred, schema),
                    ),
                    None => (
                        detected.clone(),
                        detected
                            .as_ref()
                            .and_then(|c| schemas.iter().find(|s| s.uid.as_str() == c.uid))
                            .map(|schema| mapper::build_mappings(&inferred, schema))
                            .unwrap_or_default(),
                    ),
                },
                None => (
                    detected.clone(),
                    detected
                        .as_ref()
                        .and_then(|c| schemas.iter().find(|s| s.uid.as_str() == c.uid))
                        .map(|schema| mapper::build_mappings(&inferred, schema))
                        .unwrap_or_default(),
                ),
            };
            let preview: Vec<serde_json::Value> = ds.records.iter().take(5).cloned().collect();
            datasets.push(AnalyzeFileResponse {
                filename: file.filename.clone(),
                dataset: ds.name,
                format: parser::detect_format(&file.filename, &file.content),
                record_count: ds.records.len(),
                preview,
                schema: inferred,
                suggested_mapping,
                detected_content_type: detected,
                candidates,
            });
        }
    }
    Ok(AnalyzeResponse { datasets })
}

/// Extract the `FilePayload` from a raw analyze/import shape (helper for tests).
pub fn payload(filename: &str, content: &str) -> FilePayload {
    FilePayload {
        filename: filename.to_string(),
        content: content.to_string(),
        csv_delimiter: None,
        csv_has_header: None,
    }
}

/// Convert a dataset's detected content type into a suggestion (used by the UI
/// to prefill the target). Helper for building candidates manually.
#[allow(dead_code)]
pub fn suggestion(uid: &str, display_name: &str, confidence: f32) -> ContentTypeSuggestion {
    ContentTypeSuggestion {
        uid: uid.to_string(),
        display_name: display_name.to_string(),
        confidence,
        matched_fields: vec![],
    }
}

/// DataFormat display name helper (for the UI / export filenames).
pub fn format_ext(format: DataFormat) -> &'static str {
    match format {
        DataFormat::Csv => "csv",
        DataFormat::Json => "json",
        DataFormat::Yaml => "yaml",
    }
}

// ---------------------------------------------------------------------------
// Mapping presets (persisted in the database)
// ---------------------------------------------------------------------------

/// List all saved mapping presets, oldest first.
pub async fn list_presets(ctx: &AppContext) -> Result<Vec<MappingPreset>, ServiceError> {
    let rows = Entity::find().order_by_asc(Column::Id).all(&ctx.db).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let mapping: Vec<MappingDto> = serde_json::from_value(r.mapping_json).unwrap_or_default();
        out.push(MappingPreset {
            id: Some(r.id),
            name: r.name,
            source_uid: r.source_uid,
            target_uid: r.target_uid,
            mapping,
        });
    }
    Ok(out)
}

/// Create or update a mapping preset keyed by (name, source_uid, target_uid).
pub async fn upsert_preset(
    ctx: &AppContext,
    req: &MappingPresetUpsert,
) -> Result<MappingPreset, ServiceError> {
    let now = chrono::Utc::now();
    let mapping_json = serde_json::to_value(&req.mapping)
        .map_err(|e| ServiceError::internal(format!("preset mapping serialization: {e}")))?;

    let existing = Entity::find()
        .filter(Column::Name.eq(&req.name))
        .filter(Column::SourceUid.eq(&req.source_uid))
        .filter(Column::TargetUid.eq(&req.target_uid))
        .one(&ctx.db)
        .await?;

    let saved = if let Some(m) = existing {
        let mut am: ActiveModel = m.into();
        am.mapping_json = Set(mapping_json);
        am.updated_at = Set(now);
        am.update(&ctx.db).await?
    } else {
        ActiveModel {
            name: Set(req.name.clone()),
            source_uid: Set(req.source_uid.clone()),
            target_uid: Set(req.target_uid.clone()),
            mapping_json: Set(mapping_json),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?
    };

    Ok(MappingPreset {
        id: Some(saved.id),
        name: saved.name,
        source_uid: saved.source_uid,
        target_uid: saved.target_uid,
        mapping: serde_json::from_value(saved.mapping_json).unwrap_or_default(),
    })
}

/// Delete a mapping preset by id. Returns whether a row was removed.
pub async fn delete_preset(ctx: &AppContext, id: i64) -> Result<bool, ServiceError> {
    let res = Entity::delete_by_id(id).exec(&ctx.db).await?;
    Ok(res.rows_affected > 0)
}
