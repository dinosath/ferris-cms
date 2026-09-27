//! Content CRUD service (design Part V §3-§4).
//!
//! Serves the public `/api/*` and admin `/content-manager/*` endpoints.
//! All data access goes through `dynamic-store` (SeaQuery) because tables
//! are runtime-defined.

use crate::{AppContext, ServiceError, ValidationErrorItem};
use api_types::{EntryResponse, ListResponse, Pagination, QueryParams};
use core_domain::{fk_column, relation_join_table, ContentTypeKind, RelationKind, Uid};
use core_schema::Schema;
use dynamic_store::dml;
use sea_orm::TransactionTrait;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Public content API — list
// ---------------------------------------------------------------------------

/// List entries of a collection type, with filters/sort/pagination/populate.
pub async fn cm_list(
    ctx: &AppContext,
    uid: &str,
    params: &QueryParams,
) -> Result<ListResponse<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    ensure_collection(&schema)?;

    // Admin RBAC: require `read` on this content-type for authenticated users.
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::rbac::action::READ,
        schema.uid.as_str(),
    )
    .await?;

    let pag = params.effective_pagination();

    // Build base query
    let (rows, total) = dml::query_rows(&ctx.db, ctx.db_backend(), &schema, params).await?;

    let page_count = if pag.with_count && pag.page_size > 0 {
        (total as f64 / pag.page_size as f64).ceil() as i64
    } else {
        0
    };

    Ok(ListResponse {
        data: rows,
        meta: api_types::ListMeta {
            pagination: if pag.with_count {
                Some(Pagination {
                    page: pag.page,
                    page_size: pag.page_size,
                    page_count,
                    total,
                })
            } else {
                None
            },
        },
    })
}

/// Get a single entry by document_id.
pub async fn cm_get(
    ctx: &AppContext,
    uid: &str,
    document_id: &str,
    _params: Option<&QueryParams>,
) -> Result<EntryResponse<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::rbac::action::READ,
        schema.uid.as_str(),
    )
    .await?;
    let mut row = dml::find_one_by_document_id(&ctx.db, &schema, document_id)
        .await?
        .ok_or_else(|| ServiceError::not_found(format!("entry {document_id} not found")))?;
    let row_object = row
        .as_object_mut()
        .ok_or_else(|| ServiceError::internal("stored entry is not a JSON object"))?;
    populate_relations(ctx, &schema, row_object).await?;
    Ok(EntryResponse {
        data: row,
        meta: None,
    })
}

/// Create an entry.
pub async fn cm_create(
    ctx: &AppContext,
    uid: &str,
    data: &JsonValue,
) -> Result<EntryResponse<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);

    // Validate the payload against the schema's required / min / max / pattern
    // constraints before it is handled.
    validate_payload_or_err(&schema, data, true)?;

    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::rbac::action::CREATE,
        schema.uid.as_str(),
    )
    .await?;

    let row = dml::insert_one(&ctx.db, &schema, data, user_id).await?;
    persist_relations(ctx, &schema, &row, data).await?;
    refresh_relation_computed(ctx).await?;
    // Relation formulas are persisted by the refresh above, so reload the
    // created row before returning it to the Content Manager/API.
    let row = dml::find_one_by_document_id(
        &ctx.db,
        &schema,
        row.get("documentId")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| ServiceError::internal("created entry has no documentId"))?,
    )
    .await?
    .unwrap_or(row);
    // Fire the `content.created` trigger for active workflows (async, best-effort).
    let _ = crate::workflow::triggers::dispatch_cms_event(
        ctx,
        "content.created",
        schema.uid.as_str(),
        row.clone(),
    )
    .await;
    Ok(EntryResponse {
        data: row,
        meta: None,
    })
}

/// Update an entry by document_id.
pub async fn cm_update(
    ctx: &AppContext,
    uid: &str,
    document_id: &str,
    data: &JsonValue,
) -> Result<EntryResponse<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);

    // Validate the provided fields' value constraints (required is not enforced
    // on partial updates) before the payload is handled.
    validate_payload_or_err(&schema, data, false)?;

    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::rbac::action::UPDATE,
        schema.uid.as_str(),
    )
    .await?;

    let row = dml::update_one(&ctx.db, &schema, document_id, data, user_id).await?;
    persist_relations(ctx, &schema, &row, data).await?;
    refresh_relation_computed(ctx).await?;
    // The row returned by the scalar update predates relation-link changes and
    // the generic persisted-formula refresh; return the saved values.
    let row = dml::find_one_by_document_id(&ctx.db, &schema, document_id)
        .await?
        .unwrap_or(row);
    let _ = crate::workflow::triggers::dispatch_cms_event(
        ctx,
        "content.updated",
        schema.uid.as_str(),
        row.clone(),
    )
    .await;
    Ok(EntryResponse {
        data: row,
        meta: None,
    })
}

/// Persist relation values supplied alongside a content entry. The dynamic
/// store keeps relation links separate from scalar writes; this helper makes
/// relations behave like ordinary content-manager fields for creates, updates,
/// and imports.
pub async fn persist_relations(
    ctx: &AppContext,
    schema: &core_schema::Schema,
    row: &JsonValue,
    data: &JsonValue,
) -> Result<(), ServiceError> {
    let Some(owner_id) = row.get("id").and_then(JsonValue::as_i64) else {
        return Err(ServiceError::internal("created entry has no id"));
    };
    let Some(input) = data.as_object() else {
        return Ok(());
    };

    for (name, attr) in &schema.attributes {
        if attr.attr_type != core_domain::FieldType::Relation || !input.contains_key(name) {
            continue;
        }
        let Some(target_uid) = attr.target.as_ref() else {
            continue;
        };
        let target_schema = load_schema(ctx, target_uid.as_str())?;
        let values = relation_values(input.get(name).unwrap());
        let target_ids = resolve_relation_ids(ctx, &target_schema, &values).await?;

        match attr.relation.unwrap_or(RelationKind::ManyToOne) {
            RelationKind::ManyToMany | RelationKind::ManyWay => {
                let join = relation_join_table(&schema.table_name(), name);
                dml::replace_join_links(
                    &ctx.db,
                    ctx.db_backend(),
                    &join,
                    &fk_column(&schema.info.singular_name),
                    &fk_column(&target_schema.info.singular_name),
                    &format!("{}_order", core_domain::column_name(name)),
                    owner_id,
                    &target_ids,
                )
                .await
                .map_err(ServiceError::from)?;
            }
            RelationKind::OneWay | RelationKind::OneToOne | RelationKind::ManyToOne => {
                dml::update_by_id(
                    &ctx.db,
                    ctx.db_backend(),
                    &schema.table_name(),
                    owner_id,
                    vec![(
                        fk_column(name),
                        sea_orm::Value::BigInt(target_ids.first().copied()),
                    )],
                )
                .await
                .map_err(ServiceError::from)?;
            }
            RelationKind::OneToMany => {
                // The inverse side owns the FK. Reconcile it so a form can
                // attach, replace, or remove related entries normally.
                let Some(inverse_name) = attr.mapped_by.as_deref() else {
                    continue;
                };
                let existing = dml::query_rows(
                    &ctx.db,
                    ctx.db_backend(),
                    &target_schema,
                    &QueryParams {
                        pagination: Some(api_types::PaginationParams::Page {
                            page: 1,
                            page_size: 1_000_000,
                            with_count: Some(false),
                        }),
                        ..Default::default()
                    },
                )
                .await
                .map_err(ServiceError::from)?
                .0;
                for existing_row in existing {
                    let Some(target_id) = existing_row.get("id").and_then(JsonValue::as_i64) else {
                        continue;
                    };
                    if !target_ids.contains(&target_id) {
                        dml::update_by_id(
                            &ctx.db,
                            ctx.db_backend(),
                            &target_schema.table_name(),
                            target_id,
                            vec![(fk_column(inverse_name), sea_orm::Value::BigInt(None))],
                        )
                        .await
                        .map_err(ServiceError::from)?;
                    }
                }
                for target_id in target_ids {
                    dml::update_by_id(
                        &ctx.db,
                        ctx.db_backend(),
                        &target_schema.table_name(),
                        target_id,
                        vec![(
                            fk_column(inverse_name),
                            sea_orm::Value::BigInt(Some(owner_id)),
                        )],
                    )
                    .await
                    .map_err(ServiceError::from)?;
                }
            }
        }
    }
    Ok(())
}

/// Refresh all persisted relation formulas after a content or relation
/// mutation. The formula definitions live in schemas; this service merely
/// coordinates the generic store refresh across the loaded content types.
pub async fn refresh_relation_computed(ctx: &AppContext) -> Result<(), ServiceError> {
    let schemas = ctx.schema_cache.get_all();
    for schema in &schemas {
        dynamic_store::dml::refresh_relation_computed_fields(
            &ctx.db,
            ctx.db_backend(),
            schema,
            &schemas,
        )
        .await
        .map_err(ServiceError::from)?;
    }
    Ok(())
}

/// Add relation references to a single-entry response. List responses remain
/// scalar-only for efficient tables; the editor gets relation objects/arrays
/// that can be sent back through the normal content-manager write API.
async fn populate_relations(
    ctx: &AppContext,
    schema: &core_schema::Schema,
    row: &mut serde_json::Map<String, JsonValue>,
) -> Result<(), ServiceError> {
    let Some(owner_id) = row.get("id").and_then(JsonValue::as_i64) else {
        return Ok(());
    };

    for (name, attr) in &schema.attributes {
        if attr.attr_type != core_domain::FieldType::Relation {
            continue;
        }
        let Some(target_uid) = attr.target.as_ref() else {
            continue;
        };
        let target_schema = load_schema(ctx, target_uid.as_str())?;
        let target_ids = match attr.relation.unwrap_or(RelationKind::ManyToOne) {
            RelationKind::ManyToMany | RelationKind::ManyWay => {
                let join = relation_join_table(&schema.table_name(), name);
                dml::fetch_join_links(
                    &ctx.db,
                    ctx.db_backend(),
                    &join,
                    &fk_column(&schema.info.singular_name),
                    &fk_column(&target_schema.info.singular_name),
                    &format!("{}_order", core_domain::column_name(name)),
                    owner_id,
                )
                .await
                .map_err(ServiceError::from)?
            }
            RelationKind::OneToMany => {
                let Some(inverse_name) = attr.mapped_by.as_deref() else {
                    continue;
                };
                let (rows, _) = dml::query_rows(
                    &ctx.db,
                    ctx.db_backend(),
                    &target_schema,
                    &QueryParams {
                        pagination: Some(api_types::PaginationParams::Page {
                            page: 1,
                            page_size: 1_000_000,
                            with_count: Some(false),
                        }),
                        ..Default::default()
                    },
                )
                .await
                .map_err(ServiceError::from)?;
                rows.into_iter()
                    .filter(|target| {
                        target.get(inverse_name).and_then(JsonValue::as_i64) == Some(owner_id)
                    })
                    .filter_map(|target| target.get("id").and_then(JsonValue::as_i64))
                    .collect()
            }
            RelationKind::OneWay | RelationKind::OneToOne | RelationKind::ManyToOne => row
                .get(name)
                .and_then(JsonValue::as_i64)
                .into_iter()
                .collect(),
        };

        let mut references = Vec::with_capacity(target_ids.len());
        for target_id in target_ids {
            if let Some(target) =
                dml::find_by_id(&ctx.db, ctx.db_backend(), &target_schema, target_id)
                    .await
                    .map_err(ServiceError::from)?
            {
                references.push(JsonValue::Object(target));
            }
        }

        if matches!(
            attr.relation.unwrap_or(RelationKind::ManyToOne),
            RelationKind::ManyToMany | RelationKind::ManyWay | RelationKind::OneToMany
        ) {
            row.insert(name.clone(), JsonValue::Array(references));
        } else {
            row.insert(
                name.clone(),
                references.into_iter().next().unwrap_or(JsonValue::Null),
            );
        }
    }
    Ok(())
}

fn relation_values(value: &JsonValue) -> Vec<JsonValue> {
    if value.is_null() {
        return Vec::new();
    }
    let value = value
        .as_object()
        .and_then(|object| {
            object
                .get("set")
                .or_else(|| object.get("connect"))
                .or_else(|| object.get("disconnect"))
        })
        .unwrap_or(value);
    value.as_array().cloned().unwrap_or_else(|| {
        if value.is_null() {
            Vec::new()
        } else {
            vec![value.clone()]
        }
    })
}

async fn resolve_relation_ids(
    ctx: &AppContext,
    target_schema: &core_schema::Schema,
    values: &[JsonValue],
) -> Result<Vec<i64>, ServiceError> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let (rows, _) = dml::query_rows(
        &ctx.db,
        ctx.db_backend(),
        target_schema,
        &QueryParams {
            pagination: Some(api_types::PaginationParams::Page {
                page: 1,
                page_size: 1_000_000,
                with_count: Some(false),
            }),
            ..Default::default()
        },
    )
    .await
    .map_err(ServiceError::from)?;
    values
        .iter()
        .map(|value| {
            let candidate = value
                .as_object()
                .and_then(|object| {
                    object
                        .get("id")
                        .or_else(|| object.get("documentId"))
                        .or_else(|| object.get("code"))
                        .or_else(|| object.get("sku"))
                })
                .unwrap_or(value);
            rows.iter()
                .find(|row| {
                    row.get("id") == Some(candidate)
                        || ["documentId", "code", "sku", "name"]
                            .iter()
                            .any(|field| row.get(*field) == Some(candidate))
                })
                .and_then(|row| row.get("id"))
                .and_then(JsonValue::as_i64)
                .ok_or_else(|| ServiceError::not_found(format!("relation target `{value}`")))
        })
        .collect()
}

/// Remove user-supplied values for computed fields. Bulk writes ignore them so
/// batches that echo full rows still succeed; the database derives the values.
fn strip_computed(schema: &core_schema::Schema, item: &JsonValue) -> JsonValue {
    let mut obj = item.as_object().cloned().unwrap_or_default();
    for (name, attr) in &schema.attributes {
        if attr.computed {
            obj.remove(name);
        }
    }
    JsonValue::Object(obj)
}

/// Bulk create entries. User-supplied computed values are ignored.
pub async fn cm_bulk_create(
    ctx: &AppContext,
    uid: &str,
    items: &[JsonValue],
) -> Result<Vec<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let data = strip_computed(&schema, item);
        out.push(cm_create(ctx, uid, &data).await?.data);
    }
    Ok(out)
}

/// Bulk update entries. Each item must carry `documentId`; user-supplied
/// computed values are ignored.
pub async fn cm_bulk_update(
    ctx: &AppContext,
    uid: &str,
    items: &[JsonValue],
) -> Result<Vec<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mut obj = item.as_object().cloned().unwrap_or_default();
        let doc_id = obj
            .remove("documentId")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| ServiceError::bad_payload("bulk update item requires `documentId`"))?;
        let data = strip_computed(&schema, &JsonValue::Object(obj));
        out.push(cm_update(ctx, uid, &doc_id, &data).await?.data);
    }
    Ok(out)
}

/// Delete an entry by document_id.
pub async fn cm_delete(ctx: &AppContext, uid: &str, document_id: &str) -> Result<(), ServiceError> {
    let schema = load_schema(ctx, uid)?;

    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::rbac::action::DELETE,
        schema.uid.as_str(),
    )
    .await?;

    dml::delete_one(&ctx.db, &schema, document_id).await?;
    refresh_relation_computed(ctx).await?;
    let _ = crate::workflow::triggers::dispatch_cms_event(
        ctx,
        "content.deleted",
        schema.uid.as_str(),
        serde_json::json!({ "documentId": document_id }),
    )
    .await;
    Ok(())
}

/// Publish a draft entry.
pub async fn cm_publish(
    ctx: &AppContext,
    uid: &str,
    document_id: &str,
) -> Result<EntryResponse<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    if !schema.draft_and_publish() {
        return Err(ServiceError::Conflict(
            "Draft & Publish is not enabled for this content-type".into(),
        ));
    }

    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::rbac::action::PUBLISH,
        schema.uid.as_str(),
    )
    .await?;

    // Find the draft entry, load it, insert as published, discard draft.
    let draft = dml::find_one_by_document_id(&ctx.db, &schema, document_id)
        .await?
        .ok_or_else(|| ServiceError::not_found(format!("entry {document_id} not found")))?;

    let mut published_data = draft.clone();
    if let Some(obj) = published_data.as_object_mut() {
        obj.insert(
            "publicationState".into(),
            JsonValue::String("published".into()),
        );
        obj.insert(
            "publishedAt".into(),
            JsonValue::String(chrono::Utc::now().to_rfc3339()),
        );
        // Give the published variant a fresh document id so it is a distinct,
        // findable row and does not collide with the (soon soft-deleted) draft.
        obj.insert(
            "documentId".into(),
            JsonValue::String(uuid::Uuid::new_v4().to_string()),
        );
    }

    // Insert published variant
    let txn = ctx.db.begin().await?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);
    let published = dml::insert_one(&txn, &schema, &published_data, user_id).await?;

    // Soft-delete the draft
    dml::delete_one(&txn, &schema, document_id).await?;

    txn.commit().await?;

    let _ = crate::workflow::triggers::dispatch_cms_event(
        ctx,
        "content.published",
        schema.uid.as_str(),
        published.clone(),
    )
    .await;

    Ok(EntryResponse {
        data: published,
        meta: None,
    })
}

/// Unpublish a published entry: soft-delete the published variant and create a
/// new draft from it (Strapi's "go back to draft" behavior).
pub async fn cm_unpublish(
    ctx: &AppContext,
    uid: &str,
    document_id: &str,
) -> Result<EntryResponse<JsonValue>, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    if !schema.draft_and_publish() {
        return Err(ServiceError::Conflict(
            "Draft & Publish is not enabled for this content-type".into(),
        ));
    }

    crate::rbac::enforce_action(
        &ctx.db,
        ctx.current_user.as_ref(),
        crate::rbac::action::PUBLISH,
        schema.uid.as_str(),
    )
    .await?;

    // Find the published entry.
    let published = dml::find_one_by_document_id(&ctx.db, &schema, document_id)
        .await?
        .ok_or_else(|| ServiceError::not_found(format!("entry {document_id} not found")))?;

    let mut draft_data = published.clone();
    if let Some(obj) = draft_data.as_object_mut() {
        obj.insert("publicationState".into(), JsonValue::String("draft".into()));
    }

    let txn = ctx.db.begin().await?;
    let user_id = ctx.current_user.as_ref().map(|u| u.id);

    // Insert the draft variant, then soft-delete the published one.
    let draft = dml::insert_one(&txn, &schema, &draft_data, user_id).await?;
    dml::delete_one(&txn, &schema, document_id).await?;

    txn.commit().await?;

    let _ = crate::workflow::triggers::dispatch_cms_event(
        ctx,
        "content.published",
        schema.uid.as_str(),
        draft.clone(),
    )
    .await;

    Ok(EntryResponse {
        data: draft,
        meta: None,
    })
}

/// Discard draft changes (soft-delete draft, keep published).
pub async fn cm_discard_draft(
    ctx: &AppContext,
    uid: &str,
    document_id: &str,
) -> Result<(), ServiceError> {
    let schema = load_schema(ctx, uid)?;
    dml::delete_one(&ctx.db, &schema, document_id).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Content-type navigation (returns collection + single types)
// ---------------------------------------------------------------------------

/// List content-types available in the Content Manager.
#[derive(serde::Serialize)]
pub struct ContentTypeNavItem {
    pub uid: String,
    pub kind: String,
    pub display_name: String,
    pub is_displayed: bool,
}

pub async fn cm_content_types(ctx: &AppContext) -> Vec<ContentTypeNavItem> {
    ctx.schema_cache
        .get_all()
        .into_iter()
        .filter(|s| s.kind != ContentTypeKind::Component)
        .map(|s| ContentTypeNavItem {
            uid: s.uid.as_str().to_string(),
            kind: s.kind.as_db_str().to_string(),
            display_name: s.info.display_name.clone(),
            is_displayed: true,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Content-Manager View Configuration (Part III §8)
// ---------------------------------------------------------------------------

/// Get the view configuration for a content-type.
pub async fn cm_get_configuration(
    ctx: &AppContext,
    uid: &str,
) -> Result<api_types::admin::ViewConfiguration, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    Ok(api_types::admin::ViewConfiguration::default_for(&schema))
}

/// Update the view configuration for a content-type.
pub async fn cm_update_configuration(
    ctx: &AppContext,
    uid: &str,
    config: &api_types::admin::ViewConfiguration,
) -> Result<api_types::admin::ViewConfiguration, ServiceError> {
    // In a full implementation this would persist to core_store or a dedicated table.
    // For now we just return the submitted config as accepted.
    let _schema = load_schema(ctx, uid)?;
    Ok(config.clone())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn load_schema(ctx: &AppContext, uid_str: &str) -> Result<Schema, ServiceError> {
    let uid = Uid::new(uid_str);
    ctx.schema_cache
        .get(&uid)
        .ok_or_else(|| ServiceError::not_found(format!("content-type `{uid_str}` not found")))
}

fn ensure_collection(schema: &Schema) -> Result<(), ServiceError> {
    if schema.kind != ContentTypeKind::CollectionType {
        return Err(ServiceError::Conflict(
            "This endpoint is only for collection types".into(),
        ));
    }
    Ok(())
}

/// Validate a JSON payload against a schema's field constraints before it is
/// handled, mapping any failure to a `ServiceError::Validation`.
fn validate_payload_or_err(
    schema: &Schema,
    data: &JsonValue,
    enforce_required: bool,
) -> Result<(), ServiceError> {
    let obj = data
        .as_object()
        .ok_or_else(|| ServiceError::bad_payload("payload must be a JSON object"))?;
    let errors = core_schema::validate_payload(schema, obj, enforce_required);
    if errors.is_empty() {
        return Ok(());
    }
    let items = errors
        .into_iter()
        .map(|e| ValidationErrorItem::new(vec![e.field], e.message, e.code))
        .collect();
    Err(ServiceError::validation("payload is invalid", items))
}
