//! End-to-end integration test for the OWS workflow engine against the Axum
//! router in-memory. Covers workflow CRUD, activation, manual execution,
//! execution persistence (with task runs), import/export, webhook triggers,
//! credentials, and permission enforcement.

use api_rest::{build_router, AppState};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use db::{seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{load_schema_cache, AppConfig};
use std::sync::Arc;
use tower::ServiceExt;

fn app_config() -> AppConfig {
    AppConfig {
        db_driver: "sqlite".into(),
        jwt_secret: "test-secret".into(),
        jwt_expiry_secs: 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::temp_dir()
            .join("ferris-workflow-test")
            .display()
            .to_string(),
    }
}

async fn setup() -> (axum::Router, Arc<AppState>) {
    let base = std::env::temp_dir();
    std::fs::create_dir_all(&base).unwrap();
    let db_path = base.join(format!(
        "ferris-wf-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&db_path, b"").unwrap();
    let db_url = format!("sqlite://{}", db_path.display());
    let db = db::connect(&db_url).await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let state = Arc::new(AppState::new(db.clone(), app_config()));
    load_schema_cache(&db, &state.ctx.schema_cache)
        .await
        .unwrap();
    let _ = state.ctx.init_rbac().await;
    (build_router(state.clone()), state)
}

fn json_request(
    method: &str,
    uri: &str,
    body: serde_json::Value,
    token: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("request builds")
}

fn empty_request(method: &str, uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    builder.body(Body::empty()).expect("request builds")
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

/// Build an OWS workflow document with Set → JSON output tasks.
fn sample_workflow(id: i64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "active": false,
        "version": 1,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "definition": {
            "document": {
                "dsl": "1.0.0",
                "namespace": "default",
                "name": "Demo Workflow",
                "version": "1.0.0",
                "summary": "A demo workflow"
            },
            "do": [
                { "setGreeting": { "set": { "greeting": "Hello World" } } },
                { "output": { "call": "data.json", "with": { "json": { "greeting": "${ .greeting }" } } } }
            ]
        }
    })
}

fn ows_header() -> serde_json::Value {
    serde_json::json!({
        "dsl": "1.0.0",
        "namespace": "default",
        "name": "w",
        "version": "1.0.0"
    })
}

#[tokio::test]
async fn workflow_full_lifecycle() {
    let (router, _state) = setup().await;

    // Register + login.
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"admin@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);
    let token = body_json(reg).await["data"]["token"]
        .as_str()
        .unwrap()
        .to_string();

    // 1. Node library is available (OWS function catalog).
    let lib = router
        .clone()
        .oneshot(empty_request("GET", "/admin/workflow-node-library", Some(&token)))
        .await
        .unwrap();
    assert_eq!(lib.status(), StatusCode::OK);
    let lib_json = body_json(lib).await;
    assert!(!lib_json["data"].as_array().unwrap().is_empty());
    let node_types: Vec<&str> = lib_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["nodeType"].as_str())
        .collect();
    assert!(node_types.contains(&"http.request"));
    assert!(node_types.contains(&"cms.getContent"));
    assert!(node_types.contains(&"set"));

    // 2. Create a workflow.
    let create = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/workflows",
            serde_json::json!({ "name": "Demo Workflow", "description": "A demo workflow" }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK, "create workflow");
    let wf_json = body_json(create).await;
    let wf_id = wf_json["data"]["id"].as_i64().expect("workflow id");
    assert_eq!(wf_json["data"]["definition"]["document"]["name"], "Demo Workflow");

    // 3. Save the full OWS definition.
    let def = sample_workflow(wf_id);
    let save = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/workflows/{wf_id}"),
            def.clone(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(save.status(), StatusCode::OK, "save workflow");
    let saved = body_json(save).await;
    assert_eq!(saved["data"]["version"], 2, "version bumped on save");
    assert_eq!(saved["data"]["definition"]["do"].as_array().unwrap().len(), 2);

    // 4. Get the workflow back.
    let get = router
        .clone()
        .oneshot(empty_request("GET", &format!("/admin/workflows/{wf_id}"), Some(&token)))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    let got = body_json(get).await;
    assert_eq!(got["data"]["definition"]["document"]["name"], "Demo Workflow");
    assert_eq!(got["data"]["definition"]["do"].as_array().unwrap().len(), 2);

    // 5. List workflows.
    let list = router
        .clone()
        .oneshot(empty_request("GET", "/admin/workflows", Some(&token)))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list_json = body_json(list).await;
    assert_eq!(list_json["data"].as_array().unwrap().len(), 1);
    assert_eq!(list_json["data"][0]["taskCount"], 2);

    // 6. Validate the workflow.
    let validate = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/workflows/{wf_id}/validate"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(validate.status(), StatusCode::OK);
    assert_eq!(body_json(validate).await["data"]["valid"], true);

    // 7. Execute manually.
    let exec = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/workflows/{wf_id}/execute"),
            serde_json::json!({ "data": { "name": "Ferris" } }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(exec.status(), StatusCode::OK, "execute workflow");
    let exec_json = body_json(exec).await;
    let exec_id = exec_json["data"]["executionId"].as_i64().expect("execution id");
    assert!(exec_id > 0);

    // Wait for the background execution to finish.
    let mut exec_detail = serde_json::Value::Null;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let detail = router
            .clone()
            .oneshot(empty_request("GET", &format!("/admin/executions/{exec_id}"), Some(&token)))
            .await
            .unwrap();
        exec_detail = body_json(detail).await;
        let status = exec_detail["data"]["status"].as_str().unwrap_or("running");
        if status != "running" && status != "waiting" {
            break;
        }
    }
    assert_eq!(exec_detail["data"]["status"], "success", "execution succeeded");
    let task_runs = exec_detail["nodeRuns"].as_array().unwrap();
    assert_eq!(task_runs.len(), 2, "two task runs");
    for run in task_runs {
        assert_eq!(run["status"], "success");
    }

    // 8. List executions.
    let execs = router
        .clone()
        .oneshot(empty_request("GET", &format!("/admin/executions?workflow_id={wf_id}"), Some(&token)))
        .await
        .unwrap();
    assert_eq!(execs.status(), StatusCode::OK);
    assert_eq!(body_json(execs).await["data"].as_array().unwrap().len(), 1);

    // 9. Activate the workflow.
    let activate = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/workflows/{wf_id}/activate"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(activate.status(), StatusCode::OK, "activate workflow");
    assert_eq!(body_json(activate).await["data"]["active"], true);

    // 10. Duplicate the workflow.
    let dup = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/workflows/{wf_id}/duplicate"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(dup.status(), StatusCode::OK, "duplicate workflow");
    assert_eq!(body_json(dup).await["data"]["active"], false, "copy is inactive");

    // 11. Export / import round-trip.
    let export = router
        .clone()
        .oneshot(empty_request("GET", &format!("/admin/workflows/{wf_id}/export"), Some(&token)))
        .await
        .unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    let exported = body_json(export).await;
    assert_eq!(exported["definition"]["document"]["name"], "Demo Workflow");

    let import = router
        .clone()
        .oneshot(json_request("POST", "/admin/workflows/import", exported, Some(&token)))
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK, "import workflow");
    assert_eq!(
        body_json(import).await["data"]["definition"]["do"].as_array().unwrap().len(),
        2
    );

    // 12. Credentials CRUD.
    let cred_types = router
        .clone()
        .oneshot(empty_request("GET", "/admin/workflow-credentials/types", Some(&token)))
        .await
        .unwrap();
    assert_eq!(cred_types.status(), StatusCode::OK);
    assert!(!body_json(cred_types).await["data"].as_array().unwrap().is_empty());

    let cred_create = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/workflow-credentials",
            serde_json::json!({
                "name": "My API Key",
                "credentialType": "httpApi",
                "data": { "headerName": "Authorization", "headerValue": "Bearer super-secret" }
            }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(cred_create.status(), StatusCode::OK, "create credential");
    let cred_json = body_json(cred_create).await;
    let cred_id = cred_json["data"]["id"].as_i64().unwrap();
    let cred_list = router
        .clone()
        .oneshot(empty_request("GET", "/admin/workflow-credentials", Some(&token)))
        .await
        .unwrap();
    let list_str = body_json(cred_list).await.to_string();
    assert!(!list_str.contains("super-secret"), "credential value not leaked");

    let cred_delete = router
        .clone()
        .oneshot(empty_request("DELETE", &format!("/admin/workflow-credentials/{cred_id}"), Some(&token)))
        .await
        .unwrap();
    assert_eq!(cred_delete.status(), StatusCode::OK, "delete credential");

    // 13. Workflow content types.
    let ct = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({ "schemas": [{
                "uid": "api::article.article", "kind": "collectionType",
                "info": {"singularName":"article","pluralName":"articles","displayName":"Article"},
                "attributes": {"title": {"type":"string"}}
            }] }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(ct.status(), StatusCode::OK, "create content type");
    let wct = router
        .clone()
        .oneshot(empty_request("GET", "/admin/workflow-content-types", Some(&token)))
        .await
        .unwrap();
    assert_eq!(wct.status(), StatusCode::OK);
    let wct_json = body_json(wct).await;
    assert!(
        wct_json["data"].as_array().unwrap().iter().any(|c| c["uid"] == "api::article.article"),
        "workflow content types includes article"
    );
}

#[tokio::test]
async fn workflow_permission_denied_for_unauthorized() {
    let (router, _state) = setup().await;
    let res = router
        .clone()
        .oneshot(empty_request("GET", "/admin/workflows", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn webhook_trigger_executes_workflow() {
    let (router, _state) = setup().await;
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"admin@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    let token = body_json(reg).await["data"]["token"].as_str().unwrap().to_string();

    let create = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/workflows",
            serde_json::json!({ "name": "Webhook WF" }),
            Some(&token),
        ))
        .await
        .unwrap();
    let wf_id = body_json(create).await["data"]["id"].as_i64().unwrap();

    let wf = serde_json::json!({
        "id": wf_id, "active": false, "version": 1,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
        "definition": {
            "document": ows_header(),
            "schedule": { "on": { "one": { "with": { "type": "webhook", "path": "/hook" } } } },
            "do": [
                { "echo": { "call": "data.json", "with": { "json": { "ok": true } } } }
            ]
        }
    });
    router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/workflows/{wf_id}"),
            wf,
            Some(&token),
        ))
        .await
        .unwrap();

    router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/workflows/{wf_id}/activate"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();

    let hook = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/workflow-hooks/hook",
            serde_json::json!({ "event": "ping" }),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(hook.status(), StatusCode::OK, "webhook triggers workflow");
    let exec_id = body_json(hook).await["data"]["executionId"].as_i64().unwrap();

    let mut status = String::new();
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let detail = router
            .clone()
            .oneshot(empty_request("GET", &format!("/admin/executions/{exec_id}"), Some(&token)))
            .await
            .unwrap();
        status = body_json(detail).await["data"]["status"].as_str().unwrap_or("running").to_string();
        if status != "running" {
            break;
        }
    }
    assert_eq!(status, "success", "webhook execution succeeded");
}

#[tokio::test]
async fn yaml_export_import_roundtrip() {
    let (router, _state) = setup().await;
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"admin@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    let token = body_json(reg).await["data"]["token"].as_str().unwrap().to_string();

    // Create + save an OWS workflow.
    let create = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/workflows",
            serde_json::json!({ "name": "Yaml WF" }),
            Some(&token),
        ))
        .await
        .unwrap();
    let wf_id = body_json(create).await["data"]["id"].as_i64().unwrap();

    let wf = serde_json::json!({
        "id": wf_id, "active": false, "version": 1,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
        "definition": {
            "document": ows_header(),
            "do": [
                { "setGreeting": { "set": { "greeting": "Hello from YAML" } } }
            ]
        }
    });
    router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/workflows/{wf_id}"),
            wf,
            Some(&token),
        ))
        .await
        .unwrap();

    // Export as YAML.
    let export = router
        .clone()
        .oneshot(empty_request("GET", &format!("/admin/workflows/{wf_id}/export?format=yaml"), Some(&token)))
        .await
        .unwrap();
    assert_eq!(export.status(), StatusCode::OK);
    let exported = body_json(export).await;
    assert_eq!(exported["format"], "yaml");
    let yaml_text = exported["data"].as_str().unwrap().to_string();
    assert!(yaml_text.contains("document:"), "exported YAML is well-formed");

    // Import from YAML text (sent as a JSON string body).
    let import = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/workflows/import",
            serde_json::json!(yaml_text),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK, "import YAML workflow");
    let imported = body_json(import).await;
    assert_eq!(imported["data"]["definition"]["do"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn cms_content_created_trigger_executes_workflow() {
    let (router, _state) = setup().await;
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"admin@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    let token = body_json(reg).await["data"]["token"].as_str().unwrap().to_string();

    let ct = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({ "schemas": [{
                "uid": "api::article.article", "kind": "collectionType",
                "info": {"singularName":"article","pluralName":"articles","displayName":"Article"},
                "options": {"draftAndPublish": true},
                "attributes": {
                    "title": {"type":"string", "required": true},
                    "featured": {"type":"boolean"}
                }
            }] }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(ct.status(), StatusCode::OK, "create article content type");

    let create = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/workflows",
            serde_json::json!({ "name": "On Article Created" }),
            Some(&token),
        ))
        .await
        .unwrap();
    let wf_id = body_json(create).await["data"]["id"].as_i64().unwrap();

    let wf = serde_json::json!({
        "id": wf_id, "active": false, "version": 1,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
        "definition": {
            "document": ows_header(),
            "schedule": { "on": { "one": { "with": { "type": "content.created", "contentType": "api::article.article" } } } },
            "do": [
                { "record": { "call": "data.json", "with": { "json": { "seen": true } } } }
            ]
        }
    });
    router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/workflows/{wf_id}"),
            wf,
            Some(&token),
        ))
        .await
        .unwrap();
    let activate = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/workflows/{wf_id}/activate"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(activate.status(), StatusCode::OK, "activate content workflow");

    let entry = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/content-manager/collection-types/api::article.article",
            serde_json::json!({ "data": { "title": "Hello Trigger", "featured": true } }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(entry.status(), StatusCode::OK, "create article");

    let mut found_success = false;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let list = router
            .clone()
            .oneshot(empty_request(
                "GET",
                &format!("/admin/executions?workflow_id={wf_id}"),
                Some(&token),
            ))
            .await
            .unwrap();
        let data = body_json(list).await;
        let execs = data["data"].as_array().cloned().unwrap_or_default();
        if let Some(first) = execs.first() {
            if first["status"] == "success" {
                found_success = true;
                break;
            }
        }
    }
    assert!(found_success, "content-created trigger ran the workflow to success");
}
