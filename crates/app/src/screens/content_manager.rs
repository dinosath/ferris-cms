//! Content Manager screens — table-first browsing (design doc §6).
//!
//! Navigation is hierarchical like the Workflows screen:
//!   Content Manager → content-type table → entries table → entry editor.
//! There is no persistent secondary sidebar; content types are listed on the
//! main page and entries are shown as full-width tables.

use api_types::{PaginationParams, QueryParams};
use core_domain::{ContentTypeKind, FieldType};
use core_schema::Schema;
use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{use_global, Route};
use crate::components::{
    Badge, Button, Checkbox, ConfirmDialog, Dropdown, EmptyState, IconButton, Modal, Pagination,
    Spinner, StatusIndicator, TextArea, TextField, Toggle,
};

/// Marker document id used for a brand-new entry in the edit view.
const NEW_ENTRY: &str = "__new__";
type PendingInlineLine = (String, String, String, serde_json::Value);

/// Whether an entry field's string value satisfies a filter condition.
fn filter_matches(value: &serde_json::Value, op: &str, expected: &str) -> bool {
    let actual = value.to_string().trim_matches('"').to_string();
    match op {
        "neq" => actual != expected,
        "contains" => actual.to_lowercase().contains(&expected.to_lowercase()),
        _ => actual == expected, // "eq"
    }
}

fn filter_value(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.to_string()))
}

fn saved_filter(filters: &[(String, String, String)]) -> Option<api_types::Filter> {
    let leaves: Vec<_> = filters
        .iter()
        .map(|(field, op, value)| {
            let op = match op.as_str() {
                "neq" => api_types::FilterOp::Ne,
                "contains" => api_types::FilterOp::ContainsI,
                _ => api_types::FilterOp::Eq,
            };
            api_types::Filter::Leaf {
                field: field.clone(),
                op,
                values: vec![filter_value(value)],
            }
        })
        .collect();
    match leaves.len() {
        0 => None,
        1 => leaves.into_iter().next(),
        _ => Some(api_types::Filter::And(leaves)),
    }
}

/// Turn a saved filter back into the compact chips used by the Content Manager.
/// Complex OR/NOT filters stay server-side; the editor only exposes the safe,
/// repeatable AND/leaf form it can round-trip without losing meaning.
fn filter_conditions(filter: Option<&api_types::Filter>) -> Vec<(String, String, String)> {
    fn flatten(filter: &api_types::Filter, out: &mut Vec<(String, String, String)>) {
        match filter {
            api_types::Filter::And(items) => {
                for item in items {
                    flatten(item, out);
                }
            }
            api_types::Filter::Leaf { field, op, values } => {
                let operator = match op {
                    api_types::FilterOp::Ne => "neq",
                    api_types::FilterOp::ContainsI | api_types::FilterOp::Contains => "contains",
                    _ => "eq",
                };
                let value = values.first().map(display_value).unwrap_or_default();
                out.push((field.clone(), operator.into(), value));
            }
            api_types::Filter::Or(_) | api_types::Filter::Not(_) => {}
        }
    }

    let mut out = Vec::new();
    if let Some(filter) = filter {
        flatten(filter, &mut out);
    }
    out
}

/// A comparable sort key for an entry column. `"state"` maps to publication
/// status so Draft sorts before Published; `"id"` maps to the document id.
fn sort_key(e: &serde_json::Value, field: &str) -> String {
    if field == "state" {
        let s = e
            .get("publicationState")
            .and_then(|v| v.as_str())
            .unwrap_or("draft");
        return if s == "published" {
            "1".to_string()
        } else {
            "0".to_string()
        };
    }
    if field == "id" {
        return e
            .get("documentId")
            .or_else(|| e.get("id"))
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default();
    }
    e.get(field)
        .map(|v| v.to_string().trim_matches('"').to_string())
        .unwrap_or_default()
}

/// Resolve an entry's document id (falls back to numeric id).
fn entry_id(e: &serde_json::Value) -> String {
    e.get("documentId")
        .or_else(|| e.get("id"))
        .map(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
        })
        .unwrap_or_default()
}

/// Human-friendly relative timestamp, e.g. "2 hours ago".
fn rel_time(iso: &str) -> String {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return "—".to_string();
    };
    let secs = chrono::Utc::now()
        .signed_duration_since(dt)
        .num_seconds()
        .max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86400 {
        format!("{} hours ago", secs / 3600)
    } else if secs < 86400 * 7 {
        format!("{} days ago", secs / 86400)
    } else if secs < 86400 * 30 {
        format!("{} weeks ago", secs / (86400 * 7))
    } else if secs < 86400 * 365 {
        format!("{} months ago", secs / (86400 * 30))
    } else {
        format!("{} years ago", secs / (86400 * 365))
    }
}

/// Render a badge for a content type kind (collection / single).
fn type_badge_cm(kind: ContentTypeKind) -> Element {
    match kind {
        ContentTypeKind::CollectionType => rsx! {
            Badge { text: "Collection".to_string(), kind: "published".to_string() }
        },
        _ => rsx! {
            Badge { text: "Single".to_string(), kind: "neutral".to_string() }
        },
    }
}

fn is_inline_document(schema: &Schema) -> bool {
    schema
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.labels.get("document"))
        .map(String::as_str)
        == Some("line")
}

/// Derive the entry-table columns from a content type's schema. The leading
/// scalar (non-password) fields become columns, followed by state and updated.
fn entry_columns(schema: &Schema) -> Vec<(String, String)> {
    let mut cols: Vec<(String, String)> = vec![("id".to_string(), "ID".to_string())];
    for (name, attr) in schema.attributes.iter() {
        if attr.attr_type.is_scalar_column()
            && attr.attr_type != FieldType::Password
            && cols.len() < 7
        {
            cols.push((name.clone(), name.clone()));
        }
    }
    cols.push(("state".to_string(), "State".to_string()));
    cols.push(("updatedAt".to_string(), "Updated At".to_string()));
    cols
}

/// Content Manager landing page — a table-first listing of every content type
/// (collection + single). Selecting one opens its entries (or, for single
/// types, the entry editor directly).
#[component]
pub fn ContentManager() -> Element {
    let global = use_global();
    let mut schemas: Signal<Vec<Schema>> = use_signal(|| vec![]);
    let mut loaded = use_signal(|| false);
    let mut loading = use_signal(|| true);
    let mut search = use_signal(String::new);
    let mut filter = use_signal(|| "all".to_string());
    let mut counts: Signal<Vec<(String, i64, String)>> = use_signal(|| vec![]);
    let mut status = use_signal(|| None::<String>);
    let mut route = global.route;

    // Load content types, register names for breadcrumbs, and fetch per-type
    // entry counts for collection types.
    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let mut g = g_load.clone();
            let mut sc = schemas;
            let mut ct = counts;
            spawn(async move {
                match g.client.ctb_list().await {
                    Ok(v) => {
                        let schemas_vec: Vec<Schema> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let names: Vec<(String, String)> = schemas_vec
                            .iter()
                            .filter(|s| {
                                s.kind == ContentTypeKind::CollectionType
                                    || s.kind == ContentTypeKind::SingleType
                            })
                            .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                            .collect();
                        g.ct_names.set(names);
                        sc.set(schemas_vec.clone());
                        for s in schemas_vec.iter().filter(|s| {
                            s.kind == ContentTypeKind::CollectionType && !is_inline_document(s)
                        }) {
                            let uid = s.uid.as_str().to_string();
                            let g = g.clone();
                            let mut ct = ct.clone();
                            spawn(async move {
                                let params = QueryParams {
                                    pagination: Some(PaginationParams::Page {
                                        page: 1,
                                        page_size: 1,
                                        with_count: Some(true),
                                    }),
                                    ..Default::default()
                                };
                                if let Ok(resp) = g.client.cm_list(&uid, &params).await {
                                    let total =
                                        resp.meta.pagination.as_ref().map(|p| p.total).unwrap_or(0);
                                    let updated = resp
                                        .data
                                        .first()
                                        .and_then(|e| e.get("updatedAt").map(|v| v.to_string()))
                                        .unwrap_or_default();
                                    ct.write().push((uid, total, updated));
                                }
                            });
                        }
                    }
                    Err(e) => status.set(Some(format!("Failed to load: {e}"))),
                }
                loading.set(false);
            });
        }
    });

    let all = schemas();
    let filter_kind = filter();
    let query = search().trim().to_lowercase();
    let count_map = counts();
    let mut rows: Vec<Element> = Vec::new();
    for s in all.iter().filter(|s| {
        (s.kind == ContentTypeKind::CollectionType || s.kind == ContentTypeKind::SingleType)
            && !is_inline_document(s)
    }) {
        let is_collection = s.kind == ContentTypeKind::CollectionType;
        let matches_filter = match filter_kind.as_str() {
            "collection" => is_collection,
            "single" => !is_collection,
            _ => true,
        };
        if !matches_filter {
            continue;
        }
        let name = s.info.display_name.clone();
        if !query.is_empty() && !name.to_lowercase().contains(&query) {
            continue;
        }
        let uid = s.uid.as_str().to_string();
        let kind = s.kind;
        let draft_pub = s.draft_and_publish();
        let (cnt, updated) = if is_collection {
            let found = count_map
                .iter()
                .find(|(u, _, _)| *u == uid)
                .map(|(_, c, u)| (*c, u.clone()))
                .unwrap_or((0, String::new()));
            found
        } else {
            (1i64, String::new())
        };
        let updated_display = if updated.is_empty() {
            "—".to_string()
        } else {
            rel_time(&updated)
        };
        let target = if is_collection {
            Route::ContentManagerEntries(uid.clone())
        } else {
            Route::ContentManagerEntry {
                uid: uid.clone(),
                document_id: "default".to_string(),
            }
        };
        let mut open_tr = route;
        let mut open_name = route;
        let mut open_act = route;
        let target_tr = target.clone();
        let target_name = target.clone();
        let target_act = target.clone();
        rows.push(rsx! {
            tr { class: "cm-table-row", style: "cursor:pointer;",
                onclick: move |_| open_tr.set(target_tr.clone()),
                td {
                    button { class: "cm-table-link",
                        onclick: move |_| open_name.set(target_name.clone()),
                        "{name}"
                    }
                }
                td { {type_badge_cm(kind)} }
                td { "{cnt}" }
                td { style: "color:{color::NEUTRAL_600};", "{updated_display}" }
                td {
                    if draft_pub {
                        Badge { text: "Draft & publish".to_string(), kind: "modified".to_string() }
                    } else {
                        Badge { text: "Standard".to_string(), kind: "neutral".to_string() }
                    }
                }
                td {
                    Button { label: "Open".to_string(), size: "sm".to_string(), on_click: move |_| open_act.set(target_act.clone()) }
                }
            }
        });
    }

    let count = all.len();
    rsx! {
        div { class: "cm-screen",
            div { class: "cm-page",
                div { class: "cm-heading-row",
                    div {
                        div { class: "cm-kicker", "Workspace" }
                        h1 { class: "cm-page-title", "Content Manager" }
                        p { class: "cm-page-subtitle", "Create, read, update and publish your content." }
                    }
                }

                div { class: "cm-toolbar",
                    div { class: "cm-search",
                        TextField {
                            value: search(),
                            placeholder: "Search content types".to_string(),
                        oninput: move |v| search.set(v),
                    }
                }
                    div { class: "cm-segment",
                    CmTypeChip { label: "All".to_string(), active: filter() == "all", on_click: move |_| filter.set("all".into()) }
                    CmTypeChip { label: "Collection Types".to_string(), active: filter() == "collection", on_click: move |_| filter.set("collection".into()) }
                    CmTypeChip { label: "Single Types".to_string(), active: filter() == "single", on_click: move |_| filter.set("single".into()) }
                    }
                }
            }

            if let Some(status) = status() {
                div { style: "padding:12px; margin-bottom:16px; border-radius:4px; background:{color::WARNING_100}; color:{color::WARNING_700}; font-size:{typography::BODY_SIZE};", "{status}" }
            }

            if loading() {
                div { style: "display:flex; justify-content:center; padding:48px;", Spinner { size: 28 } }
            } else if count == 0 {
                EmptyState {
                    title: "No content types available".to_string(),
                    subtitle: "Create a content type in the Content-Type Builder to start managing content.".to_string(),
                    icon: "stack".to_string(),
                }
            } else if rows.is_empty() {
                EmptyState {
                    title: "No results".to_string(),
                    subtitle: "No content types match your search or filter.".to_string(),
                    icon: "search".to_string(),
                }
            } else {
                div { class: "cm-surface",
                    div { class: "cm-surface-header",
                        span { class: "cm-surface-title", "{count} content types" }
                        span { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "Choose a type to open its entries" }
                    }
                    div { style: "overflow-x:auto;",
                        table { class: "cm-table",
                            thead {
                                tr {
                                    CmTableTh { label: "Content Type".to_string() }
                                    CmTableTh { label: "Type".to_string() }
                                    CmTableTh { label: "Entries".to_string() }
                                    CmTableTh { label: "Updated".to_string() }
                                    CmTableTh { label: "Status".to_string() }
                                    CmTableTh { label: "Actions".to_string() }
                                }
                            }
                            tbody { {rows.into_iter()} }
                        }
                    }
                }
            }
        }
    }
}

/// Entries table for one collection type. Columns are derived from the content
/// type's schema; rows support search, filters, sorting, pagination, bulk
/// selection and open/edit/delete.
#[component]
pub fn ContentManagerEntries(uid: String) -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut schemas: Signal<Vec<Schema>> = use_signal(|| vec![]);
    let mut schema_loaded = use_signal(|| false);
    let mut entries = use_signal(Vec::<serde_json::Value>::new);
    let mut total = use_signal(|| 0i64);
    let mut page = use_signal(|| 1i64);
    let mut page_size = use_signal(|| 10i64);
    let mut search = use_signal(String::new);
    let mut filters = use_signal(Vec::<(String, String, String)>::new);
    let mut filter_open = use_signal(|| false);
    let mut sort_field = use_signal(String::new);
    let mut sort_asc = use_signal(|| true);
    let mut selected_ids = use_signal(Vec::<String>::new);
    let mut status = use_signal(|| None::<String>);
    let mut pending_delete = use_signal(|| None::<String>);
    let mut load_req = use_signal(|| 1u32);
    let mut views = use_signal(Vec::<api_types::admin::ContentTypeView>::new);
    let mut active_view_id = use_signal(|| None::<i64>);
    let mut create_view_open = use_signal(|| false);
    let mut view_settings = use_signal(|| None::<i64>);
    let mut pending_view_delete = use_signal(|| None::<i64>);
    let mut route = global.route;
    let uid_schema_load = uid.clone();
    let uid_load = uid.clone();

    // Load content types (to resolve the schema + breadcrumbs).
    let g_schema = global.clone();
    use_effect(move || {
        if !schema_loaded() {
            schema_loaded.set(true);
            let mut g = g_schema.clone();
            let mut sc = schemas;
            let view_uid = uid_schema_load.clone();
            spawn(async move {
                match g.client.ctb_list().await {
                    Ok(v) => {
                        let schemas_vec: Vec<Schema> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let names: Vec<(String, String)> = schemas_vec
                            .iter()
                            .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                            .collect();
                        g.ct_names.set(names);
                        sc.set(schemas_vec);
                        if let Ok(saved_views) = g.client.cm_views(&view_uid).await {
                            let selected_view = saved_views
                                .iter()
                                .find(|v| v.is_default)
                                .or_else(|| saved_views.first());
                            filters.set(
                                selected_view
                                    .map(|v| filter_conditions(v.configuration.filters.as_ref()))
                                    .unwrap_or_default(),
                            );
                            if let Some(view) = selected_view {
                                if let Some(sort) = view.configuration.sorts.first() {
                                    sort_field.set(sort.field.clone());
                                    sort_asc.set(!sort.descending);
                                } else {
                                    sort_field.set(String::new());
                                    sort_asc.set(true);
                                }
                                if let Some(size) = view.configuration.page_size {
                                    page_size.set(size as i64);
                                }
                            }
                            active_view_id.set(selected_view.map(|v| v.id));
                            views.set(saved_views);
                        }
                    }
                    Err(e) => status.set(Some(format!("Failed to load: {e}"))),
                }
            });
        }
    });

    // Load entries when requested / pagination changes.
    let g_entries = global.clone();
    use_effect({
        let client = client.clone();
        move || {
            if load_req() > 0 {
                load_req.set(0);
                let uid = uid_load.clone();
                let mut page_signal = page;
                let mut load_signal = load_req;
                let page = page();
                let page_size = page_size();
                let g = g_entries.clone();
                let selected_view = active_view_id();
                spawn(async move {
                    let params = QueryParams {
                        pagination: Some(PaginationParams::Page {
                            page,
                            page_size,
                            with_count: Some(true),
                        }),
                        ..Default::default()
                    };
                    let result = if let Some(view_id) = selected_view {
                        g.client.cm_view_records(&uid, view_id, &params).await
                    } else {
                        g.client.cm_list(&uid, &params).await
                    };
                    match result {
                        Ok(resp) => {
                            let response_total =
                                resp.meta.pagination.as_ref().map(|p| p.total).unwrap_or(0);
                            let response_page_count =
                                (response_total as f64 / page_size.max(1) as f64)
                                    .ceil()
                                    .max(1.0) as i64;

                            // Counts can change between rendering the pagination controls and
                            // receiving the next page (for example after an entry is deleted).
                            // Do not leave the screen on an out-of-range page, which would make
                            // a valid collection look empty.
                            if page > response_page_count {
                                page_signal.set(response_page_count);
                                load_signal.set(load_signal() + 1);
                                return;
                            }

                            total.set(response_total);
                            entries.set(resp.data);
                        }
                        Err(e) => status.set(Some(format!("Failed to load entries: {e}"))),
                    }
                });
            }
        }
    });

    let schema = schemas().iter().find(|s| s.uid.as_str() == uid).cloned();
    let main_field = schema.as_ref().map(|s| s.main_field()).unwrap_or_default();
    let columns = if let Some(view) = views().iter().find(|v| Some(v.id) == active_view_id()) {
        let mut configured: Vec<(String, String)> = view
            .configuration
            .columns
            .iter()
            .filter(|column| column.visible)
            .map(|column| (column.field_id.clone(), column.field_id.clone()))
            .collect();
        configured.sort_by_key(|(field, _)| {
            view.configuration
                .columns
                .iter()
                .find(|c| &c.field_id == field)
                .map(|c| c.position)
                .unwrap_or(u32::MAX)
        });
        if view.configuration.columns.is_empty() {
            schema.as_ref().map(entry_columns).unwrap_or_default()
        } else {
            configured
        }
    } else {
        schema.as_ref().map(entry_columns).unwrap_or_default()
    };
    let header_name = schema
        .as_ref()
        .map(|s| s.info.display_name.clone())
        .unwrap_or_else(|| uid.clone());
    let active_view_name = views()
        .iter()
        .find(|v| Some(v.id) == active_view_id())
        .map(|v| v.name.clone())
        .unwrap_or_else(|| "All Records".to_string());
    let view_options = views();
    let active_view = view_options
        .iter()
        .find(|v| Some(v.id) == active_view_id())
        .cloned();
    let view_options_for_select = view_options.clone();
    let active_view_type = active_view
        .as_ref()
        .map(|v| v.view_type)
        .unwrap_or(api_types::admin::ViewType::Grid);
    let renderer_field = active_view
        .as_ref()
        .and_then(|v| v.configuration.group_by.clone())
        .or_else(|| {
            schema
                .as_ref()
                .and_then(|s| s.attributes.keys().next().cloned())
        })
        .unwrap_or_else(|| main_field.clone());

    // Client-side search + filters, then column sorting.
    let query = search().trim().to_lowercase();
    let active_filters = filters();
    let sort_f = sort_field();
    let sort_ascending = sort_asc();
    let mut filtered: Vec<serde_json::Value> = entries()
        .into_iter()
        .filter(|e| {
            if !query.is_empty() {
                let main = e
                    .get(&main_field)
                    .map(|v| v.to_string().to_lowercase())
                    .unwrap_or_default();
                if !main.contains(&query) {
                    return false;
                }
            }
            for (f, op, v) in active_filters.iter() {
                if f == "state" {
                    let state = e
                        .get("publicationState")
                        .and_then(|x| x.as_str())
                        .unwrap_or("draft");
                    let ok = match op.as_str() {
                        "neq" => state != v,
                        "contains" => state.to_lowercase().contains(&v.to_lowercase()),
                        _ => state == v,
                    };
                    if !ok {
                        return false;
                    }
                } else if !filter_matches(e.get(f).unwrap_or(&serde_json::Value::Null), op, v) {
                    return false;
                }
            }
            true
        })
        .collect();

    if sort_f.is_empty() {
        filtered.sort_by_key(|e| std::cmp::Reverse(sort_key(e, "updatedAt")));
    } else {
        filtered.sort_by(|a, b| {
            let ord = sort_key(a, &sort_f).cmp(&sort_key(b, &sort_f));
            if sort_ascending {
                ord
            } else {
                ord.reverse()
            }
        });
    }

    // Build table rows.
    let col_keys: Vec<String> = columns.iter().map(|(k, _)| k.clone()).collect();
    let mut rows: Vec<Element> = Vec::new();
    for e in filtered.iter() {
        let id = entry_id(e);
        let selected = selected_ids().contains(&id);
        let mut cells: Vec<Element> = Vec::new();
        for key in col_keys.iter() {
            cells.push(render_cell(e, key));
        }
        let mut open_entry = route.clone();
        let mut edit_entry = route.clone();
        let mut del_entry = pending_delete.clone();
        let mut toggle_entry = selected_ids.clone();
        let rid = id.clone();
        let edit_id = id.clone();
        let del_id = id.clone();
        let uid_edit = uid.clone();
        let uid_open = uid.clone();
        let cell_id = id.clone();
        rows.push(rsx! {
            tr { class: "cm-table-row", style: "cursor:pointer;",
                onclick: move |_| open_entry.set(Route::ContentManagerEntry { uid: uid_open.clone(), document_id: rid.clone() }),
                td {
                    Checkbox {
                        checked: selected,
                        label: String::new(),
                        onchange: move |on| {
                            let mut ids = toggle_entry();
                            if on { if !ids.contains(&cell_id) { ids.push(cell_id.clone()); } }
                            else { ids.retain(|x| x != &cell_id); }
                            toggle_entry.set(ids);
                        },
                    }
                }
                {cells.into_iter()}
                td {
                    div { class: "cm-row-actions",
                        IconButton { name: "pencil".to_string(), aria_label: "Edit".to_string(),
                            on_click: move |e: MouseEvent| { e.stop_propagation(); edit_entry.set(Route::ContentManagerEntry { uid: uid_edit.clone(), document_id: edit_id.clone() }); } }
                        IconButton { name: "trash".to_string(), variant: "danger".to_string(), aria_label: "Delete".to_string(),
                            on_click: move |e: MouseEvent| { e.stop_propagation(); del_entry.set(Some(del_id.clone())); } }
                    }
                }
            }
        });
    }

    let page_count = if page_size() > 0 {
        (total() as f64 / page_size() as f64).ceil().max(1.0) as i64
    } else {
        1
    };
    let sel_count = selected_ids().len();
    let g_delete = global.clone();
    let uid_create = uid.clone();
    let uid_create_empty = uid.clone();
    let uid_import = uid.clone();
    let uid_export = uid.clone();
    let uid_bulk_delete = uid.clone();
    let filter_global = global.clone();
    let filter_uid = uid.clone();
    let create_global = global.clone();
    let create_uid = uid.clone();
    let delete_view_global = global.clone();
    let delete_view_uid = uid.clone();

    // Build sortable header cells outside rsx (the parser dislikes method calls
    // on `for` loop bindings inside rsx).
    let mut header_cells: Vec<Element> = Vec::new();
    for (key, label) in columns.iter() {
        let k = key.clone();
        let l = label.clone();
        if key == "id" || key == "state" || key == "updatedAt" {
            header_cells.push(rsx! {
                th { style: "text-align:left; padding:10px 16px; font-size:{typography::LABEL_SIZE}; font-weight:600; color:{color::NEUTRAL_600};", "{l}" }
            });
        } else {
            let mut sf = sort_field;
            let mut sa = sort_asc;
            let mut pg = page;
            let sort_view = active_view.clone();
            let sort_global = global.clone();
            let sort_uid = uid.clone();
            header_cells.push(rsx! {
                th { style: "text-align:left; padding:10px 16px; font-size:{typography::LABEL_SIZE}; font-weight:600; color:{color::NEUTRAL_600}; cursor:pointer;",
                    onclick: move |_| {
                        let descending = if sf() == k { !sa() } else { false };
                        if sf() == k { sa.set(!sa()); } else { sf.set(k.clone()); sa.set(true); }
                        if let Some(view) = sort_view.clone() {
                            let mut config = view.configuration.clone();
                            config.sorts = vec![api_types::SortField { field: k.clone(), descending }];
                            let g = sort_global.clone(); let uid = sort_uid.clone();
                            spawn(async move { let _ = g.client.cm_view_update(&uid, view.id, &api_types::admin::UpdateContentTypeViewRequest { name: None, description: None, view_type: None, configuration: Some(config) }).await; });
                        }
                        pg.set(1);
                    },
                    "{l}"
                }
            });
        }
    }

    rsx! {
        div { class: "cm-screen",
            div { class: "cm-page",
            div { class: "cm-heading-row",
                div { style: "display:flex; align-items:center; gap:12px;",
                    Button { label: "← Back to Content Types".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::ContentManager) }
                    div { style: "display:flex; flex-direction:column; gap:2px;",
                        div { class: "cm-kicker", "Collection type" }
                        h1 { class: "cm-page-title", "{header_name}" }
                        span { class: "cm-page-subtitle", "{total} entries" }
                    }
                }
                div { style: "display:flex; align-items:center; gap:8px;",
                    Button { label: "Import into this content type".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::Import(Some(uid_import.clone()))) }
                    Button { label: "Export this content type".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::Export(Some(uid_export.clone()))) }
                    Button { label: "Create entry".to_string(), on_click: move |_| route.set(Route::ContentManagerEntry { uid: uid_create.clone(), document_id: NEW_ENTRY.to_string() }) }
                }
            }

            div { class: "cm-toolbar",
                if !view_options.is_empty() {
                    div { style: "display:flex; align-items:center; gap:8px; padding:6px 10px; border:1px solid {color::NEUTRAL_200}; border-radius:6px; background:#fff;",
                        span { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "View" }
                        select { style: "border:0; background:transparent; font-weight:600; color:{color::NEUTRAL_800};", value: "{active_view_id().unwrap_or_default()}", onchange: move |event| {
                            if let Ok(id) = event.value().parse::<i64>() {
                                active_view_id.set(Some(id));
                                if let Some(view) = view_options_for_select.iter().find(|v| v.id == id) {
                                    filters.set(filter_conditions(view.configuration.filters.as_ref()));
                                    if let Some(sort) = view.configuration.sorts.first() {
                                        sort_field.set(sort.field.clone());
                                        sort_asc.set(!sort.descending);
                                    } else {
                                        sort_field.set(String::new());
                                        sort_asc.set(true);
                                    }
                                    page_size.set(view.configuration.page_size.unwrap_or(10) as i64);
                                }
                                load_req.set(load_req() + 1);
                            }
                        },
                            for view in view_options.iter() { option { value: "{view.id}", selected: Some(view.id) == active_view_id(), "{view.name}" } }
                        }
                    }
                }
                Button { label: "+ New view".to_string(), variant: "secondary".to_string(), on_click: move |_| create_view_open.set(true) }
                div { class: "cm-search",
                    TextField { value: search(), label: String::new(), placeholder: "Search entries".to_string(), oninput: move |v| search.set(v) }
                }
                Button { label: "Filters".to_string(), variant: "secondary".to_string(), on_click: move |_| filter_open.set(true) }
                if let Some(view) = active_view.clone() {
                    Button { label: "Customize view".to_string(), variant: "secondary".to_string(), on_click: move |_| view_settings.set(Some(view.id)) }
                }
            }

            if view_options.len() > 1 {
                div { style: "display:flex; gap:8px; align-items:center; padding:8px 0 14px; border-bottom:1px solid {color::NEUTRAL_150}; margin-bottom:14px; overflow-x:auto;",
                    span { style: "font-size:{typography::LABEL_SIZE}; color:{color::NEUTRAL_500}; white-space:nowrap;", "Views" }
                    for (view_index, view) in view_options.iter().enumerate() {
                        {
                            let selected = Some(view.id) == active_view_id();
                            let border = if selected { color::PRIMARY_300 } else { color::NEUTRAL_200 };
                            let background = if selected { color::PRIMARY_100 } else { color::NEUTRAL_0 };
                            let text_color = if selected { color::PRIMARY_700 } else { color::NEUTRAL_700 };
                            let font_weight = if selected { "600" } else { "400" };
                            let id = view.id;
                            let view_filter = view.configuration.filters.clone();
                            let view_sort = view.configuration.sorts.first().cloned();
                            let view_page_size = view.configuration.page_size;
                            let mut active = active_view_id;
                            let mut reload = load_req;
                            let mut settings = view_settings;
                            let mut pending = pending_view_delete;
                            let mut view_list = views;
                            let g = global.clone();
                            let uid_for_action = uid.clone();
                            let order_ids: Vec<i64> = view_options.iter().map(|v| v.id).collect();
                            rsx! {
                                div { style: "display:flex; align-items:center; gap:4px; padding:4px 6px 4px 10px; border:1px solid {border}; border-radius:6px; background:{background}; white-space:nowrap;",
                                    button { style: "border:0; background:none; cursor:pointer; color:{text_color}; font-weight:{font_weight};", onclick: move |_| { active.set(Some(id)); filters.set(filter_conditions(view_filter.as_ref())); if let Some(sort) = &view_sort { sort_field.set(sort.field.clone()); sort_asc.set(!sort.descending); } else { sort_field.set(String::new()); sort_asc.set(true); } page_size.set(view_page_size.unwrap_or(10) as i64); reload.set(reload() + 1); }, "{view.name}" }
                                    button { style: "border:0; background:none; cursor:pointer; color:{color::NEUTRAL_500};", title: "View settings", onclick: move |_| settings.set(Some(id)), "⋮" }
                                    if view.is_default { span { style: "font-size:11px; color:{color::SUCCESS_600};", "default" } }
                                    if !view.is_default { button { style: "border:0; background:none; cursor:pointer; font-size:11px; color:{color::NEUTRAL_500};", title: "Set as default", onclick: move |_| { let g = g.clone(); let uid = uid_for_action.clone(); let mut list = view_list; spawn(async move { if g.client.cm_view_set_default(&uid, id).await.is_ok() { if let Ok(vs) = g.client.cm_views(&uid).await { list.set(vs); } } }); }, "Set default" } }
                                    if view_index > 0 { button { style: "border:0; background:none; cursor:pointer; color:{color::NEUTRAL_500};", title: "Move view up", onclick: { let g = global.clone(); let uid = uid.clone(); let mut ids = order_ids.clone(); move |_| { ids.swap(view_index, view_index - 1); let g = g.clone(); let uid = uid.clone(); let mut list = views; let reordered = ids.clone(); spawn(async move { if let Ok(vs) = g.client.cm_view_reorder(&uid, &reordered).await { list.set(vs); } }); } }, "↑" } }
                                    if view_index + 1 < view_options.len() { button { style: "border:0; background:none; cursor:pointer; color:{color::NEUTRAL_500};", title: "Move view down", onclick: { let g = global.clone(); let uid = uid.clone(); let mut ids = order_ids.clone(); move |_| { ids.swap(view_index, view_index + 1); let g = g.clone(); let uid = uid.clone(); let mut list = views; let reordered = ids.clone(); spawn(async move { if let Ok(vs) = g.client.cm_view_reorder(&uid, &reordered).await { list.set(vs); } }); } }, "↓" } }
                                    button { style: "border:0; background:none; cursor:pointer; color:{color::DANGER_600};", title: "Delete view", onclick: move |_| pending.set(Some(id)), "×" }
                                }
                            }
                        }
                    }
                }
            }

            if view_options.len() > 1 {
                div { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500}; margin:-8px 0 12px 2px;", "Saved view: {active_view_name}" }
            }

            if !filters().is_empty() {
                div { style: "display:flex; flex-wrap:wrap; gap:8px; padding-bottom:8px;",
                    for (idx, (f, op, v)) in filters().into_iter().enumerate() {
                        div { class: "cm-filter-chip",
                            span { "{f} {op} \"{v}\"" }
                            button { style: "background:none; border:none; color:{color::PRIMARY_700}; cursor:pointer; font-size:14px;",
                                onclick: move |_| {
                                    let mut fs = filters();
                                    if idx < fs.len() { fs.remove(idx); }
                                    filters.set(fs);
                                    page.set(1);
                                }, "×"
                            }
                        }
                    }
                }
            }

            if !selected_ids().is_empty() {
                div { style: "display:flex; align-items:center; gap:12px; padding:12px 16px; background:{color::PRIMARY_100}; margin-bottom:16px;",
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_800};", "{sel_count} entries selected" }
                    Button { label: "Delete".to_string(), variant: "danger".to_string(), on_click: move |_| {
                        let ids = selected_ids();
                        let g = g_delete.clone();
                        let uid = uid_bulk_delete.clone();
                        let mut load = load_req;
                        let mut sel = selected_ids;
                        spawn(async move {
                            for id in ids.iter() {
                                let _ = g.client.cm_delete(&uid, id).await;
                            }
                            sel.set(Vec::new());
                            load.set(load() + 1);
                        });
                    } }
                }
            }

            if active_view_type != api_types::admin::ViewType::Grid {
                {render_saved_view(active_view_type, &filtered, schema.as_ref(), route, renderer_field.clone(), uid.clone())}
            } else if filtered.is_empty() {
                if query.is_empty() && filters().is_empty() {
                    EmptyState {
                        title: "No entries yet".to_string(),
                        subtitle: "Create your first entry to start managing content.".to_string(),
                        icon: "stack".to_string(),
                        Button { label: "Create entry".to_string(), on_click: move |_| route.set(Route::ContentManagerEntry { uid: uid_create_empty.clone(), document_id: NEW_ENTRY.to_string() }) }
                    }
                } else {
                    EmptyState { title: "No results".to_string(), subtitle: "No entries match your search or filters.".to_string(), icon: "search".to_string() }
                }
            } else {
                div { class: "cm-surface",
                    div { style: "overflow-x:auto;",
                        table { class: "cm-table",
                            thead {
                                tr { style: "border-bottom:1px solid {color::NEUTRAL_150};",
                                th { style: "padding:10px 16px; width:40px; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};",
                                        Checkbox {
                                            checked: !filtered.is_empty() && selected_ids().len() == filtered.len(),
                                            label: String::new(),
                                            onchange: move |on| {
                                                let all: Vec<String> = filtered.iter().map(entry_id).collect();
                                                selected_ids.set(if on { all } else { Vec::new() });
                                            },
                                        }
                                    }
                                    {header_cells.into_iter()}
                                    th { style: "text-align:left; padding:10px 16px; font-size:{typography::LABEL_SIZE}; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Actions" }
                                }
                            }
                            tbody { {rows.into_iter()} }
                        }
                    }
                }
                Pagination {
                    page: page(),
                    page_count,
                    page_size: page_size(),
                    total: total(),
                    on_page_change: move |p| { if (1..=page_count).contains(&p) { page.set(p); load_req.set(load_req() + 1); } },
                    on_page_size_change: move |ps| { page_size.set(ps); page.set(1); load_req.set(load_req() + 1); },
                }
            }
            }
        }

        if filter_open() {
            if let Some(schema) = &schema {
                FilterModal {
                    fields: schema.attributes.keys().cloned().collect(),
                    on_add: move |cond: (String, String, String)| {
                        let mut fs = filters();
                        fs.push(cond);
                        if let Some(view) = active_view.clone() {
                            let mut config = view.configuration.clone();
                            config.filters = saved_filter(&fs);
                            let g = filter_global.clone();
                            let uid = filter_uid.clone();
                            spawn(async move {
                                let _ = g.client.cm_view_update(&uid, view.id, &api_types::admin::UpdateContentTypeViewRequest { name: None, description: None, view_type: None, configuration: Some(config) }).await;
                            });
                        }
                        filters.set(fs);
                        page.set(1);
                        filter_open.set(false);
                    },
                    on_close: move |_| filter_open.set(false),
                }
            }
        }

        if let Some(del_id) = pending_delete() {
            DeleteConfirmDialog {
                del_id,
                uid: uid.clone(),
                on_close: move |_| pending_delete.set(None),
                on_deleted: move |_| { pending_delete.set(None); load_req.set(load_req() + 1); },
            }
        }

        if create_view_open() {
            CreateViewModal {
                uid: uid.clone(),
                on_close: move |_| create_view_open.set(false),
                on_created: move |view: api_types::admin::ContentTypeView| {
                    active_view_id.set(Some(view.id));
                    let mut list = views;
                    let g = create_global.clone();
                    let uid = create_uid.clone();
                    spawn(async move { if let Ok(vs) = g.client.cm_views(&uid).await { list.set(vs); } });
                    create_view_open.set(false);
                    load_req.set(load_req() + 1);
                },
            }
        }

        if let Some(view_id) = view_settings() {
            if let Some(view) = view_options.iter().find(|v| v.id == view_id).cloned() {
                ViewSettingsModal {
                    uid: uid.clone(),
                    view,
                    fields: schema.as_ref().map(|s| s.attributes.keys().cloned().collect()).unwrap_or_default(),
                    on_close: move |_| view_settings.set(None),
                    on_saved: move |saved: api_types::admin::ContentTypeView| {
                        let saved_id = saved.id;
                        let mut updated = views();
                        if let Some(existing) = updated.iter_mut().find(|v| v.id == saved_id) {
                            *existing = saved;
                        }
                        if let Some(saved_view) = updated.iter().find(|v| v.id == saved_id) {
                            page_size.set(saved_view.configuration.page_size.unwrap_or(10) as i64);
                        }
                        views.set(updated);
                        view_settings.set(None);
                        load_req.set(load_req() + 1);
                    },
                }
            }
        }

        if let Some(view_id) = pending_view_delete() {
            ConfirmDialog {
                title: "Delete view".to_string(),
                message: "Delete this saved view? Records will remain unchanged.".to_string(),
                confirm_label: "Delete view".to_string(),
                on_cancel: move |_| pending_view_delete.set(None),
                on_confirm: move |_| {
                    let g = delete_view_global.clone(); let uid = delete_view_uid.clone(); let mut list = views; let mut active = active_view_id;
                    spawn(async move { let _ = g.client.cm_view_delete(&uid, view_id).await; if let Ok(vs) = g.client.cm_views(&uid).await { let next = vs.first().map(|v| v.id); list.set(vs); active.set(next); } });
                    pending_view_delete.set(None);
                },
            }
        }
    }
}

/// Human-readable display for a read-only (computed) value.
fn display_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "—".to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Convert a JSON value into the value used by ordinary form controls.
/// Numeric computed fields must remain visible in their normal number input
/// instead of being treated as an empty string.
fn input_value(v: Option<&serde_json::Value>) -> String {
    match v {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// Render a single cell for a column key (special-cases state + updatedAt).
fn render_cell(e: &serde_json::Value, key: &str) -> Element {
    if key == "state" {
        let state = e
            .get("publicationState")
            .and_then(|v| v.as_str())
            .unwrap_or("draft")
            .to_string();
        return rsx! {
            td { style: "padding:12px 16px;", StatusIndicator { status: state } }
        };
    }
    if key == "updatedAt" {
        let updated = e
            .get("updatedAt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let label = rel_time(&updated);
        return rsx! {
            td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_600};", "{label}" }
        };
    }
    if key == "id" {
        let id = entry_id(e);
        return rsx! {
            td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_600};", "{id}" }
        };
    }
    let value = e
        .get(key)
        .map(display_value)
        .unwrap_or_else(|| "—".to_string());
    rsx! {
        td { style: "font-size:13px; color:{color::NEUTRAL_800};", "{value}" }
    }
}

/// Render the presentation-only views. Cards always navigate to the same
/// entry editor used by Grid, so every renderer edits the shared record.
fn render_saved_view(
    view_type: api_types::admin::ViewType,
    entries: &[serde_json::Value],
    schema: Option<&Schema>,
    route: Signal<Route>,
    field: String,
    uid: String,
) -> Element {
    match view_type {
        api_types::admin::ViewType::Kanban => render_kanban(entries, route, field, uid),
        api_types::admin::ViewType::Gallery => render_gallery(entries, route, schema, field, uid),
        api_types::admin::ViewType::Calendar => render_calendar(entries, route, field, uid),
        api_types::admin::ViewType::Grid => rsx! {},
    }
}

fn render_kanban(
    entries: &[serde_json::Value],
    route: Signal<Route>,
    field: String,
    uid: String,
) -> Element {
    let mut groups: Vec<String> = entries
        .iter()
        .map(|e| display_value(e.get(&field).unwrap_or(&serde_json::Value::Null)))
        .collect();
    groups.sort();
    groups.dedup();
    if groups.is_empty() {
        groups.push("Ungrouped".into());
    }
    rsx! {
        div { style: "display:flex; gap:14px; overflow-x:auto; padding:4px 0 20px;",
            for group in groups {
                div { style: "min-width:240px; flex:1; background:{color::NEUTRAL_100}; border:1px solid {color::NEUTRAL_200}; border-radius:8px; padding:12px;",
                    div { style: "font-weight:600; color:{color::NEUTRAL_800}; margin-bottom:10px;", "{group}" }
                    for entry in entries.iter().filter(|e| display_value(e.get(&field).unwrap_or(&serde_json::Value::Null)) == group) {
                        {render_view_card(entry, route, "", &field, uid.clone())}
                    }
                }
            }
        }
    }
}

fn render_gallery(
    entries: &[serde_json::Value],
    mut route: Signal<Route>,
    schema: Option<&Schema>,
    field: String,
    uid: String,
) -> Element {
    let image_field = schema
        .and_then(|s| {
            s.attributes
                .iter()
                .find(|(_, a)| a.attr_type == FieldType::Media)
                .map(|(n, _)| n.clone())
        })
        .unwrap_or_else(|| field.clone());
    rsx! {
        div { style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(220px,1fr)); gap:16px;",
            for entry in entries {
                div { style: "background:#fff; border:1px solid {color::NEUTRAL_200}; border-radius:8px; overflow:hidden; cursor:pointer;", onclick: { let id = entry_id(entry); let uid = uid.clone(); move |_| route.set(Route::ContentManagerEntry { uid: uid.clone(), document_id: id.clone() }) },
                    div { style: "height:130px; background:{color::NEUTRAL_100}; display:flex; align-items:center; justify-content:center; color:{color::NEUTRAL_500};", "{display_value(entry.get(&image_field).unwrap_or(&serde_json::Value::Null))}" }
                    div { style: "padding:12px;", div { style: "font-weight:600; color:{color::NEUTRAL_900};", "{display_value(entry.get(&field).unwrap_or(&serde_json::Value::Null))}" }, div { style: "font-size:13px; color:{color::NEUTRAL_600}; margin-top:4px;", "{entry_id(entry)}" } }
                }
            }
        }
    }
}

fn render_calendar(
    entries: &[serde_json::Value],
    route: Signal<Route>,
    field: String,
    uid: String,
) -> Element {
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px;",
            for entry in entries {
                {render_view_card(entry, route, "Date", &field, uid.clone())}
            }
        }
    }
}

fn render_view_card(
    entry: &serde_json::Value,
    mut route: Signal<Route>,
    label: &str,
    field: &str,
    uid: String,
) -> Element {
    let id = entry_id(entry);
    let value = display_value(entry.get(field).unwrap_or(&serde_json::Value::Null));
    rsx! {
        button { style: "display:block; width:100%; text-align:left; border:1px solid {color::NEUTRAL_200}; background:#fff; border-radius:6px; padding:10px; margin-bottom:8px; cursor:pointer;", onclick: move |_| route.set(Route::ContentManagerEntry { uid: uid.clone(), document_id: id.clone() }),
            if !label.is_empty() { span { style: "font-size:12px; color:{color::NEUTRAL_500};", "{label}" } }
            div { style: "font-weight:600; color:{color::NEUTRAL_900};", "{value}" }
            span { style: "font-size:12px; color:{color::NEUTRAL_500};", "{id}" }
        }
    }
}

#[component]
fn CreateViewModal(
    uid: String,
    on_close: EventHandler<()>,
    on_created: EventHandler<api_types::admin::ContentTypeView>,
) -> Element {
    let global = use_global();
    let mut name = use_signal(String::new);
    let mut view_type = use_signal(|| "grid".to_string());
    let mut status = use_signal(|| None::<String>);
    let options = vec![
        ("grid".into(), "Grid".into()),
        ("kanban".into(), "Kanban".into()),
        ("gallery".into(), "Gallery".into()),
        ("calendar".into(), "Calendar".into()),
    ];
    rsx! {
        Modal { title: "Create view".to_string(), width: 520, on_close: move |_| on_close.call(()),
            if let Some(message) = status() { div { style: "color:{color::DANGER_600}; padding-bottom:10px;", "{message}" } }
            TextField { value: name(), label: "Name".to_string(), placeholder: "e.g. Active products".to_string(), oninput: move |v| name.set(v) }
            Dropdown { label: "View type".to_string(), options, value: view_type(), onchange: move |v| view_type.set(v) }
            div { style: "display:flex; justify-content:flex-end; gap:10px; margin-top:16px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| on_close.call(()) }
                Button { label: "Create view".to_string(), on_click: move |_| {
                    let g = global.clone(); let uid = uid.clone(); let name = name(); let kind = view_type();
                    spawn(async move {
                        let ty = match kind.as_str() { "kanban" => api_types::admin::ViewType::Kanban, "gallery" => api_types::admin::ViewType::Gallery, "calendar" => api_types::admin::ViewType::Calendar, _ => api_types::admin::ViewType::Grid };
                        match g.client.cm_view_create(&uid, &api_types::admin::CreateContentTypeViewRequest { name, view_type: ty, description: None, configuration: Default::default() }).await { Ok(view) => on_created.call(view), Err(e) => status.set(Some(e.to_string())) }
                    });
                } }
            }
        }
    }
}

#[component]
fn ViewSettingsModal(
    uid: String,
    view: api_types::admin::ContentTypeView,
    fields: Vec<String>,
    on_close: EventHandler<()>,
    on_saved: EventHandler<api_types::admin::ContentTypeView>,
) -> Element {
    let global = use_global();
    let mut name = use_signal(|| view.name.clone());
    let mut description = use_signal(|| view.description.clone().unwrap_or_default());
    let mut view_type = use_signal(|| view.view_type);
    let mut config = use_signal(|| view.configuration.clone());
    let mut status = use_signal(|| None::<String>);
    let id = view.id;
    let mut ordered_columns = config().columns.clone();
    ordered_columns.sort_by_key(|column| column.position);
    let configured_fields: Vec<String> = ordered_columns
        .iter()
        .map(|column| column.field_id.clone())
        .collect();
    let mut sort_options = vec![
        (String::new(), "No saved sort".to_string()),
        ("id".to_string(), "ID".to_string()),
        ("documentId".to_string(), "Document ID".to_string()),
        ("createdAt".to_string(), "Created at".to_string()),
        ("updatedAt".to_string(), "Updated at".to_string()),
    ];
    for field in &fields {
        if !sort_options.iter().any(|(value, _)| value == field) {
            sort_options.push((field.clone(), field.clone()));
        }
    }
    let saved_sort = config().sorts.first().cloned();
    let sort_field = saved_sort
        .as_ref()
        .map(|sort| sort.field.clone())
        .unwrap_or_default();
    let sort_direction = saved_sort
        .as_ref()
        .map(|sort| sort.descending)
        .unwrap_or(false);
    let group_options: Vec<(String, String)> =
        std::iter::once((String::new(), "No grouping".to_string()))
            .chain(fields.iter().map(|field| (field.clone(), field.clone())))
            .collect();
    let group_by = config().group_by.clone().unwrap_or_default();
    rsx! {
        Modal { title: format!("Customize view · {}", view.name), width: 780, on_close: move |_| on_close.call(()),
            if let Some(message) = status() { div { style: "color:{color::DANGER_600}; padding-bottom:10px;", "{message}" } }
            div { style: "display:grid; grid-template-columns:1fr 1fr; gap:14px;",
                TextField { value: name(), label: "View name".to_string(), oninput: move |v| name.set(v) }
                Dropdown {
                    label: "Layout".to_string(),
                    options: vec![("grid".into(), "Grid".into()), ("kanban".into(), "Kanban".into()), ("gallery".into(), "Gallery".into()), ("calendar".into(), "Calendar".into())],
                    value: match view_type() { api_types::admin::ViewType::Grid => "grid", api_types::admin::ViewType::Kanban => "kanban", api_types::admin::ViewType::Gallery => "gallery", api_types::admin::ViewType::Calendar => "calendar" }.to_string(),
                    onchange: move |v: String| view_type.set(match v.as_str() { "kanban" => api_types::admin::ViewType::Kanban, "gallery" => api_types::admin::ViewType::Gallery, "calendar" => api_types::admin::ViewType::Calendar, _ => api_types::admin::ViewType::Grid }),
                }
            }
            TextArea { value: description(), label: "Description".to_string(), placeholder: "Explain when this view is useful".to_string(), rows: 2, oninput: move |v| description.set(v) }
            div { style: "margin-top:18px; padding:16px; background:{color::NEUTRAL_50}; border:1px solid {color::NEUTRAL_150}; border-radius:6px;",
                div { style: "font-size:{typography::BODY_BOLD_SIZE}; color:{color::NEUTRAL_900};", "Columns" }
                div { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500}; margin:4px 0 12px;", "Choose what appears and arrange its order." }
                for (index, column) in ordered_columns.iter().enumerate() {
                    {
                        let field_name = column.field_id.clone();
                        let checkbox_field = field_name.clone();
                        let width_field = field_name.clone();
                        let visible = column.visible;
                        let width = column.width.map(|width| width.to_string()).unwrap_or_default();
                        let can_move_up = index > 0;
                        let can_move_down = index + 1 < ordered_columns.len();
                        rsx! {
                            div { key: "view-column-{field_name}", style: "display:grid; grid-template-columns:28px minmax(0,1fr) 92px 74px; align-items:center; gap:10px; padding:8px 0; border-top:1px solid {color::NEUTRAL_150};",
                                input { r#type: "checkbox", checked: visible, aria_label: "Show {field_name}", onchange: move |event| { let mut next = config(); if let Some(column) = next.columns.iter_mut().find(|column| column.field_id == checkbox_field) { column.visible = event.checked(); } config.set(next); } }
                                span { style: "color:{color::NEUTRAL_800};", "{field_name}" }
                                input { class: "input", r#type: "number", min: "80", max: "600", value: "{width}", placeholder: "Auto", aria_label: "Width for {field_name}", oninput: move |event| { let mut next = config(); if let Some(column) = next.columns.iter_mut().find(|column| column.field_id == width_field) { column.width = event.value().parse::<u32>().ok(); } config.set(next); } }
                                div { style: "display:flex; gap:4px; justify-content:flex-end;",
                                    button { r#type: "button", disabled: !can_move_up, aria_label: "Move column up", style: "border:1px solid {color::NEUTRAL_200}; background:{color::NEUTRAL_0}; border-radius:4px; cursor:pointer; padding:3px 7px;", onclick: move |_| { if can_move_up { let mut next = config(); next.columns.sort_by_key(|column| column.position); next.columns.swap(index, index - 1); for (position, column) in next.columns.iter_mut().enumerate() { column.position = position as u32; } config.set(next); } }, "↑" }
                                    button { r#type: "button", disabled: !can_move_down, aria_label: "Move column down", style: "border:1px solid {color::NEUTRAL_200}; background:{color::NEUTRAL_0}; border-radius:4px; cursor:pointer; padding:3px 7px;", onclick: move |_| { if can_move_down { let mut next = config(); next.columns.sort_by_key(|column| column.position); next.columns.swap(index, index + 1); for (position, column) in next.columns.iter_mut().enumerate() { column.position = position as u32; } config.set(next); } }, "↓" }
                                }
                            }
                        }
                    }
                }
                div { style: "display:flex; flex-wrap:wrap; gap:8px; margin-top:12px;",
                    for field in fields.iter().filter(|field| !configured_fields.contains(field)) {
                        { let field_name = field.clone(); rsx! { button { r#type: "button", style: "border:1px dashed {color::NEUTRAL_300}; background:{color::NEUTRAL_0}; color:{color::NEUTRAL_700}; border-radius:4px; padding:6px 10px; cursor:pointer;", onclick: move |_| { let mut next = config(); let position = next.columns.len() as u32; next.columns.push(api_types::admin::ViewColumn { field_id: field_name.clone(), visible: true, width: None, position }); config.set(next); }, "+ {field}" } } }
                    }
                }
            }
            div { style: "display:grid; grid-template-columns:1fr 1fr; gap:14px; margin-top:18px;",
                Dropdown { label: "Group records by".to_string(), options: group_options, value: group_by, onchange: move |v: String| { let mut next = config(); next.group_by = if v.is_empty() { None } else { Some(v) }; config.set(next); } }
                Dropdown { label: "Sort by".to_string(), options: sort_options, value: sort_field, onchange: move |v: String| { let mut next = config(); next.sorts = if v.is_empty() { Vec::new() } else { vec![api_types::SortField { field: v, descending: sort_direction }] }; config.set(next); } }
                Dropdown { label: "Sort direction".to_string(), options: vec![("asc".into(), "Ascending".into()), ("desc".into(), "Descending".into())], value: if sort_direction { "desc".to_string() } else { "asc".to_string() }, onchange: move |v: String| { let mut next = config(); if let Some(sort) = next.sorts.first_mut() { sort.descending = v == "desc"; } config.set(next); } }
                Dropdown { label: "Rows per page".to_string(), options: vec![("10".into(), "10".into()), ("25".into(), "25".into()), ("50".into(), "50".into()), ("100".into(), "100".into())], value: config().page_size.unwrap_or(25).to_string(), onchange: move |v: String| { let mut next = config(); next.page_size = v.parse::<u32>().ok(); config.set(next); } }
            }
            div { style: "display:flex; justify-content:flex-end; gap:10px; margin-top:22px;", Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| on_close.call(()) }, Button { label: "Save view".to_string(), on_click: move |_| { let g = global.clone(); let uid = uid.clone(); let mut next = config(); next.columns.sort_by_key(|column| column.position); for (position, column) in next.columns.iter_mut().enumerate() { column.position = position as u32; } let request = api_types::admin::UpdateContentTypeViewRequest { name: Some(name()), description: Some(description()), view_type: Some(view_type()), configuration: Some(next) }; spawn(async move { match g.client.cm_view_update(&uid, id, &request).await { Ok(saved) => on_saved.call(saved), Err(e) => status.set(Some(e.to_string())) } }); } } }
        }
    }
}

/// Entry editor for one document (create or edit). Single types open here with
/// a "default" document id; new collection entries use the NEW_ENTRY marker.
#[component]
pub fn ContentManagerEntry(uid: String, document_id: String) -> Element {
    let global = use_global();
    let mut schemas: Signal<Vec<Schema>> = use_signal(|| vec![]);
    let mut schema_loaded = use_signal(|| false);
    let mut form = use_signal(serde_json::Map::new);
    let mut route = global.route;
    let mut loading = use_signal(|| true);

    let uid_load = uid.clone();
    let document_id_load = document_id.clone();
    let g_schema = global.clone();
    use_effect(move || {
        if !schema_loaded() {
            schema_loaded.set(true);
            let mut g = g_schema.clone();
            let mut sc = schemas;
            let mut f = form;
            let uid = uid_load.clone();
            let document_id = document_id_load.clone();
            spawn(async move {
                match g.client.ctb_list().await {
                    Ok(v) => {
                        let schemas_vec: Vec<Schema> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let names: Vec<(String, String)> = schemas_vec
                            .iter()
                            .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                            .collect();
                        g.ct_names.set(names);
                        // Load the existing entry unless we are creating a new one.
                        if document_id != NEW_ENTRY {
                            if let Ok(resp) = g.client.cm_get(&uid, &document_id).await {
                                f.set(resp.data.as_object().cloned().unwrap_or_default());
                            }
                        }
                        sc.set(schemas_vec);
                    }
                    Err(e) => {
                        let _ = e;
                    }
                }
                loading.set(false);
            });
        }
    });

    let schema = schemas().iter().find(|s| s.uid.as_str() == uid).cloned();
    let is_single = schema
        .as_ref()
        .map(|s| s.kind == ContentTypeKind::SingleType)
        .unwrap_or(false);

    if let Some(schema) = schema.clone() {
        let doc = if is_single {
            "default".to_string()
        } else {
            document_id.clone()
        };
        let back_route_uid = uid.clone();
        let back_route_uid2 = uid.clone();
        let mut route_back = route.clone();
        let mut route_saved = route.clone();
        rsx! {
            EntryEditView {
                schema,
                schemas: schemas(),
                document_id: doc,
                is_single,
                form,
                on_back: move |_| {
                    if is_single {
                        route_back.set(Route::ContentManager);
                    } else {
                        route_back.set(Route::ContentManagerEntries(back_route_uid.clone()));
                    }
                },
                on_saved: move |_| {
                    if is_single {
                        route_saved.set(Route::ContentManager);
                    } else {
                        route_saved.set(Route::ContentManagerEntries(back_route_uid2.clone()));
                    }
                },
            }
        }
    } else if loading() {
        rsx! {
            div { style: "padding:48px; display:flex; justify-content:center;", Spinner { size: 28 } }
        }
    } else {
        let mut route_nf = route;
        rsx! {
            div { style: "padding:48px;",
                EmptyState {
                    title: "Content type not found".to_string(),
                    subtitle: "This content type may have been deleted.".to_string(),
                    icon: "stack".to_string(),
                    Button { label: "← Back to Content Types".to_string(), variant: "secondary".to_string(), on_click: move |_| route_nf.set(Route::ContentManager) }
                }
            }
        }
    }
}

fn relation_is_many(attr: &core_schema::Attribute) -> bool {
    matches!(
        attr.relation,
        Some(
            core_domain::RelationKind::ManyToMany
                | core_domain::RelationKind::ManyWay
                | core_domain::RelationKind::OneToMany
        )
    )
}

fn relation_document_id(value: &serde_json::Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("documentId"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_str().map(ToOwned::to_owned))
}

fn relation_selection(value: Option<&serde_json::Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    if let Some(values) = value.as_array() {
        return values.iter().filter_map(relation_document_id).collect();
    }
    relation_document_id(value).into_iter().collect()
}

fn relation_payload(ids: &[String]) -> serde_json::Value {
    serde_json::Value::Array(
        ids.iter()
            .map(|id| serde_json::json!({"documentId": id}))
            .collect(),
    )
}

fn relation_label(value: &serde_json::Value) -> String {
    let object = value.as_object();
    ["name", "title", "legal_name", "code", "sku", "documentId"]
        .iter()
        .find_map(|field| object.and_then(|object| object.get(*field)))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| relation_document_id(value))
        .or_else(|| {
            value
                .get("id")
                .and_then(|id| id.as_i64())
                .map(|id| id.to_string())
        })
        .unwrap_or_else(|| display_value(value))
}

fn relation_details(value: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .filter(|(field, _)| {
            !matches!(
                field.as_str(),
                "id" | "documentId"
                    | "__pendingId"
                    | "createdAt"
                    | "updatedAt"
                    | "publishedAt"
                    | "locale"
                    | "publicationState"
            )
        })
        .map(|(field, value)| {
            let value = match value {
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(relation_detail_value)
                    .collect::<Vec<_>>()
                    .join(", "),
                other => relation_detail_value(other),
            };
            (field.replace('_', " "), value)
        })
        .collect()
}

fn relation_detail_value(value: &serde_json::Value) -> String {
    if value.is_object() {
        relation_label(value)
    } else {
        display_value(value)
    }
}

fn relation_summary(value: &serde_json::Value) -> String {
    let details = relation_details(value);
    if details.is_empty() {
        return relation_label(value);
    }
    details
        .into_iter()
        .map(|(field, value)| format!("{field}: {value}"))
        .collect::<Vec<_>>()
        .join(" | ")
}

#[component]
fn RelationSearchModal(
    title: String,
    target_uid: String,
    search_fields: Vec<String>,
    targets: Vec<serde_json::Value>,
    selected: Vec<String>,
    on_close: EventHandler<()>,
    on_select: EventHandler<serde_json::Value>,
) -> Element {
    let global = use_global();
    let mut query = use_signal(String::new);
    let mut results = use_signal(|| targets.clone());
    let mut request_id = use_signal(|| 0u64);
    let g_search = global.clone();
    let uid_search = target_uid.clone();
    let fields_search = search_fields.clone();
    let fallback_targets = targets.clone();
    use_effect(move || {
        let search_text = query().trim().to_string();
        if search_text.is_empty() {
            request_id.set(request_id() + 1);
            results.set(fallback_targets.clone());
            return;
        }
        if fields_search.is_empty() || uid_search.is_empty() {
            request_id.set(request_id() + 1);
            results.set(Vec::new());
            return;
        }
        let current_request = request_id() + 1;
        request_id.set(current_request);
        let client = g_search.client.clone();
        let fields = fields_search.clone();
        let uid = uid_search.clone();
        spawn(async move {
            let params = QueryParams {
                filters: Some(api_types::Filter::Or(
                    fields
                        .into_iter()
                        .map(|field| api_types::Filter::Leaf {
                            field,
                            op: api_types::FilterOp::ContainsI,
                            values: vec![serde_json::Value::String(search_text.clone())],
                        })
                        .collect(),
                )),
                pagination: Some(PaginationParams::Page {
                    page: 1,
                    page_size: 50,
                    with_count: Some(false),
                }),
                ..Default::default()
            };
            if let Ok(response) = client.cm_list(&uid, &params).await {
                if request_id() == current_request {
                    results.set(response.data);
                }
            }
        });
    });
    let needle = query().trim().to_lowercase();
    let matches: Vec<_> = results()
        .iter()
        .filter(|target| {
            relation_document_id(target).is_some_and(|id| !selected.contains(&id))
                && (needle.is_empty() || relation_summary(target).to_lowercase().contains(&needle))
        })
        .cloned()
        .collect();

    rsx! {
        Modal { title, width: 720, on_close: move |_| on_close.call(()),
            TextField {
                value: query(),
                label: "Search".to_string(),
                placeholder: "Search by name or details".to_string(),
                oninput: move |value| query.set(value),
            }
            div { style: "max-height: min(56vh, 520px); overflow:auto; margin-top:12px; border:1px solid {color::NEUTRAL_150}; border-radius:4px;",
                if matches.is_empty() {
                    div { style: "padding:18px; color:{color::NEUTRAL_500}; font-size:{typography::BODY_SIZE};", "No matching records" }
                }
                for target in matches.iter() {
                    if let Some(id) = relation_document_id(target) {
                        {
                            let label = relation_label(target);
                            let summary = relation_summary(target);
                            let selected_target = target.clone();
                            rsx! {
                                button {
                                    key: "{id}",
                                    r#type: "button",
                                    style: "display:flex; flex-direction:column; align-items:flex-start; width:100%; gap:4px; padding:12px 14px; border:0; border-bottom:1px solid {color::NEUTRAL_150}; background:{color::NEUTRAL_0}; text-align:left; cursor:pointer;",
                                    onclick: move |_| on_select.call(selected_target.clone()),
                                    span { style: "font-size:{typography::BODY_BOLD_SIZE}; color:{color::NEUTRAL_800};", "{label}" }
                                    if summary != label {
                                        span { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500}; overflow-wrap:anywhere;", "{summary}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn InlineLineModal(
    schema: Schema,
    schemas: Vec<Schema>,
    inverse_field: String,
    parent_document_id: String,
    line: Option<serde_json::Value>,
    defer_save: bool,
    on_close: EventHandler<()>,
    on_saved: EventHandler<serde_json::Value>,
) -> Element {
    let global = use_global();
    let initial_line = line.clone();
    let mut form = use_signal(move || {
        initial_line
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .cloned()
            .unwrap_or_default()
    });
    let mut saving = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let pending_lines = use_signal(Vec::<PendingInlineLine>::new);
    let document_id = line.as_ref().and_then(relation_document_id);
    let is_new = document_id.is_none();
    let title = if is_new { "Add line" } else { "Edit line" };
    let line_uid = schema.uid.as_str().to_string();
    let computed_names: Vec<String> = schema
        .attributes
        .iter()
        .filter(|(_, attribute)| attribute.computed)
        .map(|(field, _)| field.clone())
        .collect();
    let scalar_fields: Vec<(String, FieldType, Vec<String>, core_schema::Attribute)> = schema
        .attributes
        .iter()
        .filter(|(_, attribute)| {
            attribute.attr_type.is_scalar_column() && attribute.attr_type != FieldType::Password
        })
        .map(|(field, attribute)| {
            (
                field.clone(),
                attribute.attr_type,
                attribute.enum_values.clone(),
                attribute.clone(),
            )
        })
        .collect();
    let relation_fields: Vec<(String, core_schema::Attribute)> = schema
        .attributes
        .iter()
        .filter(|(field, attribute)| {
            attribute.attr_type == FieldType::Relation && **field != inverse_field
        })
        .map(|(field, attribute)| (field.clone(), attribute.clone()))
        .collect();
    let save_uid = line_uid.clone();
    let save_inverse = inverse_field.clone();
    let save_parent = parent_document_id.clone();
    let save_document_id = document_id.clone();
    let save_computed = computed_names.clone();
    let cancel_close = on_close;

    rsx! {
        Modal { title: title.to_string(), width: 860, on_close: move |_| on_close.call(()),
            if let Some(message) = status() {
                div { style: "margin-bottom:14px; color:{color::DANGER_600};", "{message}" }
            }
            div { class: "cm-inline-line-grid",
                for (name, field_type, enum_values, attribute) in scalar_fields.into_iter() {
                    if attribute.is_visible(&form()) {
                        match field_type {
                            FieldType::Boolean => rsx! {
                                Toggle {
                                    checked: form().get(&name).and_then(|value| value.as_bool()).unwrap_or(false),
                                    label: name.clone(),
                                    disabled: attribute.computed,
                                    onchange: move |value| { form.write().insert(name.clone(), serde_json::Value::Bool(value)); }
                                }
                            },
                            FieldType::Enumeration => rsx! {
                                Dropdown {
                                    label: name.clone(),
                                    options: enum_values.iter().map(|value| (value.clone(), value.clone())).collect(),
                                    value: input_value(form().get(&name)),
                                    disabled: attribute.computed,
                                    onchange: move |value| { form.write().insert(name.clone(), serde_json::Value::String(value)); }
                                }
                            },
                            _ => rsx! {
                                TextField {
                                    label: name.clone(),
                                    value: input_value(form().get(&name)),
                                    disabled: attribute.computed,
                                    oninput: move |value| { form.write().insert(name.clone(), serde_json::Value::String(value)); }
                                }
                            },
                        }
                    }
                }
            }
            div { class: "cm-inline-line-relations",
                for (name, attribute) in relation_fields.into_iter() {
                    RelationField {
                        key: "line-relation-{name}",
                        name,
                        attr: attribute,
                        form,
                        schemas: schemas.clone(),
                        owner_uid: line_uid.clone(),
                        owner_document_id: document_id.clone().unwrap_or_default(),
                        pending_lines,
                    }
                }
            }
            div { style: "display:flex; justify-content:flex-end; gap:10px; margin-top:20px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| cancel_close.call(()) }
                Button {
                    label: "Save line".to_string(),
                    loading: saving(),
                    on_click: move |_| {
                        let mut data = form();
                        for field in &save_computed { data.remove(field); }
                        data.remove("id");
                        data.remove("documentId");
                        if !defer_save && !save_inverse.is_empty() {
                            data.insert(save_inverse.clone(), serde_json::json!({"documentId": save_parent}));
                        }
                        if defer_save {
                            on_saved.call(serde_json::Value::Object(data));
                            on_close.call(());
                            return;
                        }
                        let data = serde_json::Value::Object(data);
                        let global = global.clone();
                        let uid = save_uid.clone();
                        let existing_id = save_document_id.clone();
                        saving.set(true);
                        spawn(async move {
                            let result = if let Some(document_id) = existing_id {
                                global.client.cm_update(&uid, &document_id, &data).await
                            } else {
                                global.client.cm_create(&uid, &data).await
                            };
                            saving.set(false);
                            match result {
                                Ok(response) => {
                                    let line = if let Some(document_id) = relation_document_id(&response.data) {
                                        global.client.cm_get(&uid, &document_id).await
                                            .map(|entry| entry.data)
                                            .unwrap_or(response.data)
                                    } else {
                                        response.data
                                    };
                                    on_saved.call(line);
                                    on_close.call(());
                                }
                                Err(error) => status.set(Some(error.to_string())),
                            }
                        });
                    }
                }
            }
        }
    }
}

/// Schema-driven relation picker used by both new and existing entries.
/// Single relations are assigned with one dropdown; collection relations keep
/// a selected list with explicit add/remove controls.
#[component]
fn RelationField(
    name: String,
    attr: core_schema::Attribute,
    form: Signal<serde_json::Map<String, serde_json::Value>>,
    schemas: Vec<Schema>,
    owner_uid: String,
    owner_document_id: String,
    pending_lines: Signal<Vec<PendingInlineLine>>,
) -> Element {
    let global = use_global();
    let mut targets = use_signal(Vec::<serde_json::Value>::new);
    let mut loaded = use_signal(|| false);
    let target_uid = attr.target.as_ref().map(|uid| uid.as_str().to_string());
    let is_many = relation_is_many(&attr);
    let mut search_open = use_signal(|| false);
    let mut line_editor_open = use_signal(|| false);
    let mut editing_line = use_signal(|| None::<serde_json::Value>);
    let search_fields: Vec<String> = target_uid
        .as_ref()
        .and_then(|target_uid| {
            schemas
                .iter()
                .find(|schema| schema.uid.as_str() == target_uid)
        })
        .map(|schema| {
            schema
                .attributes
                .iter()
                .filter(|(_, attribute)| {
                    attribute.attr_type.is_scalar_column()
                        && attribute.attr_type != FieldType::Password
                        && !attribute.private
                })
                .map(|(field, _)| field.clone())
                .collect()
        })
        .unwrap_or_default();

    let g_load = global.clone();
    let target_uid_load = target_uid.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            if let Some(target_uid) = target_uid_load.clone() {
                let client = g_load.client.clone();
                spawn(async move {
                    let params = QueryParams {
                        pagination: Some(PaginationParams::Page {
                            page: 1,
                            page_size: 1_000,
                            with_count: Some(false),
                        }),
                        ..Default::default()
                    };
                    if let Ok(response) = client.cm_list(&target_uid, &params).await {
                        targets.set(response.data);
                    }
                });
            }
        }
    });

    let selected = relation_selection(form().get(&name));
    let mut options: Vec<(String, String)> = vec![(String::new(), "Select relation".to_string())];
    for target in targets().iter() {
        if let Some(id) = relation_document_id(target) {
            if !is_many || !selected.contains(&id) {
                options.push((
                    id,
                    if is_many {
                        relation_summary(target)
                    } else {
                        relation_label(target)
                    },
                ));
            }
        }
    }

    let line_schema = target_uid
        .as_ref()
        .and_then(|target_uid| {
            schemas
                .iter()
                .find(|schema| schema.uid.as_str() == target_uid)
        })
        .filter(|schema| is_inline_document(schema))
        .cloned();
    let inverse_field = attr.mapped_by.clone().or_else(|| {
        line_schema.as_ref().and_then(|line_schema| {
            line_schema
                .attributes
                .iter()
                .find(|(_, candidate)| {
                    candidate.attr_type == FieldType::Relation
                        && candidate
                            .target
                            .as_ref()
                            .is_some_and(|target| target.as_str() == owner_uid.as_str())
                })
                .map(|(field, _)| field.clone())
        })
    });
    if is_many && line_schema.is_some() && inverse_field.is_some() {
        let mut target_rows = targets;
        let mut form_for_saved = form;
        let field_name = name.clone();
        let inverse_field = inverse_field.unwrap_or_default();
        let mut editor_close = line_editor_open;
        let mut editor_saved_close = line_editor_open;
        let editing_line_for_editor = editing_line;
        let line_schema = line_schema.unwrap();
        let inverse_field_for_save = inverse_field.clone();
        let line_schema_for_editor = line_schema.clone();
        let schemas_for_editor = schemas.clone();
        let owner_document_for_editor = owner_document_id.clone();
        let mut pending_for_saved = pending_lines;
        return rsx! {
            div { class: "cm-relation-field cm-inline-lines",
                div { class: "cm-relation-heading",
                    span { class: "cm-relation-label", "{name}" }
                    span { class: "cm-relation-help", "Invoice lines" }
                }
                if selected.is_empty() {
                    div { class: "cm-relation-empty", "No lines added" }
                }
                for selected_id in selected.iter() {
                    {
                        let selected_id = selected_id.clone();
                        let line = form()
                            .get(&field_name)
                            .and_then(serde_json::Value::as_array)
                            .and_then(|lines| lines.iter().find(|line| relation_document_id(line).as_deref() == Some(selected_id.as_str())).cloned())
                            .or_else(|| targets()
                                .iter()
                                .find(|target| relation_document_id(target).as_deref() == Some(selected_id.as_str()))
                                .cloned());
                        let summary = line.as_ref().map(relation_summary).unwrap_or_else(|| selected_id.clone());
                        let details = line.as_ref().map(relation_details).unwrap_or_default();
                        let edit_line = line.clone();
                        let delete_id = selected_id.clone();
                        let delete_uid = line_schema.uid.as_str().to_string();
                        let delete_name = field_name.clone();
                        let mut delete_form = form;
                        let mut delete_targets = target_rows;
                        let global = global.clone();
                        rsx! {
                            div { key: "inline-line-{selected_id}", class: "cm-relation-row cm-inline-line-row",
                                div { class: "cm-relation-record",
                                    span { class: "cm-relation-name", "{summary}" }
                                    if !details.is_empty() {
                                        div { class: "cm-relation-details",
                                            for (field, value) in details {
                                                div { class: "cm-relation-detail", key: "{field}",
                                                    span { class: "cm-relation-detail-label", "{field}" }
                                                    span { class: "cm-relation-detail-value", "{value}" }
                                                }
                                            }
                                        }
                                    }
                                }
                                IconButton {
                                    name: "pencil".to_string(),
                                    aria_label: "Edit line".to_string(),
                                    on_click: move |_| {
                                        editing_line.set(edit_line.clone());
                                        line_editor_open.set(true);
                                    }
                                }
                                IconButton {
                                    name: "trash".to_string(),
                                    variant: "danger".to_string(),
                                    aria_label: "Remove line".to_string(),
                                    on_click: move |_| {
                                        let global = global.clone();
                                        let uid = delete_uid.clone();
                                        let document_id = delete_id.clone();
                                        let field = delete_name.clone();
                                        spawn(async move {
                                            if global.client.cm_delete(&uid, &document_id).await.is_ok() {
                                                delete_targets.write().retain(|target| relation_document_id(target).as_deref() != Some(document_id.as_str()));
                                                let mut ids = relation_selection(delete_form().get(&field));
                                                ids.retain(|id| id != &document_id);
                                                delete_form.write().insert(field.clone(), relation_payload(&ids));
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                for (_, _, _, pending_line) in pending_lines().iter().filter(|(relation_field, target_uid, inverse, _)| relation_field == &name && target_uid == line_schema.uid.as_str() && inverse == &inverse_field) {
                    {
                        let pending_id = pending_line.get("__pendingId").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
                        let summary = relation_summary(pending_line);
                        let details = relation_details(pending_line);
                        let line_for_edit = pending_line.clone();
                        let mut edit_pending = editing_line;
                        let mut open_editor = line_editor_open;
                        let mut pending_for_remove = pending_lines;
                        rsx! {
                            div { key: "pending-line-{pending_id}", class: "cm-relation-row cm-inline-line-row",
                                div { class: "cm-relation-record",
                                    span { class: "cm-relation-name", "{summary}" }
                                    if !details.is_empty() {
                                        div { class: "cm-relation-details",
                                            for (field, value) in details {
                                                div { class: "cm-relation-detail", key: "{field}",
                                                    span { class: "cm-relation-detail-label", "{field}" }
                                                    span { class: "cm-relation-detail-value", "{value}" }
                                                }
                                            }
                                        }
                                    }
                                }
                                IconButton {
                                    name: "pencil".to_string(),
                                    aria_label: "Edit line".to_string(),
                                    on_click: move |_| { edit_pending.set(Some(line_for_edit.clone())); open_editor.set(true); }
                                }
                                IconButton {
                                    name: "trash".to_string(),
                                    variant: "danger".to_string(),
                                    aria_label: "Remove line".to_string(),
                                    on_click: move |_| pending_for_remove.write().retain(|entry| entry.3.get("__pendingId").and_then(serde_json::Value::as_str) != Some(pending_id.as_str()))
                                }
                            }
                        }
                    }
                }
                if owner_document_id.is_empty() || owner_document_id == NEW_ENTRY {
                    Button {
                        label: "Add line".to_string(),
                        variant: "secondary".to_string(),
                        on_click: move |_| {
                            editing_line.set(None);
                            line_editor_open.set(true);
                        }
                    }
                } else {
                    Button {
                        label: "Add line".to_string(),
                        variant: "secondary".to_string(),
                        on_click: move |_| {
                            editing_line.set(None);
                            line_editor_open.set(true);
                        }
                    }
                }
                if line_editor_open() {
                    InlineLineModal {
                        schema: line_schema_for_editor,
                        schemas: schemas_for_editor,
                        inverse_field: inverse_field.clone(),
                        parent_document_id: owner_document_for_editor,
                        line: editing_line_for_editor(),
                        defer_save: owner_document_id == NEW_ENTRY || editing_line_for_editor().as_ref().is_some_and(|line| line.get("__pendingId").is_some()),
                        on_close: move |_| editor_close.set(false),
                        on_saved: move |line: serde_json::Value| {
                            if line.get("__pendingId").is_some() {
                                let pending_id = line.get("__pendingId").and_then(serde_json::Value::as_str).map(ToOwned::to_owned);
                                if let Some(pending_id) = pending_id {
                                    if let Some(existing) = pending_for_saved.write().iter_mut().find(|entry| entry.3.get("__pendingId").and_then(serde_json::Value::as_str) == Some(pending_id.as_str())) {
                                        existing.3 = line;
                                    }
                                }
                            } else if owner_document_id == NEW_ENTRY {
                                let mut line = line;
                                let pending_id = pending_for_saved()
                                    .iter()
                                    .filter_map(|entry| entry.3.get("__pendingId").and_then(serde_json::Value::as_str))
                                    .filter_map(|id| id.strip_prefix("pending-")?.parse::<u64>().ok())
                                    .max()
                                    .unwrap_or(0)
                                    + 1;
                                let pending_id = format!("pending-{pending_id}");
                                line["__pendingId"] = serde_json::Value::String(pending_id);
                                pending_for_saved.write().push((field_name.clone(), line_schema.uid.as_str().to_string(), inverse_field_for_save.clone(), line));
                            } else if let Some(document_id) = relation_document_id(&line) {
                                let mut ids = relation_selection(form_for_saved().get(&field_name));
                                if !ids.contains(&document_id) { ids.push(document_id.clone()); }
                                form_for_saved.write().insert(field_name.clone(), relation_payload(&ids));
                                target_rows.write().retain(|target| relation_document_id(target).as_deref() != Some(document_id.as_str()));
                                target_rows.write().push(line);
                            }
                            editor_saved_close.set(false);
                        },
                    }
                }
            }
        };
    }

    if !is_many {
        let selected_id = selected.first().cloned().unwrap_or_default();
        let field_name = name.clone();
        let clear_name = field_name.clone();
        let search_name = field_name.clone();
        let mut search_form = form;
        let mut search_targets = targets;
        let mut search_open_close = search_open;
        let mut search_open_select = search_open;
        return rsx! {
            div { class: "cm-relation-field",
                div { class: "cm-relation-heading",
                    span { class: "cm-relation-label", "{name}" }
                    span { class: "cm-relation-help", "One relation" }
                }
                if let Some(selected_value) = selected.first() {
                    div { class: "cm-relation-row",
                        span { class: "cm-relation-name",
                            {targets().iter().find(|target| relation_document_id(target).as_deref() == Some(selected_value.as_str())).map(relation_label).unwrap_or_else(|| selected_value.clone())}
                        }
                        button { class: "cm-relation-remove", aria_label: "Remove relation", onclick: move |_| { form.write().insert(clear_name.clone(), serde_json::Value::Null); }, "Remove" }
                    }
                }
                div { class: "cm-relation-picker",
                    div { style: "flex:1; min-width:0;",
                        Dropdown {
                            label: if selected_id.is_empty() { "Select relation".to_string() } else { "Change relation".to_string() },
                            options,
                            value: selected_id,
                            onchange: move |value: String| {
                                if value.is_empty() {
                                    form.write().insert(field_name.clone(), serde_json::Value::Null);
                                } else {
                                    form.write().insert(field_name.clone(), serde_json::json!({"documentId": value}));
                                }
                            }
                        }
                    }
                    IconButton { name: "search".to_string(), aria_label: "Search {name}".to_string(), on_click: move |_| search_open.set(true) }
                }
                if search_open() {
                    RelationSearchModal {
                        title: format!("Select {name}"),
                        target_uid: target_uid.clone().unwrap_or_default(),
                        search_fields: search_fields.clone(),
                        targets: targets(),
                        selected: vec![],
                        on_close: move |_| search_open_close.set(false),
                        on_select: move |value| {
                            if let Some(document_id) = relation_document_id(&value) {
                                search_form.write().insert(search_name.clone(), serde_json::json!({"documentId": document_id}));
                            }
                            search_targets.write().retain(|target| relation_document_id(target) != relation_document_id(&value));
                            search_targets.write().push(value);
                            search_open_select.set(false);
                        },
                    }
                }
            }
        };
    }

    let add_name = name.clone();
    let selected_for_add = selected.clone();
    let mut form_for_add = form;
    let search_name = name.clone();
    let mut form_for_search = form;
    let mut search_targets = targets;
    let selected_for_search = selected.clone();
    let mut search_open_close = search_open;
    let mut search_open_select = search_open;
    rsx! {
        div { class: "cm-relation-field",
            div { class: "cm-relation-heading",
                span { class: "cm-relation-label", "{name}" }
                span { class: "cm-relation-help", "Multiple relations · add or remove entries" }
            }
            if selected.is_empty() {
                div { class: "cm-relation-empty", "No relations selected" }
            }
            for selected_id in selected.iter() {
                {
                    let selected_id = selected_id.clone();
                    let remove_name = name.clone();
                    let remove_id = selected_id.clone();
                    let target_label = targets()
                        .iter()
                        .find(|target| relation_document_id(target).as_deref() == Some(selected_id.as_str()))
                        .map(relation_label)
                        .unwrap_or_else(|| selected_id.clone());
                    let target_details = targets()
                        .iter()
                        .find(|target| relation_document_id(target).as_deref() == Some(selected_id.as_str()))
                        .map(relation_details)
                        .unwrap_or_default();
                    rsx! {
                        div { key: "{remove_id}", class: "cm-relation-row",
                            div { class: "cm-relation-record",
                                span { class: "cm-relation-name", "{target_label}" }
                                if !target_details.is_empty() {
                                    div { class: "cm-relation-details",
                                        for (field, value) in target_details {
                                            div { class: "cm-relation-detail", key: "{field}",
                                                span { class: "cm-relation-detail-label", "{field}" }
                                                span { class: "cm-relation-detail-value", "{value}" }
                                            }
                                        }
                                    }
                                }
                            }
                            button { class: "cm-relation-remove",
                                onclick: move |_| {
                                    let mut ids = relation_selection(form().get(&remove_name));
                                    ids.retain(|id| id != &remove_id);
                                    form.write().insert(remove_name.clone(), relation_payload(&ids));
                                },
                                "Remove"
                            }
                        }
                    }
                }
            }
            div { class: "cm-relation-picker",
                div { style: "flex:1; min-width:0;",
                    Dropdown {
                        label: "Add relation".to_string(),
                        options,
                        value: String::new(),
                        onchange: move |value: String| {
                            if !value.is_empty() && !selected_for_add.contains(&value) {
                                let mut ids = relation_selection(form_for_add().get(&add_name));
                                if !ids.contains(&value) { ids.push(value); }
                                form_for_add.write().insert(add_name.clone(), relation_payload(&ids));
                            }
                        }
                    }
                }
                IconButton { name: "search".to_string(), aria_label: "Search {name}".to_string(), on_click: move |_| search_open.set(true) }
            }
            if search_open() {
                RelationSearchModal {
                    title: format!("Select {name}"),
                    target_uid: target_uid.clone().unwrap_or_default(),
                    search_fields: search_fields.clone(),
                    targets: targets(),
                    selected: selected_for_search,
                    on_close: move |_| search_open_close.set(false),
                    on_select: move |value| {
                        let Some(document_id) = relation_document_id(&value) else { return; };
                        let mut ids = relation_selection(form_for_search().get(&search_name));
                        if !ids.contains(&document_id) { ids.push(document_id); }
                        form_for_search.write().insert(search_name.clone(), relation_payload(&ids));
                        search_targets.write().retain(|target| relation_document_id(target) != relation_document_id(&value));
                        search_targets.write().push(value);
                        search_open_select.set(false);
                    },
                }
            }
        }
    }
}

#[component]
fn CmTableTh(label: String) -> Element {
    rsx! {
        th { class: "cm-table-th", "{label}" }
    }
}

#[component]
fn CmTypeChip(label: String, active: bool, on_click: EventHandler<MouseEvent>) -> Element {
    let class = if active { "cm-segment-active" } else { "" };
    rsx! {
        button { class: "{class}", onclick: move |e| on_click.call(e), "{label}" }
    }
}

/// Modal to add a condition-based filter (field, operator, value).
#[component]
fn FilterModal(
    fields: Vec<String>,
    on_add: EventHandler<(String, String, String)>,
    on_close: EventHandler<()>,
) -> Element {
    let mut field = use_signal(String::new);
    let mut op = use_signal(|| "eq".to_string());
    let mut value = use_signal(String::new);

    let field_options: Vec<(String, String)> =
        std::iter::once(("".to_string(), "Select field".to_string()))
            .chain(fields.iter().map(|f| (f.clone(), f.clone())))
            .collect();
    let op_options: Vec<(String, String)> = vec![
        ("eq".to_string(), "is".to_string()),
        ("neq".to_string(), "is not".to_string()),
        ("contains".to_string(), "contains".to_string()),
    ];

    rsx! {
        Modal { title: "Add filter".to_string(), width: 480, on_close: move |_| on_close.call(()),
            Dropdown { label: "Field".to_string(), options: field_options, value: "{field}", onchange: move |v| field.set(v) }
            Dropdown { label: "Operator".to_string(), options: op_options, value: "{op}", onchange: move |v| op.set(v) }
            TextField { value: "{value}", label: "Value".to_string(), placeholder: "Filter value".to_string(), oninput: move |v| value.set(v) }
            div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| on_close.call(()) }
                Button { label: "Apply".to_string(), variant: "primary".to_string(), on_click: move |_| {
                    if !field().is_empty() {
                        on_add.call((field(), op(), value()));
                    }
                } }
            }
        }
    }
}

/// Confirm-and-delete dialog for a single entry.
#[component]
fn DeleteConfirmDialog(
    del_id: String,
    uid: String,
    on_close: EventHandler<()>,
    on_deleted: EventHandler<()>,
) -> Element {
    let global = use_global();
    let save_uid = uid.clone();
    let save_id = del_id.clone();
    rsx! {
        ConfirmDialog {
            title: "Delete entry".to_string(),
            message: format!("Are you sure you want to delete entry {del_id}? This cannot be undone."),
            confirm_label: "Delete".to_string(),
            on_cancel: move |_| on_close.call(()),
            on_confirm: move |_| {
                let g = global.clone();
                let uid = save_uid.clone();
                let id = save_id.clone();
                on_deleted.call(());
                spawn(async move {
                    let _ = g.client.cm_delete(&uid, &id).await;
                });
            },
        }
    }
}

/// Schema-driven form for creating or editing a single entry.
/// `document_id == NEW_ENTRY` means create; otherwise update.
#[component]
fn EntryEditView(
    schema: Schema,
    schemas: Vec<Schema>,
    document_id: String,
    is_single: bool,
    form: Signal<serde_json::Map<String, serde_json::Value>>,
    on_back: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let global = use_global();
    let mut saving = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let mut delete_confirm = use_signal(|| false);
    let mut pending_lines = use_signal(Vec::<PendingInlineLine>::new);
    let mut saved_parent_id = use_signal(|| None::<String>);

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

    let back_style =
        "background:none; border:none; color:{color::NEUTRAL_700}; cursor:pointer; font-size:16px;";
    let top_bar = format!("display:flex; align-items:center; gap:12px; padding:0 32px; height:64px; border-bottom:1px solid {}; background:{};", color::NEUTRAL_150, color::NEUTRAL_0);
    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::BETA_SIZE,
        color::NEUTRAL_900
    );
    let status_style = format!("padding:12px; border-radius:4px; background:{}; color:{}; font-size:{}; margin-bottom:16px;", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);

    let scalar: Vec<(String, FieldType, Vec<String>, core_schema::Attribute)> = schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type.is_scalar_column())
        .filter(|(_, a)| a.attr_type != FieldType::Password)
        .map(|(name, a)| (name.clone(), a.attr_type, a.enum_values.clone(), a.clone()))
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
        .map(|(name, a)| {
            (
                name.clone(),
                a.components
                    .iter()
                    .map(|u| u.as_str().to_string())
                    .collect(),
            )
        })
        .collect();
    let relation_fields: Vec<(String, core_schema::Attribute)> = schema
        .attributes
        .iter()
        .filter(|(_, a)| a.attr_type == FieldType::Relation)
        .map(|(name, a)| (name.clone(), a.clone()))
        .collect();

    // Computed fields use the same controls as ordinary scalar fields, but
    // are disabled and stripped from the save payload.
    let computed_names: Vec<String> = schema
        .attributes
        .iter()
        .filter(|(_, a)| a.computed)
        .map(|(n, _)| n.clone())
        .collect();

    let g = global.clone();
    let g2 = global.clone();
    let uid = schema.uid.as_str().to_string();
    let doc = document_id.clone();
    let save_uid = uid.clone();
    let save_computed = computed_names.clone();
    let save_doc = doc.clone();
    let pub_uid = uid.clone();
    let pub_doc = doc.clone();
    let disc_uid = uid.clone();
    let disc_doc = doc.clone();
    let g3 = global.clone();
    let g4 = global.clone();
    let unpub_uid = uid.clone();
    let unpub_doc = doc.clone();
    let g_del = global.clone();
    let del_uid = uid.clone();
    let del_doc = doc.clone();

    rsx! {
        div { class: "cm-editor",
            div { class: "cm-editor-topbar", style: "{top_bar}",
                button { class: "cm-editor-back", style: "{back_style}", onclick: move |_| on_back.call(()), "← Back" }
                span { class: "cm-editor-title", style: "{title_style}", "{title}" }
                div { class: "cm-editor-actions",
                if let Some(status) = status() {
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::SUCCESS_600};", "{status}" }
                }
                if !is_new {
                    Button { label: "Delete".to_string(), variant: "danger-light".to_string(), loading: saving(),
                        on_click: move |_| delete_confirm.set(true) }
                }
                Button { label: "Save".to_string(), variant: "primary".to_string(), loading: saving(),
                    on_click: move |_| {
                        let g = g.clone();
                        let uid = save_uid.clone();
                        let doc = saved_parent_id().unwrap_or_else(|| save_doc.clone());
                        let create_new = is_new && saved_parent_id().is_none();
                        let staged_lines = pending_lines();
                        // Computed fields are read-only and must not be sent.
                        let data = {
                            let mut m = form();
                            for n in &save_computed {
                                m.remove(n);
                            }
                            serde_json::Value::Object(m)
                        };
                        saving.set(true);
                        spawn(async move {
                            let res = if create_new {
                                g.client.cm_create(&uid, &data).await
                            } else {
                                g.client.cm_update(&uid, &doc, &data).await
                            };
                            match res {
                                Ok(response) => {
                                    let parent_id = if create_new {
                                        relation_document_id(&response.data).unwrap_or_default()
                                    } else {
                                        doc
                                    };
                                    if create_new {
                                        saved_parent_id.set(Some(parent_id.clone()));
                                    }
                                    for (relation_field, line_uid, inverse_field, mut line) in staged_lines {
                                        let pending_id = line.get("__pendingId").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
                                        if let Some(object) = line.as_object_mut() {
                                            object.remove("__pendingId");
                                            object.insert(inverse_field, serde_json::json!({"documentId": parent_id}));
                                        }
                                        match g.client.cm_create(&line_uid, &line).await {
                                            Ok(created_line) => {
                                                pending_lines.write().retain(|pending| pending.3.get("__pendingId").and_then(serde_json::Value::as_str) != Some(pending_id.as_str()));
                                                let mut current_form = form();
                                                let relation = current_form.entry(relation_field).or_insert_with(|| serde_json::Value::Array(Vec::new()));
                                                if let Some(lines) = relation.as_array_mut() {
                                                    lines.push(created_line.data);
                                                }
                                                form.set(current_form);
                                            }
                                            Err(error) => {
                                                saving.set(false);
                                                status.set(Some(format!("Invoice saved, but a line could not be saved: {error}")));
                                                return;
                                            }
                                        }
                                    }
                                    saving.set(false);
                                    status.set(Some("Saved".to_string()));
                                    on_saved.call(());
                                }
                                Err(error) => {
                                    saving.set(false);
                                    status.set(Some(format!("Error: {error}")));
                                }
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
                    Button { label: "Unpublish".to_string(), variant: "secondary".to_string(), loading: saving(),
                        on_click: move |_| {
                            let g = g4.clone();
                            let uid = unpub_uid.clone();
                            let doc = unpub_doc.clone();
                            saving.set(true);
                            spawn(async move {
                                let res = g.client.cm_unpublish(&uid, &doc).await;
                                saving.set(false);
                                match res {
                                    Ok(_) => { status.set(Some("Unpublished".to_string())); on_saved.call(()); }
                                    Err(e) => status.set(Some(format!("Error: {e}"))),
                                }
                            });
                        }
                    }
                }
                }
            }
            div { class: "cm-editor-body",
                div { class: "cm-editor-main",
                    if let Some(status) = status() {
                        div { style: "{status_style}", "{status}" }
                    }
                        div { class: "cm-form-card",
                        h2 { class: "cm-form-title", "Content" }
                        div { class: "cm-field-grid",
                        for (name, ft, enum_values, attr) in scalar.into_iter() {
                            if attr.is_visible(&form()) {
                                match ft {
                                FieldType::Boolean => rsx! {
                                    div { key: "{name}", style: "margin-bottom:16px;",
                                        Toggle {
                                            checked: form().get(&name).and_then(|v| v.as_bool()).unwrap_or(false),
                                            label: name.clone(),
                                            disabled: attr.computed,
                                            onchange: move |v| { form.write().insert(name.clone(), serde_json::Value::Bool(v)); }
                                        }
                                    }
                                },
                                FieldType::Enumeration => rsx! {
                                    Dropdown {
                                        label: name.clone(),
                                        options: enum_values.iter().map(|e| (e.clone(), e.clone())).collect(),
                                        value: input_value(form().get(&name)),
                                        disabled: attr.computed,
                                        onchange: move |v| { form.write().insert(name.clone(), serde_json::Value::String(v)); }
                                    }
                                },
                                _ => rsx! {
                                    TextField {
                                        label: name.clone(),
                                        value: input_value(form().get(&name)),
                                        disabled: attr.computed,
                                        oninput: move |v| { form.write().insert(name.clone(), serde_json::Value::String(v)); }
                                    }
                                },
                            }
                            }
                        }
                        }
                        div { class: "cm-form-section",
                            h2 { class: "cm-form-title", "Relations" }
                        for (name, attr) in relation_fields.into_iter() {
                            RelationField {
                                key: "relation-{name}",
                                name,
                                attr,
                                form,
                                schemas: schemas.clone(),
                                owner_uid: uid.clone(),
                                owner_document_id: saved_parent_id().unwrap_or_else(|| doc.clone()),
                                pending_lines,
                            }
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
                div { class: "cm-info-card",
                        h2 { class: "cm-info-title", "Information" }
                        div { class: "cm-meta-list",
                            div { class: "cm-meta-row", span { class: "cm-meta-label", "State" }, StatusIndicator { status: "draft".to_string() } }
                            div { class: "cm-meta-row", span { class: "cm-meta-label", "Document ID" }, span { class: "cm-meta-value", "{document_id}" } }
                            div { class: "cm-meta-row", span { class: "cm-meta-label", "Content type" }, span { class: "cm-meta-value", "{schema.info.display_name}" } }
                        }
                }
            }
        }
    if delete_confirm() {
        ConfirmDialog {
            title: "Delete entry".to_string(),
            message: "Are you sure you want to delete this entry? This cannot be undone.".to_string(),
            confirm_label: "Delete".to_string(),
            on_cancel: move |_| delete_confirm.set(false),
            on_confirm: move |_| {
                let g = g_del.clone();
                let uid = del_uid.clone();
                let doc = del_doc.clone();
                delete_confirm.set(false);
                saving.set(true);
                spawn(async move {
                    let _ = g.client.cm_delete(&uid, &doc).await;
                    saving.set(false);
                    on_saved.call(());
                });
            },
        }
    }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_operators() {
        let v = serde_json::json!("Hello World");
        assert!(filter_matches(&v, "eq", "Hello World"));
        assert!(!filter_matches(&v, "eq", "hello world"));
        assert!(filter_matches(&v, "neq", "Other"));
        assert!(filter_matches(&v, "contains", "hello"));
        assert!(!filter_matches(&v, "contains", "xyz"));
        // Number values stringify and compare.
        let num = serde_json::json!(42);
        assert!(filter_matches(&num, "eq", "42"));
    }

    #[test]
    fn sort_key_handles_state_and_fields() {
        let draft = serde_json::json!({"publicationState": "draft", "title": "A"});
        let published = serde_json::json!({"publicationState": "published", "title": "B"});
        assert_eq!(sort_key(&draft, "state"), "0");
        assert_eq!(sort_key(&published, "state"), "1");
        assert_eq!(sort_key(&draft, "title"), "A");
        assert_eq!(sort_key(&draft, "missing"), "");
    }

    #[test]
    fn entry_id_resolves_document_then_numeric() {
        assert_eq!(
            entry_id(&serde_json::json!({"documentId": "doc1", "id": 7})),
            "doc1"
        );
        assert_eq!(entry_id(&serde_json::json!({"id": 7})), "7");
        assert_eq!(entry_id(&serde_json::json!({})), "");
    }

    #[test]
    fn sort_key_resolves_id() {
        assert_eq!(
            sort_key(&serde_json::json!({"documentId": "abc"}), "id"),
            "abc"
        );
    }
}
