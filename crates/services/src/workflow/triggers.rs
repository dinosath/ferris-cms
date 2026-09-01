//! Trigger system — how active OWS workflows start.
//!
//! In OWS, event-driven scheduling is expressed with `schedule.on` event
//! filters. Active workflows are matched to runtime events:
//! - CMS content events (`content.created`, `content.updated`, ...) fired by
//!   the content service.
//! - Webhook / HTTP events matched by path.
//! - Scheduled (`cron`/`every`) triggers (a timer calls `run_scheduled_workflows`).
//! - Manual / API executions (handled directly by the engine).
//!
//! Triggers never block the caller: they spawn an async execution.

use crate::{AppContext, ServiceError};
use ::workflow::model::{is_trigger_event, OwsDocument};
use serverless_workflow_core::models::event::{EventConsumptionStrategyDefinition, EventFilterDefinition};

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

/// Extract all event filters declared by a workflow's `schedule.on`.
pub fn event_filters(doc: &OwsDocument) -> Vec<&EventFilterDefinition> {
    let mut out = Vec::new();
    if let Some(schedule) = &doc.definition.schedule {
        if let Some(on) = &schedule.on {
            for f in on.all.iter().flat_map(|v| v.iter()) {
                out.push(f);
            }
            for f in on.any.iter().flat_map(|v| v.iter()) {
                out.push(f);
            }
            if let Some(one) = &on.one {
                out.push(one);
            }
        }
    }
    out
}

fn filter_event_type(filter: &EventFilterDefinition) -> Option<String> {
    filter
        .with
        .as_ref()
        .and_then(|w| w.get("type"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn filter_content_type(filter: &EventFilterDefinition) -> Option<String> {
    filter
        .with
        .as_ref()
        .and_then(|w| w.get("contentType"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Find active workflows whose declared schedule events match the given event
/// type (and, for content triggers, the content-type uid).
pub fn matching_workflows<'a>(
    workflows: &'a [OwsDocument],
    event_type: &str,
    uid: Option<&str>,
) -> Vec<&'a OwsDocument> {
    workflows
        .iter()
        .filter(|w| w.active)
        .filter(|w| {
            event_filters(w).iter().any(|f| {
                filter_event_type(f).map(|t| t == event_type).unwrap_or(false)
                    && uid.map_or(true, |uid| {
                        filter_content_type(f).map(|c| c == uid).unwrap_or(false)
                    })
            })
        })
        .collect()
}

/// Load all active workflows (definition) from the database.
pub async fn load_active_workflows(ctx: &AppContext) -> Result<Vec<OwsDocument>, ServiceError> {
    use db::entities::workflow;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let rows = workflow::Entity::find()
        .filter(workflow::Column::Active.eq(true))
        .all(&ctx.db)
        .await?;
    let mut out = Vec::new();
    for row in rows {
        if let Ok(w) = serde_json::from_value::<OwsDocument>(row.definition_json) {
            out.push(w);
        }
    }
    Ok(out)
}

/// Dispatch a CMS event to all matching active workflows. Returns the count.
pub async fn dispatch_cms_event(
    ctx: &AppContext,
    event: &str,
    uid: &str,
    entry: serde_json::Value,
) -> usize {
    if !is_trigger_event(event) || !EVENTS.contains(&event) {
        return 0;
    }
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let matches = matching_workflows(&workflows, event, Some(uid));
    let count = matches.len();
    for wf in matches {
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
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let matches = matching_workflows(&workflows, "media.uploaded", None);
    for wf in &matches {
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
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let matches = matching_workflows(&workflows, "user.created", None);
    for wf in &matches {
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

/// Match an active webhook/HTTP event workflow by method + path, returning its
/// workflow id.
pub async fn workflow_for_webhook(
    ctx: &AppContext,
    _method: &str,
    path: &str,
) -> Option<(i64, String)> {
    let workflows = load_active_workflows(ctx).await.ok()?;
    for wf in &workflows {
        for filter in event_filters(wf) {
            if filter_event_type(filter).as_deref() == Some("webhook") {
                let node_path = filter
                    .with
                    .as_ref()
                    .and_then(|w| w.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let normalized = node_path.trim_start_matches('/');
                let request_path = path.trim_start_matches('/');
                if normalized == request_path || node_path.trim_end_matches('/').is_empty() {
                    return Some((wf.id, filter_event_type(filter).unwrap_or_else(|| "webhook".into())));
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

/// Trigger every active workflow that has a schedule/cron. A real scheduler
/// would use cron evaluation + timezone handling; this runs the scheduled
/// workflows on demand. Returns the number of workflows triggered.
pub async fn run_scheduled_workflows(ctx: &AppContext) -> usize {
    let Ok(workflows) = load_active_workflows(ctx).await else {
        return 0;
    };
    let mut count = 0;
    for wf in &workflows {
        if wf.is_scheduled() {
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

/// A helper for extracting schedule config for the scheduler UI.
pub fn schedule_summary(doc: &OwsDocument) -> serde_json::Value {
    serde_json::json!({
        "cron": doc.cron(),
        "scheduled": doc.is_scheduled(),
        "events": event_filters(doc).iter().filter_map(|f| filter_event_type(f)).collect::<Vec<_>>(),
    })
}

/// Internal helper used by the scheduler; re-exported for completeness.
pub fn _strategy(_: &EventConsumptionStrategyDefinition) -> &'static str {
    "event"
}
