//! Trigger system — how active workflows start.
//!
//! Active workflows with trigger nodes are matched to runtime events:
//! - CMS content events (`content.created`, `content.updated`, ...) fired by
//!   the content service.
//! - Webhook / HTTP triggers matched by method + path.
//! - Scheduled triggers (a timer calls `run_due_scheduled`).
//! - Manual / API executions (handled directly by the engine).
//!
//! Triggers never block the caller: they spawn an async execution.

use crate::{AppContext, ServiceError};
use ::workflow::model::Workflow as WorkflowModel;

use super::engine::{execute_workflow, RunOptions};

/// The set of CMS events that can trigger workflows.
pub const EVENTS: &[&str] = &[
    "content.created",
    "content.updated",
    "content.published",
    "content.deleted",
    "media.uploaded",
    "user.created",
];

/// Map a CMS event to the matching trigger node type.
fn event_to_trigger_type(event: &str) -> Option<&'static str> {
    match event {
        "content.created" => Some("contentCreated"),
        "content.updated" => Some("contentUpdated"),
        "content.published" => Some("contentPublished"),
        "content.deleted" => Some("contentDeleted"),
        "media.uploaded" => Some("mediaUploaded"),
        "user.created" => Some("userCreated"),
        _ => None,
    }
}

/// Find active workflows whose trigger nodes match the given trigger type
/// (and, for content triggers, the given content-type uid).
pub fn matching_workflows<'a>(
    workflows: &'a [WorkflowModel],
    trigger_type: &str,
    uid: Option<&str>,
) -> Vec<(&'a WorkflowModel, &'a ::workflow::model::WorkflowNode)> {
    workflows
        .iter()
        .filter(|w| w.active)
        .filter_map(|w| {
            let node = w.nodes.iter().find(|n| {
                n.node_type == trigger_type
                    && uid.map_or(true, |uid| {
                        n.param_str("contentType").map(|c| c == uid).unwrap_or(false)
                    })
            });
            node.map(|n| (w, n))
        })
        .collect()
}

/// Load all active workflows (definition) from the database.
pub async fn load_active_workflows(ctx: &AppContext) -> Result<Vec<WorkflowModel>, ServiceError> {
    use db::entities::workflow;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let rows = workflow::Entity::find()
        .filter(workflow::Column::Active.eq(true))
        .all(&ctx.db)
        .await?;
    let mut out = Vec::new();
    for row in rows {
        if let Ok(w) = serde_json::from_value::<WorkflowModel>(row.definition_json) {
            out.push(w);
        }
    }
    Ok(out)
}

/// Dispatch a CMS event to all matching active workflows. Spawns an async
/// execution for each with the entry as trigger data. Never fails the caller.
pub async fn dispatch_cms_event(
    ctx: &AppContext,
    event: &str,
    uid: &str,
    entry: serde_json::Value,
) -> usize {
    let Some(trigger_type) = event_to_trigger_type(event) else {
        return 0;
    };
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let matches = matching_workflows(&workflows, trigger_type, Some(uid));
    let count = matches.len();
    for (wf, _node) in matches {
        let opts = RunOptions {
            mode: "trigger".into(),
            trigger: format!("{event}:{uid}"),
            input: entry.clone(),
            max_attempts: 3,
        };
        let _ = execute_workflow(ctx, wf.id, opts).await;
    }
    count
}

/// Dispatch a media-uploaded event (no content-type uid).
pub async fn dispatch_media_event(ctx: &AppContext, file: serde_json::Value) -> usize {
    let Some(trigger_type) = event_to_trigger_type("media.uploaded") else {
        return 0;
    };
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let matches = matching_workflows(&workflows, trigger_type, None);
    for wf in matches.iter().map(|(w, _)| *w) {
        let opts = RunOptions {
            mode: "trigger".into(),
            trigger: "media.uploaded".into(),
            input: file.clone(),
            max_attempts: 3,
        };
        let _ = execute_workflow(ctx, wf.id, opts).await;
    }
    matches.len()
}

/// Dispatch a user-created event.
pub async fn dispatch_user_event(ctx: &AppContext, user: serde_json::Value) -> usize {
    let Some(trigger_type) = event_to_trigger_type("user.created") else {
        return 0;
    };
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let matches = matching_workflows(&workflows, trigger_type, None);
    for wf in matches.iter().map(|(w, _)| *w) {
        let opts = RunOptions {
            mode: "trigger".into(),
            trigger: "user.created".into(),
            input: user.clone(),
            max_attempts: 3,
        };
        let _ = execute_workflow(ctx, wf.id, opts).await;
    }
    matches.len()
}

/// Match an active webhook/HTTP trigger by method + path, returning its
/// workflow id and the matching node.
pub async fn workflow_for_webhook(
    ctx: &AppContext,
    method: &str,
    path: &str,
) -> Option<(i64, String)> {
    let workflows = load_active_workflows(ctx).await.ok()?;
    for wf in &workflows {
        for node in &wf.nodes {
            if matches!(node.node_type.as_str(), "webhookTrigger" | "httpTrigger") {
                let node_path = node.param_str("path").unwrap_or_default();
                let normalized = node_path.trim_start_matches('/');
                let request_path = path.trim_start_matches('/');
                if normalized == request_path || node_path.trim_end_matches('/').is_empty() {
                    if let Some(m) = node.param_str("method") {
                        if !m.is_empty() && !m.eq_ignore_ascii_case(method) {
                            continue;
                        }
                    }
                    return Some((wf.id, node.id.clone()));
                }
            }
        }
    }
    None
}

/// Execute an active webhook workflow. Returns the execution id.
pub async fn execute_webhook(
    ctx: &AppContext,
    workflow_id: i64,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<i64, ServiceError> {
    let opts = RunOptions {
        mode: "webhook".into(),
        trigger: format!("{method} {path}"),
        input: body,
        max_attempts: 1,
    };
    execute_workflow(ctx, workflow_id, opts).await
}

/// Trigger every active workflow that has a schedule trigger. A real scheduler
/// would use cron expression evaluation + timezone handling; this runs the
/// scheduled workflows on demand (a timer can call it at the schedule's cadence).
/// Returns the number of workflows triggered.
pub async fn run_scheduled_workflows(ctx: &AppContext) -> usize {
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let mut count = 0;
    for wf in &workflows {
        if wf.nodes.iter().any(|n| n.node_type == "scheduleTrigger") {
            let opts = RunOptions {
                mode: "schedule".into(),
                trigger: "schedule".into(),
                input: serde_json::json!({ "now": chrono::Utc::now().to_rfc3339() }),
                max_attempts: 3,
            };
            if execute_workflow(ctx, wf.id, opts).await.is_ok() {
                count += 1;
            }
        }
    }
    count
}
