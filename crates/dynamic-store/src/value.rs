//! JSON ↔ SQL value conversion driven by the schema (design Part IV §4).

use crate::error::StoreError;
use core_domain::FieldType;
use core_schema::{Attribute, SqlFamily};
use sea_orm::DbBackend;
use sea_query::Value;
use serde_json::Value as JsonValue;

/// Convert a JSON value into a typed `sea_query::Value` for one attribute.
pub fn attr_to_value(attr: &Attribute, v: &JsonValue) -> Result<Value, StoreError> {
    let field = format!("{:?}", attr.attr_type);
    if v.is_null() {
        return Ok(null_value(attr.attr_type));
    }
    match attr.attr_type {
        FieldType::String
        | FieldType::Text
        | FieldType::Richtext
        | FieldType::Email
        | FieldType::Password
        | FieldType::Uid
        | FieldType::Enumeration => Ok(Value::String(Some(as_string(v, &field)?))),
        FieldType::Blocks | FieldType::Json => Ok(Value::Json(Some(Box::new(v.clone())))),
        FieldType::Integer => Ok(Value::Int(Some(as_i64(v, &field)? as i32))),
        FieldType::Biginteger => Ok(Value::BigInt(Some(as_i64(v, &field)?))),
        FieldType::Decimal | FieldType::Float => Ok(Value::Double(Some(as_f64(v, &field)?))),
        FieldType::Boolean => Ok(Value::Bool(Some(as_bool(v, &field)?))),
        FieldType::Datetime => {
            let s = as_string(v, &field)?;
            let dt = chrono::DateTime::parse_from_rfc3339(&s)
                .map_err(|e| StoreError::bad_value(&field, format!("invalid datetime `{s}`: {e}")))?
                .with_timezone(&chrono::Utc);
            Ok(Value::ChronoDateTimeUtc(Some(dt)))
        }
        FieldType::Date => {
            let s = as_string(v, &field)?;
            let d = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .map_err(|e| StoreError::bad_value(&field, format!("invalid date `{s}`: {e}")))?;
            Ok(Value::ChronoDate(Some(d)))
        }
        FieldType::Time => {
            let s = as_string(v, &field)?;
            let t = parse_time(&s)
                .ok_or_else(|| StoreError::bad_value(&field, format!("invalid time `{s}`")))?;
            Ok(Value::ChronoTime(Some(t)))
        }
        FieldType::Media | FieldType::Relation | FieldType::Component | FieldType::Dynamiczone => {
            Err(StoreError::Unsupported(format!(
                "{:?} is not a scalar column",
                attr.attr_type
            )))
        }
    }
}

/// A typed NULL for the attribute's column.
pub fn null_value(t: FieldType) -> Value {
    match t {
        FieldType::String
        | FieldType::Text
        | FieldType::Richtext
        | FieldType::Email
        | FieldType::Password
        | FieldType::Uid
        | FieldType::Enumeration => Value::String(None),
        FieldType::Blocks | FieldType::Json => Value::Json(None),
        FieldType::Integer => Value::Int(None),
        FieldType::Biginteger => Value::BigInt(None),
        FieldType::Decimal | FieldType::Float => Value::Double(None),
        FieldType::Boolean => Value::Bool(None),
        FieldType::Datetime => Value::ChronoDateTimeUtc(None),
        FieldType::Date => Value::ChronoDate(None),
        FieldType::Time => Value::ChronoTime(None),
        _ => Value::String(None),
    }
}

/// Coerce a filter value for a column of the given family. Lenient: accepts
/// strings that look like the target type (query params arrive as strings).
pub fn coerce_filter_value(family: SqlFamily, v: &JsonValue) -> Result<Value, StoreError> {
    if v.is_null() {
        return Ok(Value::String(None));
    }
    match family {
        SqlFamily::VarChar | SqlFamily::Text => Ok(Value::String(Some(as_string(v, "filter")?))),
        SqlFamily::Json => Ok(Value::Json(Some(Box::new(v.clone())))),
        SqlFamily::Integer => Ok(Value::Int(Some(as_i64(v, "filter")? as i32))),
        SqlFamily::BigInt => Ok(Value::BigInt(Some(as_i64(v, "filter")?))),
        SqlFamily::Decimal | SqlFamily::Double => Ok(Value::Double(Some(as_f64(v, "filter")?))),
        SqlFamily::Bool => Ok(Value::Bool(Some(as_bool(v, "filter")?))),
        SqlFamily::Timestamp => {
            let s = as_string(v, "filter")?;
            let dt = chrono::DateTime::parse_from_rfc3339(&s)
                .map_err(|e| {
                    StoreError::bad_value("filter", format!("invalid datetime `{s}`: {e}"))
                })?
                .with_timezone(&chrono::Utc);
            Ok(Value::ChronoDateTimeUtc(Some(dt)))
        }
        SqlFamily::Date => {
            let s = as_string(v, "filter")?;
            let d = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .map_err(|e| StoreError::bad_value("filter", format!("invalid date `{s}`: {e}")))?;
            Ok(Value::ChronoDate(Some(d)))
        }
        SqlFamily::Time => {
            let s = as_string(v, "filter")?;
            let t = parse_time(&s)
                .ok_or_else(|| StoreError::bad_value("filter", format!("invalid time `{s}`")))?;
            Ok(Value::ChronoTime(Some(t)))
        }
        SqlFamily::MediaLink | SqlFamily::RelationLink | SqlFamily::ComponentLink => {
            // link-table columns are addressed by id
            Ok(Value::BigInt(Some(as_i64(v, "filter")?)))
        }
    }
}

fn as_string(v: &JsonValue, field: &str) -> Result<String, StoreError> {
    match v {
        JsonValue::String(s) => Ok(s.clone()),
        JsonValue::Number(n) => Ok(n.to_string()),
        JsonValue::Bool(b) => Ok(b.to_string()),
        other => Err(StoreError::bad_value(
            field,
            format!("expected string, got {other}"),
        )),
    }
}

fn as_i64(v: &JsonValue, field: &str) -> Result<i64, StoreError> {
    match v {
        JsonValue::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.round() as i64))
            .ok_or_else(|| StoreError::bad_value(field, format!("expected integer, got {n}"))),
        JsonValue::String(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|e| StoreError::bad_value(field, format!("expected integer, got `{s}`: {e}"))),
        other => Err(StoreError::bad_value(
            field,
            format!("expected integer, got {other}"),
        )),
    }
}

fn as_f64(v: &JsonValue, field: &str) -> Result<f64, StoreError> {
    match v {
        JsonValue::Number(n) => n
            .as_f64()
            .ok_or_else(|| StoreError::bad_value(field, format!("expected number, got {n}"))),
        JsonValue::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|e| StoreError::bad_value(field, format!("expected number, got `{s}`: {e}"))),
        other => Err(StoreError::bad_value(
            field,
            format!("expected number, got {other}"),
        )),
    }
}

fn as_bool(v: &JsonValue, field: &str) -> Result<bool, StoreError> {
    match v {
        JsonValue::Bool(b) => Ok(*b),
        JsonValue::String(s) if s == "true" || s == "1" => Ok(true),
        JsonValue::String(s) if s == "false" || s == "0" => Ok(false),
        JsonValue::Number(n) => Ok(n.as_i64() != Some(0)),
        other => Err(StoreError::bad_value(
            field,
            format!("expected boolean, got {other}"),
        )),
    }
}

fn parse_time(s: &str) -> Option<chrono::NaiveTime> {
    for fmt in ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"] {
        if let Ok(t) = chrono::NaiveTime::parse_from_str(s, fmt) {
            return Some(t);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// row -> JSON
// ---------------------------------------------------------------------------

use sea_orm::{ConnectionTrait, QueryResult};

/// Read one column out of a `QueryResult` as JSON, guided by the family.
pub fn column_to_json(
    row: &QueryResult,
    col: &str,
    family: SqlFamily,
    backend: DbBackend,
) -> JsonValue {
    macro_rules! get {
        ($t:ty) => {
            row.try_get::<Option<$t>>("", col).ok().flatten()
        };
    }
    match family {
        SqlFamily::VarChar | SqlFamily::Text => get!(String).into(),
        SqlFamily::Integer | SqlFamily::BigInt => match get!(i64) {
            Some(i) => i.into(),
            None => JsonValue::Null,
        },
        SqlFamily::Decimal | SqlFamily::Double => match get!(f64) {
            Some(f) => serde_json::Number::from_f64(f).into(),
            None => JsonValue::Null,
        },
        SqlFamily::Bool => match get!(bool) {
            Some(b) => b.into(),
            None => match backend {
                DbBackend::Sqlite => match get!(i64) {
                    Some(i) => (i != 0).into(),
                    None => JsonValue::Null,
                },
                _ => JsonValue::Null,
            },
        },
        SqlFamily::Json => {
            if let Some(j) = get!(JsonValue) {
                j
            } else if let Some(s) = get!(String) {
                serde_json::from_str(&s).unwrap_or(JsonValue::String(s))
            } else {
                JsonValue::Null
            }
        }
        SqlFamily::Timestamp => {
            if let Some(dt) = get!(chrono::DateTime<chrono::Utc>) {
                JsonValue::String(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            } else if let Some(s) = get!(String) {
                JsonValue::String(normalize_datetime_str(&s))
            } else {
                JsonValue::Null
            }
        }
        SqlFamily::Date => {
            if let Some(d) = get!(chrono::NaiveDate) {
                JsonValue::String(d.format("%Y-%m-%d").to_string())
            } else if let Some(s) = get!(String) {
                JsonValue::String(s.chars().take(10).collect())
            } else {
                JsonValue::Null
            }
        }
        SqlFamily::Time => {
            if let Some(t) = get!(chrono::NaiveTime) {
                JsonValue::String(t.format("%H:%M:%S%.3f").to_string())
            } else if let Some(s) = get!(String) {
                JsonValue::String(s)
            } else {
                JsonValue::Null
            }
        }
        SqlFamily::MediaLink | SqlFamily::RelationLink | SqlFamily::ComponentLink => {
            match get!(i64) {
                Some(i) => i.into(),
                None => JsonValue::Null,
            }
        }
    }
}

/// Normalize `2026-07-31 06:00:00+00:00` / `2026-07-31 06:00:00` to RFC3339.
fn normalize_datetime_str(s: &str) -> String {
    if s.contains('T') {
        return s.to_string();
    }
    let with_t = s.replacen(' ', "T", 1);
    if with_t.ends_with('Z') || with_t.contains('+') {
        with_t
    } else {
        format!("{with_t}Z")
    }
}

/// Fetch all rows of a statement and map each with `f`.
pub async fn query_rows<C: ConnectionTrait, S: sea_orm::StatementBuilder + Sync>(
    db: &C,
    stmt: &S,
    columns: &[(String, SqlFamily)],
    backend: DbBackend,
) -> Result<Vec<serde_json::Map<String, JsonValue>>, StoreError> {
    let rows = db.query_all(stmt).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut obj = serde_json::Map::with_capacity(columns.len());
        for (col, family) in columns {
            obj.insert(api_key(col), column_to_json(row, col, *family, backend));
        }
        out.push(obj);
    }
    Ok(out)
}

/// Map a physical base column name to the camelCase API key Strapi exposes.
/// Attribute columns keep their schema-defined name; only the system columns
/// use Strapi's public naming (`documentId`, `publicationState`, `createdAt`,
/// etc.) so clients and the content UI can rely on the Strapi shape.
pub fn api_key(col: &str) -> String {
    use crate::base_columns::*;
    match col {
        DOCUMENT_ID => "documentId".to_string(),
        PUBLICATION_STATE => "publicationState".to_string(),
        CREATED_AT => "createdAt".to_string(),
        UPDATED_AT => "updatedAt".to_string(),
        PUBLISHED_AT => "publishedAt".to_string(),
        CREATED_BY => "createdBy".to_string(),
        UPDATED_BY => "updatedBy".to_string(),
        SYNC_VERSION => "syncVersion".to_string(),
        ORIGIN_NODE => "originNodeId".to_string(),
        DELETED_AT => "deletedAt".to_string(),
        _ => col.to_string(),
    }
}

/// Family of a base (non-attribute) column.
pub fn base_column_family(col: &str) -> SqlFamily {
    use crate::base_columns::*;
    match col {
        ID | SYNC_VERSION | CREATED_BY | UPDATED_BY => SqlFamily::BigInt,
        DOCUMENT_ID | LOCALE | PUBLICATION_STATE | ORIGIN_NODE => SqlFamily::VarChar,
        CREATED_AT | UPDATED_AT | PUBLISHED_AT | DELETED_AT => SqlFamily::Timestamp,
        _ => SqlFamily::VarChar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_coercions() {
        let attr = Attribute::new(FieldType::Integer);
        let v = attr_to_value(&attr, &JsonValue::from("42")).unwrap();
        assert!(matches!(v, Value::Int(Some(42))));

        let attr = Attribute::new(FieldType::Boolean);
        let v = attr_to_value(&attr, &JsonValue::from(true)).unwrap();
        assert!(matches!(v, Value::Bool(Some(true))));

        let attr = Attribute::new(FieldType::Datetime);
        let v = attr_to_value(&attr, &JsonValue::from("2026-07-31T06:00:00Z")).unwrap();
        assert!(matches!(v, Value::ChronoDateTimeUtc(Some(_))));

        let attr = Attribute::new(FieldType::String);
        assert!(matches!(
            attr_to_value(&attr, &JsonValue::Null).unwrap(),
            Value::String(None)
        ));
    }

    #[test]
    fn time_parsing() {
        assert!(parse_time("12:30").is_some());
        assert!(parse_time("12:30:59.123").is_some());
        assert!(parse_time("nope").is_none());
    }

    #[test]
    fn api_keys_are_strapi_camel_case() {
        assert_eq!(api_key("document_id"), "documentId");
        assert_eq!(api_key("publication_state"), "publicationState");
        assert_eq!(api_key("created_at"), "createdAt");
        assert_eq!(api_key("updated_at"), "updatedAt");
        assert_eq!(api_key("published_at"), "publishedAt");
        assert_eq!(api_key("id"), "id");
        // Attribute columns keep their schema-defined name.
        assert_eq!(api_key("title"), "title");
        assert_eq!(api_key("some_snake_field"), "some_snake_field");
    }
}
