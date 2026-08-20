//! Workflow execution engine (execution layer).
//!
//! Loads a workflow definition, validates it, computes a deterministic
//! topological execution order, runs each node against the CMS database and
//! external services, passes structured items between nodes (including through
//! branches), records per-node input/output, and persists the execution and
//! its node runs. Long-running executions run asynchronously (spawned on the
//! Tokio runtime) so the HTTP request lifecycle is never blocked.

use crate::{AppContext, ServiceError};
use db::entities::{workflow_execution, workflow_node_run};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set,
};
use std::collections::HashMap;
use ::workflow::model::{
    Execution, ExecutionStatus, NodeRun, NodeRunStatus, Workflow as WorkflowModel, WorkflowNode,
};

use super::executors::{self, NodeRunContext};

/// Options for running a workflow.
#[derive(Clone)]
pub struct RunOptions {
    pub mode: String,
    pub trigger: String,
    pub input: serde_json::Value,
    /// Max attempts per node (retry on transient failure). Default 1.
    pub max_attempts: i64,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            mode: "manual".into(),
            trigger: "manual".into(),
            input: serde_json::json!({}),
            max_attempts: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

async fn create_execution(
    ctx: &AppContext,
    workflow: &WorkflowModel,
    opts: &RunOptions,
) -> Result<i64, ServiceError> {
    let now = chrono::Utc::now();
    let row = workflow_execution::ActiveModel {
        workflow_id: Set(workflow.id),
        status: Set(ExecutionStatus::Running.as_str().to_string()),
        mode: Set(opts.mode.clone()),
        trigger: Set(opts.trigger.clone()),
        started_at: Set(now),
        finished_at: Set(None),
        duration_ms: Set(None),
        error: Set(None),
        data_json: Set(Some(opts.input.clone())),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    let exec_id = row.id;

    // Pre-create node-run placeholders (status = notExecuted).
    let mut order_map: HashMap<String, usize> = HashMap::new();
    if let Ok(order) = ::workflow::graph::topological_order(workflow) {
        for (i, id) in order.into_iter().enumerate() {
            order_map.insert(id, i);
        }
    }
    for node in &workflow.nodes {
        let order = order_map
            .get(&node.id)
            .copied()
            .unwrap_or(usize::MAX);
        let _ = workflow_node_run::ActiveModel {
            execution_id: Set(exec_id),
            node_id: Set(node.id.clone()),
            node_name: Set(node.name.clone()),
            node_type: Set(node.node_type.clone()),
            status: Set(NodeRunStatus::NotExecuted.as_str().to_string()),
            started_at: Set(None),
            finished_at: Set(None),
            duration_ms: Set(None),
            input_json: Set(None),
            output_json: Set(None),
            error: Set(None),
            attempts: Set(0),
            order: Set(order as i64),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(exec_id)
}

async fn update_execution(
    ctx: &AppContext,
    exec_id: i64,
    status: ExecutionStatus,
    error: Option<String>,
) -> Result<(), ServiceError> {
    let row = workflow_execution::Entity::find_by_id(exec_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("execution not found"))?;
    let started_at = row.started_at;
    let mut am: workflow_execution::ActiveModel = row.into();
    let now = chrono::Utc::now();
    am.status = Set(status.as_str().to_string());
    am.finished_at = Set(Some(now));
    am.duration_ms = Set(Some((now - started_at).num_milliseconds()));
    am.error = Set(error);
    am.update(&ctx.db).await?;
    Ok(())
}

async fn save_node_run(
    ctx: &AppContext,
    exec_id: i64,
    node_id: &str,
    status: NodeRunStatus,
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: chrono::DateTime<chrono::Utc>,
    input: &serde_json::Value,
    output: &serde_json::Value,
    error: Option<String>,
    attempts: i64,
) -> Result<(), ServiceError> {
    let existing = workflow_node_run::Entity::find()
        .filter(workflow_node_run::Column::ExecutionId.eq(exec_id))
        .filter(workflow_node_run::Column::NodeId.eq(node_id))
        .one(&ctx.db)
        .await?;
    let duration_ms = (finished_at - started_at).num_milliseconds();
    if let Some(existing) = existing {
        let mut am: workflow_node_run::ActiveModel = existing.into();
        am.status = Set(status.as_str().to_string());
        am.started_at = Set(Some(started_at));
        am.finished_at = Set(Some(finished_at));
        am.duration_ms = Set(Some(duration_ms));
        am.input_json = Set(Some(input.clone()));
        am.output_json = Set(Some(output.clone()));
        am.error = Set(error);
        am.attempts = Set(attempts);
        am.update(&ctx.db).await?;
    } else {
        let _ = workflow_node_run::ActiveModel {
            execution_id: Set(exec_id),
            node_id: Set(node_id.to_string()),
            node_name: Set(String::new()),
            node_type: Set(String::new()),
            status: Set(status.as_str().to_string()),
            started_at: Set(Some(started_at)),
            finished_at: Set(Some(finished_at)),
            duration_ms: Set(Some(duration_ms)),
            input_json: Set(Some(input.clone())),
            output_json: Set(Some(output.clone())),
            error: Set(error),
            attempts: Set(attempts),
            order: Set(0),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(())
}

async fn mark_skipped(
    ctx: &AppContext,
    exec_id: i64,
    node: &WorkflowNode,
) -> Result<(), ServiceError> {
    let now = chrono::Utc::now();
    let existing = workflow_node_run::Entity::find()
        .filter(workflow_node_run::Column::ExecutionId.eq(exec_id))
        .filter(workflow_node_run::Column::NodeId.eq(&node.id))
        .one(&ctx.db)
        .await?;
    if let Some(e) = existing {
        let mut am: workflow_node_run::ActiveModel = e.into();
        am.status = Set(NodeRunStatus::Skipped.as_str().to_string());
        am.finished_at = Set(Some(now));
        am.attempts = Set(0);
        am.update(&ctx.db).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public engine API
// ---------------------------------------------------------------------------

/// Validate a workflow and start an async execution. Returns the execution id.
/// The execution runs in the background (Tokio spawn), so long-running
/// workflows never block the calling HTTP request.
pub async fn execute_workflow(
    ctx: &AppContext,
    workflow_id: i64,
    opts: RunOptions,
) -> Result<i64, ServiceError> {
    let workflow = super::workflow_load(ctx, workflow_id).await?;
    let validation = ::workflow::validate(&workflow, ::workflow::registry());
    if !validation.valid {
        return Err(ServiceError::Validation(
            validation
                .errors
                .into_iter()
                .map(|e| crate::ValidationErrorItem::new(vec![], e.message, e.code))
                .collect(),
        ));
    }
    let exec_id = create_execution(ctx, &workflow, &opts).await?;
    let ctx = ctx.clone();
    let workflow = workflow.clone();
    // Run the execution on a dedicated worker thread with its own Tokio
    // runtime. This keeps long-running workflows off the request thread and
    // lets CMS triggers (which are themselves async) dispatch without blocking.
    // A dedicated thread also avoids stack recursion for workflow-triggered
    // workflows (each trigger spawns a fresh execution).
    std::thread::Builder::new()
        .name("workflow-exec".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("failed to build workflow runtime: {e}");
                    return;
                }
            };
            let result = rt.block_on(async {
                if let Err(e) = run_execution(&ctx, &workflow, exec_id, &opts).await {
                    update_execution(&ctx, exec_id, ExecutionStatus::Failed, Some(e.to_string()))
                        .await
                } else {
                    Ok(())
                }
            });
            if let Err(e) = result {
                tracing::warn!("workflow execution {exec_id} error: {e}");
            }
        })
        .map_err(|e| ServiceError::internal(format!("failed to spawn workflow worker: {e}")))?;
    Ok(exec_id)
}
/// Run a workflow synchronously (used by tests and the async task). Resolves
/// the execution to `Success` or `Failed`.
pub async fn run_execution(
    ctx: &AppContext,
    workflow: &WorkflowModel,
    exec_id: i64,
    opts: &RunOptions,
) -> Result<(), ServiceError> {
    let order = ::workflow::graph::topological_order(workflow)
        .map_err(|e| ServiceError::internal(format!("workflow is not executable: {e}")))?;

    // `(node_id, port) -> items` produced so far.
    let mut results: HashMap<(String, String), Vec<serde_json::Value>> = HashMap::new();
    // `node name -> output object` for expression context.
    let mut node_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    // Seed the execution metadata for expressions.
    let execution_json = serde_json::json!({
        "id": exec_id,
        "mode": opts.mode,
        "trigger": opts.trigger,
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    let workflow_json = serde_json::to_value(workflow).unwrap_or(serde_json::json!({}));
    let env: HashMap<String, String> = HashMap::new();
    let mut has_error = false;
    let mut stop = false;

    for node_id in &order {
        if stop {
            break;
        }
        let Some(node) = workflow.node(node_id) else { continue };

        // Triggers: emit the execution input as their output items.
        if ::workflow::model::is_trigger_type(&node.node_type) {
            let items = trigger_items(opts);
            results.insert((node.id.clone(), "main".to_string()), items.clone());
            node_outputs.insert(
                node.name.clone(),
                output_object(&items),
            );
            let now = chrono::Utc::now();
            let _ = save_node_run(
                ctx,
                exec_id,
                &node.id,
                NodeRunStatus::Success,
                now,
                now,
                &serde_json::json!({}),
                &serde_json::json!({ "items": items }),
                None,
                1,
            )
            .await;
            continue;
        }

        if node.disabled {
            mark_skipped(ctx, exec_id, node).await?;
            continue;
        }

        // Gather input items from all incoming connections.
        let mut input_items: Vec<serde_json::Value> = Vec::new();
        for conn in workflow.incoming(&node.id) {
            if let Some(items) = results.get(&conn.source_key()) {
                for it in items {
                    input_items.push(it.clone());
                }
            }
        }
        let had_connections = !workflow.incoming(&node.id).is_empty();
        if !had_connections {
            // Unreachable node (no trigger path) → skipped.
            mark_skipped(ctx, exec_id, node).await?;
            continue;
        }
        // A node that receives no items is skipped (e.g. the inactive branch
        // of an If/Switch).
        if input_items.is_empty() {
            mark_skipped(ctx, exec_id, node).await?;
            continue;
        }

        // Build the runtime context.
        let run_ctx = NodeRunContext {
            app: ctx,
            workflow,
            node,
            node_outputs: &node_outputs,
            env: &env,
            execution_id: exec_id,
            workflow_json: workflow_json.clone(),
            execution_json: execution_json.clone(),
        };

        // Execute with retry on transient failure.
        let started = chrono::Utc::now();
        let mut last_err: Option<String> = None;
        let mut result: Option<executors::NodeResult> = None;
        let mut attempts = 0i64;
        for attempt in 1..=opts.max_attempts {
            attempts = attempt;
            match executors::execute_node(&run_ctx, &node.node_type, &input_items).await {
                Ok(r) => {
                    result = Some(r);
                    last_err = None;
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt < opts.max_attempts {
                        tokio::time::sleep(std::time::Duration::from_millis(100 * attempt as u64))
                            .await;
                    }
                }
            }
        }

        let finished = chrono::Utc::now();
        match result {
            Some(res) => {
                for (port, items) in &res {
                    results.insert((node.id.clone(), port.clone()), items.clone());
                }
                node_outputs.insert(
                    node.name.clone(),
                    output_object(&res.get("main").cloned().unwrap_or_default()),
                );
                let _ = save_node_run(
                    ctx,
                    exec_id,
                    &node.id,
                    NodeRunStatus::Success,
                    started,
                    finished,
                    &serde_json::json!({ "items": input_items }),
                    &serde_json::to_value(&res).unwrap_or(serde_json::json!({})),
                    None,
                    attempts,
                )
                .await;
            }
            None => {
                has_error = true;
                let err = last_err.unwrap_or_else(|| "node failed".to_string());
                let _ = save_node_run(
                    ctx,
                    exec_id,
                    &node.id,
                    NodeRunStatus::Failed,
                    started,
                    finished,
                    &serde_json::json!({ "items": input_items }),
                    &serde_json::json!({}),
                    Some(err.clone()),
                    attempts,
                )
                .await;
                match node.on_error {
                    ::workflow::model::OnError::Stop => {
                        stop = true;
                    }
                    ::workflow::model::OnError::Continue => {}
                    ::workflow::model::OnError::Route => {
                        // Route error to the error output connection; downstream
                        // nodes on the error port receive an error item.
                        if let Some(port) = node.error_output.clone() {
                            let items = vec![serde_json::json!({ "error": err, "node": node.name })];
                            results.insert((node.id.clone(), port), items);
                        }
                    }
                }
            }
        }
    }

    let status = if stop {
        ExecutionStatus::Failed
    } else if has_error {
        ExecutionStatus::Failed
    } else {
        ExecutionStatus::Success
    };
    let error = if status == ExecutionStatus::Failed {
        Some("Workflow failed (one or more nodes reported an error)".to_string())
    } else {
        None
    };
    update_execution(ctx, exec_id, status, error).await
}

fn trigger_items(opts: &RunOptions) -> Vec<serde_json::Value> {
    match &opts.input {
        serde_json::Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    }
}

fn output_object(items: &[serde_json::Value]) -> serde_json::Value {
    let json = items.first().cloned().unwrap_or(serde_json::json!({}));
    serde_json::json!({
        "json": json,
        "items": items,
    })
}

// ---------------------------------------------------------------------------
// Execution queries / cancellation / retry
// ---------------------------------------------------------------------------

/// List executions (optionally filtered by workflow/status).
pub async fn execution_list(
    ctx: &AppContext,
    workflow_id: Option<i64>,
    status: Option<&str>,
) -> Result<Vec<Execution>, ServiceError> {
    super::enforce(ctx, super::action::VIEW_EXECUTIONS).await?;
    let mut query = workflow_execution::Entity::find()
        .order_by_desc(workflow_execution::Column::StartedAt);
    if let Some(wid) = workflow_id {
        query = query.filter(workflow_execution::Column::WorkflowId.eq(wid));
    }
    if let Some(s) = status {
        query = query.filter(workflow_execution::Column::Status.eq(s));
    }
    let rows = query.all(&ctx.db).await?;
    Ok(rows
        .iter()
        .map(|r| Execution {
            id: r.id,
            workflow_id: r.workflow_id,
            status: parse_execution_status(&r.status),
            mode: r.mode.clone(),
            trigger: r.trigger.clone(),
            started_at: r.started_at,
            finished_at: r.finished_at,
            duration_ms: r.duration_ms,
            error: r.error.clone(),
        })
        .collect())
}

/// Get one execution plus its node runs.
pub async fn execution_get(
    ctx: &AppContext,
    exec_id: i64,
) -> Result<(Execution, Vec<NodeRun>), ServiceError> {
    super::enforce(ctx, super::action::VIEW_EXECUTIONS).await?;
    let row = workflow_execution::Entity::find_by_id(exec_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("execution not found"))?;
    let runs = workflow_node_run::Entity::find()
        .filter(workflow_node_run::Column::ExecutionId.eq(exec_id))
        .order_by_asc(workflow_node_run::Column::Order)
        .all(&ctx.db)
        .await?;
    let execution = Execution {
        id: row.id,
        workflow_id: row.workflow_id,
        status: parse_execution_status(&row.status),
        mode: row.mode.clone(),
        trigger: row.trigger.clone(),
        started_at: row.started_at,
        finished_at: row.finished_at,
        duration_ms: row.duration_ms,
        error: row.error.clone(),
    };
    let node_runs = runs
        .iter()
        .map(|r| NodeRun {
            id: r.id,
            execution_id: r.execution_id,
            node_id: r.node_id.clone(),
            node_name: r.node_name.clone(),
            node_type: r.node_type.clone(),
            status: parse_node_status(&r.status),
            started_at: r.started_at,
            finished_at: r.finished_at,
            duration_ms: r.duration_ms,
            input: r.input_json.clone(),
            output: r.output_json.clone(),
            error: r.error.clone(),
            attempts: r.attempts,
            order: r.order,
        })
        .collect();
    Ok((execution, node_runs))
}

/// Cancel a running execution (best-effort — sets it to `cancelled`).
pub async fn execution_cancel(ctx: &AppContext, exec_id: i64) -> Result<(), ServiceError> {
    super::enforce(ctx, super::action::VIEW_EXECUTIONS).await?;
    let row = workflow_execution::Entity::find_by_id(exec_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("execution not found"))?;
    if row.status == ExecutionStatus::Running.as_str() || row.status == ExecutionStatus::Waiting.as_str() {
        update_execution(ctx, exec_id, ExecutionStatus::Cancelled, None).await?;
    }
    Ok(())
}

/// Retry a failed execution: re-run the workflow with the original input.
pub async fn execution_retry(ctx: &AppContext, exec_id: i64) -> Result<i64, ServiceError> {
    super::enforce(ctx, super::action::VIEW_EXECUTIONS).await?;
    let row = workflow_execution::Entity::find_by_id(exec_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("execution not found"))?;
    let _workflow = super::workflow_load(ctx, row.workflow_id).await?;
    let opts = RunOptions {
        mode: "retry".into(),
        trigger: row.trigger.clone(),
        input: row.data_json.clone().unwrap_or(serde_json::json!({})),
        max_attempts: 3,
    };
    execute_workflow(ctx, row.workflow_id, opts).await
}

/// Latest execution id for a workflow (list helper).
pub async fn workflow_last_execution_id(
    ctx: &AppContext,
    workflow_id: i64,
) -> Result<Option<i64>, ServiceError> {
    let row = workflow_execution::Entity::find()
        .filter(workflow_execution::Column::WorkflowId.eq(workflow_id))
        .order_by_desc(workflow_execution::Column::StartedAt)
        .one(&ctx.db)
        .await?;
    Ok(row.map(|r| r.id))
}

fn parse_execution_status(s: &str) -> ExecutionStatus {
    match s {
        "success" => ExecutionStatus::Success,
        "failed" => ExecutionStatus::Failed,
        "waiting" => ExecutionStatus::Waiting,
        "cancelled" => ExecutionStatus::Cancelled,
        _ => ExecutionStatus::Running,
    }
}

fn parse_node_status(s: &str) -> NodeRunStatus {
    match s {
        "running" => NodeRunStatus::Running,
        "success" => NodeRunStatus::Success,
        "failed" => NodeRunStatus::Failed,
        "skipped" => NodeRunStatus::Skipped,
        "waiting" => NodeRunStatus::Waiting,
        _ => NodeRunStatus::NotExecuted,
    }
}

