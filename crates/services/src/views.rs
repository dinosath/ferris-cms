//! Saved Content Manager views. A view owns presentation/query state only; it
//! never owns records or schema, which remain on the content type.

use crate::{content::load_schema, rbac, AppContext, ServiceError};
use api_types::admin::{
    ContentTypeView, CreateContentTypeViewRequest, ReorderContentTypeViewsRequest,
    UpdateContentTypeViewRequest, ViewConfigurationV2, ViewType,
};
use api_types::{Filter, PaginationParams, QueryParams};
use db::entities::content_type_view;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn default_config(schema: &core_schema::Schema) -> ViewConfigurationV2 {
    ViewConfigurationV2 {
        columns: schema
            .attributes
            .iter()
            .filter(|(_, a)| a.attr_type.is_scalar_column() && !a.private)
            .take(8)
            .enumerate()
            .map(|(position, (field_id, _))| api_types::admin::ViewColumn {
                field_id: field_id.clone(),
                visible: true,
                width: None,
                position: position as u32,
            })
            .collect(),
        page_size: Some(25),
        ..Default::default()
    }
}

fn to_dto(model: content_type_view::Model) -> Result<ContentTypeView, ServiceError> {
    let view_type = match model.view_type.as_str() {
        "grid" => ViewType::Grid,
        "kanban" => ViewType::Kanban,
        "gallery" => ViewType::Gallery,
        "calendar" => ViewType::Calendar,
        other => {
            return Err(ServiceError::internal(format!(
                "unknown view type `{other}`"
            )))
        }
    };
    Ok(ContentTypeView {
        id: model.id,
        content_type_uid: model.content_type_uid,
        name: model.name,
        slug: model.slug,
        view_type,
        description: model.description,
        is_default: model.is_default,
        position: model.position,
        configuration: serde_json::from_value(model.configuration_json)
            .map_err(|e| ServiceError::internal(format!("invalid view configuration: {e}")))?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

fn validate_config(
    schema: &core_schema::Schema,
    config: &ViewConfigurationV2,
) -> Result<(), ServiceError> {
    let valid = |field: &str| {
        field == "id"
            || field == "documentId"
            || field == "createdAt"
            || field == "updatedAt"
            || field == "publishedAt"
            || field == "publicationState"
            || schema.attributes.contains_key(field)
    };
    for column in &config.columns {
        if !valid(&column.field_id) {
            return Err(ServiceError::bad_payload(format!(
                "unknown view field `{}`",
                column.field_id
            )));
        }
    }
    fn check_filter(
        schema: &core_schema::Schema,
        valid: &dyn Fn(&str) -> bool,
        f: &Filter,
    ) -> Result<(), ServiceError> {
        match f {
            Filter::And(xs) | Filter::Or(xs) => {
                xs.iter().try_for_each(|x| check_filter(schema, valid, x))
            }
            Filter::Not(x) => check_filter(schema, valid, x),
            Filter::Leaf { field, .. } => {
                if valid(field) {
                    Ok(())
                } else {
                    Err(ServiceError::bad_payload(format!(
                        "unknown filter field `{field}`"
                    )))
                }
            }
        }
    }
    if let Some(filter) = &config.filters {
        check_filter(schema, &valid, filter)?;
    }
    for sort in &config.sorts {
        if !valid(&sort.field) {
            return Err(ServiceError::bad_payload(format!(
                "unknown sort field `{}`",
                sort.field
            )));
        }
    }
    Ok(())
}

async fn authorize(
    ctx: &AppContext,
    uid: &str,
    action: &str,
) -> Result<core_schema::Schema, ServiceError> {
    let schema = load_schema(ctx, uid)?;
    rbac::enforce_action(&ctx.db, ctx.current_user.as_ref(), action, uid).await?;
    Ok(schema)
}

async fn ensure_default(
    ctx: &AppContext,
    uid: &str,
    schema: &core_schema::Schema,
) -> Result<(), ServiceError> {
    let exists = content_type_view::Entity::find()
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .count(&ctx.db)
        .await?;
    if exists == 0 {
        let now = chrono::Utc::now();
        content_type_view::ActiveModel {
            content_type_uid: Set(uid.to_string()),
            name: Set("All Records".into()),
            slug: Set("all-records".into()),
            view_type: Set("grid".into()),
            description: Set(None),
            is_default: Set(true),
            position: Set(0),
            configuration_json: Set(serde_json::to_value(default_config(schema))
                .map_err(|e| ServiceError::internal(e.to_string()))?),
            created_at: Set(now),
            updated_at: Set(now),
            created_by: Set(ctx.current_user.as_ref().map(|u| u.id)),
            updated_by: Set(ctx.current_user.as_ref().map(|u| u.id)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(())
}

pub async fn list_views(ctx: &AppContext, uid: &str) -> Result<Vec<ContentTypeView>, ServiceError> {
    let schema = authorize(ctx, uid, rbac::action::READ).await?;
    ensure_default(ctx, uid, &schema).await?;
    let rows = content_type_view::Entity::find()
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .order_by_asc(content_type_view::Column::Position)
        .all(&ctx.db)
        .await?;
    rows.into_iter().map(to_dto).collect()
}

pub async fn get_view(
    ctx: &AppContext,
    uid: &str,
    id: i64,
) -> Result<ContentTypeView, ServiceError> {
    authorize(ctx, uid, rbac::action::READ).await?;
    let row = content_type_view::Entity::find_by_id(id)
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("view not found"))?;
    to_dto(row)
}

pub async fn create_view(
    ctx: &AppContext,
    uid: &str,
    req: &CreateContentTypeViewRequest,
) -> Result<ContentTypeView, ServiceError> {
    let schema = authorize(ctx, uid, rbac::action::UPDATE).await?;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ServiceError::bad_payload("view name is required"));
    }
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(ServiceError::bad_payload(
            "view name must contain a letter or number",
        ));
    }
    let mut config = req.configuration.clone();
    if config.columns.is_empty() {
        config = default_config(&schema);
    }
    validate_config(&schema, &config)?;
    let has_default = content_type_view::Entity::find()
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .filter(content_type_view::Column::IsDefault.eq(true))
        .count(&ctx.db)
        .await?
        > 0;
    let position = content_type_view::Entity::find()
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .count(&ctx.db)
        .await? as i64;
    if req.view_type == ViewType::Calendar
        && !schema.attributes.values().any(|a| {
            matches!(
                a.attr_type,
                core_domain::FieldType::Date | core_domain::FieldType::Datetime
            )
        })
    {
        return Err(ServiceError::bad_payload(
            "calendar views require a date or datetime field",
        ));
    }
    let now = chrono::Utc::now();
    let row = content_type_view::ActiveModel {
        content_type_uid: Set(uid.into()),
        name: Set(name.into()),
        slug: Set(slug),
        view_type: Set(format_view_type(req.view_type)),
        description: Set(req.description.clone()),
        is_default: Set(!has_default),
        position: Set(position),
        configuration_json: Set(
            serde_json::to_value(config).map_err(|e| ServiceError::internal(e.to_string()))?
        ),
        created_at: Set(now),
        updated_at: Set(now),
        created_by: Set(ctx.current_user.as_ref().map(|u| u.id)),
        updated_by: Set(ctx.current_user.as_ref().map(|u| u.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    to_dto(row)
}

pub async fn update_view(
    ctx: &AppContext,
    uid: &str,
    id: i64,
    req: &UpdateContentTypeViewRequest,
) -> Result<ContentTypeView, ServiceError> {
    let schema = authorize(ctx, uid, rbac::action::UPDATE).await?;
    let mut row = content_type_view::Entity::find_by_id(id)
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("view not found"))?
        .into_active_model();
    if let Some(name) = &req.name {
        if name.trim().is_empty() {
            return Err(ServiceError::bad_payload("view name is required"));
        }
        row.name = Set(name.trim().into());
        row.slug = Set(slugify(name));
    }
    if let Some(description) = &req.description {
        row.description = Set(Some(description.clone()));
    }
    if let Some(view_type) = req.view_type {
        row.view_type = Set(format_view_type(view_type));
    }
    if let Some(config) = &req.configuration {
        validate_config(&schema, config)?;
        row.configuration_json =
            Set(serde_json::to_value(config).map_err(|e| ServiceError::internal(e.to_string()))?);
    }
    row.updated_at = Set(chrono::Utc::now());
    row.updated_by = Set(ctx.current_user.as_ref().map(|u| u.id));
    to_dto(row.update(&ctx.db).await?)
}

pub async fn delete_view(ctx: &AppContext, uid: &str, id: i64) -> Result<(), ServiceError> {
    authorize(ctx, uid, rbac::action::UPDATE).await?;
    let view_count = content_type_view::Entity::find()
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .count(&ctx.db)
        .await?;
    if view_count <= 1 {
        return Err(ServiceError::conflict(
            "a content type must keep at least one view",
        ));
    }
    let row = content_type_view::Entity::find_by_id(id)
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("view not found"))?;
    content_type_view::Entity::delete_by_id(id)
        .exec(&ctx.db)
        .await?;
    if row.is_default {
        if let Some(next) = content_type_view::Entity::find()
            .filter(content_type_view::Column::ContentTypeUid.eq(uid))
            .order_by_asc(content_type_view::Column::Position)
            .one(&ctx.db)
            .await?
        {
            let mut active = next.into_active_model();
            active.is_default = Set(true);
            active.update(&ctx.db).await?;
        }
    }
    Ok(())
}

pub async fn duplicate_view(
    ctx: &AppContext,
    uid: &str,
    id: i64,
) -> Result<ContentTypeView, ServiceError> {
    let source = get_view(ctx, uid, id).await?;
    let req = CreateContentTypeViewRequest {
        name: format!("{} Copy", source.name),
        view_type: source.view_type,
        description: source.description,
        configuration: source.configuration,
    };
    create_view(ctx, uid, &req).await
}

pub async fn set_default_view(
    ctx: &AppContext,
    uid: &str,
    id: i64,
) -> Result<ContentTypeView, ServiceError> {
    authorize(ctx, uid, rbac::action::UPDATE).await?;
    let target = content_type_view::Entity::find_by_id(id)
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("view not found"))?;
    content_type_view::Entity::update_many()
        .filter(content_type_view::Column::ContentTypeUid.eq(uid))
        .col_expr(
            content_type_view::Column::IsDefault,
            sea_orm::sea_query::Expr::value(false),
        )
        .exec(&ctx.db)
        .await?;
    let mut active = target.into_active_model();
    active.is_default = Set(true);
    active.updated_at = Set(chrono::Utc::now());
    to_dto(active.update(&ctx.db).await?)
}

pub async fn reorder_views(
    ctx: &AppContext,
    uid: &str,
    req: &ReorderContentTypeViewsRequest,
) -> Result<Vec<ContentTypeView>, ServiceError> {
    authorize(ctx, uid, rbac::action::UPDATE).await?;
    for (position, id) in req.ids.iter().enumerate() {
        let Some(row) = content_type_view::Entity::find_by_id(*id)
            .filter(content_type_view::Column::ContentTypeUid.eq(uid))
            .one(&ctx.db)
            .await?
        else {
            return Err(ServiceError::bad_payload(
                "all reordered views must belong to the content type",
            ));
        };
        let mut active = row.into_active_model();
        active.position = Set(position as i64);
        active.updated_at = Set(chrono::Utc::now());
        active.update(&ctx.db).await?;
    }
    list_views(ctx, uid).await
}

pub fn query_params_for_view(view: &ContentTypeView, page: i64, page_size: i64) -> QueryParams {
    QueryParams {
        filters: view.configuration.filters.clone(),
        sort: view.configuration.sorts.clone(),
        pagination: Some(PaginationParams::Page {
            page,
            page_size: view
                .configuration
                .page_size
                .map(|n| n as i64)
                .unwrap_or(page_size),
            with_count: Some(true),
        }),
        ..Default::default()
    }
}

fn format_view_type(view_type: ViewType) -> String {
    match view_type {
        ViewType::Grid => "grid",
        ViewType::Kanban => "kanban",
        ViewType::Gallery => "gallery",
        ViewType::Calendar => "calendar",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slug_is_stable() {
        assert_eq!(slugify("Active Products"), "active-products");
        assert_eq!(slugify("  A / B  "), "a-b");
    }
}
