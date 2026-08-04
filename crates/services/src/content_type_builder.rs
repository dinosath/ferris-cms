//! Content-Type Builder service (design Part IV).
//!
//! Orchestrates the full CTB workflow:
//! validate (core-schema) → diff (core-schema) → DDL (dynamic-store) →
//! registry upsert (db) → schema cache rebuild.
//! Everything runs inside one transaction.

use crate::{schema_cache::SchemaCache, AppContext, ServiceError, ValidationErrorItem};
use core_domain::ContentTypeKind;
use core_schema::{diff, diff_removed, Schema, SchemaDiff, validate_schemas};
use db::entities::content_type_schema;
use sea_orm::{
    ActiveModelTrait, EntityTrait, QueryFilter, Set, TransactionTrait,
};
use std::collections::HashSet;

/// List all schemas currently in the cache (read path).
pub async fn ctb_list(ctx: &AppContext) -> Vec<Schema> {
    ctx.schema_cache.get_all()
}

/// Get a single schema by uid.
pub async fn ctb_get(ctx: &AppContext, uid: &str) -> Result<Schema, ServiceError> {
    let uid = core_domain::Uid::new(uid);
    ctx.schema_cache
        .get(&uid)
        .ok_or_else(|| ServiceError::not_found(format!("schema `{uid}` not found")))
}

/// Batch-apply the desired schema set.
pub async fn ctb_apply(
    ctx: &AppContext,
    desired: Vec<Schema>,
) -> Result<Vec<Schema>, ServiceError> {
    // 1. Validate
    let validation_errors = validate_schemas(&desired);
    if !validation_errors.is_empty() {
        let items: Vec<ValidationErrorItem> = validation_errors
            .into_iter()
            .map(|e| ValidationErrorItem {
                path: e.path.split('.').map(|s| s.to_string()).collect(),
                message: e.message,
                name: "ValidationError".into(),
            })
            .collect();
        return Err(ServiceError::Validation(items));
    }

    // 2. Load current registry.
    let current = ctx.schema_cache.get_all();

    // 3. Compute diffs.
    let mut diffs: Vec<SchemaDiff> = Vec::new();
    let desired_uids: HashSet<_> = desired.iter().map(|s| &s.uid).collect();

    for schema in &desired {
        let cur = current.iter().find(|s| s.uid == schema.uid);
        diffs.push(diff(cur, schema));
    }
    for schema in &current {
        if !desired_uids.contains(&schema.uid) {
            diffs.push(diff_removed(schema));
        }
    }

    // 4. Apply DDL inside one transaction.
    let backend = ctx.db_backend();
    let db = &ctx.db;

    let txn = db.begin().await?;

    for d in &diffs {
        if d.is_noop() {
            continue;
        }
        let _actions = dynamic_store::ddl::apply_schema_diff(&txn, backend, d, &desired).await?;
    }

    // 5. Upsert schema JSON rows.
    let now = chrono::Utc::now();
    for schema in &desired {
        let json = serde_json::to_value(schema)
            .map_err(|e| ServiceError::internal(e.to_string()))?;

        let existing = content_type_schema::Entity::find()
            .filter(content_type_schema::COLUMN.uid.eq(schema.uid.as_str()))
            .one(&txn)
            .await?;

        let is_component = schema.kind == ContentTypeKind::Component;
        let category = if is_component {
            schema.uid.component_category().map(|c| c.to_string())
        } else {
            None
        };

        if let Some(row) = existing {
            // Manually build ActiveModel from Model fields.
            let am = content_type_schema::ActiveModel {
                id: Set(row.id),
                uid: Set(schema.uid.as_str().to_string()),
                kind: Set(schema.kind.as_db_str().to_string()),
                category: Set(category),
                display_name: Set(schema.info.display_name.clone()),
                singular_api_id: Set(if is_component {
                    None
                } else {
                    Some(schema.info.singular_name.clone())
                }),
                plural_api_id: Set(if is_component {
                    None
                } else {
                    Some(schema.info.plural_name.clone())
                }),
                schema_json: Set(json),
                draft_and_publish: Set(schema.draft_and_publish()),
                i18n_localized: Set(schema.is_localized()),
                is_system: Set(row.is_system),
                version: Set(row.version + 1),
                created_at: Set(row.created_at),
                updated_at: Set(now),
                sync_version: Set(row.sync_version),
                origin_node_id: Set(row.origin_node_id),
                deleted_at: Set(row.deleted_at),
            };
            am.update(&txn).await?;
        } else {
            let am = content_type_schema::ActiveModel {
                uid: Set(schema.uid.as_str().to_string()),
                kind: Set(schema.kind.as_db_str().to_string()),
                category: Set(category),
                display_name: Set(schema.info.display_name.clone()),
                singular_api_id: Set(if is_component {
                    None
                } else {
                    Some(schema.info.singular_name.clone())
                }),
                plural_api_id: Set(if is_component {
                    None
                } else {
                    Some(schema.info.plural_name.clone())
                }),
                schema_json: Set(json),
                draft_and_publish: Set(schema.draft_and_publish()),
                i18n_localized: Set(schema.is_localized()),
                is_system: Set(false),
                version: Set(1),
                created_at: Set(now),
                updated_at: Set(now),
                sync_version: Set(0),
                origin_node_id: Set(None),
                deleted_at: Set(None),
                ..Default::default()
            };
            am.insert(&txn).await?;
        }
    }

    // Mark removed schemas.
    for d in &diffs {
        if matches!(d.kind, core_schema::DiffKind::Removed) {
            let existing = content_type_schema::Entity::find()
                .filter(content_type_schema::COLUMN.uid.eq(d.uid.as_str()))
                .one(&txn)
                .await?;
            if let Some(row) = existing {
                let am = content_type_schema::ActiveModel {
                    id: Set(row.id),
                    uid: Set(row.uid),
                    kind: Set(row.kind),
                    category: Set(row.category),
                    display_name: Set(row.display_name),
                    singular_api_id: Set(row.singular_api_id),
                    plural_api_id: Set(row.plural_api_id),
                    schema_json: Set(row.schema_json),
                    draft_and_publish: Set(row.draft_and_publish),
                    i18n_localized: Set(row.i18n_localized),
                    is_system: Set(row.is_system),
                    version: Set(-row.version),
                    created_at: Set(row.created_at),
                    updated_at: Set(now),
                    sync_version: Set(row.sync_version),
                    origin_node_id: Set(row.origin_node_id),
                    deleted_at: Set(Some(now)),
                };
                am.update(&txn).await?;
            }
        }
    }

    txn.commit().await?;

    // 6. Rebuild schema cache.
    rebuild_cache(ctx).await?;

    // 7. Register new content tables in SeaORM RBAC so users can access them,
    //    and grant granular per-content-type permissions to Editor/Author roles.
    for schema in &desired {
        if schema.kind != core_domain::ContentTypeKind::Component {
            let table = schema.table_name();
            let _ = crate::rbac::register_content_table(&ctx.db, &table).await;
            let _ = crate::rbac::grant_content_permissions(&ctx.db, schema.uid.as_str()).await;
        }
    }

    Ok(desired)
}

/// Rebuild the schema cache from the database.
pub async fn rebuild_cache(ctx: &AppContext) -> Result<(), ServiceError> {
    let rows = content_type_schema::Entity::find()
        .filter(content_type_schema::COLUMN.version.gt(0))
        .all(&ctx.db)
        .await?;

    let schemas: Vec<Schema> = rows
        .into_iter()
        .filter_map(|row| {
            serde_json::from_value::<Schema>(row.schema_json).ok()
        })
        .collect();

    ctx.schema_cache.replace(schemas);
    Ok(())
}

/// Load the schema cache from the database at startup.
pub async fn load_schema_cache(
    db: &sea_orm::DatabaseConnection,
    cache: &SchemaCache,
) -> Result<(), ServiceError> {
    let rows = content_type_schema::Entity::find()
        .filter(content_type_schema::COLUMN.version.gt(0))
        .all(db)
        .await?;

    let schemas: Vec<Schema> = rows
        .into_iter()
        .filter_map(|row| {
            serde_json::from_value::<Schema>(row.schema_json).ok()
        })
        .collect();

    cache.replace(schemas);
    Ok(())
}

/// Return reserved names that can't be used as API ids / attribute names.
pub fn ctb_reserved_names() -> Vec<String> {
    let mut names: Vec<String> = core_domain::reserved::RESERVED_API_IDS
        .iter()
        .map(|s| s.to_string())
        .collect();
    names.extend(
        core_domain::reserved::RESERVED_ATTRIBUTES
            .iter()
            .map(|s| s.to_string()),
    );
    names
}
