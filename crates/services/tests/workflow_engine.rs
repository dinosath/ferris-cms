//! Execution-engine integration tests: branching, loops, error handling, and
//! execution persistence. Uses a temp FILE sqlite so the background worker
//! thread shares the database with the test thread.

use db::{seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{AppConfig, AppContext};
use std::time::Duration;

fn app_config() -> AppConfig {
    AppConfig {
        db_driver: "sqlite".into(),
        jwt_secret: "test-secret".into(),
        ..Default::default()
    }
}

async fn setup() -> AppContext {
    let base = std::env::temp_dir();
    std::fs::create_dir_all(&base).unwrap();
    let db_path = base.join(format!(
        "ferris-wfengine-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&db_path, b"").unwrap();
    let db = db::connect(&format!("sqlite://{}", db_path.display())).await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let ctx = AppContext::new(db, app_config());
    let _ = ctx.init_rbac().await;
    ctx
}

fn node(id: &str, node_type: &str, name: &str, x: f64, y: f64, params: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": id, "nodeType": node_type, "name": name,
        "position": { "x": x, "y": y }, "parameters": params,
        "disabled": false, "onError": "stop"
    })
}
fn conn(id: &str, from: &str, out: &str, to: &str) -> serde_json::Value {
    serde_json::json!({ "id": id, "from": from, "fromOutput": out, "to": to, "toInput": "main" })
}

async fn save_and_run(ctx: &AppContext, wf: serde_json::Value, input: serde_json::Value) -> (i64, serde_json::Value) {
    let def: ::workflow::model::Workflow = serde_json::from_value(wf).unwrap();
    let saved = services::workflow_save(ctx, None, &def).await.unwrap();
    let exec_id = services::engine::execute_workflow(
        ctx,
        saved.id,
        services::engine::RunOptions {
            mode: "manual".into(),
            trigger: "manual".into(),
            input,
            max_attempts: 3,
        },
    )
    .await
    .unwrap();

    // Poll until terminal.
    let mut detail = serde_json::Value::Null;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (execution, _) = services::execution_get(ctx, exec_id).await.unwrap();
        detail = serde_json::to_value(execution).unwrap();
        let status = detail["status"].as_str().unwrap_or("running");
        if status != "running" && status != "waiting" {
            break;
        }
    }
    (exec_id, detail)
}

async fn run_statuses(ctx: &AppContext, exec_id: i64) -> Vec<(String, String)> {
    let (_, runs) = services::execution_get(ctx, exec_id).await.unwrap();
    runs.iter()
        .map(|r| (r.node_name.clone(), r.status.as_str().to_string()))
        .collect()
}

#[tokio::test]
async fn branches_on_condition() {
    let ctx = setup().await;
    let wf = serde_json::json!({
        "id": 0, "name": "Branch", "version": 1, "active": false,
        "nodes": [
            node("t", "manualTrigger", "Manual", 0.0, 0.0, serde_json::json!({})),
            node("s", "set", "Set featured", 200.0, 0.0, serde_json::json!({ "field": "featured", "value": "true" })),
            node("if", "if", "If", 400.0, 0.0, serde_json::json!({ "operator": "true", "value1": "{{ $json.featured }}", "condition": "" })),
            node("truen", "noop", "True branch", 600.0, -60.0, serde_json::json!({})),
            node("falsen", "noop", "False branch", 600.0, 60.0, serde_json::json!({})),
        ],
        "connections": [
            conn("c1", "t", "main", "s"),
            conn("c2", "s", "main", "if"),
            serde_json::json!({ "id": "c3", "from": "if", "fromOutput": "true", "to": "truen", "toInput": "main" }),
            serde_json::json!({ "id": "c4", "from": "if", "fromOutput": "false", "to": "falsen", "toInput": "main" }),
        ],
        "settings": {}, "variables": {}, "tags": [],
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z"
    });
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({ "name": "x" })).await;
    assert_eq!(detail["status"], "success", "branch execution succeeded");

    let st = run_statuses(&ctx, exec_id).await;
    // The true branch node executed; the false branch node was skipped.
    let find = |name: &str| st.iter().find(|(n, _)| n == name).map(|(_, s)| s.clone()).unwrap_or_default();
    assert_eq!(find("True branch"), "success");
    assert_eq!(find("False branch"), "skipped");
}

#[tokio::test]
async fn loop_executes_multiple_iterations() {
    let ctx = setup().await;
    let wf = serde_json::json!({
        "id": 0, "name": "Loop", "version": 1, "active": false,
        "nodes": [
            node("t", "manualTrigger", "Manual", 0.0, 0.0, serde_json::json!({})),
            node("l", "loop", "Loop 3x", 200.0, 0.0, serde_json::json!({ "count": 3 })),
            node("n", "noop", "Noop", 400.0, 0.0, serde_json::json!({})),
        ],
        "connections": [ conn("c1", "t", "main", "l"), conn("c2", "l", "main", "n") ],
        "settings": {}, "variables": {}, "tags": [],
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z"
    });
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({})).await;
    assert_eq!(detail["status"], "success", "loop execution succeeded");
    let st = run_statuses(&ctx, exec_id).await;
    assert_eq!(st.iter().filter(|(n, _)| n == "Noop").count(), 1);
    let run = st.iter().find(|(n, _)| n == "Noop").unwrap();
    assert_eq!(run.1, "success");
}

#[tokio::test]
async fn error_stops_execution() {
    let ctx = setup().await;
    // An httpRequest with an unreachable URL (no network) will fail.
    let wf = serde_json::json!({
        "id": 0, "name": "Fail", "version": 1, "active": false,
        "nodes": [
            node("t", "manualTrigger", "Manual", 0.0, 0.0, serde_json::json!({})),
            node("h", "httpRequest", "Bad HTTP", 200.0, 0.0, serde_json::json!({ "method": "GET", "url": "http://127.0.0.1:9/", "authentication": "none", "headers": {} })),
            node("n", "noop", "Downstream", 400.0, 0.0, serde_json::json!({})),
        ],
        "connections": [ conn("c1", "t", "main", "h"), conn("c2", "h", "main", "n") ],
        "settings": {}, "variables": {}, "tags": [],
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z"
    });
    // Give the HTTP request an immediate failure via onError:stop (default).
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({})).await;
    let status = detail["status"].as_str().unwrap_or("").to_string();
    assert!(
        status == "failed",
        "execution should fail when a node errors (got {status})"
    );
    let st = run_statuses(&ctx, exec_id).await;
    assert_eq!(st.iter().find(|(n, _)| n == "Bad HTTP").map(|(_, s)| s.clone()).unwrap_or_default(), "failed");
    // The node was retried up to max_attempts (3) before failing.
    let (_, runs) = services::execution_get(&ctx, exec_id).await.unwrap();
    let bad = runs.iter().find(|r| r.node_name == "Bad HTTP").unwrap();
    assert_eq!(bad.attempts, 3, "node retried up to max_attempts");
}

#[tokio::test]
async fn execution_is_persisted() {
    let ctx = setup().await;
    let wf = serde_json::json!({
        "id": 0, "name": "Persist", "version": 1, "active": false,
        "nodes": [ node("t", "manualTrigger", "Manual", 0.0, 0.0, serde_json::json!({})), node("n", "noop", "Noop", 200.0, 0.0, serde_json::json!({})) ],
        "connections": [ conn("c1", "t", "main", "n") ],
        "settings": {}, "variables": {}, "tags": [],
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z"
    });
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({})).await;
    assert_eq!(detail["status"], "success");
    // Fetching the execution again yields node runs with input/output recorded.
    let (execution, runs) = services::engine::execution_get(&ctx, exec_id).await.unwrap();
    assert_eq!(execution.id, exec_id);
    assert_eq!(execution.status, ::workflow::model::ExecutionStatus::Success);
    assert!(runs.iter().any(|r| r.node_name == "Noop"));
}
