//! Content-Type Builder screen (design doc §5).
//!
//! Implements the Strapi CTB workflow: a secondary nav grouped by collection
//! types / single types / components, a field picker covering the official
//! field set, type-aware field configuration, and a batch Save.

use core_domain::{ContentTypeKind, FieldType};
use core_schema::{
    api_uid, Attribute, I18nOptions, Schema, SchemaInfo, SchemaOptions, SchemaPluginOptions,
};
use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{use_global, Route};
use crate::components::{
    Badge, Button, Card, ConfirmDialog, Dropdown, EmptyState, Icon, IconButton, Modal, Spinner,
    TextArea, TextField, Toggle,
};

#[derive(Clone, PartialEq)]
enum ModalKind {
    None,
    CreateType,
    FieldPicker {
        ct_uid: String,
    },
    FieldConfig {
        ct_uid: String,
        field_type: FieldType,
    },
    Metadata {
        ct_uid: String,
    },
}

/// The official Strapi field picker set, in the exact order Strapi shows them.
/// Each entry maps a picker label + description to the underlying FieldType.
const PICKABLE_FIELDS: &[(FieldType, &str, &str)] = &[
    (
        FieldType::String,
        "Text",
        "Small or long text like title or description",
    ),
    (
        FieldType::Blocks,
        "Rich text (Blocks)",
        "The new JSON-based rich text editor",
    ),
    (
        FieldType::Integer,
        "Number",
        "Numbers (integer, float, decimal)",
    ),
    (
        FieldType::Datetime,
        "Date",
        "A date picker with hours, minutes and seconds",
    ),
    (
        FieldType::Boolean,
        "Boolean",
        "Yes or no, 1 or 0, true or false",
    ),
    (
        FieldType::Relation,
        "Relation",
        "Refers to a Collection Type",
    ),
    (
        FieldType::Email,
        "Email",
        "Email field with validations format",
    ),
    (
        FieldType::Password,
        "Password",
        "Password field with encryption",
    ),
    (
        FieldType::Enumeration,
        "Enumeration",
        "List of values, then pick one",
    ),
    (FieldType::Media, "Media", "Files like images, videos, etc"),
    (FieldType::Json, "JSON", "Data in JSON format"),
    (
        FieldType::Component,
        "Component",
        "A group of fields that you can repeat or reuse",
    ),
    (
        FieldType::Dynamiczone,
        "Dynamic Zone",
        "Dynamically pick components while editing content",
    ),
    (
        FieldType::Richtext,
        "Rich text (Markdown)",
        "The classic rich text editor",
    ),
    (FieldType::Uid, "UID", "Unique identifier"),
];

/// A user-triggered async action for the Content-Type Builder listing.
#[derive(Clone)]
enum CtbAction {
    Create(Schema),
    Duplicate(String),
    Delete(String),
}

/// Content-Type Builder landing page — a table-first listing of every content
/// type (collection / single / component). Browsing happens here, matching the
/// Workflows listing pattern, instead of through a persistent sidebar.
#[component]
pub fn ContentTypeBuilder() -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut working: Signal<Vec<Schema>> = use_signal(|| vec![]);
    let mut loaded = use_signal(|| false);
    let mut loading = use_signal(|| true);
    let mut search = use_signal(String::new);
    let mut filter = use_signal(|| "all".to_string());
    let mut show_create = use_signal(|| false);
    let mut to_delete: Signal<Option<String>> = use_signal(|| None);
    let mut action: Signal<Option<CtbAction>> = use_signal(|| None);
    let mut route = global.route;
    let mut status = use_signal(|| None::<String>);

    // Load the schema list once and register content-type names for breadcrumbs.
    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let mut g = g_load.clone();
            spawn(async move {
                match g.client.ctb_list().await {
                    Ok(v) => {
                        let schemas: Vec<Schema> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let names: Vec<(String, String)> = schemas
                            .iter()
                            .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                            .collect();
                        g.ct_names.set(names);
                        working.set(schemas);
                    }
                    Err(e) => status.set(Some(format!("Failed to load: {e}"))),
                }
                loading.set(false);
            });
        }
    });

    // Dispatcher for create / duplicate / delete (spawn must run from an effect).
    let g_disp = global.clone();
    use_effect({
        let client = client.clone();
        let mut g = g_disp.clone();
        move || {
            // Read the signal reactively before taking it, so this effect
            // re-runs when the action is set (Signal::take alone is a write and
            // does not subscribe the effect).
            let a = if action().is_some() {
                action.take()
            } else {
                None
            };
            match a {
                Some(CtbAction::Create(schema)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut w = working;
                    let mut show = show_create;
                    let mut route2 = route;
                    let new_uid = schema.uid.as_str().to_string();
                    spawn(async move {
                        w.write().push(schema);
                        match client.ctb_apply(w(), Vec::new()).await {
                            Ok(_) => {
                                let names: Vec<(String, String)> = w()
                                    .iter()
                                    .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                                    .collect();
                                g.ct_names.set(names);
                                g.toast("Content type created", "success");
                                route2.set(Route::ContentTypeBuilderEditor(new_uid));
                            }
                            Err(e) => g.toast(format!("Create failed: {e}"), "danger"),
                        }
                        show.set(false);
                    });
                }
                Some(CtbAction::Duplicate(uid)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut w = working;
                    spawn(async move {
                        let copy = w().iter().find(|s| s.uid.as_str() == uid).map(duplicate_schema);
                        if let Some(ns) = copy {
                            w.write().push(ns);
                        }
                        match client.ctb_apply(w(), Vec::new()).await {
                            Ok(_) => {
                                let names: Vec<(String, String)> = w()
                                    .iter()
                                    .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                                    .collect();
                                g.ct_names.set(names);
                                g.toast("Content type duplicated", "success");
                            }
                            Err(e) => g.toast(format!("Duplicate failed: {e}"), "danger"),
                        }
                    });
                }
                Some(CtbAction::Delete(uid)) => {
                    let client = client.clone();
                    let mut g = g.clone();
                    let mut w = working;
                    let mut del = to_delete;
                    spawn(async move {
                        w.write().retain(|s| s.uid.as_str() != uid);
                        match client.ctb_apply(w(), vec![uid.to_string()]).await {
                            Ok(_) => {
                                let names: Vec<(String, String)> = w()
                                    .iter()
                                    .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                                    .collect();
                                g.ct_names.set(names);
                                g.toast("Content type deleted", "success");
                            }
                            Err(e) => g.toast(format!("Delete failed: {e}"), "danger"),
                        }
                        del.set(None);
                    });
                }
                None => {}
            }
        }
    });

    // Precompute table rows under the active tab + search.
    let schemas = working();
    let filter_kind = filter();
    let query = search().trim().to_lowercase();
    let mut rows: Vec<Element> = Vec::new();
    for s in schemas.iter() {
        let matches_filter = match filter_kind.as_str() {
            "collection" => s.kind == ContentTypeKind::CollectionType,
            "single" => s.kind == ContentTypeKind::SingleType,
            "component" => s.kind == ContentTypeKind::Component,
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
        let field_count = s.attributes.len();
        let labels: Vec<String> = s
            .metadata
            .as_ref()
            .map(|m| m.labels.iter().map(|(k, v)| format!("{k}={v}")).collect())
            .unwrap_or_default();
        let namespace = s.metadata.as_ref().and_then(|m| m.namespace.clone());
        let mut open_tr = route;
        let mut open_name = route;
        let mut open_edit = route;
        let mut act = action;
        let mut del = to_delete;
        let name_uid_tr = uid.clone();
        let name_uid_name = uid.clone();
        let edit_uid = uid.clone();
        let dup_uid = uid.clone();
        let del_uid = uid.clone();
        rows.push(rsx! {
            tr { style: "border-bottom:1px solid {color::NEUTRAL_150}; cursor:pointer;",
                onclick: move |_| open_tr.set(Route::ContentTypeBuilderEditor(name_uid_tr.clone())),
                td { style: "padding:12px 16px;",
                    button { style: "background:none; border:none; color:{color::PRIMARY_600}; font-weight:600; cursor:pointer; font-size:14px; text-align:left;",
                        onclick: move |_| open_name.set(Route::ContentTypeBuilderEditor(name_uid_name.clone())),
                        "{name}"
                    }
                }
                td { style: "padding:12px 16px;",
                    {type_badge(kind)}
                }
                td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_600};", "{uid}" }
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_700};", "{field_count}" }
                td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_500};",
                    if let Some(ns) = &namespace {
                        span { style: "font-size:12px; font-weight:600; color:{color::PRIMARY_600}; margin-right:6px;", "{ns}/" }
                    }
                    for lb in labels.iter() {
                        Badge { text: lb.clone(), kind: "neutral".to_string() }
                    }
                }
                td { style: "padding:12px 16px;",
                    div { style: "display:flex; gap:4px;",
                        IconButton { name: "pencil".to_string(), aria_label: "Edit".to_string(),
                            on_click: move |e: MouseEvent| { e.stop_propagation(); open_edit.set(Route::ContentTypeBuilderEditor(edit_uid.clone())); } }
                        IconButton { name: "refresh".to_string(), aria_label: "Duplicate".to_string(),
                            on_click: move |e: MouseEvent| { e.stop_propagation(); act.set(Some(CtbAction::Duplicate(dup_uid.clone()))); } }
                        IconButton { name: "trash".to_string(), variant: "danger".to_string(), aria_label: "Delete".to_string(),
                            on_click: move |e: MouseEvent| { e.stop_propagation(); del.set(Some(del_uid.clone())); } }
                    }
                }
            }
        });
    }

    let count = schemas.len();
    rsx! {
        div { style: "padding:32px; max-width:1200px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:24px;",
                div { style: "display:flex; flex-direction:column; gap:4px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "Content-Type Builder" }
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Define and manage the structure of your content." }
                }
                Button { label: "Create content type".to_string(), on_click: move |_| show_create.set(true) }
            }

            // Toolbar: search + type tabs
            div { style: "display:flex; gap:12px; margin-bottom:16px; align-items:center; flex-wrap:wrap;",
                div { style: "flex:1; max-width:360px;",
                    TextField {
                        value: search(),
                        placeholder: "Search content types".to_string(),
                        oninput: move |v| search.set(v),
                    }
                }
                div { style: "display:flex; gap:4px; flex-wrap:wrap;",
                    TypeChip { label: "All".to_string(), active: filter() == "all", on_click: move |_| filter.set("all".into()) }
                    TypeChip { label: "Collection Types".to_string(), active: filter() == "collection", on_click: move |_| filter.set("collection".into()) }
                    TypeChip { label: "Single Types".to_string(), active: filter() == "single", on_click: move |_| filter.set("single".into()) }
                    TypeChip { label: "Components".to_string(), active: filter() == "component", on_click: move |_| filter.set("component".into()) }
                }
            }

            if let Some(status) = status() {
                div { style: "padding:12px; margin-bottom:16px; border-radius:4px; background:{color::WARNING_100}; color:{color::WARNING_700}; font-size:{typography::BODY_SIZE};", "{status}" }
            }

            if loading() {
                div { style: "display:flex; justify-content:center; padding:48px;", Spinner { size: 28 } }
            } else if count == 0 {
                EmptyState {
                    title: "No content types yet".to_string(),
                    subtitle: "Create your first collection or single type to start building your content structure.".to_string(),
                    icon: "grid".to_string(),
                    Button { label: "Create content type".to_string(), on_click: move |_| show_create.set(true) }
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
                                    TableTh { label: "Name".to_string() }
                                    TableTh { label: "Type".to_string() }
                                    TableTh { label: "API ID / UID".to_string() }
                                    TableTh { label: "Fields".to_string() }
                                    TableTh { label: "Updated".to_string() }
                                    TableTh { label: "Actions".to_string() }
                                }
                            }
                            tbody { {rows.into_iter()} }
                        }
                    }
                }
            }
        }

        // Create content type modal
        if show_create() {
            CreateTypeModal {
                on_close: move |_| show_create.set(false),
                on_create: move |schema| action.set(Some(CtbAction::Create(schema))),
            }
        }

        // Delete confirm dialog
        if let Some(uid) = to_delete() {
            ConfirmDialog {
                title: "Delete content type".to_string(),
                message: "This will permanently delete the content type and its data. This cannot be undone.".to_string(),
                confirm_label: "Delete".to_string(),
                on_cancel: move |_| to_delete.set(None),
                on_confirm: move |_| action.set(Some(CtbAction::Delete(uid.clone()))),
            }
        }
    }
}

/// Render a badge for a content type's kind.
fn type_badge(kind: ContentTypeKind) -> Element {
    match kind {
        ContentTypeKind::CollectionType => rsx! {
            Badge { text: "Collection".to_string(), kind: "published".to_string() }
        },
        ContentTypeKind::SingleType => rsx! {
            Badge { text: "Single".to_string(), kind: "neutral".to_string() }
        },
        ContentTypeKind::Component => rsx! {
            Badge { text: "Component".to_string(), kind: "modified".to_string() }
        },
    }
}

/// Build a deep copy of a schema under a new uid ("<name>-copy").
fn duplicate_schema(orig: &Schema) -> Schema {
    let mut s = orig.clone();
    let base = orig.info.singular_name.clone();
    let new_singular = if base.is_empty() {
        "copy".to_string()
    } else {
        format!("{base}-copy")
    };
    let plural = {
        use cruet::Inflector;
        new_singular.to_plural()
    };
    s.info.singular_name = new_singular.clone();
    s.info.plural_name = plural;
    s.info.display_name = format!("{} copy", orig.info.display_name);
    s.uid = api_uid(&new_singular);
    s.collection_name = None;
    s
}

/// The Content-Type Builder editor for a single content type. Browsing/selecting
/// happens on the listing page; this screen edits fields for one type only.
#[component]
pub fn ContentTypeBuilderEditor(uid: String) -> Element {
    let global = use_global();
    let mut working: Signal<Vec<Schema>> = use_signal(|| vec![]);
    let mut loaded = use_signal(|| false);
    let mut modal = use_signal(|| ModalKind::None);
    let mut is_dirty = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let mut route = global.route;

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let mut g = g_load.clone();
            spawn(async move {
                match g.client.ctb_list().await {
                    Ok(v) => {
                        let schemas: Vec<Schema> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let names: Vec<(String, String)> = schemas
                            .iter()
                            .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
                            .collect();
                        g.ct_names.set(names);
                        working.set(schemas);
                    }
                    Err(e) => status.set(Some(format!("Failed to load: {e}"))),
                }
            });
        }
    });

    let schemas = working();
    let selected = schemas
        .iter()
        .find(|s| s.uid.as_str() == uid)
        .cloned();
    let target_types: Vec<String> = schemas
        .iter()
        .filter(|s| s.kind == ContentTypeKind::CollectionType)
        .map(|s| s.uid.as_str().to_string())
        .collect();
    let component_types: Vec<(String, String)> = schemas
        .iter()
        .filter(|s| s.kind == ContentTypeKind::Component)
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();
    let sibling_fields: Vec<String> = selected
        .as_ref()
        .map(|s| s.attributes.keys().cloned().collect())
        .unwrap_or_default();
    let selected_attrs: Vec<(String, String, FieldType, bool)> = selected
        .as_ref()
        .map(|s| {
            s.attributes
                .iter()
                .map(|(n, a)| (s.uid.as_str().to_string(), n.clone(), a.attr_type, a.required))
                .collect()
        })
        .unwrap_or_default();
    let selected_display = selected.as_ref().map(|s| s.info.display_name.clone());
    let selected_uid_str = selected.as_ref().map(|s| s.uid.as_str().to_string()).unwrap_or_default();
    let uid_for_picker = uid.clone();

    let page_title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let editor_title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::BETA_SIZE,
        color::NEUTRAL_900
    );
    let field_name_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::BODY_BOLD_SIZE,
        color::NEUTRAL_800
    );
    let field_type_style = format!(
        "font-size:{}; color:{};",
        typography::PI_SIZE,
        color::NEUTRAL_500
    );
    let add_field_style = format!(
        "width:100%; padding:16px; border:1px dashed {}; border-radius:6px; background:transparent; cursor:pointer; display:flex; align-items:center; justify-content:center; gap:8px;",
        color::PRIMARY_400
    );
    let picker_uid = uid_for_picker.clone();

    let g_save = global.clone();
    let toast_signal = global.clone();

    rsx! {
        div { style: "padding:32px; max-width:1200px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:24px; gap:12px;",
                div { style: "display:flex; align-items:center; gap:12px;",
                    Button { label: "← Back to Content Types".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::ContentTypeBuilder) }
                    div { style: "display:flex; flex-direction:column; gap:2px;",
                        span { style: "{editor_title_style}",
                            if let Some(name) = &selected_display { "{name}" } else { "Content-Type Builder" }
                        }
                        span { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "{selected_uid_str}" }
                    }
                }
                div { style: "display:flex; align-items:center; gap:12px;",
                    if let Some(status) = status() {
                        span { style: "font-size:{typography::PI_SIZE}; color:{color::SUCCESS_600};", "{status}" }
                    }
                    Button { label: "Metadata".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| modal.set(ModalKind::Metadata { ct_uid: selected_uid_str.clone() }) }
                    Button {
                        label: "Save".to_string(), variant: "success".to_string(), disabled: !is_dirty(), loading: saving(),
                        on_click: move |_| {
                            let g = g_save.clone();
                            let toast = toast_signal.clone();
                            let schemas = working();
                            saving.set(true);
                            spawn(async move {
                                let mut toast = toast;
                                match g.client.ctb_apply(schemas, Vec::new()).await {
                                    Ok(_) => { is_dirty.set(false); status.set(Some("Saved".to_string())); toast.toast("Schema saved".to_string(), "success"); }
                                    Err(e) => { status.set(Some(format!("Error: {e}"))); toast.toast(format!("Save failed: {e}"), "danger"); }
                                }
                                saving.set(false);
                            });
                        },
                    }
                }
            }

            if let Some(display) = &selected_display {
                div { style: "display:flex; flex-direction:column; gap:16px;",
                    div { style: "display:flex; align-items:center; justify-content:space-between;",
                        span { style: "{page_title_style}", "{display}" }
                        Button { label: "+ Add another field".to_string(), variant: "secondary".to_string(),
                            on_click: move |_| modal.set(ModalKind::FieldPicker { ct_uid: uid_for_picker.clone() })
                        }
                    }
                    Card { padding: 24,
                        if selected_attrs.is_empty() {
                            EmptyState {
                                title: "No fields yet".to_string(),
                                subtitle: "This content type has no fields yet. Add your first field.".to_string(),
                                icon: "grid".to_string(),
                            }
                        } else {
                            div { style: "display:flex; flex-direction:column;",
                                for (row_uid, name, ft, req) in selected_attrs.into_iter() {
                                    div { style: "display:flex; align-items:center; gap:12px; padding:10px 4px; border-bottom:1px solid {color::NEUTRAL_150};",
                                        Icon { name: icon_for(ft), size: 18 }
                                        div { style: "display:flex; flex-direction:column; flex:1;",
                                            span { style: "{field_name_style}", "{name}" }
                                            span { style: "{field_type_style}", "{ft.as_str()}" }
                                        }
                                        if req {
                                            Badge { text: "required".to_string(), kind: "new".to_string() }
                                        }
                                        IconButton { name: "pencil".to_string(), aria_label: "Edit field".to_string(),
                                            on_click: move |_| modal.set(ModalKind::FieldConfig { ct_uid: row_uid.clone(), field_type: ft }) }
                                    }
                                }
                            }
                        }
                    }
                    button {
                        style: "{add_field_style}",
                        onclick: move |_| modal.set(ModalKind::FieldPicker { ct_uid: picker_uid.clone() }),
                        Icon { name: "plus".to_string(), size: 16, color: color::PRIMARY_600.to_string() }
                        span { style: "font-size:{typography::BODY_SIZE}; font-weight:600; color:{color::PRIMARY_600};", "+ Add another field" }
                    }
                }
            } else if loaded() {
                EmptyState {
                    title: "Content type not found".to_string(),
                    subtitle: "This content type may have been deleted.".to_string(),
                    icon: "grid".to_string(),
                    Button { label: "← Back to Content Types".to_string(), variant: "secondary".to_string(), on_click: move |_| route.set(Route::ContentTypeBuilder) }
                }
            } else {
                div { style: "display:flex; justify-content:center; padding:48px;", Spinner { size: 28 } }
            }
        }

        if modal() == ModalKind::CreateType {
            CreateTypeModal {
                on_close: move |_| modal.set(ModalKind::None),
                on_create: move |schema| {
                    working.write().push(schema);
                    is_dirty.set(true);
                    modal.set(ModalKind::None);
                },
            }
        }
        if let ModalKind::FieldPicker { ct_uid } = modal() {
            FieldPickerModal {
                on_close: move |_| modal.set(ModalKind::None),
                on_pick: move |ft| modal.set(ModalKind::FieldConfig { ct_uid: ct_uid.clone(), field_type: ft }),
            }
        }
        if let ModalKind::FieldConfig { ct_uid, field_type } = modal() {
            FieldConfigModal {
                field_type,
                target_types: target_types.clone(),
                component_types: component_types.clone(),
                sibling_fields: sibling_fields.clone(),
                on_close: move |_| modal.set(ModalKind::None),
                on_save: move |(name, attr): (String, Attribute)| {
                    if let Some(schema) = working.write().iter_mut().find(|s| s.uid.as_str() == ct_uid) {
                        schema.attributes.insert(name, attr);
                    }
                    is_dirty.set(true);
                    modal.set(ModalKind::None);
                },
            }
        }
        if let ModalKind::Metadata { ct_uid } = modal() {
            {
                let initial = selected.as_ref().and_then(|s| s.metadata.clone()).unwrap_or_default();
                rsx! {
                    MetadataEditorModal {
                        initial,
                        on_close: move |_| modal.set(ModalKind::None),
                        on_save: move |md: core_domain::Metadata| {
                            let uid = ct_uid.clone();
                            if let Some(schema) = working.write().iter_mut().find(|s| s.uid.as_str() == uid) {
                                schema.metadata = Some(md);
                            }
                            is_dirty.set(true);
                            modal.set(ModalKind::None);
                        },
                    }
                }
            }
        }
    }
}

/// Kubernetes-style metadata editor (namespace + labels + annotations).
#[component]
fn MetadataEditorModal(
    initial: core_domain::Metadata,
    on_close: EventHandler<MouseEvent>,
    on_save: EventHandler<core_domain::Metadata>,
) -> Element {
    let mut namespace = use_signal(|| initial.namespace.clone().unwrap_or_default());
    let mut labels: Signal<Vec<(String, String)>> =
        use_signal(|| initial.labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    let mut annotations: Signal<Vec<(String, String)>> =
        use_signal(|| initial.annotations.iter().map(|(k, v)| (k.clone(), v.clone())).collect());

    let row_style = |_| format!("display:flex; gap:8px; margin-top:6px; align-items:center;");
    let input_style = format!("flex:1; padding:8px; border:1px solid {}; border-radius:6px; font-size:13px;", color::NEUTRAL_200);

    rsx! {
        Modal { title: "Content type metadata".to_string(), width: 600, on_close: move |e| on_close.call(e),
            div { style: "display:flex; flex-direction:column; gap:18px;",
                TextField { value: namespace(), label: "Namespace".to_string(), placeholder: "e.g. marketing (used for grouping)".to_string(), oninput: move |v| namespace.set(v) }

                div {
                    span { style: "font-size:13px; font-weight:600; color:{color::NEUTRAL_700};", "Labels (group content types + workflows)" }
                    for (i, (k, v)) in labels().into_iter().enumerate() {
                        div { key: "lbl-{i}", style: "{row_style(())}",
                            input { value: k, placeholder: "key".to_string(), style: "{input_style}", oninput: move |e| { let mut l = labels(); if i < l.len() { l[i].0 = e.value(); } labels.set(l); } }
                            input { value: v, placeholder: "value".to_string(), style: "{input_style}", oninput: move |e| { let mut l = labels(); if i < l.len() { l[i].1 = e.value(); } labels.set(l); } }
                            Button { label: "×".to_string(), variant: "danger".to_string(), size: "sm".to_string(), on_click: move |_| { let mut l = labels(); if i < l.len() { l.remove(i); } labels.set(l); } }
                        }
                    }
                    Button { label: "+ Add label".to_string(), size: "sm".to_string(), on_click: move |_| { let mut l = labels(); l.push((String::new(), String::new())); labels.set(l); } }
                }

                div {
                    span { style: "font-size:13px; font-weight:600; color:{color::NEUTRAL_700};", "Annotations" }
                    for (i, (k, v)) in annotations().into_iter().enumerate() {
                        div { key: "ann-{i}", style: "{row_style(())}",
                            input { value: k, placeholder: "key".to_string(), style: "{input_style}", oninput: move |e| { let mut a = annotations(); if i < a.len() { a[i].0 = e.value(); } annotations.set(a); } }
                            input { value: v, placeholder: "value".to_string(), style: "{input_style}", oninput: move |e| { let mut a = annotations(); if i < a.len() { a[i].1 = e.value(); } annotations.set(a); } }
                            Button { label: "×".to_string(), variant: "danger".to_string(), size: "sm".to_string(), on_click: move |_| { let mut a = annotations(); if i < a.len() { a.remove(i); } annotations.set(a); } }
                        }
                    }
                    Button { label: "+ Add annotation".to_string(), size: "sm".to_string(), on_click: move |_| { let mut a = annotations(); a.push((String::new(), String::new())); annotations.set(a); } }
                }
            }
            div { style: "display:flex; justify-content:flex-end; gap:12px; margin-top:20px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |e| on_close.call(e) }
                Button { label: "Save metadata".to_string(), on_click: move |_| {
                    let mut md = core_domain::Metadata::default();
                    let ns = namespace();
                    if !ns.trim().is_empty() {
                        md.namespace = Some(ns.trim().to_string());
                    }
                    md.labels = labels().into_iter().filter(|(k, _)| !k.trim().is_empty()).map(|(k, v)| (k.trim().to_string(), v)).collect();
                    md.annotations = annotations().into_iter().filter(|(k, _)| !k.trim().is_empty()).map(|(k, v)| (k.trim().to_string(), v)).collect();
                    on_save.call(md);
                } }
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
fn TypeChip(label: String, active: bool, on_click: EventHandler<MouseEvent>) -> Element {
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

#[component]
fn CreateTypeModal(on_close: EventHandler<MouseEvent>, on_create: EventHandler<Schema>) -> Element {
    let mut display = use_signal(String::new);
    let mut singular = use_signal(String::new);
    let mut plural = use_signal(String::new);
    // Tracks whether the user has hand-edited the API ID fields, so they aren't
    // clobbered when the display name changes.
    let mut singular_manual = use_signal(|| false);
    let mut plural_manual = use_signal(|| false);
    let mut kind = use_signal(|| "collection".to_string());
    let mut draft = use_signal(|| true);
    let mut i18n = use_signal(|| false);

    // As the display name is typed, auto-populate the API IDs from it (unless
    // the user has manually overridden them).
    let on_display = move |v: String| {
        let (singular_id, plural_id) = api_ids_from_display(&v);
        display.set(v);
        if !singular_manual() {
            singular.set(singular_id);
        }
        if !plural_manual() {
            plural.set(plural_id);
        }
    };

    let is_collection = kind() == "collection";
    let title = if is_collection {
        "Create a collection type".to_string()
    } else {
        "Create a single type".to_string()
    };

    let build = move || {
        let singular = if singular().is_empty() {
            kebab_id(&display())
        } else {
            singular()
        };
        let plural = if plural().is_empty() {
            format!("{singular}s")
        } else {
            plural()
        };
        let resolved_kind = if is_collection {
            ContentTypeKind::CollectionType
        } else {
            ContentTypeKind::SingleType
        };
        Schema {
            uid: api_uid(&singular),
            kind: resolved_kind,
            collection_name: None,
            info: SchemaInfo {
                singular_name: singular.clone(),
                plural_name: plural.clone(),
                display_name: if display().is_empty() {
                    singular.clone()
                } else {
                    display()
                },
                description: None,
                icon: None,
            },
            options: SchemaOptions {
                draft_and_publish: draft(),
                comment: None,
            },
            plugin_options: if i18n() {
                Some(SchemaPluginOptions {
                    i18n: Some(I18nOptions { localized: true }),
                })
            } else {
                None
            },
            attributes: Default::default(),
        metadata: None,
        }
    };

    let seg_active = if is_collection {
        color::PRIMARY_600
    } else {
        color::NEUTRAL_0
    };
    let seg_inactive = if is_collection {
        color::NEUTRAL_0
    } else {
        color::PRIMARY_600
    };

    rsx! {
        Modal { title: title.clone(), width: 640, on_close: move |e| on_close.call(e),
            div { style: "display:flex; gap:8px; margin-bottom:16px;",
                button { style: "flex:1; padding:8px; border:1px solid {color::PRIMARY_600}; border-radius:4px; background:{seg_active}; color:{seg_inactive}; font-weight:600; cursor:pointer;",
                    onclick: move |_| kind.set("collection".to_string()),
                    "Collection type"
                }
                button { style: "flex:1; padding:8px; border:1px solid {color::PRIMARY_600}; border-radius:4px; background:{seg_inactive}; color:{seg_active}; font-weight:600; cursor:pointer;",
                    onclick: move |_| kind.set("single".to_string()),
                    "Single type"
                }
            }
            TextField { value: "{display}", label: "Display name".to_string(), placeholder: "Article".to_string(), oninput: on_display }
            TextField { value: "{singular}", label: "API ID (Singular)".to_string(), placeholder: "article".to_string(),
                oninput: move |v| { singular.set(v); singular_manual.set(true); } }
            TextField { value: "{plural}", label: "API ID (Plural)".to_string(), placeholder: "articles".to_string(),
                oninput: move |v| { plural.set(v); plural_manual.set(true); } }
            Toggle { checked: draft(), label: "Draft & publish".to_string(), onchange: move |v| draft.set(v) }
            Toggle { checked: i18n(), label: "Internationalization".to_string(), onchange: move |v| i18n.set(v) }
            div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |e| on_close.call(e) }
                Button { label: "Continue".to_string(), variant: "primary".to_string(), on_click: move |_| on_create.call(build()) }
            }
        }
    }
}

#[component]
fn FieldPickerModal(
    on_close: EventHandler<MouseEvent>,
    on_pick: EventHandler<FieldType>,
) -> Element {
    let card_style = format!(
        "display:flex; align-items:center; gap:12px; padding:12px; border:1px solid {}; border-radius:4px; background:#fff; cursor:pointer; text-align:left;",
        color::NEUTRAL_150
    );
    let name_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::BODY_BOLD_SIZE,
        color::NEUTRAL_800
    );
    let desc_style = format!(
        "font-size:{}; color:{};",
        typography::PI_SIZE,
        color::NEUTRAL_500
    );
    let pickable: Vec<(FieldType, String, String)> = PICKABLE_FIELDS
        .iter()
        .map(|(ft, l, d)| (*ft, l.to_string(), d.to_string()))
        .collect();
    rsx! {
        Modal { title: "Add new field".to_string(), width: 720, on_close: move |e| on_close.call(e),
            div { style: "display:grid; grid-template-columns:1fr 1fr; gap:8px;",
                for (ft, label, desc) in pickable.into_iter() {
                    button {
                        style: "{card_style}",
                        onclick: move |_| on_pick.call(ft),
                        Icon { name: icon_for(ft), size: 20, color: color::PRIMARY_600.to_string() }
                        div { style: "display:flex; flex-direction:column;",
                            span { style: "{name_style}", "{label}" }
                            span { style: "{desc_style}", "{desc}" }
                        }
                    }
                }
            }
        }
    }
}

/// Convert a human display name to a kebab-case API id (via `cruet`).
fn kebab_id(s: &str) -> String {
    use cruet::Inflector;
    s.to_kebab_case()
}

/// Derive the singular and plural API IDs from a display name (e.g. "Blog Post"
/// -> ("blog-post", "blog-posts")).
fn api_ids_from_display(display: &str) -> (String, String) {
    use cruet::Inflector;
    let singular = display.to_kebab_case();
    let plural = singular.to_plural();
    (singular, plural)
}

/// Coerce a user-typed conditional value into a JSON value: booleans and
/// numbers become typed; anything else stays a string.
fn parse_cond_value(s: &str) -> serde_json::Value {
    let t = s.trim();
    if t.eq_ignore_ascii_case("true") {
        serde_json::json!(true)
    } else if t.eq_ignore_ascii_case("false") {
        serde_json::json!(false)
    } else if let Ok(n) = t.parse::<i64>() {
        serde_json::json!(n)
    } else if let Ok(f) = t.parse::<f64>() {
        serde_json::json!(f)
    } else {
        serde_json::json!(t)
    }
}

/// Resolve the concrete FieldType for a picked type given the sub-format
/// chosen for Number / Date fields.
fn resolve_type(field_type: FieldType, num_format: &str, date_type: &str) -> FieldType {
    use FieldType::*;
    match field_type {
        Integer | Biginteger | Decimal | Float => match num_format {
            "bigint" => Biginteger,
            "decimal" => Decimal,
            "float" => Float,
            _ => Integer,
        },
        Date | Datetime | Time => match date_type {
            "date" => Date,
            "time" => Time,
            _ => Datetime,
        },
        other => other,
    }
}

#[component]
fn FieldConfigModal(
    field_type: FieldType,
    target_types: Vec<String>,
    component_types: Vec<(String, String)>,
    sibling_fields: Vec<String>,
    on_close: EventHandler<MouseEvent>,
    on_save: EventHandler<(String, Attribute)>,
) -> Element {
    let mut name = use_signal(String::new);
    let mut required = use_signal(|| false);
    let mut unique = use_signal(|| false);
    let mut private = use_signal(|| false);
    let mut num_format = use_signal(|| "integer".to_string());
    let mut date_type = use_signal(|| "datetime".to_string());
    let mut enum_values = use_signal(String::new);
    // Relation config.
    let mut relation_kind = use_signal(|| "oneToOne".to_string());
    let mut relation_target = use_signal(String::new);
    // Component config.
    let mut component_repeatable = use_signal(|| false);
    let mut component_uid = use_signal(String::new);
    // Dynamic zone config.
    let mut dz_components = use_signal(Vec::<String>::new);
    let mut dz_components_sel = use_signal(String::new);
    // Media config.
    let mut media_multiple = use_signal(|| false);
    let mut media_allowed = use_signal(|| "images".to_string());
    // UID config.
    let mut uid_target = use_signal(String::new);
    // Conditional visibility (Strapi conditional fields).
    let mut cond_enabled = use_signal(|| false);
    let mut cond_field = use_signal(String::new);
    let mut cond_operator = use_signal(|| "is".to_string());
    let mut cond_value = use_signal(String::new);

    let title = format!("Add a new {} field", field_type.as_str());
    let is_number = matches!(
        field_type,
        FieldType::Integer | FieldType::Biginteger | FieldType::Decimal | FieldType::Float
    );
    let is_date = matches!(
        field_type,
        FieldType::Date | FieldType::Datetime | FieldType::Time
    );
    let is_enum = matches!(field_type, FieldType::Enumeration);
    let is_relation = matches!(field_type, FieldType::Relation);
    let is_component = matches!(field_type, FieldType::Component);
    let is_dz = matches!(field_type, FieldType::Dynamiczone);
    let is_media = matches!(field_type, FieldType::Media);
    let is_uid = matches!(field_type, FieldType::Uid);

    let media_allowed_options: Vec<(String, String)> = vec![
        ("images".to_string(), "Images".to_string()),
        ("videos".to_string(), "Videos".to_string()),
        ("files".to_string(), "Files".to_string()),
        ("audios".to_string(), "Audios".to_string()),
    ];
    let uid_target_options: Vec<(String, String)> =
        std::iter::once(("".to_string(), "None".to_string()))
            .chain(sibling_fields.iter().map(|f| (f.clone(), f.clone())))
            .collect();
    let trigger_options: Vec<(String, String)> =
        std::iter::once(("".to_string(), "None".to_string()))
            .chain(sibling_fields.iter().map(|f| (f.clone(), f.clone())))
            .collect();
    let operator_options: Vec<(String, String)> = vec![
        ("is".to_string(), "is".to_string()),
        ("isNot".to_string(), "is not".to_string()),
    ];

    let relation_options: Vec<(String, String)> = vec![
        ("oneWay".to_string(), "One way".to_string()),
        ("oneToOne".to_string(), "One-to-one".to_string()),
        ("oneToMany".to_string(), "One-to-many".to_string()),
        ("manyToOne".to_string(), "Many-to-one".to_string()),
        ("manyToMany".to_string(), "Many-to-many".to_string()),
        ("manyWay".to_string(), "Many way".to_string()),
    ];
    let target_options: Vec<(String, String)> = target_types
        .iter()
        .map(|u| (u.clone(), u.clone()))
        .collect();

    let finish = move |_| {
        let attr_type = resolve_type(field_type, &num_format(), &date_type());
        let mut attr = Attribute::new(attr_type);
        attr.required = required();
        attr.unique = unique();
        attr.private = private();
        if is_enum {
            attr.enum_values = enum_values()
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
        }
        if is_relation {
            attr.relation = Some(core_domain::RelationKind::parse(&relation_kind()));
            attr.target = if relation_target().is_empty() {
                None
            } else {
                Some(core_domain::Uid::new(&relation_target()))
            };
        }
        if is_component {
            attr.component = if component_uid().is_empty() {
                None
            } else {
                Some(core_domain::Uid::new(&component_uid()))
            };
            attr.repeatable = Some(component_repeatable());
        }
        if is_dz {
            attr.components = dz_components()
                .iter()
                .map(|c| core_domain::Uid::new(c))
                .collect();
        }
        if is_media {
            attr.multiple = Some(media_multiple());
            attr.allowed_types = vec![media_allowed()];
        }
        if is_uid {
            attr.target_field = if uid_target().is_empty() {
                None
            } else {
                Some(uid_target())
            };
        }
        if cond_enabled() && !cond_field().is_empty() {
            attr.visible_when = Some(core_schema::FieldCondition {
                field: cond_field(),
                operator: if cond_operator() == "isNot" {
                    core_schema::FieldConditionOperator::IsNot
                } else {
                    core_schema::FieldConditionOperator::Is
                },
                value: parse_cond_value(&cond_value()),
            });
        }
        on_save.call((name(), attr));
    };

    rsx! {
        Modal { title: title, width: 640, on_close: move |e| on_close.call(e),
            TextField {
                value: "{name}",
                label: "Name".to_string(),
                helper: "No space is allowed for the name of the attribute.".to_string(),
                oninput: move |v| name.set(v),
            }
            if is_number {
                Dropdown {
                    label: "Number format".to_string(),
                    options: vec![
                        ("integer".to_string(), "integer".to_string()),
                        ("bigint".to_string(), "big integer".to_string()),
                        ("decimal".to_string(), "decimal".to_string()),
                        ("float".to_string(), "float".to_string()),
                    ],
                    value: "{num_format}",
                    onchange: move |v| num_format.set(v),
                }
            }
            if is_date {
                Dropdown {
                    label: "Type".to_string(),
                    options: vec![
                        ("date".to_string(), "date".to_string()),
                        ("datetime".to_string(), "datetime".to_string()),
                        ("time".to_string(), "time".to_string()),
                    ],
                    value: "{date_type}",
                    onchange: move |v| date_type.set(v),
                }
            }
            if is_enum {
                TextArea {
                    value: "{enum_values}",
                    label: "Values (one line per value)".to_string(),
                    placeholder: "one\ntwo\nthree".to_string(),
                    rows: 4,
                    oninput: move |v| enum_values.set(v),
                }
            }
            if is_relation {
                Dropdown {
                    label: "Relation type".to_string(),
                    options: relation_options,
                    value: "{relation_kind}",
                    onchange: move |v| relation_kind.set(v),
                }
                Dropdown {
                    label: "Target content type".to_string(),
                    options: target_options,
                    value: "{relation_target}",
                    onchange: move |v| relation_target.set(v),
                }
            }
            if is_component {
                Dropdown {
                    label: "Component".to_string(),
                    options: component_types.clone(),
                    value: "{component_uid}",
                    onchange: move |v| component_uid.set(v),
                }
                Toggle { checked: component_repeatable(), label: "Repeatable".to_string(), onchange: move |v| component_repeatable.set(v) }
            }
            if is_dz {
                Dropdown {
                    label: "Allowed component".to_string(),
                    options: component_types.clone(),
                    value: "{dz_components_sel}",
                    onchange: move |v: String| {
                        dz_components_sel.set(v.clone());
                        if !dz_components().contains(&v) {
                            dz_components.write().push(v);
                        }
                    },
                }
                div { style: "display:flex; flex-wrap:wrap; gap:8px; margin-top:8px;",
                    for c in dz_components().clone() {
                        div { style: "display:flex; align-items:center; gap:6px; padding:4px 10px; border-radius:999px; background:{color::PRIMARY_100}; color:{color::PRIMARY_700}; font-size:{typography::PI_SIZE};",
                            span { "{c}" }
                        }
                    }
                }
            }
            if is_media {
                Toggle { checked: media_multiple(), label: "Multiple media".to_string(), onchange: move |v| media_multiple.set(v) }
                Dropdown {
                    label: "Allowed media types".to_string(),
                    options: media_allowed_options,
                    value: "{media_allowed}",
                    onchange: move |v| media_allowed.set(v),
                }
            }
            if is_uid {
                Dropdown {
                    label: "Attached field".to_string(),
                    options: uid_target_options,
                    value: "{uid_target}",
                    onchange: move |v| uid_target.set(v),
                }
            }
            div { style: "display:flex; flex-direction:column; gap:8px; margin:16px 0;",
                Toggle { checked: required(), label: "Required field".to_string(), onchange: move |v| required.set(v) }
                Toggle { checked: unique(), label: "Unique field".to_string(), onchange: move |v| unique.set(v) }
                Toggle { checked: private(), label: "Private field (not exposed in API)".to_string(), onchange: move |v| private.set(v) }
            }
            div { style: "border-top:1px solid {color::NEUTRAL_150}; padding-top:16px; margin-top:8px;",
                div { style: "display:flex; align-items:center; justify-content:space-between;",
                    span { style: "font-size:{typography::EPSILON_SIZE}; font-weight:600; color:{color::NEUTRAL_800};", "Conditional visibility" }
                    Toggle { checked: cond_enabled(), label: String::new(), onchange: move |v| cond_enabled.set(v) }
                }
                span { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500}; display:block; margin-top:4px;",
                    "Show this field only when another field matches a value."
                }
                if cond_enabled() {
                    Dropdown {
                        label: "Trigger field".to_string(),
                        options: trigger_options,
                        value: "{cond_field}",
                        onchange: move |v| cond_field.set(v),
                    }
                    Dropdown {
                        label: "Condition".to_string(),
                        options: operator_options,
                        value: "{cond_operator}",
                        onchange: move |v| cond_operator.set(v),
                    }
                    TextField {
                        value: "{cond_value}",
                        label: "Value".to_string(),
                        placeholder: "true / false / a value".to_string(),
                        helper: "For checkboxes use true or false; for selects use the option value.".to_string(),
                        oninput: move |v| cond_value.set(v),
                    }
                }
            }
            div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |e| on_close.call(e) }
                Button { label: "Finish".to_string(), variant: "primary".to_string(), on_click: finish }
            }
        }
    }
}

fn icon_for(ft: FieldType) -> String {
    use ui::design::icons::Icon;
    let icon = Icon::for_field_type(ft);
    match icon {
        Icon::Text => "text",
        Icon::Hash => "hash",
        Icon::Calendar => "calendar",
        Icon::Toggle => "toggle",
        Icon::Envelope => "envelope",
        Icon::Lock => "lock",
        Icon::List => "list",
        Icon::Braces => "braces",
        Icon::Tag => "tag",
        Icon::File => "file",
        _ => "text",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_id_lowercases_and_dashes() {
        assert_eq!(kebab_id("Blog Post"), "blog-post");
        assert_eq!(kebab_id("  My  Article "), "my-article");
        assert_eq!(kebab_id("Article"), "article");
        assert_eq!(kebab_id("Q&A"), "q-a");
        assert_eq!(kebab_id("  "), "");
    }

    #[test]
    fn kebab_id_is_valid_api_id() {
        let id = kebab_id("FAQ Page");
        assert!(id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        assert!(!id.is_empty());
    }

    #[test]
    fn api_ids_derive_from_display_name() {
        assert_eq!(
            api_ids_from_display("Blog Post"),
            ("blog-post".to_string(), "blog-posts".to_string())
        );
        assert_eq!(
            api_ids_from_display("Article"),
            ("article".to_string(), "articles".to_string())
        );
        assert_eq!(
            api_ids_from_display("  My  Page "),
            ("my-page".to_string(), "my-pages".to_string())
        );
        // Empty display name yields an empty singular and a bare "s" plural.
        assert_eq!(api_ids_from_display(""), ("".to_string(), "s".to_string()));
    }

    #[test]
    fn parse_cond_value_coerces_types() {
        assert_eq!(parse_cond_value("true"), serde_json::json!(true));
        assert_eq!(parse_cond_value("FALSE"), serde_json::json!(false));
        assert_eq!(parse_cond_value("42"), serde_json::json!(42));
        assert_eq!(parse_cond_value("3.5"), serde_json::json!(3.5));
        assert_eq!(parse_cond_value("draft"), serde_json::json!("draft"));
        assert_eq!(parse_cond_value(""), serde_json::json!(""));
    }
}
