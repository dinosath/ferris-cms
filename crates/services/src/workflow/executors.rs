//! Node executors — the runtime implementation for each node type.
//!
//! This is the second half of the node system: `workflow::node` provides the
//! static *definition* (metadata) and this module provides the *runtime* that
//! runs against the CMS database, external HTTP services and expression data.
//! Adding a node = a definition + an `execute_*` arm here (plus optionally a
//! credential type in `credentials`).

use crate::AppContext;
use core_domain::Uid;
use core_schema::Schema;
use db::entities::{core_store, upload_file};
use dynamic_store::dml;
use indexmap::IndexMap;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use ::workflow::expression;
use ::workflow::model::{NodeCredentialRef, Workflow as WorkflowModel, WorkflowNode};

/// Runtime context passed to every executor.
pub struct NodeRunContext<'a> {
    pub app: &'a AppContext,
    pub workflow: &'a WorkflowModel,
    pub node: &'a WorkflowNode,
    /// Prior node outputs keyed by node *name* → output object (`{ json }`).
    pub node_outputs: &'a HashMap<String, serde_json::Value>,
    pub env: &'a HashMap<String, String>,
    pub execution_id: i64,
    pub workflow_json: serde_json::Value,
    pub execution_json: serde_json::Value,
}

/// Output of a node execution: `port -> items`.
pub type NodeResult = IndexMap<String, Vec<serde_json::Value>>;

/// Build an expression context for the given item.
pub fn expr_ctx(ctx: &NodeRunContext<'_>, item: &serde_json::Value) -> expression::Context {
    let mut c = expression::Context::minimal();
    c.json = item.clone();
    c.nodes = ctx.node_outputs.clone();
    c.workflow = ctx.workflow_json.clone();
    c.execution = ctx.execution_json.clone();
    c.env = ctx.env.clone();
    c
}

/// Resolve a template string (which may contain `{{ }}` expressions) against
/// an item.
pub fn resolve_template(
    ctx: &NodeRunContext<'_>,
    item: &serde_json::Value,
    template: &str,
) -> Result<serde_json::Value, String> {
    if !::workflow::expression::contains_expression(template) {
        return Ok(serde_json::Value::String(template.to_string()));
    }
    expression::evaluate(template, &expr_ctx(ctx, item)).map_err(|e| e.to_string())
}

/// Resolve a configured value to JSON. Strings containing expressions are
/// evaluated; other JSON values are used as-is.
pub fn resolve_value(
    ctx: &NodeRunContext<'_>,
    item: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match value {
        serde_json::Value::String(s) => resolve_template(ctx, item, s),
        other => Ok(other.clone()),
    }
}

/// Convenience: single-output helper.
fn main_result(items: Vec<serde_json::Value>) -> NodeResult {
    let mut m = IndexMap::new();
    m.insert("main".to_string(), items);
    m
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

/// Execute a node type over the given input items.
pub async fn execute_node(
    ctx: &NodeRunContext<'_>,
    node_type: &str,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    match node_type {
        // -- triggers (produce initial items from trigger data) --
        "manualTrigger" => Ok(main_result(items.to_vec())),
        "scheduleTrigger" => Ok(main_result(vec![serde_json::json!({
            "trigger": "schedule",
            "now": ctx.execution_json.get("startedAt").cloned().unwrap_or(serde_json::Value::Null),
        })])),
        "webhookTrigger" | "httpTrigger" | "workflowTrigger" => Ok(main_result(items.to_vec())),
        "contentCreated"
        | "contentUpdated"
        | "contentPublished"
        | "contentDeleted"
        | "mediaUploaded"
        | "userCreated" => Ok(main_result(items.to_vec())),

        // -- core --
        "noop" => Ok(main_result(items.to_vec())),
        "set" => execute_set(ctx, items).await,
        "editFields" => execute_edit_fields(ctx, items).await,
        "transform" => execute_transform(ctx, items).await,
        "code" => execute_code(ctx, items).await,

        // -- logic --
        "if" => execute_if(ctx, items).await,
        "switch" => execute_switch(ctx, items).await,
        "merge" => execute_merge(ctx, items).await,
        "split" => execute_split(ctx, items).await,
        "loop" => execute_loop(ctx, items).await,
        "forEach" => Ok(main_result(items.to_vec())),
        "filter" => execute_filter(ctx, items).await,
        "sort" => execute_sort(ctx, items).await,
        "limit" => execute_limit(ctx, items).await,
        "wait" => execute_wait(ctx, items).await,

        // -- data (CMS) --
        "getContent" => execute_get_content(ctx, items).await,
        "findContent" => execute_find_content(ctx, items).await,
        "queryContent" => execute_query_content(ctx, items).await,
        "createContent" => execute_create_content(ctx, items).await,
        "updateContent" => execute_update_content(ctx, items).await,
        "deleteContent" => execute_delete_content(ctx, items).await,
        "publishContent" => execute_publish_content(ctx, items, true).await,
        "unpublishContent" => execute_publish_content(ctx, items, false).await,
        "getMedia" => execute_get_media(ctx, items).await,
        "uploadMedia" => execute_upload_media(ctx, items).await,
        "transformData" => execute_transform_data(ctx, items).await,
        "jsonNode" => execute_json_node(ctx, items).await,
        "csvNode" => execute_csv_node(ctx, items).await,

        // -- integrations --
        "httpRequest" | "webhook" | "restApi" => execute_http(ctx, items).await,
        "graphqlRequest" => execute_graphql(ctx, items).await,
        "databaseQuery" | "postgres" => execute_database(ctx, items).await,
        "redis" => execute_redis(ctx, items).await,

        other => Err(format!("no executor registered for node type '{other}'")),
    }
}

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

async fn execute_set(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let field = ctx
        .node
        .param_str("field")
        .ok_or_else(|| "Set: missing field".to_string())?;
    let value_template = ctx.node.param_str("value").unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let mut obj = item.as_object().cloned().unwrap_or_default();
        let value = resolve_template(ctx, item, &value_template)?;
        obj.insert(field.clone(), value);
        out.push(serde_json::Value::Object(obj));
    }
    Ok(main_result(out))
}

async fn execute_edit_fields(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let field = ctx
        .node
        .param_str("field")
        .ok_or_else(|| "Edit Fields: missing field".to_string())?;
    let operation = ctx.node.param_str("operation").unwrap_or_else(|| "set".into());
    let value_template = ctx.node.param_str("value").unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let mut obj = item.as_object().cloned().unwrap_or_default();
        if operation == "delete" {
            obj.shift_remove(&field);
        } else {
            let value = resolve_template(ctx, item, &value_template)?;
            obj.insert(field.clone(), value);
        }
        out.push(serde_json::Value::Object(obj));
    }
    Ok(main_result(out))
}

async fn execute_transform(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let expr = ctx
        .node
        .param_str("transformExpression")
        .ok_or_else(|| "Transform: missing expression".to_string())?;
    let mut out = Vec::new();
    for item in items {
        let value = resolve_template(ctx, item, &expr)?;
        out.push(value);
    }
    Ok(main_result(out))
}

async fn execute_code(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let code = ctx.node.param_str("code").unwrap_or_else(|| "return item.json;".into());
    let trimmed = code.trim();
    let mut out = Vec::new();
    for item in items {
        let mut value = item.clone();
        if let Some(body) = trimmed.strip_prefix("return ").and_then(|s| s.strip_suffix(';')) {
            // `return <expr>;` — evaluate the expression with `item.json` as $json.
            let ec = expr_ctx(ctx, item);
            value = expression::eval_expression(body, &ec).map_err(|e| e.to_string())?;
        }
        out.push(value);
    }
    Ok(main_result(out))
}

// ---------------------------------------------------------------------------
// Logic
// ---------------------------------------------------------------------------

fn eval_condition(ctx: &NodeRunContext<'_>, item: &serde_json::Value) -> Result<bool, String> {
    let operator = ctx.node.param_str("operator").unwrap_or_else(|| "==".into());
    let value1 = ctx.node.param_str("value1").unwrap_or_default();
    let v1 = resolve_template(ctx, item, &value1)?;
    // If a full condition expression is provided, use it.
    if let Some(cond) = ctx.node.param_str("condition") {
        if !cond.trim().is_empty() && !cond.contains("== ") && !cond.contains(">") {
            // only treat as expression when it is not already an operator form
        }
    }
    let result = match operator.as_str() {
        "true" => ::workflow::expression::truthy(&v1),
        "false" => !::workflow::expression::truthy(&v1),
        "contains" => {
            let v1s = stringify_value(&v1);
            let value2 = ctx.node.param_str("value2").unwrap_or_default();
            let v2 = resolve_template(ctx, item, &value2)?;
            v1s.contains(&stringify_value(&v2))
        }
        op => {
            let value2 = ctx.node.param_str("value2").unwrap_or_default();
            let v2 = resolve_template(ctx, item, &value2)?;
            compare_values(op, &v1, &v2)
        }
    };
    Ok(result)
}

fn stringify_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn compare_values(op: &str, a: &serde_json::Value, b: &serde_json::Value) -> bool {
    let an = a.as_f64();
    let bn = b.as_f64();
    match op {
        "==" | "=" => match (an, bn) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
        "!=" => match (an, bn) {
            (Some(x), Some(y)) => x != y,
            _ => a != b,
        },
        ">" => an.zip(bn).map(|(x, y)| x > y).unwrap_or(false),
        ">=" => an.zip(bn).map(|(x, y)| x >= y).unwrap_or(false),
        "<" => an.zip(bn).map(|(x, y)| x < y).unwrap_or(false),
        "<=" => an.zip(bn).map(|(x, y)| x <= y).unwrap_or(false),
        _ => false,
    }
}

async fn execute_if(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let mut t = Vec::new();
    let mut f = Vec::new();
    for item in items {
        if eval_condition(ctx, item)? {
            t.push(item.clone());
        } else {
            f.push(item.clone());
        }
    }
    let mut m = IndexMap::new();
    m.insert("true".to_string(), t);
    m.insert("false".to_string(), f);
    Ok(m)
}

async fn execute_switch(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let value_expr = ctx.node.param_str("value").unwrap_or_default();
    let cases = ctx
        .node
        .parameters
        .get("cases")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(["0", "1"]));
    let case_values: Vec<String> = cases
        .as_array()
        .map(|a| a.iter().map(stringify_value).collect())
        .unwrap_or_default();
    let mut outputs: IndexMap<String, Vec<serde_json::Value>> = IndexMap::new();
    for (i, _) in case_values.iter().enumerate() {
        outputs.insert(i.to_string(), Vec::new());
    }
    for item in items {
        let v = resolve_template(ctx, item, &value_expr)?;
        let s = stringify_value(&v);
        let idx = case_values.iter().position(|c| c == &s);
        if let Some(i) = idx {
            outputs.get_mut(&i.to_string()).unwrap().push(item.clone());
        } else {
            outputs.entry("0".to_string()).or_default().push(item.clone());
        }
    }
    Ok(outputs)
}

async fn execute_merge(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    // Merge combines the `main` items (already concatenated by the engine) —
    // the two inputs are delivered as a single merged list; dedupe not needed.
    let _ = ctx;
    Ok(main_result(items.to_vec()))
}

async fn execute_split(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let field = ctx
        .node
        .param_str("field")
        .unwrap_or_else(|| "items".into());
    let mut out = Vec::new();
    for item in items {
        let value = resolve_template(ctx, item, &field)?;
        match value {
            serde_json::Value::Array(arr) => {
                for el in arr {
                    out.push(el);
                }
            }
            other => out.push(other),
        }
    }
    Ok(main_result(out))
}

async fn execute_loop(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let count = ctx
        .node
        .param_i64("count")
        .unwrap_or(1)
        .max(0) as usize;
    let mut out = Vec::new();
    for item in items {
        for i in 0..count {
            let mut obj = item.as_object().cloned().unwrap_or_default();
            obj.insert("iteration".into(), serde_json::json!(i));
            out.push(serde_json::Value::Object(obj));
        }
    }
    Ok(main_result(out))
}

async fn execute_filter(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let mut out = Vec::new();
    for item in items {
        if eval_condition(ctx, item)? {
            out.push(item.clone());
        }
    }
    Ok(main_result(out))
}

async fn execute_sort(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let field = ctx
        .node
        .param_str("field")
        .unwrap_or_else(|| "id".into());
    let desc = ctx.node.param_str("order").unwrap_or_else(|| "asc".into()) == "desc";
    let mut sorted = items.to_vec();
    sorted.sort_by(|a, b| {
        let av = a.get(&field).cloned().unwrap_or(serde_json::Value::Null);
        let bv = b.get(&field).cloned().unwrap_or(serde_json::Value::Null);
        let ord = compare_json(&av, &bv);
        if desc {
            ord.reverse()
        } else {
            ord
        }
    });
    Ok(main_result(sorted))
}

fn compare_json(a: &serde_json::Value, b: &serde_json::Value) -> std::cmp::Ordering {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => stringify_value(a).cmp(&stringify_value(b)),
    }
}

async fn execute_limit(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let n = ctx.node.param_i64("limit").unwrap_or(10).max(0) as usize;
    Ok(main_result(items.iter().take(n).cloned().collect()))
}

async fn execute_wait(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let amount = ctx.node.param_i64("amount").unwrap_or(0).max(0) as u64;
    let unit = ctx.node.param_str("unit").unwrap_or_else(|| "seconds".into());
    let secs = match unit.as_str() {
        "minutes" => amount * 60,
        "hours" => amount * 3600,
        _ => amount,
    };
    if secs > 0 {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
    }
    Ok(main_result(items.to_vec()))
}

// ---------------------------------------------------------------------------
// Data (CMS)
// ---------------------------------------------------------------------------

async fn execute_get_content(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let uid = ctx
        .node
        .param_str("contentType")
        .ok_or_else(|| "Get Content: missing content type".to_string())?;
    let schema = load_schema(ctx.app, &uid)?;
    let mut out = Vec::new();
    for item in items {
        let doc_id = resolve_template(ctx, item, &ctx.node.param_str("documentId").unwrap_or_default())?
            .as_str()
            .unwrap_or_default()
            .to_string();
        let row = dml::find_one_by_document_id(&ctx.app.db, &schema, &doc_id)
            .await
            .map_err(|e| format!("Get Content: {e}"))?;
        if let Some(r) = row {
            out.push(r);
        }
    }
    Ok(main_result(out))
}

async fn execute_find_content(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let uid = ctx
        .node
        .param_str("contentType")
        .ok_or_else(|| "Find Content: missing content type".to_string())?;
    let schema = load_schema(ctx.app, &uid)?;
    let limit = ctx.node.param_i64("limit").unwrap_or(10);
    let mut out = Vec::new();
    for item in items {
        let filters = ctx
            .node
            .parameters
            .get("filters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let filters = resolve_value(ctx, item, &filters)?;
        let params = build_query_params(&filters, limit);
        let (rows, _) = dml::query_rows(&ctx.app.db, ctx.app.db_backend(), &schema, &params)
            .await
            .map_err(|e| format!("Find Content: {e}"))?;
        for r in rows {
            out.push(r);
        }
    }
    Ok(main_result(out))
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
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let uid = ctx
        .node
        .param_str("contentType")
        .ok_or_else(|| "Query Content: missing content type".to_string())?;
    let schema = load_schema(ctx.app, &uid)?;
    let query_json = ctx
        .node
        .parameters
        .get("query")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut out = Vec::new();
    for item in items {
        let query = resolve_value(ctx, item, &query_json)?;
        let limit = query.get("limit").and_then(|v| v.as_i64()).unwrap_or(25);
        let filters = query.get("filters").cloned().unwrap_or(serde_json::json!({}));
        let params = build_query_params(&filters, limit);
        let (rows, _) = dml::query_rows(&ctx.app.db, ctx.app.db_backend(), &schema, &params)
            .await
            .map_err(|e| format!("Query Content: {e}"))?;
        for r in rows {
            out.push(r);
        }
    }
    Ok(main_result(out))
}

async fn execute_create_content(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let uid = ctx
        .node
        .param_str("contentType")
        .ok_or_else(|| "Create Content: missing content type".to_string())?;
    let schema = load_schema(ctx.app, &uid)?;
    let data_json = ctx
        .node
        .parameters
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut out = Vec::new();
    for item in items {
        let data = resolve_value(ctx, item, &data_json)?;
        let row = dml::insert_one(&ctx.app.db, &schema, &data, None)
            .await
            .map_err(|e| format!("Create Content: {e}"))?;
        out.push(row);
    }
    Ok(main_result(out))
}

async fn execute_update_content(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let uid = ctx
        .node
        .param_str("contentType")
        .ok_or_else(|| "Update Content: missing content type".to_string())?;
    let schema = load_schema(ctx.app, &uid)?;
    let data_json = ctx
        .node
        .parameters
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut out = Vec::new();
    for item in items {
        let doc_id = resolve_template(ctx, item, &ctx.node.param_str("documentId").unwrap_or_default())?
            .as_str()
            .unwrap_or_default()
            .to_string();
        let data = resolve_value(ctx, item, &data_json)?;
        let row = dml::update_one(&ctx.app.db, &schema, &doc_id, &data, None)
            .await
            .map_err(|e| format!("Update Content: {e}"))?;
        out.push(row);
    }
    Ok(main_result(out))
}

async fn execute_delete_content(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let uid = ctx
        .node
        .param_str("contentType")
        .ok_or_else(|| "Delete Content: missing content type".to_string())?;
    let schema = load_schema(ctx.app, &uid)?;
    let mut out = Vec::new();
    for item in items {
        let doc_id = resolve_template(ctx, item, &ctx.node.param_str("documentId").unwrap_or_default())?
            .as_str()
            .unwrap_or_default()
            .to_string();
        dml::delete_one(&ctx.app.db, &schema, &doc_id)
            .await
            .map_err(|e| format!("Delete Content: {e}"))?;
        out.push(item.clone());
    }
    Ok(main_result(out))
}

async fn execute_publish_content(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
    publish: bool,
) -> Result<NodeResult, String> {
    let uid = ctx
        .node
        .param_str("contentType")
        .ok_or_else(|| "content type required".to_string())?;
    let schema = load_schema(ctx.app, &uid)?;
    if !schema.draft_and_publish() {
        return Err("Draft & Publish is not enabled for this content-type".to_string());
    }
    let mut out = Vec::new();
    for item in items {
        let doc_id = resolve_template(ctx, item, &ctx.node.param_str("documentId").unwrap_or_default())?
            .as_str()
            .unwrap_or_default()
            .to_string();
        let existing = dml::find_one_by_document_id(&ctx.app.db, &schema, &doc_id)
            .await
            .map_err(|e| format!("{e}"))?
            .ok_or_else(|| format!("entry {doc_id} not found"))?;
        let mut data = existing.clone();
        if let Some(obj) = data.as_object_mut() {
            obj.insert("publicationState".into(), serde_json::json!(if publish { "published" } else { "draft" }));
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
        out.push(row);
    }
    Ok(main_result(out))
}

async fn execute_get_media(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let mut out = Vec::new();
    for item in items {
        let id = resolve_template(ctx, item, &ctx.node.param_str("id").unwrap_or_default())?
            .as_i64()
            .ok_or_else(|| "Get Media: id must be a number".to_string())?;
        let file = upload_file::Entity::find_by_id(id)
            .one(&ctx.app.db)
            .await
            .map_err(|e| format!("Get Media: {e}"))?;
        if let Some(f) = file {
            out.push(serde_json::json!({
                "id": f.id, "name": f.name, "mime": f.mime, "url": f.url,
                "size": f.size, "hash": f.hash, "ext": f.ext, "alternativeText": f.alternative_text,
            }));
        }
    }
    Ok(main_result(out))
}

async fn execute_upload_media(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let filename = ctx
        .node
        .param_str("filename")
        .ok_or_else(|| "Upload Media: missing filename".to_string())?;
    let app = ctx.app.clone();
    let mut out: Vec<serde_json::Value> = Vec::new();
    for item in items {
        let name = resolve_template(ctx, item, &filename)?.as_str().unwrap_or("file").to_string();
        let data_expr = ctx.node.param_str("data").unwrap_or_default();
        let data_value = resolve_template(ctx, item, &data_expr)?;
        let bytes = data_value
            .as_str()
            .and_then(|s| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok())
            .ok_or_else(|| "Upload Media: no base64 data provided".to_string())?;
        let file = upload_media_file(&app, &name, bytes).await?;
        out.push(file);
    }
    Ok(main_result(out))
}

/// Perform the actual media upload for the Upload Media node. Uses
/// `crate::media::media_upload`, which performs disk I/O.
async fn upload_media_file(
    app: &AppContext,
    name: &str,
    bytes: Vec<u8>,
) -> Result<serde_json::Value, String> {
    let mime = "application/octet-stream".to_string();
    let file = crate::media::media_upload(app, name, &mime, &bytes)
        .await
        .map_err(|e| format!("Upload Media: {e}"))?;
    Ok(serde_json::json!({
        "id": file.id, "name": file.name, "mime": file.mime, "url": file.url,
        "size": file.size, "hash": file.hash, "ext": file.ext, "alternativeText": file.alternative_text,
    }))
}

async fn execute_transform_data(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let direction = ctx
        .node
        .param_str("direction")
        .unwrap_or_else(|| "jsonToCsv".into());
    let mut out = Vec::new();
    for item in items {
        if direction == "csvToJson" {
            let csv_text = resolve_template(ctx, item, &ctx.node.param_str("csvData").unwrap_or_default())?
                .as_str()
                .unwrap_or_default()
                .to_string();
            out.push(serde_json::json!({ "json": csv_to_json(&csv_text)? }));
        } else {
            let rows: Vec<serde_json::Value> = item
                .as_object()
                .map(|o| vec![serde_json::Value::Object(o.clone())])
                .unwrap_or_else(|| {
                    item.as_array().cloned().unwrap_or_else(|| vec![item.clone()])
                });
            let csv = json_to_csv(&rows)?;
            out.push(serde_json::json!({ "csv": csv }));
        }
    }
    Ok(main_result(out))
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
            .map(|c| esc(&stringify_value(&row.get(c).cloned().unwrap_or(serde_json::Value::Null))))
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

async fn execute_json_node(
    ctx: &NodeRunContext<'_>,
    items: &[serde_json::Value],
) -> Result<NodeResult, String> {
    let json = ctx
        .node
        .parameters
        .get("json")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut out = Vec::new();
    for item in items {
        out.push(resolve_value(ctx, item, &json)?);
    }
    Ok(main_result(out))
}

async fn execute_csv_node(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let csv = ctx.node.param_str("csv").unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let text = resolve_template(ctx, item, &csv)?;
        out.push(serde_json::json!({ "csv": text }));
    }
    Ok(main_result(out))
}

// ---------------------------------------------------------------------------
// Integrations
// ---------------------------------------------------------------------------

/// Resolve the node's configured credential data (if any) and return it.
async fn resolve_credential(
    ctx: &NodeRunContext<'_>,
    name: &str,
) -> Result<Option<serde_json::Value>, String> {
    let Some(cr) = ctx.node.credentials.iter().find(|c: &&NodeCredentialRef| c.name == name) else {
        return Ok(None);
    };
    let data = crate::workflow::credentials::credential_get_data(ctx.app, cr.credential_id)
        .await
        .map_err(|e| format!("credential error: {e}"))?;
    Ok(Some(data))
}

async fn execute_http(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let url_t = ctx.node.param_str("url").ok_or_else(|| "HTTP: missing url".to_string())?;
    let method = ctx
        .node
        .param_str("method")
        .unwrap_or_else(|| "GET".into())
        .to_uppercase();
    let headers_json = ctx
        .node
        .parameters
        .get("headers")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let body_json = ctx.node.parameters.get("body").cloned();
    let auth = ctx.node.param_str("authentication").unwrap_or_else(|| "none".into());

    let client = reqwest::Client::new();
    let mut out = Vec::new();
    for item in items {
        let url = resolve_template(ctx, item, &url_t)?.as_str().unwrap_or("").to_string();
        let mut req = client.request(
            reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "HTTP: invalid method".to_string())?,
            &url,
        );
        let headers = resolve_value(ctx, item, &headers_json)?;
        if let Some(obj) = headers.as_object() {
            for (k, v) in obj {
                req = req.header(k, stringify_value(v));
            }
        }
        // Apply credential auth when requested.
        let mut credential_headers: Vec<(String, String)> = Vec::new();
        if auth == "predefined" {
            if let Some(cred) = resolve_credential(ctx, "httpApi").await? {
                let name = cred.get("headerName").and_then(|v| v.as_str()).unwrap_or("Authorization").to_string();
                let value = cred.get("headerValue").and_then(|v| v.as_str()).unwrap_or("").to_string();
                credential_headers.push((name, value));
            }
        }
        for (k, v) in &credential_headers {
            req = req.header(k, v);
        }
        if let Some(b) = &body_json {
            let b = resolve_value(ctx, item, b)?;
            if !b.is_null() {
                req = req.json(&b);
            }
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| format!("HTTP body: {e}"))?;
        let parsed = serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|_| serde_json::Value::String(text));
        out.push(serde_json::json!({
            "statusCode": status,
            "json": parsed,
            "headers": serde_json::json!({}),
        }));
    }
    Ok(main_result(out))
}

async fn execute_graphql(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let url_t = ctx.node.param_str("url").ok_or_else(|| "GraphQL: missing url".to_string())?;
    let query = ctx.node.param_str("query").unwrap_or_default();
    let vars = ctx.node.parameters.get("variables").cloned().unwrap_or_else(|| serde_json::json!({}));
    let client = reqwest::Client::new();
    let mut out = Vec::new();
    for item in items {
        let url = resolve_template(ctx, item, &url_t)?.as_str().unwrap_or("").to_string();
        let body = serde_json::json!({ "query": query, "variables": resolve_value(ctx, item, &vars)? });
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("GraphQL: {e}"))?;
        let status = resp.status().as_u16();
        let json: serde_json::Value = resp.json().await.map_err(|e| format!("GraphQL body: {e}"))?;
        out.push(serde_json::json!({ "statusCode": status, "json": json }));
    }
    Ok(main_result(out))
}

async fn execute_database(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    let sql = ctx.node.param_str("query").unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let sql = resolve_template(ctx, item, &sql)?.as_str().unwrap_or("").to_string();
        if sql.trim().is_empty() {
            return Err("Database Query: empty SQL".to_string());
        }
        let res = ctx
            .app
            .db
            .execute_unprepared(&sql)
            .await
            .map_err(|e| format!("Database Query: {e}"))?;
        out.push(serde_json::json!({ "rowsAffected": res.rows_affected() }));
    }
    Ok(main_result(out))
}

async fn execute_redis(ctx: &NodeRunContext<'_>, items: &[serde_json::Value]) -> Result<NodeResult, String> {
    // Redis semantics are backed by the existing generic key-value `core_store`
    // table, so no external Redis server is required.
    let operation = ctx.node.param_str("operation").unwrap_or_else(|| "get".into());
    let mut out = Vec::new();
    for item in items {
        let key = resolve_template(ctx, item, &ctx.node.param_str("key").unwrap_or_default())?
            .as_str()
            .unwrap_or("")
            .to_string();
        match operation.as_str() {
            "set" => {
                let value = resolve_template(ctx, item, &ctx.node.param_str("value").unwrap_or_default())?;
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
                out.push(serde_json::json!({ "key": key, "operation": "set", "value": value }));
            }
            "del" => {
                core_store::Entity::delete_many()
                    .filter(core_store::Column::Key.eq(key.clone()))
                    .exec(&ctx.app.db)
                    .await
                    .map_err(|e| e.to_string())?;
                out.push(serde_json::json!({ "key": key, "operation": "del" }));
            }
            _ => {
                let v = core_store::Entity::find()
                    .filter(core_store::Column::Key.eq(key.clone()))
                    .one(&ctx.app.db)
                    .await
                    .map_err(|e| e.to_string())?;
                out.push(serde_json::json!({ "key": key, "operation": "get", "value": v.and_then(|r| r.value_json).unwrap_or(serde_json::Value::Null) }));
            }
        }
    }
    Ok(main_result(out))
}

