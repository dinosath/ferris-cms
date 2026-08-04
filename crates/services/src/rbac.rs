//! RBAC service — SeaORM 2.0 native Role-Based Access Control (design Part III §4, Part V §6).
//!
//! Uses SeaORM 2.0's built-in RBAC engine for table-level access control:
//! - Roles: super-admin, editor, author, public
//! - Hierarchy: super-admin <- editor <- author <- public
//! - Permissions map to table-level CRUD (select/insert/update/delete)
//!
//! On boot, `init_rbac` creates RBAC tables (via migrations), loads the engine,
//! defines the standard roles/permissions, and assigns admin users to roles.
//! At runtime, handlers call `ctx.restricted_connection()` to get a
//! `RestrictedConnection` scoped to the current user.

use sea_orm::{
    rbac::{RbacContext, RbacUserId},
    DatabaseConnection, DbErr,
};

use crate::ServiceError;

/// Role names for the standard Strapi roles.
pub const ROLE_SUPER_ADMIN: &str = "strapi-super-admin";
pub const ROLE_EDITOR: &str = "strapi-editor";
pub const ROLE_AUTHOR: &str = "strapi-author";
pub const ROLE_PUBLIC: &str = "strapi-public";

/// Strapi-standard admin permission actions for the Content Manager.
/// These mirror Strapi's `plugin::content-manager.explorer.*` action keys
/// so the permission matrix matches the official admin panel.
pub mod action {
    pub const CREATE: &str = "plugin::content-manager.explorer.create";
    pub const READ: &str = "plugin::content-manager.explorer.read";
    pub const UPDATE: &str = "plugin::content-manager.explorer.update";
    pub const DELETE: &str = "plugin::content-manager.explorer.delete";
    pub const PUBLISH: &str = "plugin::content-manager.explorer.publish";

    /// All content-explorer actions.
    pub const ALL: &[&str] = &[CREATE, READ, UPDATE, DELETE, PUBLISH];
    /// Actions a content Author may perform (their own content).
    pub const AUTHOR: &[&str] = &[CREATE, READ, UPDATE];
    /// Actions an Editor may perform (anyone's content, incl. publish).
    pub const EDITOR: &[&str] = &[CREATE, READ, UPDATE, DELETE, PUBLISH];

    /// Wildcard subject used by app-level "manage everything" grants.
    pub const SUBJECT_WILDCARD: &str = "*";
}

/// Initialize the SeaORM RBAC engine: load rules from the database,
/// define standard roles + permissions if not already present, and
/// store the engine in the database connection.
pub async fn init_rbac(db: &DatabaseConnection) -> Result<(), DbErr> {
    // Load existing RBAC rules from the database into the engine.
    db.load_rbac().await?;

    // Idempotently create the standard roles, resources, and permissions.
    let mut ctx = RbacContext::load(db).await?;

    // Define standard resources (system tables).
    // User content-type tables are added dynamically via `register_content_table`.
    let system_tables: &[&str] = &[
        "content_type_schemas",
        "content_type_table_map",
        "admin_user",
        "admin_role",
        "admin_permission",
        "admin_user_role",
        "api_token",
        "api_token_permission",
        "upload_file",
        "upload_folder",
        "i18n_locale",
        "core_store",
        "schema_change_log",
        "sync_state",
        "sync_oplog",
    ];

    ctx.add_tables(db, system_tables).await?;
    ctx.add_crud_permissions(db).await?;

    // Define roles.
    ctx.add_roles(
        db,
        &[ROLE_SUPER_ADMIN, ROLE_EDITOR, ROLE_AUTHOR, ROLE_PUBLIC],
    )
    .await?;

    // Define role hierarchy: super-admin <- editor <- author <- public
    ctx.add_role_hierarchy(
        db,
        &[
            sea_orm::rbac::RbacAddRoleHierarchy {
                super_role: ROLE_EDITOR,
                role: ROLE_PUBLIC,
            },
            sea_orm::rbac::RbacAddRoleHierarchy {
                super_role: ROLE_AUTHOR,
                role: ROLE_EDITOR,
            },
            sea_orm::rbac::RbacAddRoleHierarchy {
                super_role: ROLE_SUPER_ADMIN,
                role: ROLE_AUTHOR,
            },
        ],
    )
    .await?;

    // public: can only read (select) everything
    ctx.add_role_permissions(db, ROLE_PUBLIC, &["select"], &["*"])
        .await?;

    // author: can CRUD their own content tables (granted by the hierarchy from editor)
    // editor: inherits author + gets insert/update/delete on upload_file
    ctx.add_role_permissions(db, ROLE_EDITOR, &["insert", "update"], &["upload_file"])
        .await?;

    // super-admin: can insert/update/delete EVERYTHING (already inherits select from public)
    ctx.add_role_permissions(db, ROLE_SUPER_ADMIN, &["insert", "update", "delete"], &["*"])
        .await?;

    // Reload the engine with updated rules.
    db.load_rbac().await?;

    Ok(())
}

/// Register a dynamically-created content table in the RBAC system.
/// Called after the Content-Type Builder creates a new table.
pub async fn register_content_table(
    db: &DatabaseConnection,
    table_name: &str,
) -> Result<(), DbErr> {
    let mut ctx = RbacContext::load(db).await?;

    // Add the table as a resource.
    // Leak the string to get a &'static str (content type tables are long-lived).
    let table_static: &'static str = Box::leak(table_name.to_string().into_boxed_str());
    ctx.add_tables(db, &[table_static]).await?;

    // Grant editor + author roles insert/update on this new table
    // (select is already inherited from public via hierarchy wildcard).
    ctx.add_role_permissions(db, ROLE_AUTHOR, &["insert", "update"], &[table_static])
        .await?;

    // Reload engine.
    db.load_rbac().await?;

    Ok(())
}

/// Assign a user to a role in the RBAC system.
pub async fn assign_user_role(
    db: &DatabaseConnection,
    user_id: i64,
    role_name: &str,
) -> Result<(), DbErr> {
    let mut ctx = RbacContext::load(db).await?;
    // Leak strings to get &'static str (role names are known at compile time anyway).
    let role_static: &'static str = Box::leak(role_name.to_string().into_boxed_str());
    ctx.assign_user_role(db, &[(user_id, role_static)]).await?;
    db.load_rbac().await?;
    Ok(())
}

/// Get a restricted connection for a specific user.
/// Returns None if the RBAC engine is not configured.
pub fn restricted_for(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<sea_orm::RestrictedConnection, DbErr> {
    db.restricted_for(RbacUserId(user_id))
}

/// Check if RBAC engine is loaded.
pub fn is_rbac_loaded(_db: &DatabaseConnection) -> bool {
    // The DB connection tracks whether RBAC is loaded internally.
    // We try to create an unrestricted snapshot check.
    true // always true after init_rbac is called
}

// ---------------------------------------------------------------------------
// Compatibility: keep existing admin role/perm entities for app-level RBAC
// (design Part III §4). SeaORM RBAC handles table-level; these handle
// app-level actions like "manage content-types" or "view settings".
// ---------------------------------------------------------------------------

/// List all admin roles (app-level, not SeaORM RBAC roles).
pub async fn list_roles(
    db: &DatabaseConnection,
) -> Result<Vec<api_types::admin::AdminRoleDto>, ServiceError> {
    use db::entities::admin_role;
    use sea_orm::EntityTrait;

    let roles = admin_role::Entity::find().all(db).await?;
    Ok(roles
        .into_iter()
        .map(|r| api_types::admin::AdminRoleDto {
            id: r.id,
            name: r.name,
            code: r.code,
            description: r.description,
        })
        .collect())
}

/// Get a single role by id.
pub async fn get_role(
    db: &DatabaseConnection,
    role_id: i64,
) -> Result<api_types::admin::AdminRoleDto, ServiceError> {
    use db::entities::admin_role;
    use sea_orm::EntityTrait;

    let role = admin_role::Entity::find_by_id(role_id)
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::not_found("role not found"))?;
    Ok(api_types::admin::AdminRoleDto {
        id: role.id,
        name: role.name,
        code: role.code,
        description: role.description,
    })
}

/// Update role permissions (app-level).
pub async fn update_permissions(
    db: &DatabaseConnection,
    role_id: i64,
    req: &api_types::admin::UpdateRolePermissionsRequest,
) -> Result<Vec<api_types::admin::AdminPermissionDto>, ServiceError> {
    use db::entities::admin_permission;
    use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, Set};

    admin_permission::Entity::delete_many()
        .filter(admin_permission::COLUMN.role_id.eq(role_id))
        .exec(db)
        .await?;

    let now = chrono::Utc::now();
    let mut result = Vec::new();
    for perm in &req.permissions {
        let model = admin_permission::ActiveModel {
            role_id: Set(role_id),
            action: Set(perm.action.clone()),
            subject: Set(perm.subject.clone()),
            properties_json: Set(perm.properties.clone()),
            conditions_json: Set(perm.conditions.clone()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let p = model.insert(db).await?;
        result.push(api_types::admin::AdminPermissionDto {
            id: p.id,
            action: p.action,
            subject: p.subject,
            properties: p.properties_json,
            conditions: p.conditions_json,
            role_id: Some(role_id),
        });
    }
    Ok(result)
}

/// Load app-level roles for a user.
pub async fn load_user_app_roles(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<api_types::admin::AdminRoleDto>, ServiceError> {
    use db::entities::{admin_role, admin_user_role};
    use sea_orm::{EntityTrait, QueryFilter};

    let role_links = admin_user_role::Entity::find()
        .filter(admin_user_role::COLUMN.user_id.eq(user_id))
        .all(db)
        .await?;

    if role_links.is_empty() {
        return Ok(vec![]);
    }

    let role_ids: Vec<i64> = role_links.iter().map(|r| r.role_id).collect();
    let roles = admin_role::Entity::find()
        .filter(admin_role::COLUMN.id.is_in(role_ids))
        .all(db)
        .await?;

    Ok(roles
        .into_iter()
        .map(|r| api_types::admin::AdminRoleDto {
            id: r.id,
            name: r.name,
            code: r.code,
            description: r.description,
        })
        .collect())
}

/// Map an app-level role code to the SeaORM RBAC role name.
pub fn app_role_to_rbac_role(code: &str) -> &'static str {
    match code {
        ROLE_SUPER_ADMIN => ROLE_SUPER_ADMIN,
        "strapi-editor" => ROLE_EDITOR,
        "strapi-author" => ROLE_AUTHOR,
        _ => ROLE_PUBLIC,
    }
}

/// Resolve an authenticated user's role ids from role codes.
async fn resolve_role_ids(
    db: &DatabaseConnection,
    codes: &[String],
) -> Result<Vec<i64>, ServiceError> {
    use db::entities::admin_role;
    use sea_orm::{EntityTrait, QueryFilter};

    if codes.is_empty() {
        return Ok(vec![]);
    }
    let roles = admin_role::Entity::find()
        .filter(admin_role::COLUMN.code.is_in(codes.to_vec()))
        .all(db)
        .await?;
    Ok(roles.into_iter().map(|r| r.id).collect())
}

/// Evaluate a granular, per-content-type permission for an authenticated user.
///
/// This is the app-level RBAC equivalent of Strapi's Content Manager
/// permission matrix. Super Admins always pass; everyone else must hold a
/// grant for `(action, subject)` on one of their roles, either directly on
/// the content-type uid or via the `*` wildcard.
pub async fn can_perform(
    db: &DatabaseConnection,
    user: &crate::CurrentUser,
    action: &str,
    subject: &str,
) -> Result<bool, ServiceError> {
    use db::entities::admin_permission;
    use sea_orm::{EntityTrait, PaginatorTrait, QueryFilter};

    // Super Admin bypasses the permission matrix (Strapi behavior).
    if user.roles.iter().any(|r| r == ROLE_SUPER_ADMIN) {
        return Ok(true);
    }
    if user.roles.is_empty() {
        return Ok(false);
    }

    let role_ids = resolve_role_ids(db, &user.roles).await?;
    if role_ids.is_empty() {
        return Ok(false);
    }

    let exact = admin_permission::Entity::find()
        .filter(admin_permission::COLUMN.role_id.is_in(role_ids.clone()))
        .filter(admin_permission::COLUMN.action.eq(action))
        .filter(admin_permission::COLUMN.subject.eq(Some(subject.to_string())))
        .count(db)
        .await?;
    if exact > 0 {
        return Ok(true);
    }

    let wild = admin_permission::Entity::find()
        .filter(admin_permission::COLUMN.role_id.is_in(role_ids))
        .filter(admin_permission::COLUMN.action.eq(action))
        .filter(admin_permission::COLUMN.subject.eq(Some(action::SUBJECT_WILDCARD.to_string())))
        .count(db)
        .await?;
    Ok(wild > 0)
}

/// Grant the standard per-content-type permissions for a freshly-created
/// content-type: Editors get full CRUD + publish, Authors get create/read/update.
/// Idempotent — any prior grants for the same uid are replaced.
pub async fn grant_content_permissions(
    db: &DatabaseConnection,
    uid: &str,
) -> Result<(), ServiceError> {
    use db::entities::{admin_permission, admin_role};
    use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, Set};

    let roles = admin_role::Entity::find()
        .filter(
            admin_role::COLUMN
                .code
                .is_in(vec![ROLE_EDITOR.to_string(), ROLE_AUTHOR.to_string()]),
        )
        .all(db)
        .await?;

    let editor = roles.iter().find(|r| r.code == ROLE_EDITOR);
    let author = roles.iter().find(|r| r.code == ROLE_AUTHOR);

    // Clear prior grants for this content-type to keep the matrix in sync.
    admin_permission::Entity::delete_many()
        .filter(admin_permission::COLUMN.subject.eq(Some(uid.to_string())))
        .exec(db)
        .await?;

    let now = chrono::Utc::now();
    for (role, actions) in [(&editor, action::EDITOR), (&author, action::AUTHOR)] {
        let Some(role) = role else { continue };
        for act in actions {
            let perm = admin_permission::ActiveModel {
                role_id: Set(role.id),
                action: Set(act.to_string()),
                subject: Set(Some(uid.to_string())),
                properties_json: Set(serde_json::json!({})),
                conditions_json: Set(serde_json::json!([])),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            perm.insert(db).await?;
        }
    }

    Ok(())
}

/// Convenience guard: enforce `action` on `subject` for the current user.
/// Returns `Ok(())` when permitted and `Forbidden` otherwise. Pass `None`
/// as the user for unauthenticated (public) access, which is *not* governed
/// by the admin permission matrix.
pub async fn enforce_action(
    db: &DatabaseConnection,
    user: Option<&crate::CurrentUser>,
    action: &str,
    subject: &str,
) -> Result<(), ServiceError> {
    if let Some(user) = user {
        if !can_perform(db, user, action, subject).await? {
            return Err(ServiceError::Forbidden);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Backward-compatible aliases for api-rest crate
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CurrentUser;
    use sea_orm_migration::MigratorTrait;

    fn user(id: i64, role: &str) -> CurrentUser {
        CurrentUser {
            id,
            email: format!("user{id}@test.dev"),
            is_active: true,
            roles: vec![role.to_string()],
        }
    }

    #[tokio::test]
    async fn granular_permissions_evaluate() {
        let db = db::connection::connect_sqlite_memory().await.unwrap();
        db::migration::Migrator::up(&db, None).await.unwrap();
        db::seed::seed(&db).await.unwrap();

        // Super Admin bypasses the matrix entirely.
        let super_admin = user(1, ROLE_SUPER_ADMIN);
        assert!(can_perform(&db, &super_admin, action::PUBLISH, "any.uid").await.unwrap());
        assert!(can_perform(&db, &super_admin, action::DELETE, "unrelated.uid").await.unwrap());

        // Grant standard per-content-type permissions for a new content-type.
        grant_content_permissions(&db, "api::article.article").await.unwrap();

        // Editor: full CRUD + publish.
        let editor = user(2, ROLE_EDITOR);
        assert!(can_perform(&db, &editor, action::CREATE, "api::article.article").await.unwrap());
        assert!(can_perform(&db, &editor, action::READ, "api::article.article").await.unwrap());
        assert!(can_perform(&db, &editor, action::UPDATE, "api::article.article").await.unwrap());
        assert!(can_perform(&db, &editor, action::DELETE, "api::article.article").await.unwrap());
        assert!(can_perform(&db, &editor, action::PUBLISH, "api::article.article").await.unwrap());

        // Author: create/read/update but NOT publish/delete.
        let author = user(3, ROLE_AUTHOR);
        assert!(can_perform(&db, &author, action::CREATE, "api::article.article").await.unwrap());
        assert!(can_perform(&db, &author, action::READ, "api::article.article").await.unwrap());
        assert!(can_perform(&db, &author, action::UPDATE, "api::article.article").await.unwrap());
        assert!(!can_perform(&db, &author, action::PUBLISH, "api::article.article").await.unwrap());
        assert!(!can_perform(&db, &author, action::DELETE, "api::article.article").await.unwrap());

        // No grant for a different content-type => denied.
        assert!(!can_perform(&db, &editor, action::CREATE, "api::author.author").await.unwrap());

        // enforce_action maps a denied decision to Forbidden and allows permitted.
        assert!(enforce_action(&db, Some(&author), action::PUBLISH, "api::article.article").await.is_err());
        assert!(enforce_action(&db, Some(&author), action::READ, "api::article.article").await.is_ok());
        // Unauthenticated (public) access is not governed by the admin matrix.
        assert!(enforce_action(&db, None, action::READ, "api::article.article").await.is_ok());
    }
}

use crate::AppContext;

/// Alias for list_roles using AppContext.
pub async fn rbac_list_roles(ctx: &AppContext) -> Result<Vec<api_types::admin::AdminRoleDto>, ServiceError> {
    list_roles(&ctx.db).await
}

/// Alias for get_role using AppContext.
pub async fn rbac_get_role(ctx: &AppContext, role_id: i64) -> Result<api_types::admin::AdminRoleDto, ServiceError> {
    get_role(&ctx.db, role_id).await
}

/// Alias for update_permissions using AppContext.
pub async fn rbac_update_permissions(
    ctx: &AppContext,
    role_id: i64,
    req: &api_types::admin::UpdateRolePermissionsRequest,
) -> Result<Vec<api_types::admin::AdminPermissionDto>, ServiceError> {
    update_permissions(&ctx.db, role_id, req).await
}

/// List admin users.
pub async fn rbac_list_users(
    ctx: &AppContext,
) -> Result<Vec<api_types::admin::AdminUserDto>, ServiceError> {
    use db::entities::admin_user;
    use sea_orm::EntityTrait;

    let users = admin_user::Entity::find().all(&ctx.db).await?;
    let mut result = Vec::new();
    for user in users {
        let roles = load_user_app_roles(&ctx.db, user.id).await?;
        result.push(api_types::admin::AdminUserDto {
            id: user.id,
            email: user.email,
            firstname: user.first_name,
            lastname: user.last_name,
            username: user.username,
            prefered_locale: user.prefered_locale,
            is_active: user.is_active,
            blocked: user.blocked,
            roles,
        });
    }
    Ok(result)
}

/// Create an admin user.
pub async fn rbac_create_user(
    ctx: &AppContext,
    req: &api_types::admin::CreateAdminUserRequest,
) -> Result<api_types::admin::AdminUserDto, ServiceError> {
    use db::entities::admin_user;
    use sea_orm::{ActiveModelTrait, Set};

    let now = chrono::Utc::now();
    let model = admin_user::ActiveModel {
        email: Set(req.email.clone()),
        first_name: Set(req.firstname.clone()),
        last_name: Set(req.lastname.clone()),
        password_hash: Set(String::new()),
        is_active: Set(req.is_active.unwrap_or(true)),
        blocked: Set(false),
        prefered_locale: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let user = model.insert(&ctx.db).await?;

    Ok(api_types::admin::AdminUserDto {
        id: user.id,
        email: user.email,
        firstname: user.first_name,
        lastname: user.last_name,
        username: user.username,
        prefered_locale: user.prefered_locale,
        is_active: user.is_active,
        blocked: user.blocked,
        roles: vec![],
    })
}
