//! OWS function executors — the runtime implementation for each `call`
//! function name.
//!
//! In OWS, a `call` task references a function (see `crate::model::function`).
//! This module provides the *runtime* that runs each function against the CMS
//! database, external HTTP services and the current input data, using the
//! function's `with` arguments.

use crate::AppContext;
use core_domain::Uid;
use core_schema::Schema;
use db::entities::{core_store, upload_file};
use dynamic_store::dml;
use indexmap::IndexMap;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use ::workflow::expression;
use ::workflow::model::{function, OwsDocument};

/// Runtime context passed to every function executor.
pub struct FunctionRunContext<'a> {
    pub app: &'a AppContext,
    pub workflow: &'a OwsDocument,
    /// The `with` arguments of the call task.
    pub with: &'a IndexMap<String, serde_json::Value>,
    /// Prior task outputs keyed by task *name* → output value.
    pub task_outputs: &'a HashMap<String, serde_json::Value>,
    pub env: &'a HashMap<String, String>,
    pub execution_id: i64,
    pub workflow_json: serde_json::Value,
    pub execution_json: serde_json::Value,
}

/// Build an expression context for the given input value.
pub fn expr_ctx(ctx: &FunctionRunContext<'_>, input: &serde_json::Value) -> expression::Context {
    let mut c = expression::Context::minimal();
    c.json = input.clone();
    c.nodes = ctx.task_outputs.clone();
    c.workflow = ctx.workflow_json.clone();
    c.execution = ctx.execution_json.clone();
    c.env = ctx.env.clone();
    c
}

/// Resolve a template string (which may contain `{{ }}` or `${ }` expressions)
/// against the given input value.
pub fn resolve_template(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
    template: &str,
) -> Result<serde_json::Value, String> {
    let norm = normalize_ows_expr(template);
    if !::workflow::expression::contains_expression(&norm) {
        return Ok(serde_json::Value::String(template.to_string()));
    }
    expression::evaluate(&norm, &expr_ctx(ctx, input)).map_err(|e| e.to_string())
}

/// Convert OWS-style `${ .a.b }` runtime expressions into the engine's
/// `{{ $json.a.b }}` syntax so the existing expression evaluator handles them.
pub fn normalize_ows_expr(template: &str) -> String {
    if template.trim_start().starts_with("${") && template.trim_end().ends_with('}') {
        let inner = template
            .trim()
            .trim_start_matches("${")
            .trim_end_matches('}')
            .trim();
        // jq-like `.a.b` and `$workflow.input[i]` → engine expression.
        if inner.starts_with('.') {
            let path = inner.trim_start_matches('.');
            return format!("{{{{ $json.{path} }}}}");
        }
        if inner.contains("$workflow.input") {
            return format!("{{{{ {inner} }}}}");
        }
        return format!("{{{{ {inner} }}}}");
    }
    template.to_string()
}

/// Resolve a configured value to JSON. Strings containing expressions are
/// evaluated; other JSON values are used as-is.
pub fn resolve_value(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match value {
        serde_json::Value::String(s) => resolve_template(ctx, input, s),
        other => Ok(other.clone()),
    }
}

/// Read a string argument from the `with` map (with expression resolution).
pub fn arg_str(ctx: &FunctionRunContext<'_>, input: &serde_json::Value, key: &str) -> Result<String, String> {
    let Some(v) = ctx.with.get(key) else {
        return Ok(String::new());
    };
    resolve_value(ctx, input, v).map(|r| match r {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    })
}

/// Read a raw JSON argument from the `with` map (defaults to an empty object).
pub fn arg_value(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
    key: &str,
) -> Result<serde_json::Value, String> {
    let Some(v) = ctx.with.get(key) else {
        return Ok(serde_json::json!({}));
    };
    resolve_value(ctx, input, v)
}

/// Load a content-type schema by uid.
pub fn load_schema(ctx: &AppContext, uid: &str) -> Result<Schema, String> {
    ctx.schema_cache
        .get(&Uid::new(uid))
        .ok_or_else(|| format!("content-type `{uid}` not found"))
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Execute an OWS function over a single input item. Returns the output value.
pub async fn execute_function(
    ctx: &FunctionRunContext<'_>,
    function_name: &str,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match function_name {
        function::GET_CONTENT => execute_get_content(ctx, input).await,
        function::FIND_CONTENT => execute_find_content(ctx, input).await,
        function::QUERY_CONTENT => execute_query_content(ctx, input).await,
        function::CREATE_CONTENT => execute_create_content(ctx, input).await,
        function::UPDATE_CONTENT => execute_update_content(ctx, input).await,
        function::DELETE_CONTENT => execute_delete_content(ctx, input).await,
        function::PUBLISH_CONTENT => execute_publish_content(ctx, input, true).await,
        function::UNPUBLISH_CONTENT => execute_publish_content(ctx, input, false).await,
        function::GET_MEDIA => execute_get_media(ctx, input).await,
        function::UPLOAD_MEDIA => execute_upload_media(ctx, input).await,
        function::TRANSFORM_DATA => execute_transform_data(ctx, input).await,
        function::JSON => execute_json_node(ctx, input).await,
        function::CSV => execute_csv_node(ctx, input).await,
        function::TRANSFORM => execute_transform(ctx, input).await,
        function::CODE => execute_code(ctx, input).await,
        function::EDIT_FIELDS => execute_edit_fields(ctx, input).await,
        function::HTTP_REQUEST | function::REST_API | function::WEBHOOK => execute_http(ctx, input).await,
        function::GRAPHQL => execute_graphql(ctx, input).await,
        function::DB_QUERY | function::POSTGRES => execute_database(ctx, input).await,
        function::REDIS => execute_redis(ctx, input).await,
        other => Err(format!("no executor registered for function '{other}'")),
    }
}

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

async fn execute_edit_fields(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let field = arg_str(ctx, input, "field")?;
    if field.is_empty() {
        return Err("Edit Fields: missing field".to_string());
    }
    let operation = ctx
        .with
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("set")
        .to_string();
    let mut obj = input.as_object().cloned().unwrap_or_default();
    if operation == "delete" {
        obj.shift_remove(&field);
    } else {
        let value = arg_value(ctx, input, "value")?;
        obj.insert(field.clone(), value);
    }
    Ok(serde_json::Value::Object(obj))
}

async fn execute_transform(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let expr = ctx
        .with
        .get("transformExpression")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Transform: missing expression".to_string())?;
    resolve_template(ctx, input, expr)
}

async fn execute_code(ctx: &FunctionRunContext<'_>, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let code = ctx.with.get("code").and_then(|v| v.as_str()).unwrap_or("return input;");
    let trimmed = code.trim();
    if let Some(body) = trimmed.strip_prefix("return ").and_then(|s| s.strip_suffix(';')) {
        return expression::eval_expression(body, &expr_ctx(ctx, input)).map_err(|e| e.to_string());
    }
    Ok(input.clone())
}

// ---------------------------------------------------------------------------
// CMS content / media
// ---------------------------------------------------------------------------

async fn execute_get_content(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let uid = arg_str(ctx, input, "contentType")?;
    if uid.is_empty() {
        return Err("Get Content: missing content type".to_string());
    }
    let schema = load_schema(ctx.app, &uid)?;
    let doc_id = arg_str(ctx, input, "documentId")?;
    let row = dml::find_one_by_document_id(&ctx.app.db, &schema, &doc_id)
        .await
        .map_err(|e| format!("Get Content: {e}"))?;
    Ok(row.unwrap_or(serde_json::Value::Null))
}

async fn execute_find_content(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let uid = arg_str(ctx, input, "contentType")?;
    if uid.is_empty() {
        return Err("Find Content: missing content type".to_string());
    }
    let schema = load_schema(ctx.app, &uid)?;
    let limit = ctx
        .with
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(10);
    let filters = arg_value(ctx, input, "filters")?;
    let params = build_query_params(&filters, limit);
    let (rows, _) = dml::query_rows(&ctx.app.db, ctx.app.db_backend(), &schema, &params)
        .await
        .map_err(|e| format!("Find Content: {e}"))?;
    Ok(serde_json::Value::Array(rows))
}

/// Build QueryParams from a simple `{ field: value }` map (equality filters).
fn build_query_params(filters: &serde_json::Value, limit: i64) -> api_types::QueryParams {
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

async fn execute_query_content(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let uid = arg_str(ctx, input, "contentType")?;
    if uid.is_empty() {
        return Err("Query Content: missing content type".to_string());
    }
    let schema = load_schema(ctx.app, &uid)?;
    let query = arg_value(ctx, input, "query")?;
    let limit = query.get("limit").and_then(|v| v.as_i64()).unwrap_or(25);
    let filters = query.get("filters").cloned().unwrap_or(serde_json::json!({}));
    let params = build_query_params(&filters, limit);
    let (rows, _) = dml::query_rows(&ctx.app.db, ctx.app.db_backend(), &schema, &params)
        .await
        .map_err(|e| format!("Query Content: {e}"))?;
    Ok(serde_json::Value::Array(rows))
}

async fn execute_create_content(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let uid = arg_str(ctx, input, "contentType")?;
    if uid.is_empty() {
        return Err("Create Content: missing content type".to_string());
    }
    let schema = load_schema(ctx.app, &uid)?;
    let data = arg_value(ctx, input, "data")?;
    let row = dml::insert_one(&ctx.app.db, &schema, &data, None)
        .await
        .map_err(|e| format!("Create Content: {e}"))?;
    Ok(row)
}

async fn execute_update_content(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let uid = arg_str(ctx, input, "contentType")?;
    if uid.is_empty() {
        return Err("Update Content: missing content type".to_string());
    }
    let schema = load_schema(ctx.app, &uid)?;
    let doc_id = arg_str(ctx, input, "documentId")?;
    let data = arg_value(ctx, input, "data")?;
    let row = dml::update_one(&ctx.app.db, &schema, &doc_id, &data, None)
        .await
        .map_err(|e| format!("Update Content: {e}"))?;
    Ok(row)
}

async fn execute_delete_content(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let uid = arg_str(ctx, input, "contentType")?;
    if uid.is_empty() {
        return Err("Delete Content: missing content type".to_string());
    }
    let schema = load_schema(ctx.app, &uid)?;
    let doc_id = arg_str(ctx, input, "documentId")?;
    dml::delete_one(&ctx.app.db, &schema, &doc_id)
        .await
        .map_err(|e| format!("Delete Content: {e}"))?;
    Ok(input.clone())
}

async fn execute_publish_content(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
    publish: bool,
) -> Result<serde_json::Value, String> {
    let uid = arg_str(ctx, input, "contentType")?;
    if uid.is_empty() {
        return Err("content type required".to_string());
    }
    let schema = load_schema(ctx.app, &uid)?;
    if !schema.draft_and_publish() {
        return Err("Draft & Publish is not enabled for this content-type".to_string());
    }
    let doc_id = arg_str(ctx, input, "documentId")?;
    let existing = dml::find_one_by_document_id(&ctx.app.db, &schema, &doc_id)
        .await
        .map_err(|e| format!("{e}"))?
        .ok_or_else(|| format!("entry {doc_id} not found"))?;
    let mut data = existing.clone();
    if let Some(obj) = data.as_object_mut() {
        obj.insert(
            "publicationState".into(),
            serde_json::json!(if publish { "published" } else { "draft" }),
        );
        if publish {
            obj.insert("publishedAt".into(), serde_json::json!(chrono::Utc::now().to_rfc3339()));
        }
        obj.insert("documentId".into(), serde_json::json!(uuid::Uuid::new_v4().to_string()));
    }
    let row = dml::insert_one(&ctx.app.db, &schema, &data, None)
        .await
        .map_err(|e| format!("{e}"))?;
    dml::delete_one(&ctx.app.db, &schema, &doc_id)
        .await
        .map_err(|e| format!("{e}"))?;
    Ok(row)
}

async fn execute_get_media(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = arg_str(ctx, input, "id")?
        .parse::<i64>()
        .map_err(|_| "Get Media: id must be a number".to_string())?;
    let file = upload_file::Entity::find_by_id(id)
        .one(&ctx.app.db)
        .await
        .map_err(|e| format!("Get Media: {e}"))?;
    Ok(if let Some(f) = file {
        serde_json::json!({
            "id": f.id, "name": f.name, "mime": f.mime, "url": f.url,
            "size": f.size, "hash": f.hash, "ext": f.ext, "alternativeText": f.alternative_text,
        })
    } else {
        serde_json::Value::Null
    })
}

async fn execute_upload_media(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let filename = arg_str(ctx, input, "filename")?;
    if filename.is_empty() {
        return Err("Upload Media: missing filename".to_string());
    }
    let data_value = arg_value(ctx, input, "data")?;
    let bytes = data_value
        .as_str()
        .and_then(|s| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok())
        .ok_or_else(|| "Upload Media: no base64 data provided".to_string())?;
    let mime = "application/octet-stream".to_string();
    let file = crate::media::media_upload(ctx.app, &filename, &mime, &bytes)
        .await
        .map_err(|e| format!("Upload Media: {e}"))?;
    Ok(serde_json::json!({
        "id": file.id, "name": file.name, "mime": file.mime, "url": file.url,
        "size": file.size, "hash": file.hash, "ext": file.ext, "alternativeText": file.alternative_text,
    }))
}

async fn execute_transform_data(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let direction = ctx
        .with
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("jsonToCsv")
        .to_string();
    if direction == "csvToJson" {
        let csv_text = arg_str(ctx, input, "csvData")?;
        Ok(serde_json::json!({ "json": csv_to_json(&csv_text)? }))
    } else {
        let rows: Vec<serde_json::Value> = input
            .as_object()
            .map(|o| vec![serde_json::Value::Object(o.clone())])
            .unwrap_or_else(|| input.as_array().cloned().unwrap_or_else(|| vec![input.clone()]));
        let csv = json_to_csv(&rows)?;
        Ok(serde_json::json!({ "csv": csv }))
    }
}

fn json_to_csv(rows: &[serde_json::Value]) -> Result<String, String> {
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
            .map(|c| esc(&stringify(&row.get(c).cloned().unwrap_or(serde_json::Value::Null))))
            .collect();
        out.push_str(&vals.join(","));
        out.push('\n');
    }
    Ok(out)
}

fn csv_to_json(csv: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut lines = csv.lines();
    let header: Vec<String> = lines
        .next()
        .ok_or_else(|| "CSV: empty input".to_string())?
        .split(',')
        .map(|s| s.trim_matches('"').to_string())
        .collect();
    let mut out = Vec::new();
    for line in lines {
        let fields: Vec<String> = line
            .split(',')
            .map(|s| s.trim_matches('"').to_string())
            .collect();
        let mut obj = serde_json::Map::new();
        for (i, col) in header.iter().enumerate() {
            obj.insert(col.clone(), serde_json::json!(fields.get(i).cloned().unwrap_or_default()));
        }
        out.push(serde_json::Value::Object(obj));
    }
    Ok(out)
}

fn stringify(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

async fn execute_json_node(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    arg_value(ctx, input, "json")
}

async fn execute_csv_node(
    ctx: &FunctionRunContext<'_>,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let csv = arg_str(ctx, input, "csv")?;
    Ok(serde_json::json!({ "csv": csv }))
}

// ---------------------------------------------------------------------------
// Integrations
// ---------------------------------------------------------------------------

/// Resolve the node's configured credential data (if any) and return it.
async fn resolve_credential(
    ctx: &FunctionRunContext<'_>,
    name: &str,
) -> Result<Option<serde_json::Value>, String> {
    let credential_id = ctx
        .with
        .get("credentialId")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if credential_id == 0 {
        return Ok(None);
    }
    let data = crate::workflow::credentials::credential_get_data(ctx.app, credential_id)
        .await
        .map_err(|e| format!("credential error: {e}"))?;
    Ok(Some(data))
}

async fn execute_http(ctx: &FunctionRunContext<'_>, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = arg_str(ctx, input, "url")?;
    if url.is_empty() {
        return Err("HTTP: missing url".to_string());
    }
    let method = ctx
        .with
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_uppercase();
    let headers = arg_value(ctx, input, "headers")?;
    let body = ctx.with.get("body").cloned();
    let auth = ctx.with.get("authentication").and_then(|v| v.as_str()).unwrap_or("none").to_string();

    let client = reqwest::Client::new();
    let mut req = client.request(
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "HTTP: invalid method".to_string())?,
        &url,
    );
    if let Some(obj) = headers.as_object() {
        for (k, v) in obj {
            req = req.header(k, stringify(v));
        }
    }
    if auth == "predefined" {
        if let Some(cred) = resolve_credential(ctx, "httpApi").await? {
            let name = cred.get("headerName").and_then(|v| v.as_str()).unwrap_or("Authorization").to_string();
            let value = cred.get("headerValue").and_then(|v| v.as_str()).unwrap_or("").to_string();
            req = req.header(name, value);
        }
    }
    if let Some(b) = &body {
        let b = resolve_value(ctx, input, b)?;
        if !b.is_null() {
            req = req.json(&b);
        }
    }
    let resp = req.send().await.map_err(|e| format!("HTTP request failed: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| format!("HTTP body: {e}"))?;
    let parsed = serde_json::from_str::<serde_json::Value>(&text)
        .unwrap_or_else(|_| serde_json::Value::String(text));
    Ok(serde_json::json!({ "statusCode": status, "json": parsed }))
}

async fn execute_graphql(ctx: &FunctionRunContext<'_>, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = arg_str(ctx, input, "url")?;
    if url.is_empty() {
        return Err("GraphQL: missing url".to_string());
    }
    let query = arg_str(ctx, input, "query")?;
    let vars = arg_value(ctx, input, "variables")?;
    let client = reqwest::Client::new();
    let body = serde_json::json!({ "query": query, "variables": vars });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("GraphQL: {e}"))?;
    let status = resp.status().as_u16();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("GraphQL body: {e}"))?;
    Ok(serde_json::json!({ "statusCode": status, "json": json }))
}

async fn execute_database(ctx: &FunctionRunContext<'_>, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let sql = arg_str(ctx, input, "query")?;
    if sql.trim().is_empty() {
        return Err("Database Query: empty SQL".to_string());
    }
    let res = ctx
        .app
        .db
        .execute_unprepared(&sql)
        .await
        .map_err(|e| format!("Database Query: {e}"))?;
    Ok(serde_json::json!({ "rowsAffected": res.rows_affected() }))
}

async fn execute_redis(ctx: &FunctionRunContext<'_>, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let operation = ctx.with.get("operation").and_then(|v| v.as_str()).unwrap_or("get").to_string();
    let key = arg_str(ctx, input, "key")?;
    match operation.as_str() {
        "set" => {
            let value = arg_value(ctx, input, "value")?;
            let now = chrono::Utc::now();
            let existing = core_store::Entity::find()
                .filter(core_store::Column::Key.eq(key.clone()))
                .one(&ctx.app.db)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(e) = existing {
                let mut am: core_store::ActiveModel = e.into();
                am.value_json = Set(Some(value.clone()));
                am.update(&ctx.app.db).await.map_err(|e| e.to_string())?;
            } else {
                core_store::ActiveModel {
                    key: Set(key.clone()),
                    value_json: Set(Some(value.clone())),
                    store_type: Set(Some("workflow-redis".into())),
                    environment: Set(None),
                    tag: Set(None),
                    ..Default::default()
                }
                .insert(&ctx.app.db)
                .await
                .map_err(|e| e.to_string())?;
            }
            Ok(serde_json::json!({ "key": key, "operation": "set", "value": value }))
        }
        "del" => {
            core_store::Entity::delete_many()
                .filter(core_store::Column::Key.eq(key.clone()))
                .exec(&ctx.app.db)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "key": key, "operation": "del" }))
        }
        _ => {
            let v = core_store::Entity::find()
                .filter(core_store::Column::Key.eq(key.clone()))
                .one(&ctx.app.db)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "key": key, "operation": "get", "value": v.and_then(|r| r.value_json).unwrap_or(serde_json::Value::Null) }))
        }
    }
}

/// Evaluate a switch condition (jq-ish) to a boolean.
pub fn eval_condition(ctx: &FunctionRunContext<'_>, input: &serde_json::Value, expr: &str) -> Result<bool, String> {
    let norm = normalize_ows_expr(expr);
    if norm.trim().is_empty() {
        return Ok(false);
    }
    let value = expression::evaluate(&norm, &expr_ctx(ctx, input)).map_err(|e| e.to_string())?;
    Ok(expression::truthy(&value))
}
