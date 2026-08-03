//! Content Manager screen — list + create entries (design doc §6).

use api_types::QueryParams;
use core_domain::FieldType;
use core_schema::Schema;
use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::use_global;
use crate::components::{Button, Card, Dropdown, Modal, NavItem, Table, TextField, Toggle};

#[component]
pub fn ContentManager() -> Element {
    let global = use_global();
    let mut schemas = use_signal(Vec::<Schema>::new);
    let mut loaded = use_signal(|| false);
    let mut selected_uid = use_signal(|| None::<String>);
    let mut entries = use_signal(Vec::<serde_json::Value>::new);
    let mut creating = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let g = g_load.clone();
            spawn(async move {
                match g.client.ctb_list().await {
                    Ok(v) => {
                        let types: Vec<Schema> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                    .filter(|s: &Schema| s.kind == core_domain::ContentTypeKind::CollectionType)
                                    .collect()
                            })
                            .unwrap_or_default();
                        schemas.set(types);
                    }
                    Err(e) => status.set(Some(format!("Failed to load: {e}"))),
                }
            });
        }
    });

    let g_entries = global.clone();
    let selected_sig = selected_uid;
    use_effect(move || {
        if let Some(uid) = selected_sig() {
            let g = g_entries.clone();
            let uid = uid.clone();
            spawn(async move {
                match g.client.cm_list(&uid, &QueryParams::default()).await {
                    Ok(resp) => entries.set(resp.data),
                    Err(e) => status.set(Some(format!("Failed to load entries: {e}"))),
                }
            });
        }
    });

    let schemas_list = schemas();
    let nav_items: Vec<(String, String)> = schemas_list
        .iter()
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();
    let selected = schemas_list
        .iter()
        .find(|s| Some(s.uid.as_str().to_string()) == selected_uid())
        .cloned();
    let main_field = selected.as_ref().map(|s| s.main_field()).unwrap_or_default();

    let sidebar_style = format!(
        "width:240px; min-width:240px; background:{}; border-right:1px solid {}; display:flex; flex-direction:column;",
        color::NEUTRAL_0, color::NEUTRAL_150
    );
    let nav_header = format!("padding:16px; font-size:{}; font-weight:600; color:{};", typography::DELTA_SIZE, color::NEUTRAL_900);
    let section_label = format!("padding:4px 16px; font-size:{}; color:{};", typography::LABEL_SIZE, color::NEUTRAL_600);
    let top_bar = format!("display:flex; align-items:center; justify-content:space-between; padding:0 32px; height:56px; border-bottom:1px solid {}; background:{};", color::NEUTRAL_150, color::NEUTRAL_0);
    let title_style = format!("font-size:{}; font-weight:600; color:{};", typography::DELTA_SIZE, color::NEUTRAL_900);
    let count_style = format!("font-size:{}; color:{};", typography::BODY_SIZE, color::NEUTRAL_500);
    let status_style = format!("padding:12px; margin-bottom:16px; border-radius:4px; background:{}; color:{}; font-size:{};", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);
    let count_display = format!("({} entries found)", entries().len());
    let selected_name = selected.as_ref().map(|s| s.info.display_name.clone());

    let g_create = global.clone();

    rsx! {
        div { style: "display:flex; min-height:100vh;",
            div { style: "{sidebar_style}",
                div { style: "{nav_header}", "Content Manager" }
                span { style: "{section_label}", "COLLECTION TYPES" }
                for (uid, display) in nav_items.into_iter() {
                    NavItem {
                        label: display,
                        icon: "stack".to_string(),
                        active: selected_uid() == Some(uid.clone()),
                        onclick: move |_| selected_uid.set(Some(uid.clone())),
                    }
                }
            }

            div { style: "flex:1; min-width:0;",
                div { style: "{top_bar}",
                    div { style: "display:flex; align-items:center; gap:12px;",
                        span { style: "{title_style}",
                            if let Some(name) = &selected_name { "{name}" } else { "Content Manager" }
                        }
                        if selected_name.is_some() {
                            span { style: "{count_style}", "{count_display}" }
                        }
                    }
                    if selected.is_some() {
                        Button { label: "+ Create new entry".to_string(), variant: "primary".to_string(), on_click: move |_| creating.set(true) }
                    }
                }

                div { style: "padding:32px;",
                    if let Some(status) = status() {
                        div { style: "{status_style}", "{status}" }
                    }

                    if selected.is_some() {
                        Card { padding: 0,
                            if entries().is_empty() {
                                div { style: "padding:40px; text-align:center; color:{color::NEUTRAL_600};",
                                    "No entries yet. Create your first entry."
                                }
                            } else {
                                Table {
                                    columns: vec![
                                        ("id".to_string(), "ID".to_string()),
                                        ("main".to_string(), main_field.clone()),
                                        ("state".to_string(), "State".to_string()),
                                    ],
                                    rows: entries().iter().map(|e| {
                                        let id = e.get("id").map(|v| v.to_string()).unwrap_or_default();
                                        let main = e.get(&main_field).map(|v| v.to_string()).unwrap_or_default();
                                        let state = e.get("publicationState").map(|v| v.to_string()).unwrap_or_else(|| "draft".into());
                                        vec![id, main, state]
                                    }).collect(),
                                }
                            }
                        }
                    } else {
                        div { style: "padding:48px; text-align:center; color:{color::NEUTRAL_500};",
                            "Select a collection type to manage its entries."
                        }
                    }
                }
            }
        }

        if creating() {
            if let Some(schema) = &selected {
                CreateEntryModal {
                    schema: schema.clone(),
                    on_close: move |_| creating.set(false),
                    on_create: move |data| {
                        let g = g_create.clone();
                        let uid = selected_uid().unwrap_or_default();
                        spawn(async move {
                            match g.client.cm_create(&uid, &data).await {
                                Ok(_) => status.set(Some("Entry created".to_string())),
                                Err(e) => status.set(Some(format!("Error: {e}"))),
                            }
                        });
                        creating.set(false);
                    },
                }
            }
        }
    }
}

/// Modal that renders a form from a schema and calls back with a JSON object.
#[component]
fn CreateEntryModal(schema: Schema, on_close: EventHandler<MouseEvent>, on_create: EventHandler<serde_json::Value>) -> Element {
    let mut form = use_signal(serde_json::Map::new);
    let scalar: Vec<(String, FieldType, Vec<String>)> = schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type.is_scalar_column())
        .filter(|(_, a)| a.attr_type != FieldType::Password)
        .map(|(name, a)| (name.clone(), a.attr_type, a.enum_values.clone()))
        .collect();

    rsx! {
        Modal { title: "Create an entry".to_string(), width: 640, on_close: move |e| on_close.call(e),
            for (name, ft, enum_values) in scalar.into_iter() {
                match ft {
                    FieldType::Boolean => rsx! {
                        div { key: "{name}", style: "margin-bottom:16px;",
                            Toggle {
                                checked: form().get(&name).and_then(|v| v.as_bool()).unwrap_or(false),
                                label: name.clone(),
                                onchange: move |v| { form.write().insert(name.clone(), serde_json::Value::Bool(v)); }
                            }
                        }
                    },
                    FieldType::Enumeration => rsx! {
                        Dropdown {
                            label: name.clone(),
                            options: enum_values.iter().map(|e| (e.clone(), e.clone())).collect(),
                            value: form().get(&name).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                            onchange: move |v| { form.write().insert(name.clone(), serde_json::Value::String(v)); }
                        }
                    },
                    _ => rsx! {
                        TextField {
                            label: name.clone(),
                            value: form().get(&name).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                            oninput: move |v| { form.write().insert(name.clone(), serde_json::Value::String(v)); }
                        }
                    },
                }
            }
            div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |e| on_close.call(e) }
                Button { label: "Save".to_string(), variant: "primary".to_string(), on_click: move |_| on_create.call(serde_json::Value::Object(form())) }
            }
        }
    }
}
