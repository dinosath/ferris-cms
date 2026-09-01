//! Visual OWS workflow editor — a task-list editor over Open Workflow DSL
//! definitions.
//!
//! The editor is a thin client over the backend: all state is an `OwsDocument`
//! (the canonical Open Workflow DSL JSON) that is saved/loaded through
//! `client-core`. It lists the workflow's tasks (`definition.do`), lets the user
//! add tasks from the catalog, edit their configuration, remove/reorder them,
//! execute the workflow and inspect the resulting task runs.

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{use_global, Route};
use crate::components::{Button, Icon, IconButton, TextField};

/// Extract the ordered list of `(task_name, task_json)` from an OWS document.
fn tasks_of(wf: &serde_json::Value) -> Vec<(String, serde_json::Value)> {
    wf["definition"]["do"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    entry.as_object().and_then(|obj| obj.iter().next()).map(|(k, v)| (k.clone(), v.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn set_tasks(wf: &mut serde_json::Value, tasks: Vec<(String, serde_json::Value)>) {
    let arr: Vec<serde_json::Value> = tasks
        .into_iter()
        .map(|(k, v)| serde_json::json!({ k: v }))
        .collect();
    wf["definition"]["do"] = serde_json::Value::Array(arr);
}

/// The editor component.
#[component]
pub fn WorkflowEditor(workflow_id: i64) -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut route = global.route;

    let mut wf: Signal<Option<serde_json::Value>> = use_signal(|| None);
    let mut library: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut content_types: Signal<Vec<(String, String)>> = use_signal(|| vec![]);
    let mut selected: Signal<Option<String>> = use_signal(|| None);
    let mut loading = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut executing = use_signal(|| false);
    let mut dirty = use_signal(|| false);
    let mut search = use_signal(|| String::new());
    let mut category = use_signal(|| String::from("All"));
    let mut execution: Signal<Option<serde_json::Value>> = use_signal(|| None);
    let mut new_task = use_signal(|| String::new());

    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            let id = workflow_id;
            spawn(async move {
                if let Ok(v) = client.workflow_get(id).await {
                    wf.set(Some(v["data"].clone()));
                }
                if let Ok(l) = client.workflow_node_library().await {
                    library.set(l["data"].as_array().cloned().unwrap_or_default());
                }
                if let Ok(ct) = client.workflow_content_types().await {
                    let opts = ct["data"].as_array().map(|a| a.iter().filter_map(|x| {
                        let uid = x["uid"].as_str().unwrap_or("").to_string();
                        let name = x["displayName"].as_str().unwrap_or("").to_string();
                        if uid.is_empty() { None } else { Some((uid, name)) }
                    }).collect()).unwrap_or_default();
                    content_types.set(opts);
                }
                loading.set(false);
            });
        }
    });

    let tasks = wf().as_ref().map(tasks_of).unwrap_or_default();
    let wf_name = wf().as_ref().and_then(|w| w["definition"]["document"]["name"].as_str().map(|s| s.to_string())).unwrap_or_else(|| "Untitled".into());
    let active = wf().as_ref().and_then(|w| w["active"].as_bool()).unwrap_or(false);
    let selected_id = selected();
    let selected_task = selected_id.as_ref().and_then(|id| tasks.iter().find(|(n, _)| n == id).cloned());

    // Async actions (spawn must run from an effect).
    let mut save_req = use_signal(|| 0u32);
    let mut execute_req = use_signal(|| 0u32);
    let mut act_req: Signal<Option<bool>> = use_signal(|| None);

    use_effect({
        let client = client.clone();
        move || {
            if save_req() > 0 {
                let n = save_req();
                save_req.set(0);
                let client = client.clone();
                let mut saving2 = saving;
                let mut dirty2 = dirty;
                let def = wf().clone().unwrap_or(serde_json::json!({}));
                let id = workflow_id;
                spawn(async move {
                    let _ = n;
                    saving2.set(true);
                    let _ = client.workflow_save(id, &def).await;
                    saving2.set(false);
                    dirty2.set(false);
                });
            }
        }
    });

    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if execute_req() > 0 {
                let n = execute_req();
                execute_req.set(0);
                let client = client.clone();
                let mut g = g.clone();
                let mut executing2 = executing;
                let mut exec = execution;
                let id = workflow_id;
                spawn(async move {
                    let _ = n;
                    executing2.set(true);
                    match client.workflow_execute(id, &serde_json::json!({})).await {
                        Ok(v) => {
                            if let Some(eid) = v["data"]["executionId"].as_i64() {
                                if let Ok(d) = client.execution_get(eid).await {
                                    exec.set(Some(d.clone()));
                                }
                            }
                            g.toast("Execution started", "success");
                        }
                        Err(e) => g.toast(format!("Execution failed: {e}"), "danger"),
                    }
                    executing2.set(false);
                });
            }
        }
    });

    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if let Some(active) = act_req() {
                act_req.set(None);
                let client = client.clone();
                let mut g = g.clone();
                let mut wf2 = wf;
                let id = workflow_id;
                spawn(async move {
                    if let Ok(v) = client.workflow_set_active(id, active).await {
                        wf2.set(Some(v["data"].clone()));
                        g.toast(if active { "Workflow activated" } else { "Workflow deactivated" }, "success");
                    }
                });
            }
        }
    });

    // Task cards.
    let mut task_cards: Vec<Element> = Vec::new();
    for (name, task) in tasks.iter() {
        let tname = name.clone();
        let is_sel = selected_id == Some(tname.clone());
        let ttype = task_type_label(task);
        let def = library().iter().find(|d| d["nodeType"] == ttype || d["displayName"] == ttype).cloned();
        let cat = def.as_ref().and_then(|d| d["category"].as_str()).unwrap_or("Core").to_string();
        let cat_color = category_color(&cat).to_string();
        let mut sel = selected;
        let tname_click = tname.clone();
        let border = if is_sel { color::PRIMARY_600 } else { color::NEUTRAL_200 };
        let bg = if is_sel { color::PRIMARY_100 } else { "#fff" };
        task_cards.push(rsx! {
            div { style: "display:flex; align-items:center; gap:10px; padding:10px 14px; border:1px solid {border}; border-left:4px solid {cat_color}; border-radius:6px; background:{bg}; cursor:pointer;",
                onclick: move |_| sel.set(Some(tname_click.clone())),
                div { style: "flex:1; min-width:0;",
                    div { style: "font-size:13px; font-weight:600; color:{color::NEUTRAL_900};", "{tname}" }
                    div { style: "font-size:11px; color:{color::NEUTRAL_500};", "{ttype}" }
                }
            }
        });
    }

    // Execution overlay badges.
    let mut exec_badges: Vec<Element> = Vec::new();
    let mut overlay_exec_id = 0i64;
    let mut overlay_exec_status = String::new();
    let mut status_bg = color::NEUTRAL_100;
    let mut status_fg = color::NEUTRAL_600;
    if let Some(exec) = execution().as_ref() {
        overlay_exec_id = exec["data"]["id"].as_i64().unwrap_or(0);
        overlay_exec_status = exec["data"]["status"].as_str().unwrap_or("-").to_string();
        status_bg = if overlay_exec_status == "success" { color::SUCCESS_100 } else if overlay_exec_status == "failed" { color::DANGER_100 } else { color::NEUTRAL_100 };
        status_fg = if overlay_exec_status == "success" { color::SUCCESS_700 } else if overlay_exec_status == "failed" { color::DANGER_700 } else { color::NEUTRAL_600 };
        let runs = exec["nodeRuns"].as_array().cloned().unwrap_or_default();
        for run in runs.iter() {
            let task_name = run["taskName"].as_str().unwrap_or("-").to_string();
            let status = run["status"].as_str().unwrap_or("notExecuted").to_string();
            let sc = match status.as_str() { "success" => color::SUCCESS_600, "failed" => color::DANGER_600, "running" => color::WARNING_600, _ => color::NEUTRAL_300 };
            exec_badges.push(rsx! {
                div { style: "border:1px solid {color::NEUTRAL_150}; border-left:4px solid {sc}; border-radius:4px; padding:6px 10px; font-size:12px; color:{color::NEUTRAL_800}; background:{color::NEUTRAL_0};",
                    "{task_name} · {status}"
                }
            });
        }
    }

    // Category chips (precomputed to avoid rsx `let` parsing edge cases).
    let mut cat_chips: Vec<Element> = Vec::new();
    for cat_label in ["All", "Trigger", "Flow", "Data", "Integration", "Core"] {
        let cat = cat_label.to_string();
        let active_c = category() == cat;
        let style = if active_c {
            format!("padding:4px 8px; border-radius:4px; border:none; background:{p}; color:#fff; font-size:12px; font-weight:600; cursor:pointer;", p = color::PRIMARY_600)
        } else {
            format!("padding:4px 8px; border-radius:4px; border:none; background:{n}; color:{t}; font-size:12px; font-weight:600; cursor:pointer;", n = color::NEUTRAL_100, t = color::NEUTRAL_700)
        };
        let mut cat_set = category;
        cat_chips.push(rsx! {
            button { style: "{style}", onclick: move |_| cat_set.set(cat.clone()), "{cat_label}" }
        });
    }

    // Library items (precomputed).
    let mut lib_items: Vec<Element> = Vec::new();
    for d in library() {
        let matches_cat = category() == "All" || d["category"].as_str() == Some(category().as_str());
        let q = search().to_lowercase();
        let matches_search = q.is_empty()
            || d["displayName"].as_str().unwrap_or("").to_lowercase().contains(&q)
            || d["nodeType"].as_str().unwrap_or("").to_lowercase().contains(&q);
        if matches_cat && matches_search {
            let node_type = d["nodeType"].as_str().unwrap_or("").to_string();
            let display = d["displayName"].as_str().unwrap_or("").to_string();
            let cat2 = d["category"].as_str().unwrap_or("Core").to_string();
            let cat_color = category_color(&cat2).to_string();
            let mut wf2 = wf;
            let mut dirty2 = dirty;
            let mut newt = new_task;
            lib_items.push(rsx! {
                button { style: "display:flex; align-items:center; gap:10px; padding:8px 10px; border:none; background:transparent; border-radius:6px; cursor:pointer; text-align:left; width:100%;",
                    onclick: move |_| {
                        newt.set(node_type.clone());
                        add_task(&mut wf2, &mut dirty2, node_type.clone());
                    },
                    div { style: "width:24px;height:24px;border-radius:4px;background:{cat_color}22;display:flex;align-items:center;justify-content:center;color:{cat_color};", Icon { name: "puzzle".to_string(), size: 13 } }
                    div { style: "flex:1; min-width:0;",
                        div { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_900};", "{display}" }
                        div { style: "font-size:10px; color:{color::NEUTRAL_500}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;", "{node_type}" }
                    }
                }
            });
        }
    }

    // Properties panel (precomputed).
    let props_panel = selected_task.as_ref().map(|(tname, task)| {
        let tname_name = tname.clone();
        let tname_del = tname.clone();
        let tname_upd = tname.clone();
        let mut sel = selected;
        let mut wf2 = wf;
        let mut dirty2 = dirty;
        let ct = content_types();
        rsx! {
            PropertiesPanel {
                name: tname_name,
                task_type: task_type_label(task),
                content_types: ct,
                on_close: move |_| sel.set(None),
                on_delete: move |_| { remove_task(&mut wf2, &mut dirty2, &tname_del); sel.set(None); },
                on_update: move |(k, v): (String, String)| { update_task(&mut wf2, &mut dirty2, &tname_upd, k, v); },
            }
        }
    });

    rsx! {
        div { style: "display:flex; flex-direction:column; height:calc(100vh - 56px); overflow:hidden;",
            div { style: "display:flex; align-items:center; gap:12px; height:56px; padding:0 16px; border-bottom:1px solid {color::NEUTRAL_150}; background:#fff; flex-shrink:0;",
                IconButton { name: "close".to_string(), aria_label: "Back".to_string(), on_click: move |_| route.set(Route::Workflows) }
                div { style: "display:flex; flex-direction:column; min-width:0;",
                    span { style: "font-size:14px; font-weight:600; color:{color::NEUTRAL_900}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;", "{wf_name}" }
                    span { style: "font-size:11px; color:{color::NEUTRAL_500};",
                        if active { "Active" } else { "Inactive" }
                        if dirty() { " · unsaved changes" }
                    }
                }
                div { style: "flex:1;" }
                Button { label: "Save".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), disabled: !dirty() || saving(), loading: saving(), on_click: move |_| save_req.set(save_req() + 1) }
                Button { label: "Execute".to_string(), size: "sm".to_string(), loading: executing(), disabled: executing(), on_click: move |_| execute_req.set(execute_req() + 1) }
                if active {
                    Button { label: "Deactivate".to_string(), variant: "danger-light".to_string(), size: "sm".to_string(), on_click: move |_| act_req.set(Some(false)) }
                } else {
                    Button { label: "Activate".to_string(), variant: "success".to_string(), size: "sm".to_string(), on_click: move |_| act_req.set(Some(true)) }
                }
            }

            div { style: "display:flex; flex:1; min-height:0;",
                // Left: library
                div { style: "width:260px; min-width:260px; border-right:1px solid {color::NEUTRAL_150}; background:#fff; display:flex; flex-direction:column; overflow:hidden;",
                    div { style: "padding:12px; border-bottom:1px solid {color::NEUTRAL_150};",
                        span { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; margin-bottom:8px; display:block;", "OWS CATALOG" }
                        TextField { value: search, placeholder: "Search tasks".to_string(), oninput: move |v| search.set(v) }
                    }
                    div { style: "display:flex; flex-wrap:wrap; gap:4px; padding:8px 12px; border-bottom:1px solid {color::NEUTRAL_150};",
                        {cat_chips.into_iter()}
                    }
                    div { style: "flex:1; overflow-y:auto; padding:8px; display:flex; flex-direction:column; gap:4px;",
                        {lib_items.into_iter()}
                    }
                }

                // Center: task list
                div { style: "flex:1; overflow-y:auto; padding:20px; display:flex; flex-direction:column; gap:10px; background:#F7F7FA;",
                    if tasks.is_empty() {
                        div { style: "text-align:center; color:{color::NEUTRAL_500}; font-size:13px; padding:48px;", "No tasks yet. Add tasks from the OWS catalog on the left." }
                    } else {
                        {task_cards.into_iter()}
                    }
                }

                // Right: properties panel
                {props_panel}
            }

            if let Some(exec) = execution() {
                div { style: "border-top:1px solid {color::NEUTRAL_150}; background:#fff; max-height:220px; overflow:auto;",
                    div { style: "padding:12px 16px; display:flex; align-items:center; gap:12px;",
                        span { style: "font-weight:600; font-size:14px; color:{color::NEUTRAL_900};", "Execution #{overlay_exec_id}" }
                        span { style: "background:{status_bg}; color:{status_fg}; padding:4px 10px; border-radius:4px; font-size:12px; font-weight:600;", "{overlay_exec_status}" }
                        span { style: "flex:1;" }
                        Button { label: "Close".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| execution.set(None) }
                    }
                    div { style: "display:flex; flex-wrap:wrap; gap:8px; padding:0 16px 12px;", {exec_badges.into_iter()} }
                }
            }
        }
    }
}

fn task_type_label(task: &serde_json::Value) -> String {
    if task.get("call").is_some() {
        task["call"].as_str().unwrap_or("call").to_string()
    } else if task.get("set").is_some() {
        "set".to_string()
    } else if task.get("switch").is_some() {
        "switch".to_string()
    } else if task.get("for").is_some() {
        "for".to_string()
    } else if task.get("do").is_some() {
        "do".to_string()
    } else if task.get("wait").is_some() {
        "wait".to_string()
    } else if task.get("try").is_some() {
        "try".to_string()
    } else if task.get("emit").is_some() {
        "emit".to_string()
    } else if task.get("listen").is_some() {
        "listen".to_string()
    } else if task.get("fork").is_some() {
        "fork".to_string()
    } else if task.get("raise").is_some() {
        "raise".to_string()
    } else if task.get("run").is_some() {
        "run".to_string()
    } else {
        "task".to_string()
    }
}

fn category_color(cat: &str) -> &'static str {
    match cat {
        "Trigger" => color::PRIMARY_600,
        "Flow" => color::WARNING_600,
        "Data" => color::SUCCESS_600,
        "Integration" => color::DANGER_600,
        _ => color::NEUTRAL_600,
    }
}

fn add_task(wf: &mut Signal<Option<serde_json::Value>>, dirty: &mut Signal<bool>, node_type: String) {
    let mut w = wf().clone().unwrap_or(serde_json::json!({}));
    let mut tasks = tasks_of(&w);
    let count = tasks.len();
    let name = format!("{node_type}_{}", count + 1);
    // Create a task skeleton based on the catalog kind.
    let task = if node_type == "set" {
        serde_json::json!({ "set": {} })
    } else {
        serde_json::json!({ "call": node_type, "with": {} })
    };
    tasks.push((name, task));
    set_tasks(&mut w, tasks);
    wf.set(Some(w));
    dirty.set(true);
}

fn remove_task(wf: &mut Signal<Option<serde_json::Value>>, dirty: &mut Signal<bool>, name: &str) {
    let mut w = wf().clone().unwrap_or(serde_json::json!({}));
    let tasks: Vec<_> = tasks_of(&w).into_iter().filter(|(n, _)| n != name).collect();
    set_tasks(&mut w, tasks);
    wf.set(Some(w));
    dirty.set(true);
}

fn update_task(wf: &mut Signal<Option<serde_json::Value>>, dirty: &mut Signal<bool>, name: &str, key: String, value: String) {
    let mut w = wf().clone().unwrap_or(serde_json::json!({}));
    let mut tasks = tasks_of(&w);
    for (n, task) in tasks.iter_mut() {
        if n == name {
            if task.get("set").is_some() {
                if let Some(set) = task["set"].as_object_mut() {
                    set.insert(key.clone(), serde_json::Value::String(value.clone()));
                }
            } else if task.get("call").is_some() {
                if let Some(with) = task["with"].as_object_mut() {
                    with.insert(key.clone(), serde_json::Value::String(value.clone()));
                }
            }
        }
    }
    set_tasks(&mut w, tasks);
    wf.set(Some(w));
    dirty.set(true);
}

/// Right sidebar: task configuration + inspector.
#[component]
fn PropertiesPanel(
    name: String,
    task_type: String,
    content_types: Vec<(String, String)>,
    on_close: EventHandler<MouseEvent>,
    on_delete: EventHandler<MouseEvent>,
    on_update: EventHandler<(String, String)>,
) -> Element {
    rsx! {
        div { style: "width:320px; min-width:320px; border-left:1px solid {color::NEUTRAL_150}; background:#fff; display:flex; flex-direction:column; overflow:hidden;",
            div { style: "padding:12px 16px; border-bottom:1px solid {color::NEUTRAL_150}; display:flex; align-items:center; justify-content:space-between;",
                div { style: "display:flex; flex-direction:column;",
                    span { style: "font-size:14px; font-weight:600; color:{color::NEUTRAL_900};", "{name}" }
                    span { style: "font-size:11px; color:{color::NEUTRAL_500};", "{task_type}" }
                }
                IconButton { name: "close".to_string(), aria_label: "Close".to_string(), on_click: on_close }
            }
            div { style: "flex:1; overflow-y:auto; padding:16px; display:flex; flex-direction:column; gap:12px;",
                span { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "CONFIGURATION" }
                if task_type.contains('.') || task_type == "call" {
                    TextField { value: "method".to_string(), label: "Method".to_string(), placeholder: "GET / POST".to_string(), oninput: move |v| on_update.call(("method".to_string(), v)) }
                    TextField { value: "url".to_string(), label: "URL".to_string(), placeholder: "https://...".to_string(), oninput: move |v| on_update.call(("url".to_string(), v)) }
                } else if task_type == "set" {
                    TextField { value: "key".to_string(), label: "Field name".to_string(), placeholder: "field".to_string(), oninput: move |v| on_update.call(("key".to_string(), v)) }
                    TextField { value: "value".to_string(), label: "Value".to_string(), placeholder: "value".to_string(), oninput: move |v| on_update.call(("value".to_string(), v)) }
                } else {
                    div { style: "font-size:12px; color:{color::NEUTRAL_500};", "Configure this task in the raw workflow JSON or use the catalog to compose tasks." }
                }
                div { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; margin-top:8px;", "TASKS" }
                span { style: "font-size:12px; color:{color::NEUTRAL_500};", "This workflow has {task_type} configured above." }
            }
            div { style: "padding:12px 16px; border-top:1px solid {color::NEUTRAL_150}; display:flex; justify-content:space-between;",
                Button { label: "Delete task".to_string(), variant: "danger".to_string(), size: "sm".to_string(), on_click: on_delete }
            }
        }
    }
}
