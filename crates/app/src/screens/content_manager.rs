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
    Badge, Button, Card, Checkbox, ConfirmDialog, Dropdown, EmptyState, IconButton, Modal,
    Pagination, Spinner, StatusIndicator, TextField, Toggle,
};

/// Marker document id used for a brand-new entry in the edit view.
const NEW_ENTRY: &str = "__new__";

/// Whether an entry field's string value satisfies a filter condition.
fn filter_matches(value: &serde_json::Value, op: &str, expected: &str) -> bool {
    let actual = value.to_string().trim_matches('"').to_string();
    match op {
        "neq" => actual != expected,
        "contains" => actual.to_lowercase().contains(&expected.to_lowercase()),
        _ => actual == expected, // "eq"
    }
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
    let secs = chrono::Utc::now().signed_duration_since(dt).num_seconds().max(0);
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
                        for s in schemas_vec
                            .iter()
                            .filter(|s| s.kind == ContentTypeKind::CollectionType)
                        {
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
                                    let total = resp
                                        .meta
                                        .pagination
                                        .as_ref()
                                        .map(|p| p.total)
                                        .unwrap_or(0);
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
    for s in all
        .iter()
        .filter(|s| {
            s.kind == ContentTypeKind::CollectionType || s.kind == ContentTypeKind::SingleType
        })
    {
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
            tr { style: "border-bottom:1px solid {color::NEUTRAL_150}; cursor:pointer;",
                onclick: move |_| open_tr.set(target_tr.clone()),
                td { style: "padding:12px 16px;",
                    button { style: "background:none; border:none; color:{color::PRIMARY_600}; font-weight:600; cursor:pointer; font-size:14px; text-align:left;",
                        onclick: move |_| open_name.set(target_name.clone()),
                        "{name}"
                    }
                }
                td { style: "padding:12px 16px;", {type_badge_cm(kind)} }
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_700};", "{cnt}" }
                td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_600};", "{updated_display}" }
                td { style: "padding:12px 16px;",
                    if draft_pub {
                        Badge { text: "Draft & publish".to_string(), kind: "modified".to_string() }
                    } else {
                        Badge { text: "Standard".to_string(), kind: "neutral".to_string() }
                    }
                }
                td { style: "padding:12px 16px;",
                    Button { label: "Open".to_string(), size: "sm".to_string(), on_click: move |_| open_act.set(target_act.clone()) }
                }
            }
        });
    }

    let count = all.len();
    rsx! {
        div { style: "padding:32px; max-width:1200px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:24px;",
                div { style: "display:flex; flex-direction:column; gap:4px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "Content Manager" }
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Create, read, update and delete your content." }
                }
            }

            div { style: "display:flex; gap:12px; margin-bottom:16px; align-items:center; flex-wrap:wrap;",
                div { style: "flex:1; max-width:360px;",
                    TextField {
                        value: search(),
                        placeholder: "Search content types".to_string(),
                        oninput: move |v| search.set(v),
                    }
                }
                div { style: "display:flex; gap:4px; flex-wrap:wrap;",
                    CmTypeChip { label: "All".to_string(), active: filter() == "all", on_click: move |_| filter.set("all".into()) }
                    CmTypeChip { label: "Collection Types".to_string(), active: filter() == "collection", on_click: move |_| filter.set("collection".into()) }
                    CmTypeChip { label: "Single Types".to_string(), active: filter() == "single", on_click: move |_| filter.set("single".into()) }
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
                Card {
                    header: format!("{count} content types"),
                    div { style: "overflow-x:auto;",
                        table { style: "width:100%; border-collapse:collapse;",
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
    let mut configuring = use_signal(|| false);
    let mut pending_delete = use_signal(|| None::<String>);
    let mut load_req = use_signal(|| 1u32);
    let mut route = global.route;

    // Load content types (to resolve the schema + breadcrumbs).
    let g_schema = global.clone();
    use_effect(move || {
        if !schema_loaded() {
            schema_loaded.set(true);
            let mut g = g_schema.clone();
            let mut sc = schemas;
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
                    }
                    Err(e) => status.set(Some(format!("Failed to load: {e}"))),
                }
            });
        }
    });

    // Load entries when requested / pagination changes.
    let g_entries = global.clone();
    let uid_load = uid.clone();
    use_effect({
        let client = client.clone();
        move || {
            if load_req() > 0 {
                load_req.set(0);
                let uid = uid_load.clone();
                let page = page();
                let page_size = page_size();
                let g = g_entries.clone();
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
                            total.set(
                                resp.meta
                                    .pagination
                                    .as_ref()
                                    .map(|p| p.total)
                                    .unwrap_or(0),
                            );
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
    let columns = schema.as_ref().map(entry_columns).unwrap_or_default();
    let header_name = schema
        .as_ref()
        .map(|s| s.info.display_name.clone())
        .unwrap_or_else(|| uid.clone());

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
            tr { style: "border-bottom:1px solid {color::NEUTRAL_150}; cursor:pointer;",
                onclick: move |_| open_entry.set(Route::ContentManagerEntry { uid: uid_open.clone(), document_id: rid.clone() }),
                td { style: "padding:12px 16px;",
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
                td { style: "padding:12px 16px;",
                    div { style: "display:flex; gap:4px;",
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
    let uid_bulk_delete = uid.clone();

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
            header_cells.push(rsx! {
                th { style: "text-align:left; padding:10px 16px; font-size:{typography::LABEL_SIZE}; font-weight:600; color:{color::NEUTRAL_600}; cursor:pointer;",
                    onclick: move |_| {
                        if sf() == k { sa.set(!sa()); } else { sf.set(k.clone()); sa.set(true); }
                        pg.set(1);
                    },
                    "{l}"
                }
            });
        }
    }

    rsx! {
        div { style: "padding:32px; max-width:1200px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:24px; gap:12px;",
                div { style: "display:flex; align-items:center; gap:12px;",
                    Button { label: "← Back to Content Types".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::ContentManager) }
                    div { style: "display:flex; flex-direction:column; gap:2px;",
                        span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "{header_name}" }
                        span { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "Collection type · {total} entries" }
                    }
                }
                Button { label: "Create entry".to_string(), on_click: move |_| route.set(Route::ContentManagerEntry { uid: uid_create.clone(), document_id: NEW_ENTRY.to_string() }) }
            }

            div { style: "display:flex; gap:12px; margin-bottom:16px; align-items:center;",
                div { style: "flex:1; max-width:320px;",
                    TextField { value: search(), label: String::new(), placeholder: "Search entries".to_string(), oninput: move |v| search.set(v) }
                }
                Button { label: "Filters".to_string(), variant: "secondary".to_string(), on_click: move |_| filter_open.set(true) }
                Button { label: "Configure the view".to_string(), variant: "secondary".to_string(), on_click: move |_| configuring.set(true) }
            }

            if !filters().is_empty() {
                div { style: "display:flex; flex-wrap:wrap; gap:8px; padding-bottom:8px;",
                    for (idx, (f, op, v)) in filters().into_iter().enumerate() {
                        div { style: "display:flex; align-items:center; gap:6px; padding:4px 10px; border-radius:999px; background:{color::PRIMARY_100}; color:{color::PRIMARY_700}; font-size:{typography::PI_SIZE};",
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

            if filtered.is_empty() {
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
                Card { padding: 0,
                    div { style: "overflow-x:auto;",
                        table { style: "width:100%; border-collapse:collapse; background:#fff;",
                            thead {
                                tr { style: "border-bottom:1px solid {color::NEUTRAL_150};",
                                    th { style: "padding:10px 16px; width:40px;",
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
                                    th { style: "text-align:left; padding:10px 16px; font-size:{typography::LABEL_SIZE}; font-weight:600; color:{color::NEUTRAL_600};", "Actions" }
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
                    on_page_change: move |p| { if p >= 1 { page.set(p); load_req.set(load_req() + 1); } },
                    on_page_size_change: move |ps| { page_size.set(ps); page.set(1); load_req.set(load_req() + 1); },
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
                        filters.set(fs);
                        page.set(1);
                        filter_open.set(false);
                    },
                    on_close: move |_| filter_open.set(false),
                }
            }
        }

        if configuring() {
            if let Some(schema) = &schema {
                ConfigureViewModal {
                    uid: schema.uid.as_str().to_string(),
                    on_close: move |_| configuring.set(false),
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
        .map(|v| v.to_string())
        .unwrap_or_default();
    rsx! {
        td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_800};", "{value}" }
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

#[component]
fn CmTableTh(label: String) -> Element {
    rsx! {
        th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "{label}" }
    }
}

#[component]
fn CmTypeChip(label: String, active: bool, on_click: EventHandler<MouseEvent>) -> Element {
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

/// Content Manager list-view configuration modal (design doc §6.5).
/// Loads the current configuration and lets the user choose which columns to
/// display and the page size, then persists via PUT.
#[component]
fn ConfigureViewModal(uid: String, on_close: EventHandler<()>) -> Element {
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
                        if let Ok(c) = serde_json::from_value(
                            v.get("data").cloned().unwrap_or(serde_json::Value::Null),
                        ) {
                            config.set(Some(c));
                        }
                    }
                    Err(e) => status.set(Some(format!("Failed to load config: {e}"))),
                }
            });
        }
    });

    let label_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_700
    );
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
    let mut delete_confirm = use_signal(|| false);

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
    let g4 = global.clone();
    let unpub_uid = uid.clone();
    let unpub_doc = doc.clone();
    let g_del = global.clone();
    let del_uid = uid.clone();
    let del_doc = doc.clone();

    rsx! {
        div { style: "flex:1; min-width:0;",
            div { style: "{top_bar}",
                button { style: "{back_style}", onclick: move |_| on_back.call(()), "←" }
                span { style: "{title_style}", "{title}" }
                div { style: "flex:1;" }
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
            div { style: "display:flex; gap:32px; padding:32px;",
                div { style: "flex:1; max-width:900px;",
                    if let Some(status) = status() {
                        div { style: "{status_style}", "{status}" }
                    }
                    Card { padding: 24,
                        for (name, ft, enum_values, attr) in scalar.into_iter() {
                            if attr.is_visible(&form()) {
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
                            div { style: "display:flex; justify-content:space-between; align-items:center;", span { "State" }, StatusIndicator { status: "draft".to_string() } }
                            div { style: "display:flex; justify-content:space-between;", span { "Document ID" }, span { "{document_id}" } }
                            div { style: "display:flex; justify-content:space-between;", span { "Content type" }, span { "{schema.info.display_name}" } }
                        }
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
        assert_eq!(entry_id(&serde_json::json!({"documentId": "doc1", "id": 7})), "doc1");
        assert_eq!(entry_id(&serde_json::json!({"id": 7})), "7");
        assert_eq!(entry_id(&serde_json::json!({})), "");
    }

    #[test]
    fn sort_key_resolves_id() {
        assert_eq!(sort_key(&serde_json::json!({"documentId": "abc"}), "id"), "abc");
    }
}
