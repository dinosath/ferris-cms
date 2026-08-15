//! Schema diff → DDL (design Part IV §4-§8).
//!
//! Emission order within one call: CREATE TABLE (new) → ADD COLUMN →
//! join/link tables → compatible ALTERs → incompatible changes (detach+add,
//! data retained) → removals (unmap, never hard-drop). `services` runs all of
//! a batch's diffs inside ONE transaction.

use crate::error::StoreError;
use core_domain::{
    column_name, component_link_table, fk_column, media_link_table, relation_join_table,
    ContentTypeKind, FieldType, RelationKind,
};
use core_schema::{Attribute, DiffKind, Schema, SchemaDiff};
use sea_orm::{ConnectionTrait, DbBackend};
use sea_query::{
    Alias, ColumnDef, Index, IndexCreateStatement, MysqlQueryBuilder, PostgresQueryBuilder,
    SchemaStatementBuilder, SqliteQueryBuilder, Table,
};

/// Human/audit-log friendly description of what was applied.
pub type DdlActions = Vec<String>;

/// Apply one schema diff. `all_schemas` is the full *desired* registry
/// (needed to resolve relation targets for cross-table FK columns).
pub async fn apply_schema_diff<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    diff: &SchemaDiff,
    all_schemas: &[Schema],
) -> Result<DdlActions, StoreError> {
    let mut actions = Vec::new();
    match diff.kind {
        DiffKind::Created => {
            let schema = diff.desired.as_ref().expect("created diff has desired");
            create_host_table(db, backend, schema, all_schemas, &mut actions).await?;
        }
        DiffKind::Updated => {
            let schema = diff.desired.as_ref().expect("updated diff has desired");
            apply_update(db, backend, diff, schema, all_schemas, &mut actions).await?;
        }
        DiffKind::Removed => {
            // Default: unmap, don't hard-drop (Part IV §8).
            actions.push(format!("unmapped table {} (retained)", diff.table));
        }
        DiffKind::Unchanged => {}
    }
    Ok(actions)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Build the SQL string of a schema statement for the backend and execute it.
async fn exec_schema<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    stmt: &impl SchemaStatementBuilder,
) -> Result<(), StoreError> {
    let sql = match backend {
        DbBackend::Sqlite => stmt.build(SqliteQueryBuilder),
        DbBackend::Postgres => stmt.build(PostgresQueryBuilder),
        DbBackend::MySql => stmt.build(MysqlQueryBuilder),
        other => {
            return Err(StoreError::Unsupported(format!(
                "backend {other:?} not supported for DDL"
            )))
        }
    };
    db.execute_unprepared(&sql).await?;
    Ok(())
}

fn col_def(name: &str, attr: &Attribute, nullable: bool) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    match attr.attr_type {
        FieldType::String
        | FieldType::Email
        | FieldType::Password
        | FieldType::Uid
        | FieldType::Enumeration => {
            c.string_len(255);
        }
        FieldType::Text | FieldType::Richtext => {
            c.text();
        }
        FieldType::Blocks | FieldType::Json => {
            c.json();
        }
        FieldType::Integer => {
            c.integer();
        }
        FieldType::Biginteger => {
            c.big_integer();
        }
        FieldType::Decimal => {
            c.decimal();
        }
        FieldType::Float => {
            c.double();
        }
        FieldType::Date => {
            c.date();
        }
        FieldType::Datetime => {
            c.date_time();
        }
        FieldType::Time => {
            c.time();
        }
        FieldType::Boolean => {
            c.boolean();
        }
        FieldType::Media | FieldType::Relation | FieldType::Component | FieldType::Dynamiczone => {
            unreachable!("non-scalar attribute has no column")
        }
    }
    if !nullable {
        c.not_null();
    }
    c
}

/// Scalar attribute → physical column list, in schema order.
pub fn scalar_columns(schema: &Schema) -> Vec<(String, String, Attribute)> {
    schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type.is_scalar_column())
        .map(|(n, a)| (n.clone(), column_name(n), a.clone()))
        .collect()
}

/// Relations whose storage is an FK on this schema's own table.
pub fn owner_fk_columns<'a>(
    schema: &'a Schema,
    all: &'a [Schema],
) -> Vec<(String, String, Attribute, &'a Schema)> {
    let mut out = Vec::new();
    for (name, attr) in &schema.attributes {
        if attr.attr_type != FieldType::Relation {
            continue;
        }
        let Some(kind) = attr.relation else { continue };
        if !kind.owner_has_fk() {
            continue;
        }
        let Some(target_uid) = &attr.target else { continue };
        let Some(target) = all.iter().find(|s| &s.uid == target_uid) else {
            continue;
        };
        out.push((name.clone(), fk_column(name), attr.clone(), target));
    }
    out
}

/// oneToMany attrs: the FK column lives on the *target* table.
pub fn inverse_fk_attrs<'a>(
    schema: &'a Schema,
    all: &'a [Schema],
) -> Vec<(String, Attribute, &'a Schema)> {
    let mut out = Vec::new();
    for (name, attr) in &schema.attributes {
        if attr.attr_type != FieldType::Relation || attr.relation != Some(RelationKind::OneToMany)
        {
            continue;
        }
        let Some(target_uid) = &attr.target else { continue };
        let Some(target) = all.iter().find(|s| &s.uid == target_uid) else {
            continue;
        };
        out.push((name.clone(), attr.clone(), target));
    }
    out
}

/// m2m / many-way attrs → join tables.
pub fn join_table_attrs<'a>(
    schema: &'a Schema,
    all: &'a [Schema],
) -> Vec<(String, Attribute, &'a Schema)> {
    let mut out = Vec::new();
    for (name, attr) in &schema.attributes {
        if attr.attr_type != FieldType::Relation {
            continue;
        }
        let Some(kind) = attr.relation else { continue };
        if !kind.uses_join_table() {
            continue;
        }
        let Some(target_uid) = &attr.target else { continue };
        let Some(target) = all.iter().find(|s| &s.uid == target_uid) else {
            continue;
        };
        out.push((name.clone(), attr.clone(), target));
    }
    out
}

pub fn media_attrs(schema: &Schema) -> Vec<(String, Attribute)> {
    schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type == FieldType::Media)
        .map(|(n, a)| (n.clone(), a.clone()))
        .collect()
}

pub fn component_attrs(schema: &Schema) -> Vec<(String, Attribute)> {
    schema
        .attributes
        .iter()
        .filter(|(_, a)| {
            matches!(a.attr_type, FieldType::Component | FieldType::Dynamiczone)
        })
        .map(|(n, a)| (n.clone(), a.clone()))
        .collect()
}

fn pk() -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new("id"));
    c.big_integer().not_null().auto_increment().primary_key();
    c
}

fn bigint_null(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.big_integer();
    c
}

fn ts(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    // The dynamic store reads/writes all values (including timestamps) as
    // JSON/text so they behave uniformly on SQLite and Postgres. A `date_time`
    // column on Postgres would reject the string values the DML writes.
    c.string();
    c
}

fn ts_not_null(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.string().not_null();
    c
}

fn str_not_null(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.string().not_null();
    c
}

fn str_null(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(Alias::new(name));
    c.string();
    c
}

// ---------------------------------------------------------------------------
// CREATE
// ---------------------------------------------------------------------------

async fn create_host_table<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    schema: &Schema,
    all: &[Schema],
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let table = schema.table_name();
    let is_component = schema.kind == ContentTypeKind::Component;

    let mut t = Table::create();
    t.table(Alias::new(&table)).if_not_exists();
    t.col(pk());

    if !is_component {
        t.col(str_not_null("document_id"));
        t.col(str_null("locale"));
        {
            let mut c = ColumnDef::new(Alias::new("publication_state"));
            c.string().not_null().default("draft");
            t.col(&mut c);
        }
    }

    for (_, col, attr) in scalar_columns(schema) {
        // Required on create only; added columns later are nullable and the
        // service enforces required-ness at write time.
        t.col(col_def(&col, &attr, !attr.required));
    }
    for (_, col, attr, _) in owner_fk_columns(schema, all) {
        let mut c = bigint_null(&col);
        if attr.relation == Some(RelationKind::OneToOne) {
            c.unique_key();
        }
        t.col(c);
    }

    if !is_component {
        t.col(ts_not_null("created_at"));
        t.col(ts_not_null("updated_at"));
        t.col(ts("published_at"));
        t.col(bigint_null("created_by_id"));
        t.col(bigint_null("updated_by_id"));
        // sync columns from day one (Part III note)
        {
            let mut c = ColumnDef::new(Alias::new("sync_version"));
            c.big_integer().not_null().default(1_i64);
            t.col(&mut c);
        }
        t.col(str_null("origin_node_id"));
        t.col(ts("deleted_at"));
    }

    exec_schema(db, backend, &t).await?;
    actions.push(format!("created table {table}"));

    if !is_component {
        // unique variant identity: (document_id, locale, publication_state)
        create_index_stmt(
            db,
            backend,
            &table,
            &["document_id", "locale", "publication_state"],
            true,
            actions,
        )
        .await?;
        create_index_stmt(db, backend, &table, &["document_id"], false, actions).await?;
    }

    // unique attribute indexes (per-locale when i18n)
    for (_, col, attr) in scalar_columns(schema) {
        if attr.unique || attr.attr_type == FieldType::Uid {
            let localized = schema.is_localized() && attr.is_localized();
            let cols: Vec<&str> = if localized {
                vec![col.as_str(), "locale"]
            } else {
                vec![col.as_str()]
            };
            create_index_stmt(db, backend, &table, &cols, true, actions).await?;
        }
    }

    // Auxiliary tables (relation join tables, media/component link tables) and
    // inverse FK columns are created separately by `apply_aux`, which must run
    // only after every host table in the batch exists. See the two-phase call
    // in `ctb_apply`.

    Ok(())
}

/// Create the auxiliary tables and inverse FK columns for one schema: relation
/// join tables, media link tables, component/dynamic-zone link tables, and
/// one-to-many FK columns on the *target* tables.
///
/// Must be called after all host tables in the batch have been created, so the
/// referenced target tables already exist. All aux tables use `if_not_exists`,
/// so calling this again for an already-created schema is safe (idempotent).
pub async fn apply_aux<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    schema: &Schema,
    all: &[Schema],
) -> Result<DdlActions, StoreError> {
    let mut actions = Vec::new();
    let table = schema.table_name();

    for (name, attr, target) in join_table_attrs(schema, all) {
        create_join_table(db, backend, &table, &name, schema, target, &attr, &mut actions).await?;
    }
    for (name, _) in media_attrs(schema) {
        create_media_link_table(db, backend, &table, &name, &mut actions).await?;
    }
    if !component_attrs(schema).is_empty() {
        create_component_link_table(db, backend, &table, &mut actions).await?;
    }

    // oneToMany: FK column on the *target* table
    for (name, attr, target) in inverse_fk_attrs(schema, all) {
        let mapped_by = attr.mapped_by.clone().unwrap_or_else(|| name.clone());
        let fk_col = fk_column(&mapped_by);
        // If the target already declares an owner FK column with this name (a
        // manyToOne pairing), that column is created on the target's own table
        // and must not be added again here ("duplicate column name").
        let target_owns = owner_fk_columns(target, all).iter().any(|(_, c, _, _)| *c == fk_col);
        if target_owns {
            continue;
        }
        let target_table = target.table_name();
        add_column_if_supported(db, backend, &target_table, bigint_null(&fk_col), &mut actions)
            .await?;
    }

    Ok(actions)
}

async fn create_join_table<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    owner_table: &str,
    attr_name: &str,
    owner: &Schema,
    target: &Schema,
    _attr: &Attribute,
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let join = relation_join_table(owner_table, attr_name);
    let owner_col = format!("{}_id", owner.info.singular_name.replace('-', "_"));
    let target_col = format!("{}_id", target.info.singular_name.replace('-', "_"));
    let order_col = format!("{}_order", column_name(attr_name));

    let mut t = Table::create();
    t.table(Alias::new(&join)).if_not_exists();
    t.col(pk());
    t.col({
        let mut c = ColumnDef::new(Alias::new(&owner_col));
        c.big_integer().not_null();
        c
    });
    t.col({
        let mut c = ColumnDef::new(Alias::new(&target_col));
        c.big_integer().not_null();
        c
    });
    {
        let mut c = ColumnDef::new(Alias::new(&order_col));
        c.double();
        t.col(&mut c);
    }
    exec_schema(db, backend, &t).await?;
    actions.push(format!("created join table {join}"));
    create_index_stmt(db, backend, &join, &[owner_col.as_str()], false, actions).await?;
    create_index_stmt(db, backend, &join, &[target_col.as_str()], false, actions).await?;
    Ok(())
}

async fn create_media_link_table<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    host_table: &str,
    attr_name: &str,
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let link = media_link_table(host_table, attr_name);
    let mut t = Table::create();
    t.table(Alias::new(&link)).if_not_exists();
    t.col(pk());
    t.col({
        let mut c = ColumnDef::new(Alias::new("entry_id"));
        c.big_integer().not_null();
        c
    });
    t.col({
        let mut c = ColumnDef::new(Alias::new("file_id"));
        c.big_integer().not_null();
        c
    });
    {
        let mut c = ColumnDef::new(Alias::new("order"));
        c.double();
        t.col(&mut c);
    }
    exec_schema(db, backend, &t).await?;
    actions.push(format!("created media link table {link}"));
    create_index_stmt(db, backend, &link, &["entry_id"], false, actions).await?;
    Ok(())
}

async fn create_component_link_table<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    host_table: &str,
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let link = component_link_table(host_table);
    let mut t = Table::create();
    t.table(Alias::new(&link)).if_not_exists();
    t.col(pk());
    t.col({
        let mut c = ColumnDef::new(Alias::new("entry_id"));
        c.big_integer().not_null();
        c
    });
    t.col(str_not_null("component_uid"));
    t.col({
        let mut c = ColumnDef::new(Alias::new("component_id"));
        c.big_integer().not_null();
        c
    });
    t.col(str_not_null("field"));
    {
        let mut c = ColumnDef::new(Alias::new("order"));
        c.double();
        t.col(&mut c);
    }
    exec_schema(db, backend, &t).await?;
    actions.push(format!("created component link table {link}"));
    create_index_stmt(db, backend, &link, &["entry_id", "field"], false, actions).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// UPDATE
// ---------------------------------------------------------------------------

async fn apply_update<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    diff: &SchemaDiff,
    schema: &Schema,
    all: &[Schema],
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let table = &diff.table;

    // added attributes
    for (name, attr) in &diff.added_attrs {
        add_attribute_storage(db, backend, table, schema, name, attr, all, actions).await?;
    }

    // changed attributes
    for change in &diff.changed_attrs {
        if change.compatible {
            if backend == DbBackend::Postgres
                && change.from.sql_family() != change.to.sql_family()
            {
                // same family per diff(); only reached if families equal, so nothing
            }
            // Compatible changes (flags, constraints) need no DDL on SQLite.
            // Postgres type widening within a family is left as-is (safe).
            if change.to.unique && !change.from.unique {
                let col = column_name(&change.name);
                let localized = schema.is_localized() && change.to.is_localized();
                let cols: Vec<&str> = if localized {
                    vec![col.as_str(), "locale"]
                } else {
                    vec![col.as_str()]
                };
                create_index_stmt(db, backend, table, &cols, true, actions).await?;
            }
        } else {
            apply_incompatible_change(db, backend, table, schema, change, all, actions).await?;
        }
    }

    // removed attributes: unmap only (Part IV §8 default)
    for name in &diff.removed_attrs {
        actions.push(format!("unmapped column {}.{} (retained)", table, column_name(name)));
    }

    Ok(())
}

async fn add_attribute_storage<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    table: &str,
    _schema: &Schema,
    name: &str,
    attr: &Attribute,
    _all: &[Schema],
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    if attr.attr_type.is_scalar_column() {
        let col = column_name(name);
        // Added columns are always nullable; service enforces `required`.
        add_column_if_supported(db, backend, table, col_def(&col, attr, true), actions).await?;
        if attr.unique || attr.attr_type == FieldType::Uid {
            create_index_stmt(db, backend, table, &[col.as_str()], true, actions).await?;
        }
        return Ok(());
    }
    match attr.attr_type {
        FieldType::Relation => {
            let kind = attr.relation.unwrap_or(RelationKind::OneWay);
            if kind.owner_has_fk() {
                add_column_if_supported(db, backend, table, bigint_null(&fk_column(name)), actions)
                    .await?;
            }
            // Join tables (many-to-many) and one-to-many inverse FK columns are
            // created by `apply_aux` after every host table in the batch exists
            // (two-phase DDL), so they are not created inline here.
        }
        FieldType::Media | FieldType::Component | FieldType::Dynamiczone => {
            // Media / component link tables are created by `apply_aux`.
        }
        _ => unreachable!("scalars handled above"),
    }
    Ok(())
}

/// Incompatible change: detach the old column (rename, data retained) and add
/// the new one (Part IV §8).
async fn apply_incompatible_change<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    table: &str,
    schema: &Schema,
    change: &core_schema::AttrChange,
    all: &[Schema],
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let old_col = column_name(&change.name);
    let both_scalar =
        change.from.attr_type.is_scalar_column() && change.to.attr_type.is_scalar_column();

    if both_scalar {
        let detached = format!("{old_col}__detached");
        let stmt = Table::alter()
            .table(Alias::new(table))
            .rename_column(Alias::new(&old_col), Alias::new(&detached))
            .to_owned();
        exec_schema(db, backend, &stmt).await?;
        actions.push(format!(
            "detached {table}.{old_col} -> {detached} (type change {:?} -> {:?})",
            change.from.attr_type, change.to.attr_type
        ));
        add_column_if_supported(
            db,
            backend,
            table,
            col_def(&old_col, &change.to, true),
            actions,
        )
        .await?;
    } else {
        // Storage-mechanism change (scalar<->relation etc.): detach old
        // storage where applicable, then add the new mechanism.
        if change.from.attr_type.is_scalar_column() {
            let detached = format!("{old_col}__detached");
            let stmt = Table::alter()
                .table(Alias::new(table))
                .rename_column(Alias::new(&old_col), Alias::new(&detached))
                .to_owned();
            exec_schema(db, backend, &stmt).await?;
            actions.push(format!("detached {table}.{old_col} -> {detached}"));
        } else {
            actions.push(format!(
                "unmapped old storage of {}.{change} (retained)",
                table,
                change = change.name
            ));
        }
        add_attribute_storage(db, backend, table, schema, &change.name, &change.to, all, actions)
            .await?;
    }
    Ok(())
}

async fn add_column_if_supported<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    table: &str,
    col: ColumnDef,
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let name = col.get_column_name();
    let stmt = Table::alter()
        .table(Alias::new(table))
        .add_column_if_not_exists(col)
        .to_owned();
    exec_schema(db, backend, &stmt).await?;
    actions.push(format!("added column {table}.{name}"));
    Ok(())
}

async fn create_index_stmt<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    table: &str,
    cols: &[&str],
    unique: bool,
    actions: &mut DdlActions,
) -> Result<(), StoreError> {
    let mut idx = Index::create();
    idx.table(Alias::new(table))
        .if_not_exists()
        .name(format!("{}_{}_{}", if unique { "uidx" } else { "idx" }, table, cols.join("_")));
    if unique {
        idx.unique();
    }
    for c in cols {
        idx.col(Alias::new(*c));
    }
    let stmt: IndexCreateStatement = idx.to_owned();
    exec_schema(db, backend, &stmt).await?;
    actions.push(format!(
        "created {} index on {}({})",
        if unique { "unique" } else { "plain" },
        table,
        cols.join(",")
    ));
    Ok(())
}


