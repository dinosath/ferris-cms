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

use crate::app::use_global;
use crate::components::{
    Badge, Button, Card, Dropdown, EmptyState, Icon, IconButton, Modal, NavItem, TextArea,
    TextField, Toggle,
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

const SECTION_LABEL: &str = "padding:4px 16px; font-size:{font}; color:{col};";
const LABEL_FONT: &str = "12px";
const NEUTRAL_600: &str = "#666687";

#[component]
pub fn ContentTypeBuilder() -> Element {
    let global = use_global();
    let mut working = use_signal(Vec::<Schema>::new);
    let mut loaded = use_signal(|| false);
    let mut selected_uid = use_signal(|| None::<String>);
    let mut modal = use_signal(|| ModalKind::None);
    let mut is_dirty = use_signal(|| false);
    let mut saving = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let g = g_load.clone();
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
                        working.set(schemas);
                    }
                    Err(e) => status.set(Some(format!("Failed to load: {e}"))),
                }
            });
        }
    });

    let schemas = working();
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
    let sibling_fields: Vec<String> = schemas
        .iter()
        .find(|s| Some(s.uid.as_str().to_string()) == selected_uid())
        .map(|s| s.attributes.keys().cloned().collect())
        .unwrap_or_default();
    let selected = schemas
        .iter()
        .find(|s| Some(s.uid.as_str().to_string()) == selected_uid())
        .cloned();

    let collection_items: Vec<(String, String)> = schemas
        .iter()
        .filter(|s| s.kind == ContentTypeKind::CollectionType)
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();
    let single_items: Vec<(String, String)> = schemas
        .iter()
        .filter(|s| s.kind == ContentTypeKind::SingleType)
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();
    let component_items: Vec<(String, String)> = schemas
        .iter()
        .filter(|s| s.kind == ContentTypeKind::Component)
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();

    let selected_display = selected.as_ref().map(|s| s.info.display_name.clone());
    let selected_uid_str = selected.as_ref().map(|s| s.uid.as_str().to_string());
    let selected_attrs: Vec<(String, String, FieldType, bool)> = selected
        .as_ref()
        .map(|s| {
            s.attributes
                .iter()
                .map(|(n, a)| {
                    (
                        s.uid.as_str().to_string(),
                        n.clone(),
                        a.attr_type,
                        a.required,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let uid_for_picker = selected_uid_str.clone().unwrap_or_default();

    let sidebar_style = format!(
        "width:240px; min-width:240px; background:{}; border-right:1px solid {}; display:flex; flex-direction:column;",
        color::NEUTRAL_0, color::NEUTRAL_150
    );
    let header_style = format!(
        "padding:16px; font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let editor_top_style = format!(
        "display:flex; align-items:center; justify-content:space-between; padding:0 32px; height:64px; border-bottom:1px solid {}; background:{};",
        color::NEUTRAL_150, color::NEUTRAL_100
    );
    let editor_title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::BETA_SIZE,
        color::NEUTRAL_900
    );
    let page_title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
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
    let g_save = global.clone();
    let toast_signal = global.clone();
    // Owned copy for the bottom dashed add-field button (the header button's
    // `move` closure already moves `uid_for_picker`).
    let picker_uid = uid_for_picker.clone();

    rsx! {
        div { style: "display:flex; min-height:100vh;",
            div { style: "{sidebar_style}",
                div { style: "{header_style}", "Content-Type Builder" }
                span { style: SECTION_LABEL.replace("{font}", LABEL_FONT).replace("{col}", NEUTRAL_600), "COLLECTION TYPES" }
                for (uid, display) in collection_items.into_iter() {
                    NavItem {
                        label: display,
                        icon: "stack".to_string(),
                        active: selected_uid() == Some(uid.clone()),
                        onclick: move |_| selected_uid.set(Some(uid.clone())),
                    }
                }
                button { style: "background:none; border:none; color:{color::PRIMARY_600}; cursor:pointer; text-align:left; padding:8px 16px; font-size:{typography::BODY_SIZE};",
                    onclick: move |_| modal.set(ModalKind::CreateType),
                    "+ Create new collection type"
                }
                span { style: SECTION_LABEL.replace("{font}", LABEL_FONT).replace("{col}", NEUTRAL_600), "SINGLE TYPES" }
                for (uid, display) in single_items.into_iter() {
                    NavItem {
                        label: display,
                        icon: "grid".to_string(),
                        active: selected_uid() == Some(uid.clone()),
                        onclick: move |_| selected_uid.set(Some(uid.clone())),
                    }
                }
                button { style: "background:none; border:none; color:{color::PRIMARY_600}; cursor:pointer; text-align:left; padding:8px 16px; font-size:{typography::BODY_SIZE};",
                    onclick: move |_| modal.set(ModalKind::CreateType),
                    "+ Create new single type"
                }
                span { style: SECTION_LABEL.replace("{font}", LABEL_FONT).replace("{col}", NEUTRAL_600), "COMPONENTS" }
                for (uid, display) in component_items.into_iter() {
                    NavItem { label: display, icon: "puzzle".to_string(), active: false, onclick: move |_| selected_uid.set(Some(uid.clone())) }
                }
            }

            div { style: "flex:1; min-width:0;",
                div { style: "{editor_top_style}",
                    span { style: "{editor_title_style}",
                        if let Some(name) = &selected_display { "{name}" } else { "Select a content type" }
                    }
                    div { style: "display:flex; align-items:center; gap:12px;",
                        if let Some(status) = status() {
                            span { style: "font-size:{typography::PI_SIZE}; color:{color::SUCCESS_600};", "{status}" }
                        }
                        Button {
                            label: "Save".to_string(), variant: "success".to_string(), disabled: !is_dirty(), loading: saving(),
                            on_click: move |_| {
                                let g = g_save.clone();
                                let toast = toast_signal.clone();
                                let schemas = working();
                                saving.set(true);
                                spawn(async move {
                                    let mut toast = toast;
                                    match g.client.ctb_apply(schemas).await {
                                        Ok(_) => { is_dirty.set(false); status.set(Some("Saved".to_string())); toast.toast("Schema saved".to_string(), "success"); }
                                        Err(e) => { status.set(Some(format!("Error: {e}"))); toast.toast(format!("Save failed: {e}"), "danger"); }
                                    }
                                    saving.set(false);
                                });
                            },
                        }
                    }
                }

                div { style: "padding:32px;",
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
                    } else {
                        div { style: "padding:48px;",
                            EmptyState {
                                title: "Select a content type".to_string(),
                                subtitle: "Select a content type or create a new one to begin.".to_string(),
                                icon: "grid".to_string(),
                            }
                        }
                    }
                }
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
    }
}

#[component]
fn CreateTypeModal(on_close: EventHandler<MouseEvent>, on_create: EventHandler<Schema>) -> Element {
    let mut display = use_signal(String::new);
    let mut singular = use_signal(String::new);
    let mut plural = use_signal(String::new);
    let mut kind = use_signal(|| "collection".to_string());
    let mut draft = use_signal(|| true);
    let mut i18n = use_signal(|| false);

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
            TextField { value: "{display}", label: "Display name".to_string(), placeholder: "Article".to_string(), oninput: move |v| display.set(v) }
            TextField { value: "{singular}", label: "API ID (Singular)".to_string(), placeholder: "article".to_string(), oninput: move |v| singular.set(v) }
            TextField { value: "{plural}", label: "API ID (Plural)".to_string(), placeholder: "articles".to_string(), oninput: move |v| plural.set(v) }
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

/// Derive a kebab-case api id from a human display name ("Blog Post" -> "blog-post").
/// The API id regex only allows lowercase letters, digits and dashes, so spaces
/// and other punctuation must become dashes (Strapi behaviour).
fn kebab_id(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.trim().chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
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
}
