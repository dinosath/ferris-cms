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

use std::sync::Mutex;

use api_types::{
    AnalyzeFileResponse, AnalyzeRequest, AnalyzeResponse, ContentTypeSuggestion, DataFormat,
    FilePayload, MappingPreset, MappingPresetUpsert,
};
use once_cell::sync::Lazy;

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
        let parsed =
            parser::parse_content(&file.filename, &file.content).map_err(ServiceError::Internal)?;
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
// Mapping presets (in-process store; not yet persisted across restarts)
// ---------------------------------------------------------------------------

static PRESETS: Lazy<Mutex<Vec<MappingPreset>>> = Lazy::new(|| Mutex::new(Vec::new()));

pub fn list_presets() -> Vec<MappingPreset> {
    PRESETS.lock().unwrap().clone()
}

pub fn upsert_preset(req: &MappingPresetUpsert) -> MappingPreset {
    let mut store = PRESETS.lock().unwrap();
    if let Some(existing) = store.iter_mut().find(|p| {
        p.name == req.name && p.source_uid == req.source_uid && p.target_uid == req.target_uid
    }) {
        existing.mapping = req.mapping.clone();
        return existing.clone();
    }
    let id = (store.len() as i64) + 1;
    let preset = MappingPreset {
        id: Some(id),
        name: req.name.clone(),
        source_uid: req.source_uid.clone(),
        target_uid: req.target_uid.clone(),
        mapping: req.mapping.clone(),
    };
    store.push(preset.clone());
    preset
}

pub fn delete_preset(id: i64) -> bool {
    let mut store = PRESETS.lock().unwrap();
    let before = store.len();
    store.retain(|p| p.id != Some(id));
    store.len() != before
}
