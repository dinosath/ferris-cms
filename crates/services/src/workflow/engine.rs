//! OWS workflow execution engine (execution layer).
//!
//! Loads an `OwsDocument`, validates it, resolves a deterministic task order,
//! and runs each OWS task against the CMS database and external services,
//! passing the workflow context between tasks and following `then`
//! transitions. Records per-task input/output and persists the execution and
//! its task runs. Long-running executions run asynchronously (spawned on the
//! Tokio runtime) so the HTTP request lifecycle is never blocked.

use crate::{AppContext, ServiceError};
use db::entities::{workflow_execution, workflow_node_run};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use std::collections::HashMap;
use std::sync::Arc;
use ::workflow::model::{
    OwsExecution, OwsExecutionStatus, OwsDocument, OwsTaskRun, OwsTaskRunStatus,
};
use serverless_workflow_core::models::task::{TaskDefinition, TaskDefinition as Task};

use super::executors::{self, FunctionRunContext};

/// Options for running a workflow.
#[derive(Clone)]
pub struct RunOptions {
    pub mode: String,
    pub trigger: String,
    pub input: serde_json::Value,
    /// Max attempts per task (retry on transient failure). Default 1.
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
    workflow: &OwsDocument,
    opts: &RunOptions,
) -> Result<i64, ServiceError> {
    let now = chrono::Utc::now();
    let row = workflow_execution::ActiveModel {
        workflow_id: Set(workflow.id),
        status: Set(OwsExecutionStatus::Running.as_str().to_string()),
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

    // Pre-create task-run placeholders in declaration order.
    for (i, (name, task)) in ::workflow::model::task_entries(&workflow.definition).into_iter().enumerate()
    {
        let _ = workflow_node_run::ActiveModel {
            execution_id: Set(exec_id),
            node_id: Set(name.clone()),
            node_name: Set(name.clone()),
            node_type: Set(task_type_label(task)),
            status: Set(OwsTaskRunStatus::NotExecuted.as_str().to_string()),
            started_at: Set(None),
            finished_at: Set(None),
            duration_ms: Set(None),
            input_json: Set(None),
            output_json: Set(None),
            error: Set(None),
            attempts: Set(0),
            order: Set(i as i64),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await?;
    }
    Ok(exec_id)
}

fn task_type_label(task: &TaskDefinition) -> String {
    use serverless_workflow_core::models::task::TaskDefinition as T;
    match task {
        T::Call(_) => "call".into(),
        T::Do(_) => "do".into(),
        T::Emit(_) => "emit".into(),
        T::For(_) => "for".into(),
        T::Fork(_) => "fork".into(),
        T::Listen(_) => "listen".into(),
        T::Raise(_) => "raise".into(),
        T::Run(_) => "run".into(),
        T::Set(_) => "set".into(),
        T::Switch(_) => "switch".into(),
        T::Try(_) => "try".into(),
        T::Wait(_) => "wait".into(),
    }
}

async fn update_execution(
    ctx: &AppContext,
    exec_id: i64,
    status: OwsExecutionStatus,
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


// ---------------------------------------------------------------------------
// Public engine API
// ---------------------------------------------------------------------------

/// Validate a workflow and start an async execution. Returns the execution id.
pub async fn execute_workflow(
    ctx: &AppContext,
    workflow_id: i64,
    opts: RunOptions,
) -> Result<i64, ServiceError> {
    let workflow = super::workflow_load(ctx, workflow_id).await?;
    let validation = ::workflow::validate_workflow(&workflow);
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
                    update_execution(&ctx, exec_id, OwsExecutionStatus::Failed, Some(e.to_string()))
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

/// Run a workflow synchronously using the `ows-runtime` execution engine.
pub async fn run_execution(
    ctx: &AppContext,
    workflow: &OwsDocument,
    exec_id: i64,
    opts: &RunOptions,
) -> Result<(), ServiceError> {
    let names = workflow.task_names();
    if names.is_empty() {
        update_execution(ctx, exec_id, OwsExecutionStatus::Success, None).await?;
        return Ok(());
    }

    // Build an OWS runtime with the FerrisCMS functions registered.
    let invoker: Arc<dyn ows_runtime::service::FunctionInvoker> =
        Arc::new(super::runtime_fn::CmsFunctionInvoker {
            app: Arc::new(ctx.clone()),
        });
    let mut builder = ows_runtime::Runtime::builder();
    for name in ::workflow::model::function::ALL {
        builder = builder.register_function(name, invoker.clone());
    }
    let runtime = builder
        .build()
        .map_err(|e| ServiceError::internal(format!("failed to build OWS runtime: {e}")))?;
    let compiled = runtime
        .register_definition(&workflow.definition)
        .map_err(|e| ServiceError::internal(format!("failed to compile workflow: {e}")))?;

    let result = runtime
        .run(compiled, opts.input.clone())
        .await;
    let task_order = runtime.take_task_order();

    match result {
        Ok(output) => {
            persist_task_runs(ctx, exec_id, workflow, &task_order, false).await?;
            update_execution(ctx, exec_id, OwsExecutionStatus::Success, None).await?;
            let _ = output;
        }
        Err(err) => {
            persist_task_runs(ctx, exec_id, workflow, &task_order, true).await?;
            update_execution(
                ctx,
                exec_id,
                OwsExecutionStatus::Failed,
                Some(err.to_string()),
            )
            .await?;
        }
    }
    Ok(())
}

/// Mark the executed tasks (per the runtime's recorded task order) as
/// `success` (or `failed` when the run errored) and leave the rest as-is.
async fn persist_task_runs(
    ctx: &AppContext,
    exec_id: i64,
    workflow: &OwsDocument,
    executed: &[String],
    failed: bool,
) -> Result<(), ServiceError> {
    let now = chrono::Utc::now();
    for (i, name) in executed.iter().enumerate() {
        let status = if failed && i == executed.len() - 1 {
            OwsTaskRunStatus::Failed
        } else {
            OwsTaskRunStatus::Success
        };
        let _ = workflow_node_run::Entity::find()
            .filter(workflow_node_run::Column::ExecutionId.eq(exec_id))
            .filter(workflow_node_run::Column::NodeId.eq(name))
            .one(&ctx.db)
            .await;
        if let Some(existing) = workflow_node_run::Entity::find()
            .filter(workflow_node_run::Column::ExecutionId.eq(exec_id))
            .filter(workflow_node_run::Column::NodeId.eq(name))
            .one(&ctx.db)
            .await?
        {
            let mut am: workflow_node_run::ActiveModel = existing.into();
            am.status = Set(status.as_str().to_string());
            am.started_at = Set(Some(now));
            am.finished_at = Set(Some(now));
            am.output_json = Set(Some(serde_json::json!({ "executed": true })));
            am.attempts = Set(1);
            am.update(&ctx.db).await?;
        }
    }
    let _ = workflow;
    Ok(())
}

// ---------------------------------------------------------------------------
// Execution queries / cancellation / retry
// ---------------------------------------------------------------------------

/// List executions (optionally filtered by workflow/status).
pub async fn execution_list(
    ctx: &AppContext,
    workflow_id: Option<i64>,
    status: Option<&str>,
) -> Result<Vec<OwsExecution>, ServiceError> {
    super::enforce(ctx, super::action::VIEW_EXECUTIONS).await?;
    let mut query = workflow_execution::Entity::find().order_by_desc(workflow_execution::Column::StartedAt);
    if let Some(wid) = workflow_id {
        query = query.filter(workflow_execution::Column::WorkflowId.eq(wid));
    }
    if let Some(s) = status {
        query = query.filter(workflow_execution::Column::Status.eq(s));
    }
    let rows = query.all(&ctx.db).await?;
    Ok(rows
        .iter()
        .map(|r| OwsExecution {
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

/// Get one execution plus its task runs.
pub async fn execution_get(
    ctx: &AppContext,
    exec_id: i64,
) -> Result<(OwsExecution, Vec<OwsTaskRun>), ServiceError> {
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
    let execution = OwsExecution {
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
    let task_runs = runs
        .iter()
        .map(|r| OwsTaskRun {
            id: r.id,
            execution_id: r.execution_id,
            task_name: r.node_name.clone(),
            task_type: r.node_type.clone(),
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
    Ok((execution, task_runs))
}

/// Cancel a running execution (best-effort — sets it to `cancelled`).
pub async fn execution_cancel(ctx: &AppContext, exec_id: i64) -> Result<(), ServiceError> {
    super::enforce(ctx, super::action::VIEW_EXECUTIONS).await?;
    let row = workflow_execution::Entity::find_by_id(exec_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::not_found("execution not found"))?;
    if row.status == OwsExecutionStatus::Running.as_str() || row.status == OwsExecutionStatus::Waiting.as_str() {
        update_execution(ctx, exec_id, OwsExecutionStatus::Cancelled, None).await?;
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
    let _ = super::workflow_load(ctx, row.workflow_id).await?;
    let opts = RunOptions {
        mode: "retry".into(),
        trigger: row.trigger.clone(),
        input: row.data_json.clone().unwrap_or(serde_json::json!({})),
        max_attempts: 3,
    };
    execute_workflow(ctx, row.workflow_id, opts).await
}

fn parse_execution_status(s: &str) -> OwsExecutionStatus {
    match s {
        "success" => OwsExecutionStatus::Success,
        "failed" => OwsExecutionStatus::Failed,
        "waiting" => OwsExecutionStatus::Waiting,
        "cancelled" => OwsExecutionStatus::Cancelled,
        _ => OwsExecutionStatus::Running,
    }
}

fn parse_node_status(s: &str) -> OwsTaskRunStatus {
    match s {
        "running" => OwsTaskRunStatus::Running,
        "success" => OwsTaskRunStatus::Success,
        "failed" => OwsTaskRunStatus::Failed,
        "skipped" => OwsTaskRunStatus::Skipped,
        "waiting" => OwsTaskRunStatus::Waiting,
        _ => OwsTaskRunStatus::NotExecuted,
    }
}
