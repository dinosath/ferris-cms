//! System table migrations (design Part III).
//!
//! Fixed system tables migrate via `sea-orm-migration`; user content-type
//! tables are runtime DDL handled by `dynamic-store`. Written with SeaQuery
//! DDL so the same statements run on SQLite and Postgres.

use sea_orm::DbErr;
use sea_orm_migration::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Migrator;

/// Whether the current migration run targets Postgres. SeaORM maps the entity
/// `DateTimeUtc` fields to `TIMESTAMPTZ` on Postgres but `TIMESTAMP` on SQLite,
/// so the DDL must match per backend. Set at the start of each migration.
static IS_POSTGRES: AtomicBool = AtomicBool::new(false);

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(M20260731Init), Box::new(M20260731Rbac)]
    }
}

fn id_bigint(table: TableCreateStatement) -> TableCreateStatement {
    // PK handled inline per-table; this helper only exists for readability.
    table
}

fn pk_col() -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new("id"));
    c.big_integer().not_null().auto_increment().primary_key();
    c
}

fn str_col(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.string().not_null();
    c
}

fn str_opt(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.string();
    c
}

fn bool_col(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.boolean().not_null();
    c
}

fn ts_col(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    if IS_POSTGRES.load(Ordering::Relaxed) {
        c.timestamp_with_time_zone().not_null();
    } else {
        c.date_time().not_null();
    }
    c
}

fn ts_opt(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    if IS_POSTGRES.load(Ordering::Relaxed) {
        c.timestamp_with_time_zone();
    } else {
        c.date_time();
    }
    c
}

fn json_col(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.json().not_null();
    c
}

fn int_col(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.big_integer().not_null();
    c
}

fn int_opt(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.big_integer();
    c
}

/// Columns every syncable table carries from day one (Part III note).
fn sync_cols(t: &mut TableCreateStatement) {
    t.col(int_col("sync_version"));
    t.col(str_opt("origin_node_id"));
    t.col(ts_opt("deleted_at"));
}

fn timestamps(t: &mut TableCreateStatement) {
    t.col(ts_col("created_at"));
    t.col(ts_col("updated_at"));
}

struct M20260731Init;

impl MigrationName for M20260731Init {
    fn name(&self) -> &str {
        "m20260731_000001_init_system_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for M20260731Init {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        IS_POSTGRES.store(
            matches!(
                manager.get_database_backend(),
                sea_orm::DatabaseBackend::Postgres
            ),
            Ordering::Relaxed,
        );
        // ---- content_type_schemas ----
        let mut t = Table::create()
            .table(Alias::new("content_type_schemas"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("uid"));
        t.col(str_col("kind"));
        t.col(str_opt("category"));
        t.col(str_col("display_name"));
        t.col(str_opt("singular_api_id"));
        t.col(str_opt("plural_api_id"));
        t.col(json_col("schema_json"));
        t.col(bool_col("draft_and_publish"));
        t.col(bool_col("i18n_localized"));
        t.col(bool_col("is_system"));
        t.col(int_col("version"));
        timestamps(&mut t);
        sync_cols(&mut t);
        manager.create_table(t).await?;
        create_unique_index(manager, "content_type_schemas", &["uid"]).await?;

        // ---- content_type_table_map ----
        let mut t = Table::create()
            .table(Alias::new("content_type_table_map"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("schema_uid"));
        t.col(str_col("physical_table"));
        t.col(str_opt("logical_attr"));
        t.col(str_opt("physical_column"));
        t.col(str_opt("join_table"));
        manager.create_table(t).await?;
        create_index(manager, "content_type_table_map", &["schema_uid"]).await?;

        // ---- admin_user ----
        let mut t = Table::create()
            .table(Alias::new("admin_user"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("email"));
        t.col(str_opt("first_name"));
        t.col(str_opt("last_name"));
        t.col(str_opt("username"));
        t.col(str_col("password_hash"));
        t.col(bool_col("is_active"));
        t.col(bool_col("blocked"));
        t.col(str_opt("prefered_locale"));
        timestamps(&mut t);
        manager.create_table(t).await?;
        create_unique_index(manager, "admin_user", &["email"]).await?;

        // ---- admin_role ----
        let mut t = Table::create()
            .table(Alias::new("admin_role"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("name"));
        t.col(str_col("code"));
        t.col(str_opt("description"));
        timestamps(&mut t);
        manager.create_table(t).await?;
        create_unique_index(manager, "admin_role", &["name"]).await?;
        create_unique_index(manager, "admin_role", &["code"]).await?;

        // ---- admin_permission ----
        let mut t = Table::create()
            .table(Alias::new("admin_permission"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(int_col("role_id"));
        t.col(str_col("action"));
        t.col(str_opt("subject"));
        t.col(json_col("properties_json"));
        t.col(json_col("conditions_json"));
        timestamps(&mut t);
        manager.create_table(t).await?;
        create_index(manager, "admin_permission", &["role_id"]).await?;

        // ---- admin_user_role ----
        let mut t = Table::create()
            .table(Alias::new("admin_user_role"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(int_col("user_id"));
        t.col(int_col("role_id"));
        manager.create_table(t).await?;
        create_unique_index(manager, "admin_user_role", &["user_id", "role_id"]).await?;

        // ---- api_token ----
        let mut t = Table::create()
            .table(Alias::new("api_token"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("name"));
        t.col(str_opt("description"));
        t.col(str_col("type"));
        t.col(str_col("access_key_hash"));
        t.col(ts_opt("last_used_at"));
        t.col(ts_opt("expires_at"));
        t.col(int_opt("lifespan"));
        timestamps(&mut t);
        manager.create_table(t).await?;

        // ---- api_token_permission ----
        let mut t = Table::create()
            .table(Alias::new("api_token_permission"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(int_col("token_id"));
        t.col(str_col("action"));
        manager.create_table(t).await?;
        create_index(manager, "api_token_permission", &["token_id"]).await?;

        // ---- upload_file ----
        let mut t = Table::create()
            .table(Alias::new("upload_file"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("document_id"));
        t.col(str_col("name"));
        t.col(str_opt("alternative_text"));
        t.col(str_opt("caption"));
        t.col(int_opt("width"));
        t.col(int_opt("height"));
        {
            let mut c = ColumnDef::new(Alias::new("formats_json"));
            c.json();
            t.col(&mut c);
        }
        t.col(str_col("hash"));
        t.col(str_opt("ext"));
        t.col(str_col("mime"));
        {
            let mut c = ColumnDef::new(Alias::new("size"));
            c.double().not_null();
            t.col(&mut c);
        }
        t.col(str_col("url"));
        t.col(str_opt("preview_url"));
        t.col(str_col("provider"));
        t.col(int_opt("folder_id"));
        timestamps(&mut t);
        sync_cols(&mut t);
        manager.create_table(t).await?;
        create_unique_index(manager, "upload_file", &["document_id"]).await?;
        create_index(manager, "upload_file", &["folder_id"]).await?;

        // ---- upload_folder ----
        let mut t = Table::create()
            .table(Alias::new("upload_folder"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("name"));
        t.col(int_col("path_id"));
        t.col(str_col("path"));
        t.col(int_opt("parent_id"));
        timestamps(&mut t);
        sync_cols(&mut t);
        manager.create_table(t).await?;
        create_unique_index(manager, "upload_folder", &["path_id"]).await?;

        // ---- i18n_locale ----
        let mut t = Table::create()
            .table(Alias::new("i18n_locale"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("code"));
        t.col(str_col("name"));
        t.col(bool_col("is_default"));
        timestamps(&mut t);
        manager.create_table(t).await?;
        create_unique_index(manager, "i18n_locale", &["code"]).await?;

        // ---- core_store ----
        let mut t = Table::create()
            .table(Alias::new("core_store"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("key"));
        {
            let mut c = ColumnDef::new(Alias::new("value_json"));
            c.json();
            t.col(&mut c);
        }
        t.col(str_opt("type"));
        t.col(str_opt("environment"));
        t.col(str_opt("tag"));
        manager.create_table(t).await?;
        create_unique_index(manager, "core_store", &["key"]).await?;

        // ---- schema_change_log ----
        let mut t = Table::create()
            .table(Alias::new("schema_change_log"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("schema_uid"));
        t.col(int_col("from_version"));
        t.col(int_col("to_version"));
        t.col(json_col("diff_json"));
        t.col(ts_col("applied_at"));
        t.col(str_opt("applied_by"));
        manager.create_table(t).await?;
        create_index(manager, "schema_change_log", &["schema_uid"]).await?;

        // ---- sync_state ----
        let mut t = Table::create()
            .table(Alias::new("sync_state"))
            .if_not_exists()
            .to_owned();
        {
            let mut c = ColumnDef::new(Alias::new("node_id"));
            c.string().not_null().primary_key();
            t.col(&mut c);
        }
        t.col(str_opt("remote_url"));
        t.col(int_col("last_pulled_version"));
        t.col(int_col("last_pushed_version"));
        t.col(ts_opt("last_synced_at"));
        manager.create_table(t).await?;

        // ---- sync_oplog ----
        let mut t = Table::create()
            .table(Alias::new("sync_oplog"))
            .if_not_exists()
            .to_owned();
        t.col(pk_col());
        t.col(str_col("entity"));
        t.col(str_col("document_id"));
        t.col(str_col("op"));
        t.col(int_col("sync_version"));
        {
            let mut c = ColumnDef::new(Alias::new("payload_json"));
            c.json();
            t.col(&mut c);
        }
        t.col(ts_col("created_at"));
        t.col(bool_col("pushed"));
        manager.create_table(t).await?;
        create_index(manager, "sync_oplog", &["pushed"]).await?;

        let _ = id_bigint;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "sync_oplog",
            "sync_state",
            "schema_change_log",
            "core_store",
            "i18n_locale",
            "upload_folder",
            "upload_file",
            "api_token_permission",
            "api_token",
            "admin_user_role",
            "admin_permission",
            "admin_role",
            "admin_user",
            "content_type_table_map",
            "content_type_schemas",
        ] {
            manager
                .drop_table(
                    Table::drop()
                        .table(Alias::new(table))
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

/// SeaORM 2.0 RBAC tables.
/// Creates the standard RBAC schema using SeaORM's built-in schema module.
struct M20260731Rbac;

impl MigrationName for M20260731Rbac {
    fn name(&self) -> &str {
        "m20260731_000002_init_rbac_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for M20260731Rbac {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        sea_orm::rbac::schema::create_tables(db, Default::default()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in sea_orm::rbac::schema::all_tables() {
            manager
                .drop_table(
                    Table::drop()
                        .table(Alias::new(table))
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

async fn create_index(
    manager: &SchemaManager<'_>,
    table: &str,
    cols: &[&str],
) -> Result<(), DbErr> {
    let mut idx = Index::create();
    idx.table(Alias::new(table)).if_not_exists();
    idx.name(format!("idx_{table}_{}", cols.join("_")));
    for c in cols {
        idx.col(Alias::new(*c));
    }
    manager.create_index(idx.to_owned()).await
}

async fn create_unique_index(
    manager: &SchemaManager<'_>,
    table: &str,
    cols: &[&str],
) -> Result<(), DbErr> {
    let mut idx = Index::create();
    idx.table(Alias::new(table)).if_not_exists().unique();
    idx.name(format!("uidx_{table}_{}", cols.join("_")));
    for c in cols {
        idx.col(Alias::new(*c));
    }
    manager.create_index(idx.to_owned()).await
}
