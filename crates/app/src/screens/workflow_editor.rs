//! Visual workflow editor — an n8n-style canvas with a node library, drag to
//! move, visual connections, a properties panel, and execution overlay.
//!
//! The editor is a thin client over the backend: all state is a `Workflow`
//! JSON that is saved/loaded through `client-core`. The canvas is never the
//! source of truth.

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{use_global, Route};
use crate::components::{Button, Icon, IconButton, TextField};

const NODE_W: f64 = 200.0;
const NODE_H: f64 = 70.0;

/// The editor component.
#[component]
pub fn WorkflowEditor(workflow_id: i64) -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut route = global.route;

    let mut wf: Signal<Option<serde_json::Value>> = use_signal(|| None);
    let mut library: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut selected: Signal<Option<String>> = use_signal(|| None);
    let mut loading = use_signal(|| true);
    let mut saving = use_signal(|| false);
    let mut executing = use_signal(|| false);
    let mut search = use_signal(|| String::new());
    let mut category = use_signal(|| String::from("All"));
    let mut drag: Signal<Option<(String, f64, f64)>> = use_signal(|| None);
    let mut zoom: Signal<f64> = use_signal(|| 1.0);
    let mut pan: Signal<(f64, f64)> = use_signal(|| (0.0, 0.0));
    let mut dirty = use_signal(|| false);
    let mut undo_stack: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut redo_stack: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut execution: Signal<Option<serde_json::Value>> = use_signal(|| None);
    let mut connect_src: Signal<Option<(String, String)>> = use_signal(|| None);

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
                loading.set(false);
            });
        }
    });

    let nodes = wf().as_ref().and_then(|w| w["nodes"].as_array().cloned()).unwrap_or_default();
    let connections = wf().as_ref().and_then(|w| w["connections"].as_array().cloned()).unwrap_or_default();
    let selected_id = selected();
    let wf_name = wf().as_ref().and_then(|w| w["name"].as_str().map(|s| s.to_string())).unwrap_or_else(|| "Untitled".into());
    let active = wf().as_ref().and_then(|w| w["active"].as_bool()).unwrap_or(false);

    // Precompute node elements.
    let mut node_elements: Vec<Element> = Vec::new();
    for node in nodes.iter() {
        let node_id = node["id"].as_str().unwrap_or("").to_string();
        let name = node["name"].as_str().unwrap_or("Node").to_string();
        let node_type = node["nodeType"].as_str().unwrap_or("").to_string();
        let x = node["position"]["x"].as_f64().unwrap_or(0.0);
        let y = node["position"]["y"].as_f64().unwrap_or(0.0);
        let is_sel = selected_id == Some(node_id.clone());
        let is_dragging = drag().as_ref().map(|d| d.0 == node_id).unwrap_or(false);
        let opacity = if is_dragging { "0.85" } else { "1.0" };
        let border_color = if is_sel { color::PRIMARY_600 } else { color::NEUTRAL_200 };
        let def = library().iter().find(|d| d["nodeType"] == node_type).cloned();
        let cat = def.as_ref().and_then(|d| d["category"].as_str()).unwrap_or("Core").to_string();
        let cat_color = category_color(&cat).to_string();
        let icon = node_icon(&node_type).to_string();
        let zoom_at_drag = zoom();
        let mut sel2 = selected;
        let mut drag2 = drag;
        let nid_click = node_id.clone();
        let nid_down = node_id.clone();
        node_elements.push(rsx! {
            div {
                style: "position:absolute; left:{x}px; top:{y}px; width:{NODE_W}px; user-select:none; cursor:grab; opacity:{opacity};",
                onclick: move |_| sel2.set(Some(nid_click.clone())),
                onpointerdown: move |e| {
                    drag2.set(Some((nid_down.clone(), e.client_coordinates().x as f64 - x * zoom_at_drag, e.client_coordinates().y as f64 - y * zoom_at_drag)));
                },
                onpointerup: move |_| drag2.set(None),
                div { style: "background:#fff; border:1px solid {border_color}; border-left:4px solid {cat_color}; border-radius:6px; box-shadow:0 1px 4px rgba(33,33,52,0.10);",
                    div { style: "display:flex; align-items:center; gap:8px; padding:10px 12px;",
                        div { style: "width:24px;height:24px;border-radius:4px;background:{cat_color}22;display:flex;align-items:center;justify-content:center;color:{cat_color};", Icon { name: icon, size: 14 } }
                        div { style: "flex:1; min-width:0;",
                            div { style: "font-size:13px; font-weight:600; color:{color::NEUTRAL_900}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;", "{name}" }
                            div { style: "font-size:11px; color:{color::NEUTRAL_500};", "{node_type}" }
                        }
                    }
                }
            }
        });
    }

    // Precompute connection paths.
    let pos = |id: &str| -> Option<(f64, f64)> {
        nodes.iter().find(|n| n["id"].as_str() == Some(id)).map(|n| {
            let x = n["position"]["x"].as_f64().unwrap_or(0.0);
            let y = n["position"]["y"].as_f64().unwrap_or(0.0);
            (x + NODE_W / 2.0, y + NODE_H / 2.0)
        })
    };
    let mut conn_paths: Vec<Element> = Vec::new();
    for c in connections.iter() {
        let from = c["from"].as_str().unwrap_or("").to_string();
        let to = c["to"].as_str().unwrap_or("").to_string();
        if let (Some((x1, y1)), Some((x2, y2))) = (pos(&from), pos(&to)) {
            let mid = (x1 + x2) / 2.0;
            let path = format!("M {x1} {y1} C {mid} {y1}, {mid} {y2}, {x2} {y2}");
            let color_v = color::PRIMARY_400;
            conn_paths.push(rsx! { path { d: "{path}", stroke: "{color_v}", stroke_width: "2", fill: "none" } });
        }
    }

    // Precompute execution overlay badges.
    let mut exec_badges: Vec<Element> = Vec::new();
    let mut overlay_exec_id = 0i64;
    let mut overlay_exec_status = String::new();
    if let Some(exec) = execution().as_ref() {
        overlay_exec_id = exec["data"]["id"].as_i64().unwrap_or(0);
        overlay_exec_status = exec["data"]["status"].as_str().unwrap_or("-").to_string();
        let runs = exec["nodeRuns"].as_array().cloned().unwrap_or_default();
        for run in runs.iter() {
            let node_name = run["nodeName"].as_str().unwrap_or("-").to_string();
            let status = run["status"].as_str().unwrap_or("notExecuted").to_string();
            let sc = match status.as_str() { "success" => color::SUCCESS_600, "failed" => color::DANGER_600, "running" => color::WARNING_600, _ => color::NEUTRAL_300 };
            exec_badges.push(rsx! {
                div { style: "border:1px solid {color::NEUTRAL_150}; border-left:4px solid {sc}; border-radius:4px; padding:6px 10px; font-size:12px; color:{color::NEUTRAL_800}; background:{color::NEUTRAL_0};",
                    "{node_name} · {status}"
                }
            });
        }
    }

    let selected_node = selected_id.as_ref().and_then(|id| nodes.iter().find(|n| n["id"].as_str() == Some(id.as_str())).cloned());
    let g_exec = global.clone();
    let g_act = global.clone();
    let g_deact = global.clone();
    let client_save = client.clone();
    let client_exec = client.clone();
    let client_act = client.clone();
    let client_deact = client.clone();

    rsx! {
        div { style: "display:flex; flex-direction:column; height:calc(100vh - 56px); overflow:hidden;",
            EditorTopBar {
                name: wf_name,
                active,
                dirty: dirty(),
                saving: saving(),
                executing: executing(),
                on_back: move |_| route.set(Route::Workflows),
                on_undo: move |_| {
                    let snap = wf().clone().unwrap_or(serde_json::json!({}));
                    if let Some(prev) = undo_stack.write().pop() {
                        redo_stack.write().push(snap);
                        wf.set(Some(prev));
                        dirty.set(true);
                    }
                },
                on_redo: move |_| {
                    let snap = wf().clone().unwrap_or(serde_json::json!({}));
                    if let Some(next) = redo_stack.write().pop() {
                        undo_stack.write().push(snap);
                        wf.set(Some(next));
                        dirty.set(true);
                    }
                },
                on_save: move |_| {
                    let client = client_save.clone();
                    let mut saving2 = saving;
                    let def = wf().clone().unwrap_or(serde_json::json!({}));
                    let id = workflow_id;
                    spawn(async move {
                        saving2.set(true);
                        let _ = client.workflow_save(id, &def).await;
                        saving2.set(false);
                        dirty.set(false);
                    });
                },
                on_execute: move |_| {
                    let client = client_exec.clone();
                    let mut executing2 = executing;
                    let mut exec = execution;
                    let mut g = g_exec.clone();
                    let id = workflow_id;
                    spawn(async move {
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
                },
                on_activate: move |_| {
                    let client = client_act.clone();
                    let mut g = g_act.clone();
                    let mut wf2 = wf;
                    let id = workflow_id;
                    spawn(async move {
                        if let Ok(v) = client.workflow_set_active(id, true).await {
                            wf2.set(Some(v["data"].clone()));
                            g.toast("Workflow activated", "success");
                        }
                    });
                },
                on_deactivate: move |_| {
                    let client = client_deact.clone();
                    let mut g = g_deact.clone();
                    let mut wf2 = wf;
                    let id = workflow_id;
                    spawn(async move {
                        if let Ok(v) = client.workflow_set_active(id, false).await {
                            wf2.set(Some(v["data"].clone()));
                            g.toast("Workflow deactivated", "success");
                        }
                    });
                },
            }

            div { style: "display:flex; flex:1; min-height:0;",
                NodeLibrary {
                    library: library(),
                    search: search(),
                    category: category(),
                    on_search: move |v| search.set(v),
                    on_category: move |c| category.set(c),
                    on_add: move |node_type| {
                        undo_stack.write().push(wf().clone().unwrap_or(serde_json::json!({})));
                        let mut wf2 = wf;
                        let mut sel = selected;
                        let mut dirty2 = dirty;
                        let lib = library();
                        spawn(async move {
                            let def = lib.iter().find(|d| d["nodeType"] == node_type).cloned();
                            let node_id = uuid_v4();
                            let posx = 40.0;
                            let posy = 60.0 + (wf2().as_ref().and_then(|w| w["nodes"].as_array()).map(|a| a.len() as f64 * 12.0).unwrap_or(0.0));
                            let mut node = serde_json::json!({
                                "id": node_id,
                                "nodeType": node_type,
                                "name": def.as_ref().and_then(|d| d["displayName"].as_str()).unwrap_or("Node").to_string(),
                                "position": { "x": posx, "y": posy },
                                "parameters": {},
                                "disabled": false
                            });
                            if let Some(d) = def {
                                if let Some(defaults) = d["fields"].as_array() {
                                    let mut params = serde_json::Map::new();
                                    for f in defaults {
                                        if let (Some(n), Some(defv)) = (f["name"].as_str(), f.get("default")) {
                                            params.insert(n.to_string(), defv.clone());
                                        }
                                    }
                                    node["parameters"] = serde_json::Value::Object(params);
                                }
                            }
                            let mut w = wf2().clone().unwrap_or(serde_json::json!({}));
                            let mut arr = w["nodes"].as_array().cloned().unwrap_or_default();
                            arr.push(node);
                            w["nodes"] = serde_json::Value::Array(arr);
                            wf2.set(Some(w));
                            sel.set(Some(node_id));
                            dirty2.set(true);
                        });
                    },
                }

                div {
                    style: "flex:1; position:relative; overflow:hidden; background:#F7F7FA;",
                    onpointermove: move |e| {
                        if let Some((id, ox, oy)) = drag() {
                            let mut wf2 = wf;
                            let mut dirty2 = dirty;
                            let mut w = wf2().clone().unwrap_or(serde_json::json!({}));
                            let mut arr = w["nodes"].as_array().cloned().unwrap_or_default();
                            for n in arr.iter_mut() {
                                if n["id"] == serde_json::json!(id) {
                                    n["position"]["x"] = serde_json::json!((e.client_coordinates().x as f64 - ox) / zoom());
                                    n["position"]["y"] = serde_json::json!((e.client_coordinates().y as f64 - oy) / zoom());
                                }
                            }
                            w["nodes"] = serde_json::Value::Array(arr);
                            wf2.set(Some(w));
                            dirty2.set(true);
                        }
                    },
                    onpointerup: move |_| drag.set(None),
                    onpointercancel: move |_| drag.set(None),
                    svg { style: "position:absolute; inset:0; width:100%; height:100%; pointer-events:none; z-index:1;", {conn_paths.into_iter()} }
                    div { style: "position:absolute; left:16px; top:16px; display:flex; gap:8px; background:#fff; border:1px solid {color::NEUTRAL_150}; border-radius:6px; padding:6px 8px; z-index:5;",
                        button { style: "background:none; border:none; cursor:pointer; color:{color::NEUTRAL_700}; font-size:16px;", onclick: move |_| { let z = zoom(); zoom.set((z * 1.2).clamp(0.4, 2.0)); }, "+" }
                        span { style: "font-size:12px; color:{color::NEUTRAL_600}; align-self:center;", "{((zoom()*100.0) as i32)}%" }
                        button { style: "background:none; border:none; cursor:pointer; color:{color::NEUTRAL_700}; font-size:16px;", onclick: move |_| { let z = zoom(); zoom.set((z * 0.8).clamp(0.4, 2.0)); }, "−" }
                    }
                    div {
                        style: "position:absolute; inset:0; transform:translate({pan().0}px,{pan().1}px) scale({zoom()}); transform-origin:0 0;",
                        {node_elements.into_iter()}
                    }
                    if let Some((src_id, port)) = connect_src() {
                        div { style: "position:absolute; right:16px; bottom:16px; background:#fff; border:1px solid {color::PRIMARY_600}; border-radius:6px; padding:8px 12px; font-size:13px; color:{color::NEUTRAL_800}; z-index:10;",
                            "Connecting from {src_id} ({port}). Click a target node."
                            button { style: "margin-left:8px; background:none; border:none; color:{color::PRIMARY_600}; cursor:pointer;", onclick: move |_| connect_src.set(None), "Cancel" }
                        }
                    }
                }

                PropertiesPanel {
                    node: selected_node,
                    selected: selected_id,
                    on_close: move |_| selected.set(None),
                    on_delete: move |_| {
                        if let Some(id) = selected() {
                            undo_stack.write().push(wf().clone().unwrap_or(serde_json::json!({})));
                            let mut wf2 = wf;
                            let mut sel = selected;
                            let mut dirty2 = dirty;
                            let mut w = wf2().clone().unwrap_or(serde_json::json!({}));
                            let nodes2: Vec<_> = w["nodes"].as_array().cloned().unwrap_or_default().into_iter().filter(|n| n["id"] != serde_json::json!(id)).collect();
                            let conns2: Vec<_> = w["connections"].as_array().cloned().unwrap_or_default().into_iter().filter(|c| c["from"] != serde_json::json!(id) && c["to"] != serde_json::json!(id)).collect();
                            w["nodes"] = serde_json::Value::Array(nodes2);
                            w["connections"] = serde_json::Value::Array(conns2);
                            wf2.set(Some(w));
                            sel.set(None);
                            dirty2.set(true);
                        }
                    },
                    on_update: move |(key, value): (String, String)| {
                        if let Some(id) = selected() {
                            undo_stack.write().push(wf().clone().unwrap_or(serde_json::json!({})));
                            let mut wf2 = wf;
                            let mut dirty2 = dirty;
                            let mut w = wf2().clone().unwrap_or(serde_json::json!({}));
                            let mut arr = w["nodes"].as_array().cloned().unwrap_or_default();
                            for n in arr.iter_mut() {
                                if n["id"] == serde_json::json!(id) {
                                    n["parameters"][&key] = serde_json::Value::String(value.clone());
                                }
                            }
                            w["nodes"] = serde_json::Value::Array(arr);
                            wf2.set(Some(w));
                            dirty2.set(true);
                        }
                    },
                    on_rename: move |new_name: String| {
                        if let Some(id) = selected() {
                            undo_stack.write().push(wf().clone().unwrap_or(serde_json::json!({})));
                            let mut wf2 = wf;
                            let mut dirty2 = dirty;
                            let mut w = wf2().clone().unwrap_or(serde_json::json!({}));
                            let mut arr = w["nodes"].as_array().cloned().unwrap_or_default();
                            for n in arr.iter_mut() {
                                if n["id"] == serde_json::json!(id) {
                                    n["name"] = serde_json::Value::String(new_name.clone());
                                }
                            }
                            w["nodes"] = serde_json::Value::Array(arr);
                            wf2.set(Some(w));
                            dirty2.set(true);
                        }
                    },
                    on_start_connect: move |node_id| connect_src.set(Some((node_id, "main".to_string()))),
                }
            }

            if let Some(exec) = execution() {
                div { style: "border-top:1px solid {color::NEUTRAL_150}; background:#fff; max-height:220px; overflow:auto;",
                    div { style: "padding:12px 16px; display:flex; align-items:center; gap:12px;",
                        span { style: "font-weight:600; font-size:14px; color:{color::NEUTRAL_900};", "Execution #{overlay_exec_id}" }
                        BadgeSmall { text: overlay_exec_status.clone() }
                        span { style: "flex:1;" }
                        Button { label: "Close".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| execution.set(None) }
                    }
                    div { style: "display:flex; flex-wrap:wrap; gap:8px; padding:0 16px 12px;", {exec_badges.into_iter()} }
                }
            }
        }
    }
}

#[component]
fn BadgeSmall(text: String) -> Element {
    let kind = match text.as_str() {
        "success" => color::SUCCESS_100,
        "failed" => color::DANGER_100,
        "running" => color::WARNING_100,
        _ => color::NEUTRAL_100,
    };
    let fg = match text.as_str() {
        "success" => color::SUCCESS_700,
        "failed" => color::DANGER_700,
        "running" => color::WARNING_700,
        _ => color::NEUTRAL_600,
    };
    rsx! {
        span { style: "background:{kind}; color:{fg}; padding:4px 10px; border-radius:4px; font-size:12px; font-weight:600;", "{text}" }
    }
}

fn category_color(cat: &str) -> &'static str {
    match cat {
        "Trigger" => color::PRIMARY_600,
        "Logic" => color::WARNING_600,
        "Data" => color::SUCCESS_600,
        "Integration" => color::DANGER_600,
        _ => color::NEUTRAL_600,
    }
}

fn node_icon(node_type: &str) -> &'static str {
    match node_type {
        "manualTrigger" | "scheduleTrigger" | "webhookTrigger" | "httpTrigger" => "globe",
        "if" | "switch" | "merge" | "split" | "loop" | "forEach" | "filter" | "sort" | "limit" | "wait" => "filter",
        "getContent" | "findContent" | "queryContent" | "createContent" | "updateContent" | "deleteContent" | "publishContent" | "unpublishContent" => "file",
        "httpRequest" | "webhook" | "restApi" | "graphqlRequest" => "globe",
        _ => "puzzle",
    }
}

fn uuid_v4() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("node-{n}-{t}")
}

/// Top toolbar for the editor.
#[component]
fn EditorTopBar(
    name: String,
    active: bool,
    dirty: bool,
    saving: bool,
    executing: bool,
    on_back: EventHandler<MouseEvent>,
    on_undo: EventHandler<MouseEvent>,
    on_redo: EventHandler<MouseEvent>,
    on_save: EventHandler<MouseEvent>,
    on_execute: EventHandler<MouseEvent>,
    on_activate: EventHandler<MouseEvent>,
    on_deactivate: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        div { style: "display:flex; align-items:center; gap:12px; height:56px; padding:0 16px; border-bottom:1px solid {color::NEUTRAL_150}; background:#fff; flex-shrink:0;",
            IconButton { name: "close".to_string(), aria_label: "Back".to_string(), on_click: on_back }
            div { style: "display:flex; flex-direction:column; min-width:0;",
                span { style: "font-size:14px; font-weight:600; color:{color::NEUTRAL_900}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;", "{name}" }
                span { style: "font-size:11px; color:{color::NEUTRAL_500};",
                    if active { "Active" } else { "Inactive" }
                    if dirty { " · unsaved changes" }
                }
            }
            div { style: "flex:1;" }
            IconButton { name: "refresh".to_string(), aria_label: "Undo".to_string(), on_click: on_undo }
            IconButton { name: "refresh".to_string(), aria_label: "Redo".to_string(), on_click: on_redo }
            Button { label: "Save".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), disabled: !dirty || saving, loading: saving, on_click: on_save }
            Button { label: "Execute".to_string(), size: "sm".to_string(), loading: executing, disabled: executing, on_click: on_execute }
            if active {
                Button { label: "Deactivate".to_string(), variant: "danger-light".to_string(), size: "sm".to_string(), on_click: on_deactivate }
            } else {
                Button { label: "Activate".to_string(), variant: "success".to_string(), size: "sm".to_string(), on_click: on_activate }
            }
        }
    }
}

/// Left sidebar: searchable, categorized node library.
#[component]
fn NodeLibrary(
    library: Vec<serde_json::Value>,
    search: String,
    category: String,
    on_search: EventHandler<String>,
    on_category: EventHandler<String>,
    on_add: EventHandler<String>,
) -> Element {
    let categories = vec!["All", "Trigger", "Logic", "Data", "Integration", "Core"];
    let mut cat_chips: Vec<Element> = Vec::new();
    for c in categories.into_iter() {
        let active = category == c;
        let style = if active {
            format!("padding:4px 8px; border-radius:4px; border:none; background:{p}; color:#fff; font-size:12px; font-weight:600; cursor:pointer;", p = color::PRIMARY_600)
        } else {
            format!("padding:4px 8px; border-radius:4px; border:none; background:{n}; color:{t}; font-size:12px; font-weight:600; cursor:pointer;", n = color::NEUTRAL_100, t = color::NEUTRAL_700)
        };
        let cat = c.to_string();
        cat_chips.push(rsx! {
            button { style: "{style}", onclick: move |_| on_category.call(cat.clone()), "{c}" }
        });
    }

    let mut items: Vec<Element> = Vec::new();
    for d in library.iter() {
        let matches_cat = category == "All" || d["category"].as_str() == Some(category.as_str());
        let q = search.to_lowercase();
        let matches_search = q.is_empty()
            || d["displayName"].as_str().unwrap_or("").to_lowercase().contains(&q)
            || d["nodeType"].as_str().unwrap_or("").to_lowercase().contains(&q)
            || d["description"].as_str().unwrap_or("").to_lowercase().contains(&q);
        if !matches_cat || !matches_search {
            continue;
        }
        let node_type = d["nodeType"].as_str().unwrap_or("").to_string();
        let display = d["displayName"].as_str().unwrap_or("").to_string();
        let desc = d["description"].as_str().unwrap_or("").to_string();
        let cat = d["category"].as_str().unwrap_or("Core").to_string();
        let cat_color = category_color(&cat).to_string();
        let icon = node_icon(&node_type).to_string();
        items.push(rsx! {
            button {
                style: "display:flex; align-items:center; gap:10px; padding:8px 10px; border:none; background:transparent; border-radius:6px; cursor:pointer; text-align:left; width:100%;",
                onclick: move |_| on_add.call(node_type.clone()),
                div { style: "width:28px;height:28px;border-radius:6px;background:{cat_color}22;display:flex;align-items:center;justify-content:center;color:{cat_color};flex-shrink:0;", Icon { name: icon, size: 16 } }
                div { style: "flex:1; min-width:0;",
                    div { style: "font-size:13px; font-weight:600; color:{color::NEUTRAL_900};", "{display}" }
                    div { style: "font-size:11px; color:{color::NEUTRAL_500}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;", "{desc}" }
                }
            }
        });
    }

    rsx! {
        div { style: "width:280px; min-width:280px; border-right:1px solid {color::NEUTRAL_150}; background:#fff; display:flex; flex-direction:column; overflow:hidden;",
            div { style: "padding:12px; border-bottom:1px solid {color::NEUTRAL_150};",
                span { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; margin-bottom:8px; display:block;", "NODES" }
                TextField { value: search, placeholder: "Search nodes".to_string(), oninput: on_search }
            }
            div { style: "display:flex; flex-wrap:wrap; gap:4px; padding:8px 12px; border-bottom:1px solid {color::NEUTRAL_150};", {cat_chips.into_iter()} }
            div { style: "flex:1; overflow-y:auto; padding:8px; display:flex; flex-direction:column; gap:4px;", {items.into_iter()} }
        }
    }
}

/// Right sidebar: node configuration + inspector.
#[component]
fn PropertiesPanel(
    node: Option<serde_json::Value>,
    selected: Option<String>,
    on_close: EventHandler<MouseEvent>,
    on_delete: EventHandler<MouseEvent>,
    on_update: EventHandler<(String, String)>,
    on_rename: EventHandler<String>,
    on_start_connect: EventHandler<String>,
) -> Element {
    let mut param_fields: Vec<Element> = Vec::new();
    let mut name = String::new();
    let mut node_type = String::new();
    let mut node_id = String::new();
    if let Some(n) = node.as_ref() {
        node_type = n["nodeType"].as_str().unwrap_or("").to_string();
        name = n["name"].as_str().unwrap_or("Node").to_string();
        node_id = n["id"].as_str().unwrap_or("").to_string();
        if let Some(params) = n["parameters"].as_object() {
            for (key, value) in params.iter() {
                let k = key.clone();
                let v = value.as_str().unwrap_or("").to_string();
                let mut on_upd = on_update;
                param_fields.push(rsx! {
                    TextField { value: v, label: format_node_label(&k), oninput: move |nv| on_upd.call((k.clone(), nv)) }
                });
            }
        }
    }
    let has_params = !param_fields.is_empty();

    rsx! {
        div { style: "width:320px; min-width:320px; border-left:1px solid {color::NEUTRAL_150}; background:#fff; display:flex; flex-direction:column; overflow:hidden;",
            if node.is_some() {
                div { style: "padding:12px 16px; border-bottom:1px solid {color::NEUTRAL_150}; display:flex; align-items:center; justify-content:space-between;",
                    div { style: "display:flex; flex-direction:column;",
                        span { style: "font-size:14px; font-weight:600; color:{color::NEUTRAL_900};", "{name}" }
                        span { style: "font-size:11px; color:{color::NEUTRAL_500};", "{node_type}" }
                    }
                    IconButton { name: "close".to_string(), aria_label: "Close".to_string(), on_click: on_close }
                }
                div { style: "flex:1; overflow-y:auto; padding:16px; display:flex; flex-direction:column; gap:12px;",
                    TextField { value: name, label: "Name".to_string(), oninput: on_rename }
                    div { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; margin-top:8px;", "PARAMETERS" }
                    {param_fields.into_iter()}
                    if !has_params {
                        div { style: "font-size:12px; color:{color::NEUTRAL_500};", "This node has no configuration parameters." }
                    }
                    div { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; margin-top:8px;", "CONNECTIONS" }
                    Button { label: "Connect to another node".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| on_start_connect.call(node_id.clone()) }
                }
                div { style: "padding:12px 16px; border-top:1px solid {color::NEUTRAL_150}; display:flex; justify-content:space-between;",
                    Button { label: "Delete node".to_string(), variant: "danger".to_string(), size: "sm".to_string(), on_click: on_delete }
                }
            } else {
                div { style: "padding:24px; text-align:center; color:{color::NEUTRAL_500}; font-size:13px;",
                    div { style: "font-size:28px; margin-bottom:8px;", "↗" }
                    "Select a node to configure it. Use the node library to add nodes, and the canvas to connect them."
                }
            }
        }
    }
}

fn format_node_label(key: &str) -> String {
    let mut out = String::new();
    for (i, ch) in key.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
        }
        out.push(ch);
    }
    let mut chars = out.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => out,
    }
}
