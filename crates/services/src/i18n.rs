//! i18n service (design Part V §8).
//!
//! Locale CRUD and localized content lookup.

use crate::{AppContext, ServiceError};
use api_types::admin::{CreateLocaleRequest, LocaleDto};
use db::entities::i18n_locale;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

/// List all locales.
pub async fn i18n_list(ctx: &AppContext) -> Result<Vec<LocaleDto>, ServiceError> {
    let locales = i18n_locale::Entity::find().all(&ctx.db).await?;
    Ok(locales
        .into_iter()
        .map(|l| LocaleDto {
            id: l.id,
            code: l.code,
            name: l.name,
            is_default: l.is_default,
            created_at: l.created_at,
        })
        .collect())
}

/// Create a new locale.
pub async fn i18n_create(
    ctx: &AppContext,
    req: &CreateLocaleRequest,
) -> Result<LocaleDto, ServiceError> {
    let now = chrono::Utc::now();
    let model = i18n_locale::ActiveModel {
        code: Set(req.code.clone()),
        name: Set(req.name.clone().unwrap_or_else(|| req.code.clone())),
        is_default: Set(req.is_default.unwrap_or(false)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let l = model.insert(&ctx.db).await?;
    Ok(LocaleDto {
        id: l.id,
        code: l.code,
        name: l.name,
        is_default: l.is_default,
        created_at: l.created_at,
    })
}

/// Delete a locale by id.
pub async fn i18n_delete(ctx: &AppContext, locale_id: i64) -> Result<(), ServiceError> {
    let result = i18n_locale::Entity::delete_by_id(locale_id)
        .exec(&ctx.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(ServiceError::not_found("locale not found"));
    }
    Ok(())
}
