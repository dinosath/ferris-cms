//! CRUD + filter/sort/pagination for dynamic tables (design Part II §4,
//! Part IV §8, Part V §3).

use crate::base_columns as base;
use crate::error::StoreError;
use crate::value::{
    attr_to_value, base_column_family, coerce_filter_value, query_rows as raw_query_rows,
};
use api_types::{Filter, FilterOp, QueryParams, SortField};
use core_domain::{column_name, fk_column, FieldType, PublicationState};
use core_schema::{Schema, SqlFamily};
use sea_orm::{ConnectionTrait, DbBackend};
use sea_query::{
    Alias, Asterisk, Condition, Expr, ExprTrait, Func, Order, Query, SimpleExpr, Value,
};
use serde_json::{Map as JsonMap, Value as JsonValue};

/// Resolved scalar + FK columns of a schema: (attr name, physical column, family).
pub fn column_map(schema: &Schema) -> Vec<(String, String, SqlFamily)> {
    let mut out = Vec::new();
    for (name, attr) in &schema.attributes {
        if attr.attr_type.is_scalar_column() {
            out.push((name.clone(), column_name(name), attr.sql_family()));
        } else if attr.attr_type == FieldType::Relation
            && attr.relation.is_some_and(|k| k.owner_has_fk())
        {
            out.push((name.clone(), fk_column(name), SqlFamily::BigInt));
        }
    }
    out
}

/// Resolve a user-facing field name (filter/sort) to (column, family).
/// Schema attributes win over base-field aliases so an attribute named
/// e.g. `state` doesn't collide with the publication_state alias.
pub fn resolve_field(schema: &Schema, field: &str) -> Result<(String, SqlFamily), StoreError> {
    if let Some(attr) = schema.attributes.get(field) {
        if attr.attr_type.is_scalar_column() {
            return Ok((column_name(field), attr.sql_family()));
        }
        if attr.attr_type == FieldType::Relation
            && attr.relation.is_some_and(|k| k.owner_has_fk())
        {
            return Ok((fk_column(field), SqlFamily::BigInt));
        }
        return Err(StoreError::Unsupported(format!(
            "cannot filter/sort by non-scalar field `{field}` (use relation id or populate)"
        )));
    }
    if let Some(col) = base::resolve_field(field) {
        return Ok((col.to_string(), base_column_family(col)));
    }
    Err(StoreError::Unsupported(format!(
        "unknown field `{field}` on {}",
        schema.uid
    )))
}

// ---------------------------------------------------------------------------
// filters
// ---------------------------------------------------------------------------

pub fn filter_to_condition(schema: &Schema, filter: &Filter) -> Result<Condition, StoreError> {
    let mut cond = Condition::all();
    apply_filter(schema, &mut cond, filter)?;
    Ok(cond)
}

fn apply_filter(schema: &Schema, cond: &mut Condition, filter: &Filter) -> Result<(), StoreError> {
    match filter {
        Filter::And(items) => {
            for item in items {
                let inner = filter_to_condition(schema, item)?;
                *cond = std::mem::replace(cond, Condition::all()).add(inner);
            }
        }
        Filter::Or(items) => {
            let mut any = Condition::any();
            for item in items {
                any = any.add(filter_to_condition(schema, item)?);
            }
            *cond = std::mem::replace(cond, Condition::all()).add(any);
        }
        Filter::Not(inner) => {
            let inner_cond = filter_to_condition(schema, inner)?;
            *cond = std::mem::replace(cond, Condition::all()).add(inner_cond.not());
        }
        Filter::Leaf { field, op, values } => {
            *cond = std::mem::replace(cond, Condition::all()).add(leaf_to_expr(schema, field, *op, values)?);
        }
    }
    Ok(())
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn leaf_to_expr(
    schema: &Schema,
    field: &str,
    op: FilterOp,
    values: &[JsonValue],
) -> Result<SimpleExpr, StoreError> {
    let (col, family) = resolve_field(schema, field)?;
    let col_expr = || Expr::col(Alias::new(col.clone()));
    let stringy = matches!(family, SqlFamily::VarChar | SqlFamily::Text);

    let val = |idx: usize| -> Result<Value, StoreError> {
        let v = values
            .get(idx)
            .ok_or_else(|| StoreError::bad_value(field, format!("operator {op:?} needs a value")))?;
        coerce_filter_value(family, v)
    };
    let pattern = |idx: usize| -> Result<String, StoreError> {
        match values.get(idx) {
            Some(JsonValue::String(s)) => Ok(like_escape(s)),
            Some(JsonValue::Number(n)) => Ok(like_escape(&n.to_string())),
            _ => Err(StoreError::bad_value(
                field,
                format!("operator {op:?} needs a string value"),
            )),
        }
    };

    use FilterOp::*;
    let expr: SimpleExpr = match op {
        Eq => col_expr().eq(val(0)?),
        Eqi => {
            let lowered = pattern(0)?.to_lowercase();
            Expr::expr(Func::lower(col_expr())).eq(lowered)
        }
        Ne => col_expr().ne(val(0)?),
        Lt => col_expr().lt(val(0)?),
        Lte => col_expr().lte(val(0)?),
        Gt => col_expr().gt(val(0)?),
        Gte => col_expr().gte(val(0)?),
        In => col_expr().is_in(collect(values, family, field)?),
        NotIn => col_expr().is_not_in(collect(values, family, field)?),
        Contains => {
            if !stringy {
                return Err(StoreError::bad_value(field, "$contains requires a string field"));
            }
            col_expr().like(format!("%{}%", pattern(0)?))
        }
        NotContains => {
            if !stringy {
                return Err(StoreError::bad_value(field, "$notContains requires a string field"));
            }
            col_expr().not_like(format!("%{}%", pattern(0)?))
        }
        ContainsI => Expr::expr(Func::lower(col_expr())).like(format!("%{}%", pattern(0)?.to_lowercase())),
        NotContainsI => {
            Expr::expr(Func::lower(col_expr())).not_like(format!("%{}%", pattern(0)?.to_lowercase()))
        }
        StartsWith => col_expr().like(format!("{}%", pattern(0)?)),
        StartsWithI => {
            Expr::expr(Func::lower(col_expr())).like(format!("{}%", pattern(0)?.to_lowercase()))
        }
        EndsWith => col_expr().like(format!("%{}", pattern(0)?)),
        EndsWithI => {
            Expr::expr(Func::lower(col_expr())).like(format!("%{}", pattern(0)?.to_lowercase()))
        }
        Null => col_expr().is_null(),
        NotNull => col_expr().is_not_null(),
        Between => col_expr().between(val(0)?, val(1)?),
    };
    Ok(expr)
}

fn collect(values: &[JsonValue], family: SqlFamily, field: &str) -> Result<Vec<Value>, StoreError> {
    if values.is_empty() {
        return Err(StoreError::bad_value(field, "list operator needs values"));
    }
    values
        .iter()
        .map(|v| coerce_filter_value(family, v))
        .collect()
}

// ---------------------------------------------------------------------------
// SELECT
// ---------------------------------------------------------------------------

/// Everything needed to run a content SELECT.
pub struct SelectSpec<'a> {
    pub query: Option<&'a QueryParams>,
    pub locale: Option<&'a str>,
    pub state: Option<PublicationState>,
    pub default_sort: &'a [SortField],
}

/// All columns selected for a content row: base + schema scalars/FKs.
pub fn select_columns(schema: &Schema) -> Vec<(String, SqlFamily)> {
    let mut cols: Vec<(String, SqlFamily)> = [
        base::ID,
        base::DOCUMENT_ID,
        base::LOCALE,
        base::PUBLICATION_STATE,
        base::CREATED_AT,
        base::UPDATED_AT,
        base::PUBLISHED_AT,
        base::CREATED_BY,
        base::UPDATED_BY,
        base::SYNC_VERSION,
        base::ORIGIN_NODE,
        base::DELETED_AT,
    ]
    .into_iter()
    .map(|c| (c.to_string(), base_column_family(c)))
    .collect();
    for (_, col, family) in column_map(schema) {
        cols.push((col, family));
    }
    cols
}

fn base_condition(spec: &SelectSpec, schema: &Schema) -> Condition {
    let mut cond = Condition::all().add(Expr::col(Alias::new(base::DELETED_AT)).is_null());
    if let Some(locale) = spec.locale {
        if schema.is_localized() {
            cond = cond.add(Expr::col(Alias::new(base::LOCALE)).eq(locale));
        }
    }
    if let Some(state) = spec.state {
        cond = cond.add(
            Expr::col(Alias::new(base::PUBLICATION_STATE)).eq(state.as_db_str()),
        );
    }
    cond
}

fn build_where(
    schema: &Schema,
    spec: &SelectSpec,
) -> Result<Condition, StoreError> {
    let mut cond = base_condition(spec, schema);
    if let Some(q) = spec.query {
        if let Some(f) = &q.filters {
            cond = cond.add(filter_to_condition(schema, f)?);
        }
    }
    Ok(cond)
}

fn apply_sort(
    mut sel: sea_query::SelectStatement,
    schema: &Schema,
    spec: &SelectSpec,
) -> Result<sea_query::SelectStatement, StoreError> {
    let mut sorts: &[SortField] = spec.default_sort;
    if let Some(q) = spec.query {
        if !q.sort.is_empty() {
            sorts = &q.sort;
        }
    }
    if sorts.is_empty() {
        return Ok(sel.order_by(Alias::new(base::ID), Order::Asc).to_owned());
    }
    for s in sorts {
        let (col, _) = resolve_field(schema, &s.field)?;
        sel = sel
            .order_by(
                Alias::new(col),
                if s.descending { Order::Desc } else { Order::Asc },
            )
            .to_owned();
    }
    Ok(sel)
}

/// Run a content SELECT; returns (rows, total).
pub async fn select<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    schema: &Schema,
    spec: &SelectSpec<'_>,
) -> Result<(Vec<JsonMap<String, JsonValue>>, i64), StoreError> {
    let table = schema.table_name();
    let where_cond = build_where(schema, spec)?;
    let eff = spec
        .query
        .map(|q| q.effective_pagination())
        .unwrap_or(api_types::EffectivePagination {
            limit: 25,
            offset: 0,
            page: 1,
            page_size: 25,
            with_count: true,
        });

    let mut sel = Query::select();
    sel.from(Alias::new(&table));
    let cols = select_columns(schema);
    for (col, _) in &cols {
        sel.column(Alias::new(col));
    }
    sel.cond_where(where_cond.clone());
    let sel = apply_sort(sel.to_owned(), schema, spec)?;
    let mut sel = sel;
    sel.limit(eff.limit as u64).offset(eff.offset as u64);

    let rows = raw_query_rows(db, &sel, &cols, backend).await?;
    let total = if eff.with_count {
        let mut count_q = Query::select();
        count_q
            .from(Alias::new(&table))
            .expr_as(Expr::col(Asterisk).count(), Alias::new("count"))
            .cond_where(where_cond);
        let row = db
            .query_one(&count_q)
            .await?
            .ok_or_else(|| StoreError::Db(sea_orm::DbErr::RecordNotFound("count".into())))?;
        row.try_get::<i64>("", "count").unwrap_or(0)
    } else {
        -1
    };

    Ok((rows, total))
}

/// Find one row by document_id (+ locale/state variant).
pub async fn find_by_document_id<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    schema: &Schema,
    document_id: &str,
    locale: Option<&str>,
    state: Option<PublicationState>,
) -> Result<Option<JsonMap<String, JsonValue>>, StoreError> {
    let table = schema.table_name();
    let mut cond = Condition::all()
        .add(Expr::col(Alias::new(base::DOCUMENT_ID)).eq(document_id))
        .add(Expr::col(Alias::new(base::DELETED_AT)).is_null());
    if let Some(locale) = locale {
        cond = cond.add(Expr::col(Alias::new(base::LOCALE)).eq(locale));
    }
    if let Some(state) = state {
        cond = cond.add(Expr::col(Alias::new(base::PUBLICATION_STATE)).eq(state.as_db_str()));
    }
    let cols = select_columns(schema);
    let mut sel = Query::select();
    sel.from(Alias::new(&table));
    for (col, _) in &cols {
        sel.column(Alias::new(col));
    }
    sel.cond_where(cond).limit(1);
    let mut rows = raw_query_rows(db, &sel, &cols, backend).await?;
    Ok(rows.pop())
}

/// Find one row by primary id.
pub async fn find_by_id<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    schema: &Schema,
    id: i64,
) -> Result<Option<JsonMap<String, JsonValue>>, StoreError> {
    let table = schema.table_name();
    let cols = select_columns(schema);
    let mut sel = Query::select();
    sel.from(Alias::new(&table));
    for (col, _) in &cols {
        sel.column(Alias::new(col));
    }
    sel.cond_where(Condition::all().add(Expr::col(Alias::new(base::ID)).eq(id)))
        .limit(1);
    let mut rows = raw_query_rows(db, &sel, &cols, backend).await?;
    Ok(rows.pop())
}

// ---------------------------------------------------------------------------
// INSERT / UPDATE / DELETE
// ---------------------------------------------------------------------------

/// Insert a row, returning its id. Uses `RETURNING id` (SQLite 3.35+ / PG).
pub async fn insert<C: ConnectionTrait>(
    db: &C,
    _backend: DbBackend,
    table: &str,
    values: Vec<(String, Value)>,
) -> Result<i64, StoreError> {
    let cols: Vec<Alias> = values.iter().map(|(c, _)| Alias::new(c)).collect();
    let vals: Vec<Expr> = values.into_iter().map(|(_, v)| Expr::val(v).into()).collect();
    let stmt = Query::insert()
        .into_table(Alias::new(table))
        .columns(cols)
        .values_panic(vals)
        .returning_col(Alias::new(base::ID))
        .to_owned();
    let row = db
        .query_one(&stmt)
        .await?
        .ok_or_else(|| StoreError::Db(sea_orm::DbErr::RecordNotInserted))?;
    let id = row.try_get::<i64>("", base::ID)?;
    Ok(id)
}

pub async fn update_by_id<C: ConnectionTrait>(
    db: &C,
    _backend: DbBackend,
    table: &str,
    id: i64,
    values: Vec<(String, Value)>,
) -> Result<(), StoreError> {
    if values.is_empty() {
        return Ok(());
    }
    let pairs: Vec<(Alias, Expr)> = values
        .into_iter()
        .map(|(c, v)| (Alias::new(c), Expr::val(v).into()))
        .collect();
    let stmt = Query::update()
        .table(Alias::new(table))
        .values(pairs)
        .and_where(Expr::col(Alias::new(base::ID)).eq(id))
        .to_owned();
    db.execute(&stmt).await?;
    Ok(())
}

pub async fn delete_where<C: ConnectionTrait>(
    db: &C,
    _backend: DbBackend,
    table: &str,
    cond: Condition,
) -> Result<u64, StoreError> {
    let stmt = Query::delete()
        .from_table(Alias::new(table))
        .cond_where(cond)
        .to_owned();
    let res = db.execute(&stmt).await?;
    Ok(res.rows_affected())
}

// ---------------------------------------------------------------------------
// link tables (m2m / media / components)
// ---------------------------------------------------------------------------

/// Replace all rows of a many-to-many join table for one owner entry.
pub async fn replace_join_links<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    join_table: &str,
    owner_col: &str,
    target_col: &str,
    order_col: &str,
    owner_id: i64,
    target_ids: &[i64],
) -> Result<(), StoreError> {
    delete_where(
        db,
        backend,
        join_table,
        Condition::all().add(Expr::col(Alias::new(owner_col)).eq(owner_id)),
    )
    .await?;
    for (idx, target_id) in target_ids.iter().enumerate() {
        insert(
            db,
            backend,
            join_table,
            vec![
                (owner_col.to_string(), Value::BigInt(Some(owner_id))),
                (target_col.to_string(), Value::BigInt(Some(*target_id))),
                (order_col.to_string(), Value::Double(Some(idx as f64 + 1.0))),
            ],
        )
        .await?;
    }
    Ok(())
}

/// Fetch ordered target ids of a join table for one owner entry.
pub async fn fetch_join_links<C: ConnectionTrait>(
    db: &C,
    _backend: DbBackend,
    join_table: &str,
    owner_col: &str,
    target_col: &str,
    order_col: &str,
    owner_id: i64,
) -> Result<Vec<i64>, StoreError> {
    let mut sel = Query::select();
    sel.from(Alias::new(join_table))
        .column(Alias::new(target_col))
        .cond_where(Condition::all().add(Expr::col(Alias::new(owner_col)).eq(owner_id)))
        .order_by(Alias::new(order_col), Order::Asc);
    let rows = db.query_all(&sel).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(r.try_get::<i64>("", target_col)?);
    }
    Ok(out)
}

/// Generic link-row fetch: returns all rows of `table` where `key_col = key`.
pub async fn fetch_link_rows<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    table: &str,
    columns: &[&str],
    key_col: &str,
    key: i64,
    extra: Option<Condition>,
    order_by: Option<&str>,
) -> Result<Vec<JsonMap<String, JsonValue>>, StoreError> {
    let mut sel = Query::select();
    sel.from(Alias::new(table));
    for c in columns {
        sel.column(Alias::new(*c));
    }
    let mut cond = Condition::all().add(Expr::col(Alias::new(key_col)).eq(key));
    if let Some(e) = extra {
        cond = cond.add(e);
    }
    sel.cond_where(cond);
    if let Some(ob) = order_by {
        sel.order_by(Alias::new(ob), Order::Asc);
    }
    let cols: Vec<(String, SqlFamily)> = columns
        .iter()
        .map(|c| {
            let family = match *c {
                "order" => SqlFamily::Double,
                "component_uid" | "field" => SqlFamily::VarChar,
                _ => SqlFamily::BigInt,
            };
            (c.to_string(), family)
        })
        .collect();
    raw_query_rows(db, &sel, &cols, backend).await
}

/// Insert one link row (component link, media link).
pub async fn insert_link_row<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    table: &str,
    values: Vec<(String, Value)>,
) -> Result<i64, StoreError> {
    insert(db, backend, table, values).await
}

// ---------------------------------------------------------------------------
// typed write-value assembly
// ---------------------------------------------------------------------------

/// Build typed (column, Value) pairs from user JSON for one schema.
/// Unknown and non-scalar attributes are skipped (relations/components/media
/// are written through their link tables by the service).
pub fn build_write_values(
    schema: &Schema,
    data: &JsonMap<String, JsonValue>,
    for_update: bool,
) -> Result<Vec<(String, Value)>, StoreError> {
    let mut out = Vec::new();
    for (name, attr) in &schema.attributes {
        let Some(v) = data.get(name) else { continue };
        if !attr.attr_type.is_scalar_column() {
            continue;
        }
        if for_update && matches!(attr.attr_type, FieldType::Uid) && v.is_null() {
            continue; // never null out a uid on update
        }
        out.push((column_name(name), attr_to_value(attr, v)?));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Convenience wrappers — higher-level API used by the services crate
// ---------------------------------------------------------------------------

/// Run a content SELECT with query params (simpler signature).
pub async fn query_rows<C: ConnectionTrait>(
    db: &C,
    backend: DbBackend,
    schema: &Schema,
    params: &QueryParams,
) -> Result<(Vec<JsonValue>, i64), StoreError> {
    let default_sort: Vec<SortField> = vec![];
    let spec = SelectSpec {
        query: Some(params),
        locale: params.locale.as_deref(),
        state: params.status,
        default_sort: &default_sort,
    };
    let (rows, total) = select(db, backend, schema, &spec).await?;
    let rows: Vec<_> = rows
        .into_iter()
        .map(|m| JsonValue::Object(m))
        .collect();
    Ok((rows, total))
}

/// Find one entry by document_id (returns JSON object).
pub async fn find_one_by_document_id<C: ConnectionTrait>(
    db: &C,
    schema: &Schema,
    document_id: &str,
) -> Result<Option<JsonValue>, StoreError> {
    let backend = DbBackend::Sqlite; // FIXME: pass through
    let row = find_by_document_id(db, backend, schema, document_id, None, None).await?;
    Ok(row.map(JsonValue::Object))
}

/// Insert one content entry from JSON data. Returns the created row.
pub async fn insert_one<C: ConnectionTrait>(
    db: &C,
    schema: &Schema,
    data: &JsonValue,
    user_id: Option<i64>,
) -> Result<JsonValue, StoreError> {
    let backend = DbBackend::Sqlite;
    let table = schema.table_name();
    let obj = data
        .as_object()
        .ok_or_else(|| StoreError::bad_value("data", "expected JSON object"))?;

    let doc_id = obj
        .get("documentId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let now = chrono::Utc::now().to_rfc3339();
    let locale = obj.get("locale").and_then(|v| v.as_str()).unwrap_or("en");
    let state = obj
        .get("publicationState")
        .and_then(|v| v.as_str())
        .unwrap_or("draft");

    let mut values = vec![
        ("document_id".to_string(), Value::String(Some(doc_id.clone()))),
        ("locale".to_string(), Value::String(Some(locale.to_string()))),
        (
            "publication_state".to_string(),
            Value::String(Some(state.to_string())),
        ),
        ("created_at".to_string(), Value::String(Some(now.clone()))),
        ("updated_at".to_string(), Value::String(Some(now))),
    ];

    let scalar_values = build_write_values(schema, obj, false)?;
    for (col, val) in scalar_values {
        values.push((col, val));
    }

    if let Some(uid) = user_id {
        values.push(("created_by_id".to_string(), Value::BigInt(Some(uid))));
        values.push(("updated_by_id".to_string(), Value::BigInt(Some(uid))));
    }

    let id = insert(db, backend, &table, values).await?;
    find_by_id(db, backend, schema, id)
        .await?
        .map(JsonValue::Object)
        .ok_or_else(|| StoreError::NotFound("inserted row not found".into()))
}

/// Update one content entry by document_id.
pub async fn update_one<C: ConnectionTrait>(
    db: &C,
    schema: &Schema,
    document_id: &str,
    data: &JsonValue,
    user_id: Option<i64>,
) -> Result<JsonValue, StoreError> {
    let backend = DbBackend::Sqlite;
    let table = schema.table_name();

    let existing = find_by_document_id(db, backend, schema, document_id, None, None)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("entry {document_id} not found")))?;

    let existing_id = existing
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| StoreError::NotFound("entry has no id".into()))?;

    let obj = data
        .as_object()
        .ok_or_else(|| StoreError::bad_value("data", "expected JSON object"))?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut values = vec![("updated_at".to_string(), Value::String(Some(now)))];
    let scalar_values = build_write_values(schema, obj, true)?;
    for (col, val) in scalar_values {
        values.push((col, val));
    }
    if let Some(uid) = user_id {
        values.push(("updated_by_id".to_string(), Value::BigInt(Some(uid))));
    }

    update_by_id(db, backend, &table, existing_id, values).await?;
    find_by_id(db, backend, schema, existing_id)
        .await?
        .map(JsonValue::Object)
        .ok_or_else(|| StoreError::NotFound("updated row not found".into()))
}

/// Soft-delete an entry by document_id (sets deleted_at).
pub async fn delete_one<C: ConnectionTrait>(
    db: &C,
    schema: &Schema,
    document_id: &str,
) -> Result<(), StoreError> {
    let _backend = DbBackend::Sqlite;
    let table = schema.table_name();
    let now = chrono::Utc::now().to_rfc3339();
    let cond = Condition::all()
        .add(Expr::col(Alias::new(base::DOCUMENT_ID)).eq(document_id))
        .add(Expr::col(Alias::new(base::DELETED_AT)).is_null());
    let stmt = Query::update()
        .table(Alias::new(&table))
        .values(vec![(
            Alias::new(base::DELETED_AT),
            Expr::val(Value::String(Some(now))).into(),
        )])
        .cond_where(cond)
        .to_owned();
    db.execute(&stmt).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::Uid;
    use core_schema::{Attribute, SchemaInfo};
    use indexmap::IndexMap;

    fn article() -> Schema {
        let mut attrs = IndexMap::new();
        attrs.insert("title".to_string(), Attribute::new(FieldType::String));
        let mut views = Attribute::new(FieldType::Integer);
        views.default = Some(JsonValue::from(0));
        attrs.insert("views".to_string(), views);
        Schema {
            uid: Uid::new("api::article.article"),
            kind: core_domain::ContentTypeKind::CollectionType,
            collection_name: None,
            info: SchemaInfo {
                singular_name: "article".into(),
                plural_name: "articles".into(),
                display_name: "Article".into(),
                description: None,
                icon: None,
            },
            options: Default::default(),
            plugin_options: None,
            attributes: attrs,
        }
    }

    #[test]
    fn resolve_fields() {
        let s = article();
        assert_eq!(resolve_field(&s, "title").unwrap().0, "title");
        assert_eq!(resolve_field(&s, "createdAt").unwrap().0, "created_at");
        assert_eq!(resolve_field(&s, "documentId").unwrap().0, "document_id");
        assert!(resolve_field(&s, "nope").is_err());
    }

    #[test]
    fn write_values_skip_non_scalars() {
        let s = article();
        let mut data = JsonMap::new();
        data.insert("title".into(), JsonValue::from("hello"));
        data.insert("unknown".into(), JsonValue::from(1));
        let vals = build_write_values(&s, &data, false).unwrap();
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0].0, "title");
    }
}
