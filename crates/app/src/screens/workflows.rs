//! Workflows list / management screen.

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{use_global, Route};
use crate::components::{Badge, Button, Card, ConfirmDialog, EmptyState, IconButton, Spinner, TextField};

/// A user-triggered async action. The handler only sets the signal; a
/// `use_effect` performs the actual async work (spawn does not run when called
/// directly from an event handler on the wasm target).
#[derive(Clone)]
enum WfAction {
    Create(String),
    SetActive(i64, bool),
    Duplicate(i64),
    Delete(i64),
    Export(i64, String),
    Import(serde_json::Value),
}

/// The Workflows overview: list, search, filters, activate/deactivate,
/// duplicate/delete, create, and open the visual editor.
#[component]
pub fn Workflows() -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut items: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut loading = use_signal(|| true);
    let mut search = use_signal(|| String::new());
    let mut active_filter = use_signal(|| String::from("all"));
    let mut to_delete: Signal<Option<i64>> = use_signal(|| None);
    let mut route = global.route;
    let mut action: Signal<Option<WfAction>> = use_signal(|| None);
    let mut create_name = use_signal(|| String::new());
    let mut creating = use_signal(|| false);
    let mut show_create = use_signal(|| false);
    let mut show_import = use_signal(|| false);
    let mut import_text = use_signal(|| String::new());
    let mut importing = use_signal(|| false);
    let mut export_data: Signal<Option<(String, String)>> = use_signal(|| None);

    // Initial load.
    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            spawn(async move {
                if let Ok(v) = client.workflow_list(None, None).await {
                    items.set(v["data"].as_array().cloned().unwrap_or_default());
                }
                loading.set(false);
            });
        }
    });

    // Dispatcher for all async workflow actions.
    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            // Read reactively before taking so the effect re-runs when the
            // action is set (Signal::take alone is a write, not a read).
            let a = if action().is_some() {
                action.take()
            } else {
                None
            };
            match a {
                Some(WfAction::Create(name)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut creating2 = creating;
                    let mut its = items;
                    let mut show = show_create;
                    let mut route2 = route;
                    spawn(async move {
                        creating2.set(true);
                        match client.workflow_create(&name).await {
                            Ok(v) => {
                                if let Some(id) = v["data"]["id"].as_i64() {
                                    if let Ok(list) = client.workflow_list(None, None).await {
                                        its.set(list["data"].as_array().cloned().unwrap_or_default());
                                    }
                                    route2.set(Route::WorkflowEditor(id));
                                }
                                g.toast("Workflow created", "success");
                            }
                            Err(e) => g.toast(format!("Create failed: {e}"), "danger"),
                        }
                        creating2.set(false);
                        show.set(false);
                    });
                }
                Some(WfAction::SetActive(id, active)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut its = items;
                    spawn(async move {
                        if let Ok(_) = client.workflow_set_active(id, active).await {
                            if let Ok(v) = client.workflow_list(None, None).await {
                                its.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast(if active { "Workflow activated" } else { "Workflow deactivated" }, "success");
                        }
                    });
                }
                Some(WfAction::Duplicate(id)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut its = items;
                    spawn(async move {
                        if let Ok(_) = client.workflow_duplicate(id).await {
                            if let Ok(v) = client.workflow_list(None, None).await {
                                its.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast("Workflow duplicated", "success");
                        }
                    });
                }
                Some(WfAction::Delete(id)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut its = items;
                    let mut del = to_delete;
                    spawn(async move {
                        if let Ok(_) = client.workflow_delete(id).await {
                            if let Ok(v) = client.workflow_list(None, None).await {
                                its.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast("Workflow deleted", "success");
                        }
                        del.set(None);
                    });
                }
                Some(WfAction::Export(id, name)) => {
                    let client = client.clone();
                    let mut exp = export_data;
                    let en = name.clone();
                    spawn(async move {
                        if let Ok(v) = client.workflow_export(id).await {
                            exp.set(Some((en.clone(), serde_json::to_string_pretty(&v).unwrap_or_default())));
                        }
                    });
                }
                Some(WfAction::Import(v)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut its = items;
                    let mut importing2 = importing;
                    let mut show = show_import;
                    spawn(async move {
                        importing2.set(true);
                        match client.workflow_import(&v).await {
                            Ok(_) => {
                                if let Ok(list) = client.workflow_list(None, None).await {
                                    its.set(list["data"].as_array().cloned().unwrap_or_default());
                                }
                                g.toast("Workflow imported", "success");
                            }
                            Err(e) => g.toast(format!("Import failed: {e}"), "danger"),
                        }
                        importing2.set(false);
                        show.set(false);
                    });
                }
                None => {}
            }
        }
    });

    // Precompute the workflow table rows.
    let mut workflow_rows: Vec<Element> = Vec::new();
    let filter = active_filter();
    for item in items().iter() {
        let id = item["id"].as_i64().unwrap_or(0);
        let name = item["name"].as_str().unwrap_or("").to_string();
        let active = item["active"].as_bool().unwrap_or(false);
        if filter == "active" && !active {
            continue;
        }
        if filter == "inactive" && active {
            continue;
        }
        if !search().is_empty() && !name.to_lowercase().contains(&search().to_lowercase()) {
            continue;
        }
        let trigger = item["trigger"].as_str().unwrap_or("-").to_string();
        let nodes = item["taskCount"].as_i64().unwrap_or(0);
        let runs = item["executionCount"].as_i64().unwrap_or(0);
        let last = item["lastExecution"]["status"].as_str().unwrap_or("-").to_string();
        let mut open_editor = route;
        let mut act = action;
        let mut del = to_delete;
        let exp_n = name.clone();
        let nid = id;
        workflow_rows.push(rsx! {
            tr { style: "border-bottom:1px solid {color::NEUTRAL_150};",
                td { style: "padding:12px 16px;",
                    button { style: "background:none; border:none; color:{color::PRIMARY_600}; font-weight:600; cursor:pointer; font-size:14px; text-align:left;",
                        onclick: move |_| open_editor.set(Route::WorkflowEditor(nid)),
                        "{name}"
                    }
                }
                td { style: "padding:12px 16px;",
                    if active { Badge { text: "Active".to_string(), kind: "published".to_string() } } else { Badge { text: "Inactive".to_string(), kind: "neutral".to_string() } }
                }
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_700};", "{trigger}" }
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_700};", "{nodes}" }
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_700};", "{runs}" }
                td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_600};", "{last}" }
                td { style: "padding:12px 16px;",
                    div { style: "display:flex; gap:4px;",
                        IconButton { name: "toggle".to_string(), aria_label: if active { "Deactivate".to_string() } else { "Activate".to_string() }, on_click: move |_| act.set(Some(WfAction::SetActive(nid, !active))) }
                        IconButton { name: "refresh".to_string(), aria_label: "Duplicate".to_string(), on_click: move |_| act.set(Some(WfAction::Duplicate(nid))) }
                        IconButton { name: "braces".to_string(), aria_label: "Export".to_string(), on_click: move |_| act.set(Some(WfAction::Export(nid, exp_n.clone()))) }
                        IconButton { name: "trash".to_string(), variant: "danger".to_string(), aria_label: "Delete".to_string(), on_click: move |_| del.set(Some(nid)) }
                    }
                }
            }
        });
    }

    rsx! {
        div { style: "padding:32px; max-width:1200px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:24px;",
                div { style: "display:flex; flex-direction:column; gap:4px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "Workflows" }
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Build and run visual workflow automations." }
                }
                div { style: "display:flex; gap:8px;",
                    Button { label: "Import".to_string(), variant: "secondary".to_string(), on_click: move |_| show_import.set(true) }
                    Button { label: "Create Workflow".to_string(), on_click: move |_| show_create.set(true) }
                }
            }

            // Toolbar: search + filter
            div { style: "display:flex; gap:12px; margin-bottom:16px; align-items:center;",
                div { style: "flex:1; max-width:360px;",
                    TextField {
                        value: search(),
                        placeholder: "Search workflows".to_string(),
                        oninput: move |v| search.set(v),
                    }
                }
                div { style: "display:flex; gap:4px;",
                    FilterChip { label: "All".to_string(), active: active_filter() == "all", on_click: move |_| active_filter.set("all".into()) }
                    FilterChip { label: "Active".to_string(), active: active_filter() == "active", on_click: move |_| active_filter.set("active".into()) }
                    FilterChip { label: "Inactive".to_string(), active: active_filter() == "inactive", on_click: move |_| active_filter.set("inactive".into()) }
                }
            }

            if loading() {
                div { style: "display:flex; justify-content:center; padding:48px;", Spinner { size: 28 } }
            } else if items().is_empty() {
                EmptyState {
                    title: "No workflows yet".to_string(),
                    subtitle: "Create your first workflow to start automating your content pipeline.".to_string(),
                    icon: "layers".to_string(),
                    Button { label: "Create Workflow".to_string(), on_click: move |_| show_create.set(true) }
                }
            } else {
                Card {
                    header: format!("{} workflows", items().len()),
                    div { style: "overflow-x:auto;",
                        table { style: "width:100%; border-collapse:collapse;",
                            thead {
                                tr {
                                    TableTh { label: "Name".to_string() }
                                    TableTh { label: "Status".to_string() }
                                    TableTh { label: "Trigger".to_string() }
                                    TableTh { label: "Nodes".to_string() }
                                    TableTh { label: "Runs".to_string() }
                                    TableTh { label: "Last run".to_string() }
                                    TableTh { label: "Actions".to_string() }
                                }
                            }
                            tbody {
                                {workflow_rows.into_iter()}
                            }
                        }
                    }
                }
            }
        }

        // Create workflow modal
        if show_create() {
            crate::components::Modal {
                title: "Create Workflow".to_string(),
                on_close: move |_| show_create.set(false),
                div { style: "display:flex; flex-direction:column; gap:16px;",
                    TextField {
                        value: create_name(),
                        label: "Workflow name".to_string(),
                        placeholder: "e.g. Publish Notification".to_string(),
                        oninput: move |v| create_name.set(v),
                    }
                    div { style: "display:flex; justify-content:flex-end; gap:12px;",
                        Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_create.set(false) }
                        Button {
                            label: "Create".to_string(),
                            loading: creating(),
                            disabled: create_name().trim().is_empty(),
                            on_click: move |_| action.set(Some(WfAction::Create(create_name()))),
                        }
                    }
                }
            }
        }

        // Delete confirm dialog
        if let Some(id) = to_delete() {
            ConfirmDialog {
                title: "Delete Workflow".to_string(),
                message: "This will permanently delete the workflow and its execution history.".to_string(),
                confirm_label: "Delete".to_string(),
                on_cancel: move |_| to_delete.set(None),
                on_confirm: move |_| action.set(Some(WfAction::Delete(id))),
            }
        }

        // Import workflow modal
        if show_import() {
            crate::components::Modal {
                title: "Import Workflow".to_string(),
                on_close: move |_| show_import.set(false),
                div { style: "display:flex; flex-direction:column; gap:16px;",
                    crate::components::TextArea {
                        value: import_text(),
                        label: "Workflow JSON".to_string(),
                        rows: 10,
                        hint: "Paste a workflow JSON document exported from Ferris.".to_string(),
                        oninput: move |v| import_text.set(v),
                    }
                    div { style: "display:flex; justify-content:flex-end; gap:12px;",
                        Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_import.set(false) }
                        Button { label: "Import".to_string(), loading: importing(), disabled: import_text().trim().is_empty(),
                            on_click: move |_| {
                                let parsed = serde_json::from_str::<serde_json::Value>(&import_text()).unwrap_or(serde_json::Value::Null);
                                action.set(Some(WfAction::Import(parsed)));
                            }
                        }
                    }
                }
            }
        }

        // Export workflow modal
        if let Some((exp_name, exp_json)) = export_data().as_ref() {
            crate::components::Modal {
                title: format!("Export: {exp_name}"),
                on_close: move |_| export_data.set(None),
                div { style: "display:flex; flex-direction:column; gap:16px;",
                    pre { style: "background:{color::NEUTRAL_100}; padding:12px; border-radius:6px; font-size:12px; max-height:60vh; overflow:auto; color:{color::NEUTRAL_800}; white-space:pre-wrap;", "{exp_json}" }
                    div { style: "display:flex; justify-content:flex-end;",
                        Button { label: "Close".to_string(), variant: "secondary".to_string(), on_click: move |_| export_data.set(None) }
                    }
                }
            }
        }
    }
}

#[component]
fn TableTh(label: String) -> Element {
    rsx! {
        th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "{label}" }
    }
}

#[component]
fn FilterChip(label: String, active: bool, on_click: EventHandler<MouseEvent>) -> Element {
    let style = if active {
        format!(
            "padding:8px 14px; border-radius:4px; border:1px solid {p}; background:{p}; color:#fff; font-size:13px; font-weight:600; cursor:pointer;",
            p = color::PRIMARY_600
        )
    } else {
        format!(
            "padding:8px 14px; border-radius:4px; border:1px solid {c}; background:#fff; color:{t}; font-size:13px; font-weight:600; cursor:pointer;",
            c = color::NEUTRAL_200, t = color::NEUTRAL_700
        )
    };
    rsx! {
        button { style: "{style}", onclick: move |e| on_click.call(e), "{label}" }
    }
}
