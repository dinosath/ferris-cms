//! First-boot seed data (design Part III §9).
//!
//! Seeds: Super Admin / Editor / Author roles, default locale `en`.
//! The first Super Admin *user* is created via registration, never seeded.

use crate::entities::{admin_role, i18n_locale};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{DatabaseConnection, DbErr, EntityTrait, Set};

/// `do_nothing` upserts report `RecordNotInserted` when the row already
/// exists; that is the idempotent success case for seeding.
fn ignore_not_inserted(
    res: Result<sea_orm::InsertResult<impl sea_orm::ActiveModelTrait>, DbErr>,
) -> Result<(), DbErr> {
    match res {
        Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
        Err(e) => Err(e),
    }
}

pub const ROLE_SUPER_ADMIN: &str = "strapi-super-admin";
pub const ROLE_EDITOR: &str = "strapi-editor";
pub const ROLE_AUTHOR: &str = "strapi-author";

/// Idempotent: safe to call on every boot.
pub async fn seed(db: &DatabaseConnection) -> Result<(), DbErr> {
    seed_roles(db).await?;
    seed_locales(db).await?;
    seed_workflow_permissions(db).await?;
    Ok(())
}

/// Workflow automation permission actions (app-level RBAC matrix).
pub mod workflow_actions {
    pub const ALL: &[&str] = &[
        "plugin::workflow.workflow.read",
        "plugin::workflow.workflow.create",
        "plugin::workflow.workflow.update",
        "plugin::workflow.workflow.delete",
        "plugin::workflow.workflow.execute",
        "plugin::workflow.workflow.activate",
        "plugin::workflow.execution.read",
        "plugin::workflow.credential.read",
        "plugin::workflow.credential.manage",
    ];
    pub const SUBJECT_WORKFLOW: &str = "plugin::workflow.workflow";
}

/// Grant the workflow permission actions to the Editor role (Super Admins
/// bypass the matrix entirely). Idempotent — replaces prior grants for the
/// subject to keep the matrix in sync with the seeded action set.
async fn seed_workflow_permissions(db: &DatabaseConnection) -> Result<(), DbErr> {
    use crate::entities::{admin_permission, admin_role};
    use sea_orm::QueryFilter;

    let roles = admin_role::Entity::find()
        .filter(
            admin_role::COLUMN
                .code
                .is_in(vec![ROLE_EDITOR.to_string(), ROLE_SUPER_ADMIN.to_string()]),
        )
        .all(db)
        .await?;
    if roles.is_empty() {
        return Ok(());
    }

    // Clear prior workflow grants so seeding stays in sync.
    admin_permission::Entity::delete_many()
        .filter(
            admin_permission::COLUMN
                .subject
                .eq(Some(workflow_actions::SUBJECT_WORKFLOW.to_string())),
        )
        .exec(db)
        .await?;

    let now = chrono::Utc::now();
    for role in &roles {
        for action in workflow_actions::ALL {
            let model = admin_permission::ActiveModel {
                role_id: Set(role.id),
                action: Set(action.to_string()),
                subject: Set(Some(workflow_actions::SUBJECT_WORKFLOW.to_string())),
                properties_json: Set(serde_json::json!({})),
                conditions_json: Set(serde_json::json!([])),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            ignore_not_inserted(admin_permission::Entity::insert(model).exec(db).await)?;
        }
    }
    Ok(())
}

async fn seed_roles(db: &DatabaseConnection) -> Result<(), DbErr> {
    let now = Utc::now();
    for (name, code, description) in [
        (
            "Super Admin",
            ROLE_SUPER_ADMIN,
            "Super Admins can access and manage all features and settings.",
        ),
        (
            "Editor",
            ROLE_EDITOR,
            "Editors can manage and publish contents including those of other users.",
        ),
        (
            "Author",
            ROLE_AUTHOR,
            "Authors can manage the contents they have created.",
        ),
    ] {
        let model = admin_role::ActiveModel {
            name: Set(name.to_string()),
            code: Set(code.to_string()),
            description: Set(Some(description.to_string())),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        ignore_not_inserted(
            admin_role::Entity::insert(model)
                .on_conflict(
                    OnConflict::column(admin_role::COLUMN.code)
                        .do_nothing()
                        .to_owned(),
                )
                .exec(db)
                .await,
        )?;
    }
    Ok(())
}

async fn seed_locales(db: &DatabaseConnection) -> Result<(), DbErr> {
    let now = Utc::now();
    let model = i18n_locale::ActiveModel {
        code: Set("en".to_string()),
        name: Set("English (en)".to_string()),
        is_default: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    ignore_not_inserted(
        i18n_locale::Entity::insert(model)
            .on_conflict(
                OnConflict::column(i18n_locale::COLUMN.code)
                    .do_nothing()
                    .to_owned(),
            )
            .exec(db)
            .await,
    )?;
    Ok(())
}

/// True once at least one admin user exists (drives first-run registration).
pub async fn has_admin(db: &DatabaseConnection) -> Result<bool, DbErr> {
    use sea_orm::{EntityTrait, PaginatorTrait};
    let count = crate::entities::admin_user::Entity::find()
        .count(db)
        .await?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::connect_sqlite_memory;
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn migrate_and_seed_sqlite() {
        let db = connect_sqlite_memory().await.unwrap();
        crate::migration::Migrator::up(&db, None).await.unwrap();
        seed(&db).await.unwrap();
        // idempotent
        seed(&db).await.unwrap();
        assert!(!has_admin(&db).await.unwrap());

        use sea_orm::{EntityTrait, PaginatorTrait};
        let roles = admin_role::Entity::find().count(&db).await.unwrap();
        assert_eq!(roles, 3);
        let locales = i18n_locale::Entity::find().count(&db).await.unwrap();
        assert_eq!(locales, 1);
    }
}
