//! Import orchestration: parse a dataset, build target records via field
//! mappings + transformations, validate, then create / update / upsert entries
//! through the dynamic store, collecting detailed results.

use std::collections::HashMap;

use api_types::{
    FileImportConfig, ImportErrorDto, ImportMode, ImportResponse, ImportState, ImportSummary,
    MappingStatus,
};
use core_schema::Schema;
use serde_json::Value as JsonValue;

use crate::AppContext;
use crate::ServiceError;

use super::parser;
use super::transformer::apply_transform;
use super::validator;

/// Run an import for all datasets in the request.
pub async fn run_import(
    ctx: &AppContext,
    req: &api_types::ImportRequest,
) -> Result<ImportResponse, ServiceError> {
    let mut summaries = Vec::new();
    let mut all_errors = Vec::new();
    let mut created = 0;
    let mut updated = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for cfg in &req.files {
        let schema = crate::content::load_schema(ctx, &cfg.uid)?;
        // Import may create and/or update; enforce the relevant permissions.
        match cfg.mode {
            ImportMode::CreateOnly => enforce(ctx, &schema, crate::rbac::action::CREATE).await?,
            ImportMode::UpdateOnly => enforce(ctx, &schema, crate::rbac::action::UPDATE).await?,
            ImportMode::Upsert => {
                enforce(ctx, &schema, crate::rbac::action::CREATE).await?;
                enforce(ctx, &schema, crate::rbac::action::UPDATE).await?;
            }
        }

        let (summary, errors) = import_dataset(ctx, cfg, &schema).await?;
        created += summary.created;
        updated += summary.updated;
        skipped += summary.skipped;
        failed += summary.failed;
        all_errors.extend(errors);
        summaries.push(summary);
    }

    Ok(ImportResponse {
        completed: failed == 0,
        summaries,
        errors: all_errors,
        created,
        updated,
        skipped,
        failed,
    })
}

async fn enforce(ctx: &AppContext, schema: &Schema, action: &str) -> Result<(), ServiceError> {
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        action,
        schema.uid.as_str(),
    )
    .await
}

/// Import a single dataset into a content type.
pub async fn import_dataset(
    ctx: &AppContext,
    cfg: &FileImportConfig,
    schema: &Schema,
) -> Result<(ImportSummary, Vec<ImportErrorDto>), ServiceError> {
    let datasets =
        parser::parse_content(&cfg.filename, &cfg.content).map_err(ServiceError::Internal)?;
    let dataset = datasets
        .into_iter()
        .find(|d| d.name == cfg.dataset)
        .unwrap_or_else(|| {
            parser::parse_content(&cfg.filename, &cfg.content)
                .ok()
                .and_then(|mut ds| ds.pop())
                .unwrap_or(parser::Dataset {
                    name: cfg.dataset.clone(),
                    records: vec![],
                })
        });

    let total = dataset.records.len();
    let mut created = 0;
    let mut updated = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut warnings = 0;
    let mut errors = Vec::new();

    // Existing entries keyed by the match field value → document id.
    let existing = if cfg.match_field.as_deref().is_some() {
        load_existing(ctx, schema, cfg.match_field.as_deref().unwrap_or("")).await?
    } else {
        HashMap::new()
    };

    let user_id = ctx.current_user.as_ref().map(|u| u.id);

    for (i, record) in dataset.records.iter().enumerate() {
        let target = build_target_record(schema, record, cfg);

        // Validate.
        let issues = validator::validate_record(schema, &target);
        if !issues.is_empty() {
            failed += 1;
            let first = issues.into_iter().next().unwrap();
            errors.push(ImportErrorDto {
                file: cfg.filename.clone(),
                row: i + 1,
                source: record.clone(),
                target_field: first.field,
                message: first.message,
                suggested_fix: first.suggested_fix,
            });
            continue;
        }

        // Determine match value + existing document.
        let match_value = cfg
            .match_field
            .as_deref()
            .map(|mf| {
                target
                    .get(mf)
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let existing_doc = if match_value.is_empty() {
            None
        } else {
            existing.get(&match_value).cloned()
        };

        let result: Result<WriteOutcome, ServiceError> = match cfg.mode {
            ImportMode::CreateOnly => {
                dml_insert(ctx, schema, &target, user_id, i, cfg, record, &mut errors).await
            }
            ImportMode::UpdateOnly => match existing_doc {
                Some(doc) => {
                    dml_update(
                        ctx,
                        schema,
                        &doc,
                        &target,
                        user_id,
                        i,
                        cfg,
                        record,
                        &mut errors,
                    )
                    .await
                }
                None => Ok(WriteOutcome::Skipped),
            },
            ImportMode::Upsert => match existing_doc {
                Some(doc) => {
                    dml_update(
                        ctx,
                        schema,
                        &doc,
                        &target,
                        user_id,
                        i,
                        cfg,
                        record,
                        &mut errors,
                    )
                    .await
                }
                None => {
                    dml_insert(ctx, schema, &target, user_id, i, cfg, record, &mut errors).await
                }
            },
        };

        match result {
            Ok(WriteOutcome::Created) => created += 1,
            Ok(WriteOutcome::Updated) => updated += 1,
            Ok(WriteOutcome::Skipped) => skipped += 1,
            Err(_) => failed += 1, // error already recorded in `errors`
        }
    }

    let summary = ImportSummary {
        uid: schema.uid.as_str().to_string(),
        display_name: schema.info.display_name.clone(),
        total,
        valid: total - failed,
        created,
        updated,
        skipped,
        warnings,
        failed,
    };
    Ok((summary, errors))
}

/// Outcome of writing one record.
#[derive(Clone, Copy, PartialEq)]
enum WriteOutcome {
    Created,
    Updated,
    Skipped,
}

/// Insert a record. `Ok(Created)` on success; on write failure the error is
/// recorded and `Err` is returned (counted as failed).
async fn dml_insert(
    ctx: &AppContext,
    schema: &Schema,
    target: &serde_json::Map<String, JsonValue>,
    user_id: Option<i64>,
    row: usize,
    cfg: &FileImportConfig,
    source: &JsonValue,
    errors: &mut Vec<ImportErrorDto>,
) -> Result<WriteOutcome, ServiceError> {
    let data = JsonValue::Object(target.clone());
    match dynamic_store::dml::insert_one(&ctx.db, schema, &data, user_id).await {
        Ok(_) => Ok(WriteOutcome::Created),
        Err(e) => {
            errors.push(ImportErrorDto {
                file: cfg.filename.clone(),
                row: row + 1,
                source: source.clone(),
                target_field: None,
                message: format!("insert failed: {e}"),
                suggested_fix: Some("fix the record and retry".into()),
            });
            Err(ServiceError::Internal(format!("insert failed: {e}")))
        }
    }
}

/// Update a record by document id. `Ok(Updated)` on success.
async fn dml_update(
    ctx: &AppContext,
    schema: &Schema,
    doc: &str,
    target: &serde_json::Map<String, JsonValue>,
    user_id: Option<i64>,
    row: usize,
    cfg: &FileImportConfig,
    source: &JsonValue,
    errors: &mut Vec<ImportErrorDto>,
) -> Result<WriteOutcome, ServiceError> {
    let data = JsonValue::Object(target.clone());
    match dynamic_store::dml::update_one(&ctx.db, schema, doc, &data, user_id).await {
        Ok(_) => Ok(WriteOutcome::Updated),
        Err(e) => {
            errors.push(ImportErrorDto {
                file: cfg.filename.clone(),
                row: row + 1,
                source: source.clone(),
                target_field: None,
                message: format!("update failed: {e}"),
                suggested_fix: Some("fix the record and retry".into()),
            });
            Err(ServiceError::Internal(format!("update failed: {e}")))
        }
    }
}

/// Load existing entries and index them by the value of `match_field`.
async fn load_existing(
    ctx: &AppContext,
    schema: &Schema,
    match_field: &str,
) -> Result<HashMap<String, String>, ServiceError> {
    let params = api_types::QueryParams {
        pagination: Some(api_types::PaginationParams::Page {
            page: 1,
            page_size: 1_000_000,
            with_count: Some(true),
        }),
        ..Default::default()
    };
    let list = crate::content::cm_list(ctx, schema.uid.as_str(), &params).await?;
    let mut map = HashMap::new();
    for entry in list.data {
        let doc = entry
            .get("documentId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if let Some(v) = entry.get(match_field) {
            let key = v.to_string().trim_matches('"').to_string();
            if !key.is_empty() && !doc.is_empty() {
                map.insert(key, doc);
            }
        }
    }
    Ok(map)
}

/// Build a target record (keys = target field names) from a source record.
fn build_target_record(
    schema: &Schema,
    record: &JsonValue,
    cfg: &FileImportConfig,
) -> serde_json::Map<String, JsonValue> {
    let mut target = serde_json::Map::new();
    for m in &cfg.mapping {
        if m.status == MappingStatus::Ignored {
            continue;
        }
        let Some(tf) = m.target_field.as_deref() else {
            continue;
        };
        if !schema.attributes.contains_key(tf) {
            continue;
        }
        let src = record.get(&m.source_field).unwrap_or(&JsonValue::Null);
        let transformed = apply_transform(&m.transform, src);
        // Only include if the transform didn't produce a null when the target
        // is being created fresh (except explicit EmptyToNull).
        target.insert(tf.to_string(), transformed);
    }

    // Locale.
    let locale = if let Some(lf) = &cfg.locale_field {
        record
            .get(lf)
            .and_then(|v| v.as_str())
            .unwrap_or(&cfg.locale)
            .to_string()
    } else {
        cfg.locale.clone()
    };
    target.insert("locale".to_string(), JsonValue::String(locale));

    // Publication state.
    let state = if let Some(sf) = &cfg.state_field {
        record
            .get(sf)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "draft".to_string())
    } else {
        match cfg.import_state {
            ImportState::Draft => "draft".to_string(),
            ImportState::Published => "published".to_string(),
            ImportState::Preserve => "draft".to_string(),
        }
    };
    target.insert("publicationState".to_string(), JsonValue::String(state));

    target
}
