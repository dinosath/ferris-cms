//! OWS runtime function invokers.
//!
//! The `ows-runtime` engine dispatches `call` tasks to a `FunctionInvoker`
//! registered under the function name. The runtime resolves `${...}` runtime
//! expressions on `with` arguments *before* invoking, so the `args` received
//! here are already-resolved JSON values. This module implements the FerrisCMS
//! CMS/media/HTTP/integration functions as invokers.

use std::collections::HashMap;
use std::sync::Arc;

use core_domain::Uid;
use core_schema::Schema;
use db::entities::{core_store, upload_file};
use dynamic_store::dml;
use ows_runtime::error::runtime_error;
use ows_runtime::service::{FunctionInvoker, FunctionRequest};
use ows_runtime_core::WorkflowError;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use serde_json::{json, Value};
use ::workflow::model::function;

use crate::AppContext;

fn arg_str<'a>(args: &'a HashMap<String, Value>, key: &str) -> Result<String, WorkflowError> {
    Ok(args
        .get(key)
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default())
}

fn arg_value(args: &HashMap<String, Value>, key: &str) -> Result<Value, WorkflowError> {
    Ok(args.get(key).cloned().unwrap_or(json!({})))
}

fn load_schema(app: &AppContext, uid: &str) -> Result<Schema, WorkflowError> {
    app.schema_cache
        .get(&Uid::new(uid))
        .ok_or_else(|| runtime_error(format!("content-type `{uid}` not found")))
}

/// The invoker that dispatches all FerrisCMS functions.
pub struct CmsFunctionInvoker {
    pub app: Arc<AppContext>,
}

#[async_trait::async_trait]
impl FunctionInvoker for CmsFunctionInvoker {
    async fn invoke(&self, req: FunctionRequest<'_>) -> Result<Value, WorkflowError> {
        let args = &req.args;
        let app = &self.app;
        let out = match req.name.as_str() {
            function::GET_CONTENT => get_content(app, args).await?,
            function::FIND_CONTENT => find_content(app, args).await?,
            function::QUERY_CONTENT => query_content(app, args).await?,
            function::CREATE_CONTENT => create_content(app, args).await?,
            function::UPDATE_CONTENT => update_content(app, args).await?,
            function::DELETE_CONTENT => delete_content(app, args).await?,
            function::PUBLISH_CONTENT => publish_content(app, args, true).await?,
            function::UNPUBLISH_CONTENT => publish_content(app, args, false).await?,
            function::GET_MEDIA => get_media(app, args).await?,
            function::UPLOAD_MEDIA => upload_media(app, args).await?,
            function::TRANSFORM_DATA => transform_data(app, args).await?,
            function::JSON => arg_value(args, "json")?,
            function::CSV => json!({ "csv": arg_str(args, "csv")? }),
            function::HTTP_REQUEST | function::REST_API | function::WEBHOOK => {
                http_request(app, args).await?
            }
            function::GRAPHQL => graphql(app, args).await?,
            function::DB_QUERY | function::POSTGRES => database(app, args).await?,
            function::REDIS => redis(app, args).await?,
            other => return Err(runtime_error(format!("unknown function `{other}`"))),
        };
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// CMS content / media
// ---------------------------------------------------------------------------

async fn get_content(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let uid = arg_str(args, "contentType")?;
    if uid.is_empty() {
        return Err(runtime_error("Get Content: missing content type"));
    }
    let schema = load_schema(app, &uid)?;
    let doc_id = arg_str(args, "documentId")?;
    let row = dml::find_one_by_document_id(&app.db, &schema, &doc_id)
        .await
        .map_err(|e| runtime_error(format!("Get Content: {e}")))?;
    Ok(row.unwrap_or(Value::Null))
}

async fn find_content(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let uid = arg_str(args, "contentType")?;
    if uid.is_empty() {
        return Err(runtime_error("Find Content: missing content type"));
    }
    let schema = load_schema(app, &uid)?;
    let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);
    let filters = arg_value(args, "filters")?;
    let params = build_query_params(&filters, limit);
    let (rows, _) = dml::query_rows(&app.db, app.db_backend(), &schema, &params)
        .await
        .map_err(|e| runtime_error(format!("Find Content: {e}")))?;
    Ok(Value::Array(rows))
}

fn build_query_params(filters: &Value, limit: i64) -> api_types::QueryParams {
    use api_types::{Filter, FilterOp};
    let mut leaves = Vec::new();
    if let Some(obj) = filters.as_object() {
        for (field, value) in obj {
            if field == "$and" {
                continue;
            }
            leaves.push(Filter::Leaf {
                field: field.clone(),
                op: FilterOp::Eq,
                values: vec![value.clone()],
            });
        }
    }
    let mut params = api_types::QueryParams::default();
    if !leaves.is_empty() {
        params.filters = Some(if leaves.len() == 1 {
            leaves.pop().unwrap()
        } else {
            Filter::And(leaves)
        });
    }
    params.pagination = Some(api_types::PaginationParams::Page {
        page: 1,
        page_size: limit,
        with_count: Some(false),
    });
    params
}

async fn query_content(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let uid = arg_str(args, "contentType")?;
    if uid.is_empty() {
        return Err(runtime_error("Query Content: missing content type"));
    }
    let schema = load_schema(app, &uid)?;
    let query = arg_value(args, "query")?;
    let limit = query.get("limit").and_then(|v| v.as_i64()).unwrap_or(25);
    let filters = query.get("filters").cloned().unwrap_or(json!({}));
    let params = build_query_params(&filters, limit);
    let (rows, _) = dml::query_rows(&app.db, app.db_backend(), &schema, &params)
        .await
        .map_err(|e| runtime_error(format!("Query Content: {e}")))?;
    Ok(Value::Array(rows))
}

async fn create_content(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let uid = arg_str(args, "contentType")?;
    if uid.is_empty() {
        return Err(runtime_error("Create Content: missing content type"));
    }
    let schema = load_schema(app, &uid)?;
    let data = arg_value(args, "data")?;
    dml::insert_one(&app.db, &schema, &data, None)
        .await
        .map_err(|e| runtime_error(format!("Create Content: {e}")))
}

async fn update_content(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let uid = arg_str(args, "contentType")?;
    if uid.is_empty() {
        return Err(runtime_error("Update Content: missing content type"));
    }
    let schema = load_schema(app, &uid)?;
    let doc_id = arg_str(args, "documentId")?;
    let data = arg_value(args, "data")?;
    dml::update_one(&app.db, &schema, &doc_id, &data, None)
        .await
        .map_err(|e| runtime_error(format!("Update Content: {e}")))
}

async fn delete_content(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let uid = arg_str(args, "contentType")?;
    if uid.is_empty() {
        return Err(runtime_error("Delete Content: missing content type"));
    }
    let schema = load_schema(app, &uid)?;
    let doc_id = arg_str(args, "documentId")?;
    dml::delete_one(&app.db, &schema, &doc_id)
        .await
        .map_err(|e| runtime_error(format!("Delete Content: {e}")))?;
    Ok(Value::Null)
}

async fn publish_content(
    app: &AppContext,
    args: &HashMap<String, Value>,
    publish: bool,
) -> Result<Value, WorkflowError> {
    let uid = arg_str(args, "contentType")?;
    if uid.is_empty() {
        return Err(runtime_error("content type required"));
    }
    let schema = load_schema(app, &uid)?;
    if !schema.draft_and_publish() {
        return Err(runtime_error("Draft & Publish is not enabled for this content-type"));
    }
    let doc_id = arg_str(args, "documentId")?;
    let existing = dml::find_one_by_document_id(&app.db, &schema, &doc_id)
        .await
        .map_err(|e| runtime_error(format!("{e}")))?
        .ok_or_else(|| runtime_error(format!("entry {doc_id} not found")))?;
    let mut data = existing.clone();
    if let Some(obj) = data.as_object_mut() {
        obj.insert("publicationState".into(), json!(if publish { "published" } else { "draft" }));
        if publish {
            obj.insert("publishedAt".into(), json!(chrono::Utc::now().to_rfc3339()));
        }
        obj.insert("documentId".into(), json!(uuid::Uuid::new_v4().to_string()));
    }
    let row = dml::insert_one(&app.db, &schema, &data, None)
        .await
        .map_err(|e| runtime_error(format!("{e}")))?;
    dml::delete_one(&app.db, &schema, &doc_id)
        .await
        .map_err(|e| runtime_error(format!("{e}")))?;
    Ok(row)
}

async fn get_media(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let id = arg_str(args, "id")?
        .parse::<i64>()
        .map_err(|_| runtime_error("Get Media: id must be a number"))?;
    let file = upload_file::Entity::find_by_id(id)
        .one(&app.db)
        .await
        .map_err(|e| runtime_error(format!("Get Media: {e}")))?;
    Ok(if let Some(f) = file {
        json!({
            "id": f.id, "name": f.name, "mime": f.mime, "url": f.url,
            "size": f.size, "hash": f.hash, "ext": f.ext, "alternativeText": f.alternative_text,
        })
    } else {
        Value::Null
    })
}

async fn upload_media(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let filename = arg_str(args, "filename")?;
    if filename.is_empty() {
        return Err(runtime_error("Upload Media: missing filename"));
    }
    let data_value = arg_value(args, "data")?;
    let bytes = data_value
        .as_str()
        .and_then(|s| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok())
        .ok_or_else(|| runtime_error("Upload Media: no base64 data provided"))?;
    let mime = "application/octet-stream".to_string();
    let file = crate::media::media_upload(app, &filename, &mime, &bytes)
        .await
        .map_err(|e| runtime_error(format!("Upload Media: {e}")))?;
    Ok(json!({
        "id": file.id, "name": file.name, "mime": file.mime, "url": file.url,
        "size": file.size, "hash": file.hash, "ext": file.ext, "alternativeText": file.alternative_text,
    }))
}

async fn transform_data(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("jsonToCsv");
    if direction == "csvToJson" {
        let csv_text = arg_str(args, "csvData")?;
        Ok(json!({ "json": csv_to_json(&csv_text)? }))
    } else {
        let rows: Vec<Value> = args
            .get("rows")
            .cloned()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let csv = json_to_csv(&rows)?;
        Ok(json!({ "csv": csv }))
    }
}

fn stringify(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn json_to_csv(rows: &[Value]) -> Result<String, WorkflowError> {
    if rows.is_empty() {
        return Ok(String::new());
    }
    let mut cols: Vec<String> = Vec::new();
    for row in rows {
        if let Some(obj) = row.as_object() {
            for k in obj.keys() {
                if !cols.contains(k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    let esc = |s: &str| -> String {
        if s.contains(',') || s.contains('"') || s.contains('\n') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let mut out = String::new();
    out.push_str(&cols.iter().map(|c| esc(c)).collect::<Vec<_>>().join(","));
    out.push('\n');
    for row in rows {
        let vals: Vec<String> = cols
            .iter()
            .map(|c| esc(&stringify(&row.get(c).cloned().unwrap_or(Value::Null))))
            .collect();
        out.push_str(&vals.join(","));
        out.push('\n');
    }
    Ok(out)
}

fn csv_to_json(csv: &str) -> Result<Vec<Value>, WorkflowError> {
    let mut lines = csv.lines();
    let header: Vec<String> = lines
        .next()
        .ok_or_else(|| runtime_error("CSV: empty input"))?
        .split(',')
        .map(|s| s.trim_matches('"').to_string())
        .collect();
    let mut out = Vec::new();
    for line in lines {
        let fields: Vec<String> = line.split(',').map(|s| s.trim_matches('"').to_string()).collect();
        let mut obj = serde_json::Map::new();
        for (i, col) in header.iter().enumerate() {
            obj.insert(col.clone(), json!(fields.get(i).cloned().unwrap_or_default()));
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Integrations
// ---------------------------------------------------------------------------

async fn http_request(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let url = arg_str(args, "url")?;
    if url.is_empty() {
        return Err(runtime_error("HTTP: missing url"));
    }
    let method = args.get("method").and_then(|v| v.as_str()).unwrap_or("GET").to_uppercase();
    let headers = arg_value(args, "headers")?;
    let body = args.get("body").cloned();
    let client = reqwest::Client::new();
    let mut req = client.request(
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| runtime_error("HTTP: invalid method"))?,
        &url,
    );
    if let Some(obj) = headers.as_object() {
        for (k, v) in obj {
            req = req.header(k, stringify(v));
        }
    }
    if let Some(b) = &body {
        if !b.is_null() {
            req = req.json(b);
        }
    }
    let resp = req
        .send()
        .await
        .map_err(|e| runtime_error(format!("HTTP request failed: {e}")))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| runtime_error(format!("HTTP body: {e}")))?;
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| Value::String(text));
    Ok(json!({ "statusCode": status, "json": parsed }))
}

async fn graphql(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let url = arg_str(args, "url")?;
    if url.is_empty() {
        return Err(runtime_error("GraphQL: missing url"));
    }
    let query = arg_str(args, "query")?;
    let vars = arg_value(args, "variables")?;
    let client = reqwest::Client::new();
    let body = json!({ "query": query, "variables": vars });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| runtime_error(format!("GraphQL: {e}")))?;
    let status = resp.status().as_u16();
    let json: Value = resp.json().await.map_err(|e| runtime_error(format!("GraphQL body: {e}")))?;
    Ok(json!({ "statusCode": status, "json": json }))
}

async fn database(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let sql = arg_str(args, "query")?;
    if sql.trim().is_empty() {
        return Err(runtime_error("Database Query: empty SQL"));
    }
    let res = app
        .db
        .execute_unprepared(&sql)
        .await
        .map_err(|e| runtime_error(format!("Database Query: {e}")))?;
    Ok(json!({ "rowsAffected": res.rows_affected() }))
}

async fn redis(app: &AppContext, args: &HashMap<String, Value>) -> Result<Value, WorkflowError> {
    let operation = args.get("operation").and_then(|v| v.as_str()).unwrap_or("get").to_string();
    let key = arg_str(args, "key")?;
    match operation.as_str() {
        "set" => {
            let value = arg_value(args, "value")?;
            let existing = core_store::Entity::find()
                .filter(core_store::Column::Key.eq(key.clone()))
                .one(&app.db)
                .await
                .map_err(|e| runtime_error(e.to_string()))?;
            if let Some(e) = existing {
                let mut am: core_store::ActiveModel = e.into();
                am.value_json = Set(Some(value.clone()));
                am.update(&app.db).await.map_err(|e| runtime_error(e.to_string()))?;
            } else {
                core_store::ActiveModel {
                    key: Set(key.clone()),
                    value_json: Set(Some(value.clone())),
                    store_type: Set(Some("workflow-redis".into())),
                    environment: Set(None),
                    tag: Set(None),
                    ..Default::default()
                }
                .insert(&app.db)
                .await
                .map_err(|e| runtime_error(e.to_string()))?;
            }
            Ok(json!({ "key": key, "operation": "set", "value": value }))
        }
        "del" => {
            core_store::Entity::delete_many()
                .filter(core_store::Column::Key.eq(key.clone()))
                .exec(&app.db)
                .await
                .map_err(|e| runtime_error(e.to_string()))?;
            Ok(json!({ "key": key, "operation": "del" }))
        }
        _ => {
            let v = core_store::Entity::find()
                .filter(core_store::Column::Key.eq(key.clone()))
                .one(&app.db)
                .await
                .map_err(|e| runtime_error(e.to_string()))?;
            Ok(json!({ "key": key, "operation": "get", "value": v.and_then(|r| r.value_json).unwrap_or(Value::Null) }))
        }
    }
}
