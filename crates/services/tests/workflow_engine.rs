//! Execution-engine integration tests: branching, loops, error handling, and
//! execution persistence. Uses a temp FILE sqlite so the background worker
//! thread shares the database with the test thread.

use db::{seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{AppConfig, AppContext};
use serverless_workflow_core::models::task::TaskDefinition;
use serverless_workflow_core::models::workflow::{WorkflowDefinition, WorkflowDefinitionMetadata};
use std::time::Duration;
use ::workflow::model::OwsDocument;

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

/// Build an OWS document with the given named tasks.
fn make_doc(name: &str, tasks: Vec<(String, TaskDefinition)>) -> OwsDocument {
    let metadata = WorkflowDefinitionMetadata::new("default", name, "1.0.0", None, None, None);
    let mut definition = WorkflowDefinition::new(metadata);
    for (n, t) in tasks {
        definition.do_.add(n, t);
    }
    OwsDocument {
        id: 0,
        active: false,
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        definition,
    }
}

fn set_task(set: serde_json::Value) -> TaskDefinition {
    use serverless_workflow_core::models::task::SetValue;
    let mut t = serverless_workflow_core::models::task::SetTaskDefinition::new();
    if let Some(obj) = set.as_object() {
        t.set = SetValue::Map(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    }
    TaskDefinition::Set(t)
}

fn call_fn(name: &str, with: serde_json::Value) -> TaskDefinition {
    let with = with.as_object().map(|o| {
        o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    });
    TaskDefinition::Call(serverless_workflow_core::models::task::CallTaskDefinition::new(name, with, None))
}

/// Set a task's `then` flow directive.
fn then_of(task: &mut TaskDefinition, then: &str) {
    use serverless_workflow_core::models::task::TaskDefinition as T;
    match task {
        T::Call(t) => t.common.then = Some(then.to_string()),
        T::Do(t) => t.common.then = Some(then.to_string()),
        T::Emit(t) => t.common.then = Some(then.to_string()),
        T::For(t) => t.common.then = Some(then.to_string()),
        T::Fork(t) => t.common.then = Some(then.to_string()),
        T::Listen(t) => t.common.then = Some(then.to_string()),
        T::Raise(t) => t.common.then = Some(then.to_string()),
        T::Run(t) => t.common.then = Some(then.to_string()),
        T::Set(t) => t.common.then = Some(then.to_string()),
        T::Switch(t) => t.common.then = Some(then.to_string()),
        T::Try(t) => t.common.then = Some(then.to_string()),
        T::Wait(t) => t.common.then = Some(then.to_string()),
    }
}

fn switch_task(cases: Vec<(String, String, Option<String>)>) -> TaskDefinition {
    use serverless_workflow_core::models::task::{SwitchCaseDefinition, SwitchTaskDefinition};
    let mut sw = SwitchTaskDefinition::new();
    for (name, when, then) in cases {
        let case = SwitchCaseDefinition { when: Some(when), then };
        let mut m = std::collections::HashMap::new();
        m.insert(name, case);
        sw.switch.entries.push(m);
    }
    TaskDefinition::Switch(sw)
}

fn for_task(each: &str, in_: &str, body: TaskDefinition) -> TaskDefinition {
    use serverless_workflow_core::models::task::{ForLoopDefinition, ForTaskDefinition};
    let mut m = std::collections::HashMap::new();
    m.insert("body".to_string(), body);
    let mut do_ = serverless_workflow_core::models::map::Map::new();
    do_.entries.push(m);
    let loop_def = ForLoopDefinition::new(each, in_, None, None);
    TaskDefinition::For(ForTaskDefinition::new(loop_def, do_, None))
}

async fn save_and_run(ctx: &AppContext, wf: OwsDocument, input: serde_json::Value) -> (i64, serde_json::Value) {
    let saved = services::workflow_save(ctx, None, &wf).await.unwrap();
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
        .map(|r| (r.task_name.clone(), r.status.as_str().to_string()))
        .collect()
}

#[tokio::test]
async fn branches_on_condition() {
    let ctx = setup().await;
    let mut true_branch = call_fn("data.json", serde_json::json!({ "json": { "path": "true" } }));
    let mut false_branch = call_fn("data.json", serde_json::json!({ "json": { "path": "false" } }));
    then_of(&mut true_branch, "exit");
    then_of(&mut false_branch, "exit");
    let wf = make_doc(
        "Branch",
        vec![
            (
                "setFeatured".to_string(),
                set_task(serde_json::json!({ "featured": true })),
            ),
            (
                "branch".to_string(),
                switch_task(vec![
                    ("featured".to_string(), "${ .featured }".to_string(), Some("trueBranch".to_string())),
                    ("default".to_string(), String::new(), Some("falseBranch".to_string())),
                ]),
            ),
            ("trueBranch".to_string(), true_branch),
            ("falseBranch".to_string(), false_branch),
        ],
    );
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({ "name": "x" })).await;
    assert_eq!(detail["status"], "success", "branch execution succeeded");
    let st = run_statuses(&ctx, exec_id).await;
    let find = |name: &str| st.iter().find(|(n, _)| n == name).map(|(_, s)| s.clone()).unwrap_or_default();
    assert_eq!(find("trueBranch"), "success");
    assert_eq!(find("falseBranch"), "notExecuted");
}

#[tokio::test]
async fn loop_executes_multiple_iterations() {
    let ctx = setup().await;
    let wf = make_doc(
        "Loop",
        vec![
            (
                "loop".to_string(),
                for_task("item", ".items", call_fn("data.json", serde_json::json!({ "json": { "ok": true } }))),
            ),
            ("after".to_string(), call_fn("data.json", serde_json::json!({ "json": { "done": true } }))),
        ],
    );
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({ "items": [1, 2, 3] })).await;
    assert_eq!(detail["status"], "success", "loop execution succeeded");
    let st = run_statuses(&ctx, exec_id).await;
    assert!(st.iter().any(|(n, s)| n == "loop" && s == "success"));
    assert!(st.iter().any(|(n, s)| n == "after" && s == "success"));
}

#[tokio::test]
async fn error_stops_execution() {
    let ctx = setup().await;
    // An http.request with an unreachable URL (no network) will fail.
    let wf = make_doc(
        "Fail",
        vec![
            (
                "bad".to_string(),
                call_fn(
                    "http.request",
                    serde_json::json!({ "method": "GET", "url": "http://127.0.0.1:9/", "authentication": "none" }),
                ),
            ),
            ("downstream".to_string(), call_fn("data.json", serde_json::json!({ "json": {} }))),
        ],
    );
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({})).await;
    let status = detail["status"].as_str().unwrap_or("").to_string();
    assert!(status == "failed", "execution should fail when a task errors (got {status})");
    let st = run_statuses(&ctx, exec_id).await;
    assert_eq!(
        st.iter().find(|(n, _)| n == "bad").map(|(_, s)| s.clone()).unwrap_or_default(),
        "failed"
    );
}

#[tokio::test]
async fn execution_is_persisted() {
    let ctx = setup().await;
    let wf = make_doc(
        "Persist",
        vec![
            ("set".to_string(), set_task(serde_json::json!({ "greeting": "hello" }))),
            ("output".to_string(), call_fn("data.json", serde_json::json!({ "json": { "done": true } }))),
        ],
    );
    let (exec_id, detail) = save_and_run(&ctx, wf, serde_json::json!({})).await;
    assert_eq!(detail["status"], "success");
    let (execution, runs) = services::engine::execution_get(&ctx, exec_id).await.unwrap();
    assert_eq!(execution.id, exec_id);
    assert_eq!(execution.status, ::workflow::model::OwsExecutionStatus::Success);
    assert!(runs.iter().any(|r| r.task_name == "output"));
}
