//! Workflow Executions screens: list + detail (with per-node runs).

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{use_global, Route};
use crate::components::{Badge, Button, Card, EmptyState, Spinner};

fn status_kind(status: &str) -> &'static str {
    match status {
        "success" => "published",
        "failed" => "danger",
        "running" => "modified",
        "waiting" => "draft",
        "cancelled" => "neutral",
        _ => "neutral",
    }
}

/// The Workflow Executions list screen.
#[component]
pub fn Executions() -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut items: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut loading = use_signal(|| true);
    let mut status_filter = use_signal(|| String::from("all"));
    let route = global.route;

    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            spawn(async move {
                if let Ok(v) = client.execution_list(None, None).await {
                    items.set(v["data"].as_array().cloned().unwrap_or_default());
                }
                loading.set(false);
            });
        }
    });

    let mut rows: Vec<Element> = Vec::new();
    let filter = status_filter();
    for wf_item in items().iter() {
        let id = wf_item["id"].as_i64().unwrap_or(0);
        let status = wf_item["status"].as_str().unwrap_or("").to_string();
        if filter != "all" && filter != status {
            continue;
        }
        let trigger = wf_item["trigger"].as_str().unwrap_or("-").to_string();
        let started = wf_item["startedAt"].as_str().unwrap_or("-").to_string();
        let dur = wf_item["durationMs"].as_i64().map(|d| format!("{d} ms")).unwrap_or_else(|| "-".to_string());
        let mut open = route;
        rows.push(rsx! {
            tr { style: "border-bottom:1px solid {color::NEUTRAL_150}; cursor:pointer;", onclick: move |_| open.set(Route::Execution(id)),
                td { style: "padding:12px 16px; font-size:14px; color:{color::PRIMARY_600}; font-weight:600;", "#{id}" }
                td { style: "padding:12px 16px;", Badge { text: status.clone(), kind: status_kind(&status).to_string() } }
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_700};", "{trigger}" }
                td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_600};", "{started}" }
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_700};", "{dur}" }
            }
        });
    }

    rsx! {
        div { style: "padding:32px; max-width:1200px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:24px;",
                div { style: "display:flex; flex-direction:column; gap:4px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "Workflow Executions" }
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Inspect every run of your workflows." }
                }
                Button { label: "Refresh".to_string(), variant: "secondary".to_string(), on_click: move |_| {
                    let client = client.clone();
                    let mut its = items;
                    let mut loading2 = loading;
                    spawn(async move {
                        loading2.set(true);
                        if let Ok(v) = client.execution_list(None, None).await {
                            its.set(v["data"].as_array().cloned().unwrap_or_default());
                        }
                        loading2.set(false);
                    });
                } }
            }

            div { style: "display:flex; gap:4px; margin-bottom:16px;",
                ExecFilterChip { label: "All".to_string(), active: status_filter() == "all", on_click: move |_| status_filter.set("all".into()) }
                ExecFilterChip { label: "Success".to_string(), active: status_filter() == "success", on_click: move |_| status_filter.set("success".into()) }
                ExecFilterChip { label: "Failed".to_string(), active: status_filter() == "failed", on_click: move |_| status_filter.set("failed".into()) }
                ExecFilterChip { label: "Running".to_string(), active: status_filter() == "running", on_click: move |_| status_filter.set("running".into()) }
            }

            if loading() {
                div { style: "display:flex; justify-content:center; padding:48px;", Spinner { size: 28 } }
            } else if items().is_empty() {
                EmptyState {
                    title: "No executions yet".to_string(),
                    subtitle: "Run a workflow to see its executions here.".to_string(),
                    icon: "list".to_string(),
                }
            } else {
                Card {
                    header: "Execution history".to_string(),
                    table { style: "width:100%; border-collapse:collapse;",
                        thead {
                            tr {
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "ID" }
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Status" }
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Trigger" }
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Started" }
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Duration" }
                            }
                        }
                        tbody { {rows.into_iter()} }
                    }
                }
            }
        }
    }
}

/// Execution detail: shows the workflow's nodes overlaid with run status.
#[component]
pub fn ExecutionDetail(execution_id: i64) -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut data: Signal<Option<serde_json::Value>> = use_signal(|| None);
    let mut loading = use_signal(|| true);
    let mut route = global.route;

    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            let id = execution_id;
            spawn(async move {
                if let Ok(v) = client.execution_get(id).await {
                    data.set(Some(v));
                }
                loading.set(false);
            });
        }
    });

    let mut run_cards: Vec<Element> = Vec::new();
    let mut exec_status = String::new();
    let mut exec_trigger = String::new();
    let mut exec_dur = String::new();
    let mut exec_error = String::new();
    let mut has_data = false;
    if let Some(d) = data().as_ref() {
        let execution = &d["data"];
        has_data = true;
        exec_status = execution["status"].as_str().unwrap_or("-").to_string();
        exec_trigger = execution["trigger"].as_str().unwrap_or("-").to_string();
        exec_dur = execution["durationMs"].as_i64().map(|x| format!("{x} ms")).unwrap_or_else(|| "-".to_string());
        exec_error = execution["error"].as_str().unwrap_or("").to_string();
        let runs = d["nodeRuns"].as_array().cloned().unwrap_or_default();
        for run in runs.iter() {
            let node_name = run["nodeName"].as_str().unwrap_or("-").to_string();
            let node_type = run["nodeType"].as_str().unwrap_or("").to_string();
            let status = run["status"].as_str().unwrap_or("notExecuted").to_string();
            let dur = run["durationMs"].as_i64().map(|x| format!("{x} ms")).unwrap_or_else(|| "-".to_string());
            let err = run["error"].as_str().unwrap_or("").to_string();
            let input = serde_json::to_string_pretty(&run["input"]).unwrap_or_default();
            let output = serde_json::to_string_pretty(&run["output"]).unwrap_or_default();
            run_cards.push(rsx! {
                div { style: "display:flex; flex-direction:column; border-bottom:1px solid {color::NEUTRAL_150}; padding:12px 16px;",
                    div { style: "display:flex; align-items:center; gap:12px;",
                        Badge { text: status.clone(), kind: status_kind(&status).to_string() }
                        span { style: "font-weight:600; color:{color::NEUTRAL_900}; font-size:14px;", "{node_name}" }
                        span { style: "font-size:12px; color:{color::NEUTRAL_500};", "{node_type}" }
                        span { style: "flex:1;" }
                        span { style: "font-size:12px; color:{color::NEUTRAL_500};", "{dur}" }
                    }
                    if !err.is_empty() {
                        div { style: "margin-top:8px; font-size:13px; color:{color::DANGER_700};", "Error: {err}" }
                    }
                    if !input.is_empty() {
                        div { style: "margin-top:8px;",
                            span { style: "font-size:11px; font-weight:600; color:{color::NEUTRAL_500};", "INPUT" }
                            pre { style: "background:{color::NEUTRAL_100}; padding:8px; border-radius:4px; font-size:12px; overflow:auto; color:{color::NEUTRAL_700};", "{input}" }
                        }
                    }
                    if !output.is_empty() {
                        div { style: "margin-top:8px;",
                            span { style: "font-size:11px; font-weight:600; color:{color::NEUTRAL_500};", "OUTPUT" }
                            pre { style: "background:{color::NEUTRAL_100}; padding:8px; border-radius:4px; font-size:12px; overflow:auto; color:{color::NEUTRAL_700};", "{output}" }
                        }
                    }
                }
            });
        }
    }

    rsx! {
        div { style: "padding:32px; max-width:1200px;",
            div { style: "display:flex; align-items:center; gap:16px; margin-bottom:24px;",
                Button { label: "← Executions".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::WorkflowExecutions) }
                span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "Execution #{execution_id}" }
            }

            if loading() {
                div { style: "display:flex; justify-content:center; padding:48px;", Spinner { size: 28 } }
            } else if has_data {
                div { style: "display:flex; gap:24px; margin-bottom:24px;",
                    StatCard { label: "Status".to_string(), value: exec_status.clone(), kind: status_kind(&exec_status).to_string() }
                    StatCard { label: "Trigger".to_string(), value: exec_trigger.clone(), kind: "neutral".to_string() }
                    StatCard { label: "Duration".to_string(), value: exec_dur.clone(), kind: "neutral".to_string() }
                }
                if !exec_error.is_empty() {
                    div { style: "background:{color::DANGER_100}; border:1px solid {color::DANGER_600}; color:{color::DANGER_700}; padding:12px 16px; border-radius:4px; margin-bottom:24px; font-size:14px;", "Error: {exec_error}" }
                }
                div { style: "display:flex; justify-content:flex-end; gap:8px; margin-bottom:16px;",
                    Button { label: "Retry".to_string(), size: "sm".to_string(), on_click: move |_| {
                        let client = client.clone();
                        let mut loading2 = loading;
                        let mut data2 = data;
                        spawn(async move {
                            if let Ok(_) = client.execution_retry(execution_id).await {
                                if let Ok(v) = client.execution_get(execution_id).await {
                                    data2.set(Some(v));
                                }
                            }
                            loading2.set(false);
                        });
                    } }
                }
                Card {
                    header: "Node runs".to_string(),
                    div { style: "display:flex; flex-direction:column;", {run_cards.into_iter()} }
                }
            }
        }
    }
}

#[component]
fn StatCard(label: String, value: String, kind: String) -> Element {
    let color_val = match kind.as_str() {
        "published" => color::SUCCESS_700,
        "danger" => color::DANGER_700,
        "modified" => color::WARNING_700,
        _ => color::NEUTRAL_800,
    };
    rsx! {
        div { style: "background:#fff; border:1px solid {color::NEUTRAL_150}; border-radius:8px; padding:16px 20px; min-width:160px;",
            span { style: "display:block; font-size:12px; font-weight:600; color:{color::NEUTRAL_500}; margin-bottom:4px;", "{label}" }
            span { style: "font-size:16px; font-weight:600; color:{color_val};", "{value}" }
        }
    }
}

#[component]
fn ExecFilterChip(label: String, active: bool, on_click: EventHandler<MouseEvent>) -> Element {
    let style = if active {
        format!("padding:8px 14px; border-radius:4px; border:1px solid {p}; background:{p}; color:#fff; font-size:13px; font-weight:600; cursor:pointer;", p = color::PRIMARY_600)
    } else {
        format!("padding:8px 14px; border-radius:4px; border:1px solid {c}; background:#fff; color:{t}; font-size:13px; font-weight:600; cursor:pointer;", c = color::NEUTRAL_200, t = color::NEUTRAL_700)
    };
    rsx! {
        button { style: "{style}", onclick: move |e| on_click.call(e), "{label}" }
    }
}
