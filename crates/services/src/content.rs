//! Content CRUD service (design Part V §3-§4).
//!
//! Serves the public `/api/*` and admin `/content-manager/*` endpoints.
//! All data access goes through `dynamic-store` (SeaQuery) because tables
//! are runtime-defined.

use crate::{AppContext, ServiceError, ValidationErrorItem};
use api_types::{EntryResponse, ListResponse, Pagination, QueryParams};
use core_domain::{ContentTypeKind, Uid};
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
    let row = dml::find_one_by_document_id(&ctx.db, &schema, document_id)
        .await?
        .ok_or_else(|| ServiceError::not_found(format!("entry {document_id} not found")))?;
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
