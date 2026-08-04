//! Content Manager screen — Strapi-style list + entry edit views (design doc §6).
//!
//! Secondary navigation groups content-types into COLLECTION TYPES and
//! SINGLE TYPES, matching the official Strapi Content Manager. The list view
//! renders a configurable table with state badges, bulk actions and
//! pagination. The edit view renders a schema-driven form with
//! Save / Publish / Discard controls.

use api_types::{PaginationParams, QueryParams};
use core_domain::{ContentTypeKind, FieldType};
use core_schema::Schema;
use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::use_global;
use crate::components::{Badge, Button, Card, Checkbox, Dropdown, Modal, NavItem, TextField, Toggle};

/// Marker document id used for a brand-new entry in the edit view.
const NEW_ENTRY: &str = "__new__";

#[component]
pub fn ContentManager() -> Element {
    let global = use_global();
    let mut schemas = use_signal(Vec::<Schema>::new);
    let mut loaded = use_signal(|| false);
    let mut selected_uid = use_signal(|| None::<String>);
    let mut entries = use_signal(Vec::<serde_json::Value>::new);
    let mut total = use_signal(|| 0i64);
    let mut page = use_signal(|| 1i64);
    let mut page_size = use_signal(|| 10i64);
    let mut search = use_signal(String::new);
    let mut creating = use_signal(|| false);
    let mut editing_doc = use_signal(|| None::<String>);
    let mut editing_map = use_signal(serde_json::Map::new);
    let mut selected_ids = use_signal(Vec::<String>::new);
    let mut status = use_signal(|| None::<String>);
    let mut configuring = use_signal(|| false);

    // Load the content-type list once.
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

    // Load entries whenever the selection / pagination changes.
    let g_entries = global.clone();
    use_effect(move || {
        let uid = selected_uid();
        if let Some(uid) = uid {
            let g = g_entries.clone();
            let uid = uid.clone();
            let page = page();
            let page_size = page_size();
            let is_collection = schemas()
                .iter()
                .any(|s| s.uid.as_str() == uid && s.kind == ContentTypeKind::CollectionType);
            let is_single = schemas()
                .iter()
                .any(|s| s.uid.as_str() == uid && s.kind == ContentTypeKind::SingleType);
            // Single types have no list: load their single entry into the edit view.
            if is_single {
                editing_doc.set(Some("default".to_string()));
                spawn(async move {
                    match g.client.cm_get(&uid, "default").await {
                        Ok(resp) => editing_map.set(resp.data.as_object().cloned().unwrap_or_default()),
                        Err(_) => editing_map.set(serde_json::Map::new()),
                    }
                });
                return;
            }
            // Only collection types have a list view.
            if !is_collection {
                return;
            }
            spawn(async move {
                let params = QueryParams {
                    pagination: Some(PaginationParams::Page {
                        page,
                        page_size,
                        with_count: Some(true),
                    }),
                    ..Default::default()
                };
                match g.client.cm_list(&uid, &params).await {
                    Ok(resp) => {
                        total.set(resp.meta.pagination.as_ref().map(|p| p.total).unwrap_or(0));
                        entries.set(resp.data);
                    }
                    Err(e) => status.set(Some(format!("Failed to load entries: {e}"))),
                }
            });
        }
    });

    let all_schemas = schemas();
    let collection_items: Vec<(String, String)> = all_schemas
        .iter()
        .filter(|s| s.kind == ContentTypeKind::CollectionType)
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();
    let single_items: Vec<(String, String)> = all_schemas
        .iter()
        .filter(|s| s.kind == ContentTypeKind::SingleType)
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();

    let selected = all_schemas
        .iter()
        .find(|s| Some(s.uid.as_str().to_string()) == selected_uid())
        .cloned();
    let main_field = selected.as_ref().map(|s| s.main_field()).unwrap_or_default();

    // Client-side search filter on the main field.
    let query = search().trim().to_lowercase();
    let filtered: Vec<serde_json::Value> = if query.is_empty() {
        entries()
    } else {
        entries()
            .into_iter()
            .filter(|e| {
                e.get(&main_field)
                    .map(|v| v.to_string().to_lowercase().contains(&query))
                    .unwrap_or(false)
            })
            .collect()
    };

    // Which view we are showing: editing an entry (collection) or a single type.
    let is_single = selected.as_ref().map(|s| s.kind == ContentTypeKind::SingleType).unwrap_or(false);
    let show_edit = editing_doc().is_some() || is_single;

    // Precompute table rows (id, main field, state, updated, selected).
    let rows_data: Vec<(String, String, String, String, bool)> = filtered
        .iter()
        .map(|e| {
            let id = e.get("documentId").map(|v| v.to_string())
                .or_else(|| e.get("id").map(|v| v.to_string()))
                .unwrap_or_default();
            let main = e.get(&main_field).map(|v| v.to_string()).unwrap_or_default();
            let state = e.get("publicationState").and_then(|v| v.as_str()).unwrap_or("draft").to_string();
            let updated = e.get("updatedAt").map(|v| v.to_string()).unwrap_or_default();
            let selected = selected_ids().contains(&id);
            (id, main, state, updated, selected)
        })
        .collect();

    // Single Fn callbacks reused by every row (capture signals by copy).
    let open_global = global.clone();
    let open_entry = move |id: String| {
        editing_doc.set(Some(id.clone()));
        let g = open_global.clone();
        let uid = selected_uid().unwrap_or_default();
        spawn(async move {
            match g.client.cm_get(&uid, &id).await {
                Ok(resp) => editing_map.set(resp.data.as_object().cloned().unwrap_or_default()),
                Err(_) => editing_map.set(serde_json::Map::new()),
            }
        });
    };
    let toggle_entry = move |(id, on): (String, bool)| {
        let mut ids = selected_ids();
        if on {
            if !ids.contains(&id) {
                ids.push(id);
            }
        } else {
            ids.retain(|x| x != &id);
        }
        selected_ids.set(ids);
    };
    let delete_global = global.clone();
    let delete_entry = move |id: String| {
        let g = delete_global.clone();
        let uid = selected_uid().unwrap_or_default();
        spawn(async move {
            let _ = g.client.cm_delete(&uid, &id).await;
            let params = QueryParams {
                pagination: Some(PaginationParams::Page { page: page(), page_size: page_size(), with_count: Some(true) }),
                ..Default::default()
            };
            if let Ok(resp) = g.client.cm_list(&uid, &params).await {
                total.set(resp.meta.pagination.as_ref().map(|p| p.total).unwrap_or(0));
                entries.set(resp.data);
            }
        });
    };
    let edit_global = global.clone();
    let edit_entry = move |id: String| {
        editing_doc.set(Some(id.clone()));
        let g = edit_global.clone();
        let uid = selected_uid().unwrap_or_default();
        spawn(async move {
            match g.client.cm_get(&uid, &id).await {
                Ok(resp) => editing_map.set(resp.data.as_object().cloned().unwrap_or_default()),
                Err(_) => editing_map.set(serde_json::Map::new()),
            }
        });
    };

    if show_edit {
        if let Some(schema) = selected.clone() {
            let doc = if is_single {
                "default".to_string()
            } else {
                editing_doc().unwrap_or_default()
            };
            let g2 = global.clone();
            return rsx! {
                EntryEditView {
                    schema,
                    document_id: doc,
                    is_single,
                    form: editing_map,
                    on_back: move |_| {
                        editing_doc.set(None);
                        editing_map.set(serde_json::Map::new());
                        selected_uid.set(None);
                    },
                    on_saved: move |_| {
                        let g = g2.clone();
                        let uid = selected_uid();
                        editing_doc.set(None);
                        editing_map.set(serde_json::Map::new());
                        if let Some(uid) = uid {
                            spawn(async move {
                                let params = QueryParams {
                                    pagination: Some(PaginationParams::Page { page: page(), page_size: page_size(), with_count: Some(true) }),
                                    ..Default::default()
                                };
                                let _ = g.client.cm_list(&uid, &params).await;
                            });
                        }
                    },
                }
            };
        }
    }

    // Styles
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
    let toolbar_style = format!("display:flex; align-items:center; gap:12px; padding:16px 32px;");
    let count_display = format!("({} entries found)", total());

    let selected_name = selected.as_ref().map(|s| s.info.display_name.clone());

    let g_create = global.clone();
    let g_delete = g_create.clone();
    let sel_count = selected_ids().len();

    let page_count = if page_size() > 0 {
        (total() as f64 / page_size() as f64).ceil().max(1.0) as i64
    } else {
        1
    };

    rsx! {
        div { style: "display:flex; min-height:100vh;",
            div { style: "{sidebar_style}",
                div { style: "{nav_header}", "Content Manager" }
                span { style: "{section_label}", "COLLECTION TYPES" }
                for (uid, display) in collection_items.into_iter() {
                    NavItem {
                        label: display,
                        icon: "stack".to_string(),
                        active: selected_uid() == Some(uid.clone()),
                        onclick: move |_| {
                            selected_uid.set(Some(uid.clone()));
                            editing_doc.set(None);
                            editing_map.set(serde_json::Map::new());
                            selected_ids.set(Vec::new());
                            page.set(1);
                        },
                    }
                }
                span { style: "{section_label}", "SINGLE TYPES" }
                for (uid, display) in single_items.into_iter() {
                    NavItem {
                        label: display,
                        icon: "grid".to_string(),
                        active: selected_uid() == Some(uid.clone()),
                        onclick: move |_| {
                            // Single types open directly in the edit view.
                            selected_uid.set(Some(uid.clone()));
                            editing_doc.set(Some("default".to_string()));
                        },
                    }
                }
            }

            div { style: "flex:1; min-width:0;",
                div { style: "{top_bar}",
                    div { style: "display:flex; align-items:center; gap:12px;",
                        span { style: "{title_style}",
                            if let Some(name) = &selected_name { "{name}" } else { "Content Manager" }
                        }
                        if selected.is_some() {
                            span { style: "{count_style}", "{count_display}" }
                        }
                    }
                    if selected.is_some() {
                        Button { label: "+ Create new entry".to_string(), variant: "primary".to_string(), on_click: move |_| creating.set(true) }
                    }
                }

                div { style: "padding:0 32px 32px;",
                    if let Some(status) = status() {
                        div { style: "{status_style}", "{status}" }
                    }

                    if selected.is_some() {
                        div { style: "{toolbar_style}",
                            div { style: "flex:1; max-width:320px;",
                                TextField {
                                    value: "{search}",
                                    label: String::new(),
                                    placeholder: "Search".to_string(),
                                    oninput: move |v| search.set(v),
                                }
                            }
                            Button { label: "Configure the view".to_string(), variant: "secondary".to_string(), on_click: move |_| configuring.set(true) }
                        }

                        if !selected_ids().is_empty() {
                            div { style: "display:flex; align-items:center; gap:12px; padding:12px 32px; background:{color::PRIMARY_100};",
                                span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_800};",
                                    "{sel_count} entries selected"
                                }
                                Button { label: "Delete".to_string(), variant: "danger".to_string(), on_click: move |_| {
                                    let ids = selected_ids();
                                    let g = g_delete.clone();
                                    let uid = selected_uid().unwrap_or_default();
                                    spawn(async move {
                                        for id in ids.iter() {
                                            let _ = g.client.cm_delete(&uid, id).await;
                                        }
                                        selected_ids.set(Vec::new());
                                        let params = QueryParams { pagination: Some(PaginationParams::Page { page: 1, page_size: page_size(), with_count: Some(true) }), ..Default::default() };
                                        if let Ok(resp) = g.client.cm_list(&uid, &params).await {
                                            total.set(resp.meta.pagination.as_ref().map(|p| p.total).unwrap_or(0));
                                            entries.set(resp.data);
                                        }
                                    });
                                } }
                            }
                        }

                        Card { padding: 0,
                            if filtered.is_empty() {
                                div { style: "padding:40px; text-align:center; color:{color::NEUTRAL_600};",
                                    if query.is_empty() { "No entries yet. Create your first entry." } else { "No results match your search." }
                                }
                            } else {
                                table { style: "width:100%; border-collapse:collapse; background:#fff;",
                                    thead {
                                        tr { style: "border-bottom:1px solid {color::NEUTRAL_150};",
                                            th { style: "padding:10px 16px; width:40px;",
                                                Checkbox {
                                                    checked: !filtered.is_empty() && selected_ids().len() == filtered.len(),
                                                    label: String::new(),
                                                    onchange: move |on| {
                                                        let all: Vec<String> = filtered.iter().filter_map(|e| e.get("documentId").map(|v| v.to_string()).or_else(|| e.get("id").map(|v| v.to_string()))).collect();
                                                        selected_ids.set(if on { all } else { Vec::new() });
                                                    },
                                                }
                                            }
                                            for (_, label) in [
                                                ("id".to_string(), "ID".to_string()),
                                                ("main".to_string(), main_field.clone()),
                                                ("state".to_string(), "State".to_string()),
                                                ("updatedAt".to_string(), "Updated At".to_string()),
                                                ("actions".to_string(), String::new()),
                                            ] {
                                                th { style: "text-align:left; padding:10px 16px; font-size:{typography::LABEL_SIZE}; font-weight:600; color:{color::NEUTRAL_600};", "{label}" }
                                            }
                                        }
                                    }
                                    tbody {
                                        for (id, main, state, updated, selected) in rows_data {
                                            EntryRow {
                                                id,
                                                main,
                                                state,
                                                updated,
                                                selected,
                                                on_open_entry: open_entry.clone(),
                                                on_toggle_entry: toggle_entry.clone(),
                                                on_edit: edit_entry.clone(),
                                                on_delete: delete_entry.clone(),
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        div { style: "display:flex; align-items:center; justify-content:space-between; padding:16px 32px;",
                            div { style: "display:flex; align-items:center; gap:8px;",
                                span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Rows per page" }
                                select { style: "padding:6px 8px; border:1px solid {color::NEUTRAL_200}; border-radius:4px;",
                                    value: "{page_size}",
                                    onchange: move |e| {
                                        if let Ok(v) = e.value().parse::<i64>() { page_size.set(v); page.set(1); }
                                    },
                                    for n in [10, 25, 50, 100] {
                                        option { value: "{n}", "{n}" }
                                    }
                                }
                            }
                            div { style: "display:flex; align-items:center; gap:12px;",
                                span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Page {page} of {page_count}" }
                                Button { label: "‹ Prev".to_string(), variant: "secondary".to_string(), disabled: page() <= 1,
                                    on_click: move |_| { if page() > 1 { page.set(page() - 1); } } }
                                Button { label: "Next ›".to_string(), variant: "secondary".to_string(), disabled: page() >= page_count,
                                    on_click: move |_| { page.set(page() + 1); } }
                            }
                        }
                    } else {
                        div { style: "padding:48px; text-align:center; color:{color::NEUTRAL_500};",
                            "Select a content type to manage its entries."
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
                                Ok(_) => {
                                    status.set(Some("Entry created".to_string()));
                                    let params = QueryParams { pagination: Some(PaginationParams::Page { page: page(), page_size: page_size(), with_count: Some(true) }), ..Default::default() };
                                    if let Ok(resp) = g.client.cm_list(&uid, &params).await {
                                        total.set(resp.meta.pagination.as_ref().map(|p| p.total).unwrap_or(0));
                                        entries.set(resp.data);
                                    }
                                }
                                Err(e) => status.set(Some(format!("Error: {e}"))),
                            }
                        });
                        creating.set(false);
                    },
                }
            }
        }

        if configuring() {
            if let Some(schema) = &selected {
                ConfigureViewModal {
                    uid: schema.uid.as_str().to_string(),
                    on_close: move |_| configuring.set(false),
                }
            }
        }
    }
}

/// Content Manager list-view configuration modal (design doc §6.5).
/// Loads the current configuration and lets the user choose which columns to
/// display and the page size, then persists via PUT.
#[component]
fn ConfigureViewModal(
    uid: String,
    on_close: EventHandler<()>,
) -> Element {
    let global = use_global();
    let mut config = use_signal(|| None::<api_types::admin::ViewConfiguration>);
    let mut status = use_signal(|| None::<String>);

    let g_load = global.clone();
    let uid_load = uid.clone();
    use_effect(move || {
        if config().is_none() {
            let g = g_load.clone();
            let uid = uid_load.clone();
            spawn(async move {
                match g.client.cm_get_configuration(&uid).await {
                    Ok(v) => {
                        if let Ok(c) = serde_json::from_value(v.get("data").cloned().unwrap_or(serde_json::Value::Null)) {
                            config.set(Some(c));
                        }
                    }
                    Err(e) => status.set(Some(format!("Failed to load config: {e}"))),
                }
            });
        }
    });

    let label_style = format!("font-size:{}; font-weight:600; color:{};", typography::LABEL_SIZE, color::NEUTRAL_700);
    let status_style = format!("padding:12px; margin-bottom:12px; border-radius:4px; background:{}; color:{}; font-size:{};", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);
    let g_save = global.clone();
    let uid_save = uid.clone();
    let page_size = config().as_ref().map(|c| c.settings.page_size);
    let cols = config().as_ref().map(|c| c.layouts.list.clone());
    let ps = page_size.unwrap_or(10);
    let col_list = cols.unwrap_or_default();

    rsx! {
        Modal { title: "Configure the view".to_string(), width: 720, on_close: move |_| on_close.call(()),
            if config().is_some() {
                div { style: "display:flex; flex-direction:column; gap:16px;",
                    if let Some(status) = status() {
                        div { style: "{status_style}", "{status}" }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        span { style: "{label_style}", "Entries per page" }
                        select { style: "padding:8px 16px; border:1px solid {color::NEUTRAL_200}; border-radius:4px;",
                            value: "{ps}",
                            onchange: move |e| {
                                if let Ok(v) = e.value().parse::<i64>() {
                                    if let Some(c) = config().as_mut() { c.settings.page_size = v; }
                                }
                            },
                            for n in [10, 25, 50, 100] {
                                option { value: "{n}", "{n}" }
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        span { style: "{label_style}", "Displayed columns" }
                        for col in col_list.clone().into_iter() {
                            div { style: "display:flex; align-items:center; gap:8px; font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_700};",
                                input { r#type: "checkbox", checked: true, onchange: move |_| {} }
                                span { "{col}" }
                            }
                        }
                    }
                    div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                        Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| on_close.call(()) }
                        Button { label: "Save".to_string(), variant: "primary".to_string(), on_click: move |_| {
                            if let Some(cfg) = config() {
                                let g = g_save.clone();
                                let uid = uid_save.clone();
                                spawn(async move {
                                    let _ = g.client.cm_update_configuration(&uid, &cfg).await;
                                });
                            }
                            on_close.call(());
                        } }
                    }
                }
            } else {
                div { style: "padding:32px; text-align:center; color:{color::NEUTRAL_500};", "Loading…" }
            }
        }
    }
}

/// A single Content Manager table row with bulk-selection checkbox and
/// click-to-open editing.
#[component]
fn EntryRow(
    id: String,
    main: String,
    state: String,
    updated: String,
    selected: bool,
    on_open_entry: EventHandler<String>,
    on_toggle_entry: EventHandler<(String, bool)>,
    on_edit: EventHandler<String>,
    on_delete: EventHandler<String>,
) -> Element {
    let gid = id.clone();
    let tid = id.clone();
    let eid = id.clone();
    let did = id.clone();
    let border = color::NEUTRAL_150;
    let td_style = "padding:10px 16px;";
    let action_btn = "background:none; border:none; color:{color::NEUTRAL_500}; cursor:pointer; font-size:14px;";
    rsx! {
        tr {
            style: "border-bottom:1px solid {border}; cursor:pointer;",
            onclick: move |_| on_open_entry.call(gid.clone()),
            td { style: "{td_style}",
                Checkbox {
                    checked: selected,
                    label: String::new(),
                    onchange: move |on| on_toggle_entry.call((tid.clone(), on)),
                }
            }
            td { style: "padding:10px 16px; font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_800};", "{id}" }
            td { style: "padding:10px 16px; font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_800};", "{main}" }
            td { style: "padding:10px 16px;",
                Badge { text: state.clone(), kind: state.clone() }
            }
            td { style: "padding:10px 16px; font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "{updated}" }
            td { style: "padding:10px 16px;",
                div { style: "display:flex; gap:8px;",
                    button { style: "{action_btn}", onclick: move |e| { e.stop_propagation(); on_edit.call(eid.clone()); }, "✎" }
                    button { style: "{action_btn}", onclick: move |e| { e.stop_propagation(); on_delete.call(did.clone()); }, "🗑" }
                }
            }
        }
    }
}

/// Schema-driven form for creating or editing a single entry.
/// `document_id == NEW_ENTRY` means create; otherwise update.
#[component]
fn EntryEditView(
    schema: Schema,
    document_id: String,
    is_single: bool,
    form: Signal<serde_json::Map<String, serde_json::Value>>,
    on_back: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let global = use_global();
    let mut saving = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);

    let is_new = document_id == NEW_ENTRY;
    let draft_and_publish = schema.draft_and_publish();
    let title = if is_new {
        "Create an entry".to_string()
    } else {
        form()
            .get(&schema.main_field())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Edit entry".to_string())
    };

    let back_style = "background:none; border:none; color:{color::NEUTRAL_700}; cursor:pointer; font-size:16px;";
    let top_bar = format!("display:flex; align-items:center; gap:12px; padding:0 32px; height:64px; border-bottom:1px solid {}; background:{};", color::NEUTRAL_150, color::NEUTRAL_0);
    let title_style = format!("font-size:{}; font-weight:600; color:{};", typography::BETA_SIZE, color::NEUTRAL_900);
    let status_style = format!("padding:12px; border-radius:4px; background:{}; color:{}; font-size:{}; margin-bottom:16px;", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);

    let scalar: Vec<(String, FieldType, Vec<String>)> = schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type.is_scalar_column())
        .filter(|(_, a)| a.attr_type != FieldType::Password)
        .map(|(name, a)| (name.clone(), a.attr_type, a.enum_values.clone()))
        .collect();

    // Non-scalar fields: component (single/repeatable) and dynamic zones.
    let component_fields: Vec<(String, String, Option<String>, bool)> = schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type == FieldType::Component)
        .map(|(name, a)| {
            let cu = a.component.as_ref().map(|u| u.as_str().to_string());
            let label = if a.repeatable.unwrap_or(false) {
                format!("{name} (repeatable)")
            } else {
                name.clone()
            };
            (label, name.clone(), cu, a.repeatable.unwrap_or(false))
        })
        .collect();
    let dz_fields: Vec<(String, Vec<String>)> = schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type == FieldType::Dynamiczone)
        .map(|(name, a)| (name.clone(), a.components.iter().map(|u| u.as_str().to_string()).collect()))
        .collect();

    let g = global.clone();
    let g2 = global.clone();
    let uid = schema.uid.as_str().to_string();
    let doc = document_id.clone();
    let save_uid = uid.clone();
    let save_doc = doc.clone();
    let pub_uid = uid.clone();
    let pub_doc = doc.clone();
    let disc_uid = uid.clone();
    let disc_doc = doc.clone();
    let g3 = global.clone();

    rsx! {
        div { style: "flex:1; min-width:0;",
            div { style: "{top_bar}",
                button { style: "{back_style}", onclick: move |_| on_back.call(()), "←" }
                span { style: "{title_style}", "{title}" }
                div { style: "flex:1;" }
                if let Some(status) = status() {
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::SUCCESS_600};", "{status}" }
                }
                Button { label: "Save".to_string(), variant: "primary".to_string(), loading: saving(),
                    on_click: move |_| {
                        let g = g.clone();
                        let uid = save_uid.clone();
                        let doc = save_doc.clone();
                        let data = serde_json::Value::Object(form());
                        saving.set(true);
                        spawn(async move {
                            let res = if is_new {
                                g.client.cm_create(&uid, &data).await
                            } else {
                                g.client.cm_update(&uid, &doc, &data).await
                            };
                            saving.set(false);
                            match res {
                                Ok(_) => { status.set(Some("Saved".to_string())); on_saved.call(()); }
                                Err(e) => status.set(Some(format!("Error: {e}"))),
                            }
                        });
                    }
                }
                if draft_and_publish && !is_new {
                    Button { label: "Publish".to_string(), variant: "success".to_string(), loading: saving(),
                        on_click: move |_| {
                            let g = g2.clone();
                            let uid = pub_uid.clone();
                            let doc = pub_doc.clone();
                            saving.set(true);
                            spawn(async move {
                                let res = g.client.cm_publish(&uid, &doc).await;
                                saving.set(false);
                                match res {
                                    Ok(_) => { status.set(Some("Published".to_string())); on_saved.call(()); }
                                    Err(e) => status.set(Some(format!("Error: {e}"))),
                                }
                            });
                        }
                    }
                    Button { label: "Discard changes".to_string(), variant: "secondary".to_string(), loading: saving(),
                        on_click: move |_| {
                            let g = g3.clone();
                            let uid = disc_uid.clone();
                            let doc = disc_doc.clone();
                            saving.set(true);
                            spawn(async move {
                                let res = g.client.cm_discard(&uid, &doc).await;
                                saving.set(false);
                                match res {
                                    Ok(_) => { status.set(Some("Changes discarded".to_string())); on_saved.call(()); }
                                    Err(e) => status.set(Some(format!("Error: {e}"))),
                                }
                            });
                        }
                    }
                }
            }
            div { style: "display:flex; gap:32px; padding:32px;",
                div { style: "flex:1; max-width:900px;",
                    if let Some(status) = status() {
                        div { style: "{status_style}", "{status}" }
                    }
                    Card { padding: 24,
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
                        for (label, name, comp_uid, _repeatable) in component_fields.into_iter() {
                            div { key: "comp-{name}", style: "margin:16px 0; border:1px solid {color::NEUTRAL_150}; border-radius:4px; padding:12px;",
                                div { style: "font-size:{typography::BODY_BOLD_SIZE}; color:{color::NEUTRAL_800}; margin-bottom:8px;",
                                    "{label}"
                                }
                                if let Some(cu) = &comp_uid {
                                    div { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "Component: {cu}" }
                                }
                                TextField {
                                    value: form().get(&name).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                                    label: String::new(),
                                    placeholder: "Component value (JSON)".to_string(),
                                    oninput: move |v| { form.write().insert(name.clone(), serde_json::Value::String(v)); }
                                }
                            }
                        }
                        for (name, allowed) in dz_fields.into_iter() {
                            div { key: "dz-{name}", style: "margin:16px 0; border:1px solid {color::NEUTRAL_150}; border-radius:4px; padding:12px;",
                                div { style: "font-size:{typography::BODY_BOLD_SIZE}; color:{color::NEUTRAL_800}; margin-bottom:8px;", "{name} (Dynamic Zone)" }
                                div { style: "display:flex; flex-wrap:wrap; gap:8px; margin-bottom:8px;",
                                    for c in allowed.iter() {
                                        div { style: "padding:2px 8px; border-radius:999px; background:{color::PRIMARY_100}; color:{color::PRIMARY_700}; font-size:{typography::PI_SIZE};", "{c}" }
                                    }
                                }
                                TextField {
                                    value: form().get(&name).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                                    label: String::new(),
                                    placeholder: "Dynamic zone entries (JSON)".to_string(),
                                    oninput: move |v| { form.write().insert(name.clone(), serde_json::Value::String(v)); }
                                }
                            }
                        }
                    }
                }
                div { style: "width:320px; min-width:320px;",
                    Card { padding: 24,
                        div { style: "font-size:{typography::EPSILON_SIZE}; font-weight:600; color:{color::NEUTRAL_900}; margin-bottom:12px;", "Information" }
                        div { style: "display:flex; flex-direction:column; gap:8px; font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};",
                            div { style: "display:flex; justify-content:space-between;", span { "State" }, Badge { text: "draft".to_string(), kind: "draft".to_string() } }
                            div { style: "display:flex; justify-content:space-between;", span { "Document ID" }, span { "{document_id}" } }
                            div { style: "display:flex; justify-content:space-between;", span { "Content type" }, span { "{schema.info.display_name}" } }
                        }
                    }
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
