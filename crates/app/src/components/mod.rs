//! Base Dioxus widgets for the ferriscms admin UI.
//!
//! These mirror the framework-agnostic widget specs in the `ui` crate
//! (`crates/ui/src/widgets`) and are styled from the shared design tokens via
//! the class-based stylesheet in `theme::component_styles()`. Keeping the
//! markup class-driven (rather than inline-styled per call site) means every
//! screen shares one source of truth for spacing, typography and interaction
//! states, and real `:hover` / `:focus` / `:disabled` behaviour comes for free.

pub mod icon;

pub use icon::Icon;

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

/// Map a semantic variant name to its button CSS class.
fn button_class(variant: &str) -> &'static str {
    match variant {
        "secondary" => "btn-secondary",
        "danger" => "btn-danger",
        "danger-light" => "btn-danger-light",
        "success" => "btn-success",
        "tertiary" => "btn-tertiary",
        "ghost" => "btn-ghost",
        _ => "btn-primary",
    }
}

/// A styled button with several variants, sizes and interaction states.
#[component]
pub fn Button(
    label: String,
    #[props(default)] variant: String,
    #[props(default)] disabled: bool,
    #[props(default)] loading: bool,
    #[props(default)] full_width: bool,
    #[props(default)] size: String,
    #[props(default)] on_click: Option<EventHandler<MouseEvent>>,
) -> Element {
    let size_class = match size.as_str() {
        "sm" => "btn-sm",
        "lg" => "btn-lg",
        _ => "btn-md",
    };
    let mut classes = vec!["btn", button_class(&variant), size_class];
    if loading {
        classes.push("btn-loading");
    }
    if full_width {
        classes.push("btn-block");
    }
    let class = classes.join(" ");
    rsx! {
        button {
            "class": "{class}",
            disabled: disabled,
            onclick: move |e| { if let Some(h) = on_click.as_ref() { h.call(e); } },
            "{label}"
        }
    }
}

/// A compact icon-only button (row actions, header controls).
#[component]
pub fn IconButton(
    name: String,
    #[props(default)] variant: String,
    #[props(default)] size: u32,
    #[props(default)] aria_label: String,
    #[props(default)] on_click: Option<EventHandler<MouseEvent>>,
) -> Element {
    let mut class = "btn-icon".to_string();
    if variant == "danger" {
        class.push_str(" btn-icon-danger");
    }
    let label = if aria_label.is_empty() {
        name.clone()
    } else {
        aria_label
    };
    rsx! {
        button {
            "class": "{class}",
            r#type: "button",
            onclick: move |e| { if let Some(h) = on_click.as_ref() { h.call(e); } },
            aria_label: "{label}",
            Icon { name, size: if size > 0 { size } else { 16 } }
        }
    }
}

/// A labelled field wrapper that standardises the Label → Input → Hint →
/// Validation layout used across every form.
#[component]
pub fn Field(
    #[props(default)] label: String,
    #[props(default)] hint: String,
    #[props(default)] error: String,
    #[props(default)] required: bool,
    children: Element,
) -> Element {
    rsx! {
        div { "class": "field",
            if !label.is_empty() {
                label { "class": "field-label",
                    "{label}"
                    if required { span { "class": "field-required", " *" } }
                }
            }
            {children}
            if !hint.is_empty() {
                span { "class": "field-hint", "{hint}" }
            }
            if !error.is_empty() {
                span { "class": "field-error", "{error}" }
            }
        }
    }
}

/// A labelled text input with hint/error state.
#[component]
pub fn TextField(
    value: String,
    #[props(default)] label: String,
    #[props(default)] placeholder: String,
    #[props(default)] input_type: String,
    #[props(default)] error: String,
    #[props(default)] helper: String,
    #[props(default)] required: bool,
    #[props(default)] disabled: bool,
    oninput: EventHandler<String>,
) -> Element {
    let mut input_class = "input".to_string();
    if !error.is_empty() {
        input_class.push_str(" input-error");
    }
    rsx! {
        Field {
            label,
            hint: helper,
            error,
            required,
            input {
                "class": "{input_class}",
                r#type: if input_type.is_empty() { "text".to_string() } else { input_type },
                value: "{value}",
                placeholder: "{placeholder}",
                disabled: disabled,
                oninput: move |e| oninput.call(e.value()),
            }
        }
    }
}

/// A multiline text area.
#[component]
pub fn TextArea(
    value: String,
    #[props(default)] label: String,
    #[props(default)] placeholder: String,
    #[props(default)] rows: u32,
    #[props(default)] hint: String,
    oninput: EventHandler<String>,
) -> Element {
    rsx! {
        Field {
            label,
            hint,
            textarea {
                "class": "input textarea",
                value: "{value}",
                placeholder: "{placeholder}",
                rows: rows,
                oninput: move |e| oninput.call(e.value()),
            }
        }
    }
}

/// A bordered card container.
#[component]
pub fn Card(
    #[props(default)] padding: u32,
    #[props(default)] header: String,
    children: Element,
) -> Element {
    let body_pad = if header.is_empty() { padding } else { 24 };
    rsx! {
        div { "class": "card", style: "overflow:hidden;",
            if !header.is_empty() {
                div { "class": "card-header", "{header}" }
            }
            div { "class": "card-body", style: "padding:{body_pad}px;", {children} }
        }
    }
}

/// Map a badge kind to its CSS class (kept in sync with `tokens::badge_colors`).
fn badge_class(kind: &str) -> &'static str {
    match kind {
        "draft" => "badge-draft",
        "published" | "P" => "badge-published",
        "modified" | "M" => "badge-modified",
        "new" | "N" => "badge-new",
        "deleted" | "D" | "danger" => "badge-danger",
        _ => "badge-neutral",
    }
}

/// A small status badge.
#[component]
pub fn Badge(text: String, #[props(default)] kind: String) -> Element {
    let class = format!("badge {}", badge_class(&kind));
    rsx! { span { "class": "{class}", "{text}" } }
}

/// A labelled checkbox.
#[component]
pub fn Checkbox(
    checked: bool,
    #[props(default)] label: String,
    #[props(default)] disabled: bool,
    onchange: EventHandler<bool>,
) -> Element {
    rsx! {
        label { style: "display:inline-flex; align-items:center; gap:8px; font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_700}; cursor:pointer;",
            input {
                "class": "checkbox",
                r#type: "checkbox",
                checked: checked,
                disabled: disabled,
                onchange: move |e| onchange.call(e.checked()),
            }
            if !label.is_empty() {
                span { "{label}" }
            }
        }
    }
}

/// A labelled toggle switch.
#[component]
pub fn Toggle(
    checked: bool,
    #[props(default)] label: String,
    #[props(default)] disabled: bool,
    onchange: EventHandler<bool>,
) -> Element {
    let mut class = "switch".to_string();
    if checked {
        class.push_str(" switch-checked");
    }
    rsx! {
        div { style: "display:flex; align-items:center; gap:8px;",
            button {
                "class": "{class}",
                r#type: "button",
                disabled: disabled,
                onclick: move |_| onchange.call(!checked),
                span { "class": "switch-knob" }
            }
            if !label.is_empty() {
                span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_700};", "{label}" }
            }
        }
    }
}

/// A labelled dropdown select.
#[component]
pub fn Dropdown(
    value: String,
    options: Vec<(String, String)>,
    #[props(default)] label: String,
    #[props(default)] disabled: bool,
    onchange: EventHandler<String>,
) -> Element {
    rsx! {
        Field {
            label,
            select {
                "class": "select",
                value: "{value}",
                disabled: disabled,
                onchange: move |e| onchange.call(e.value()),
                for (opt_value, opt_label) in options.iter() {
                    option { value: "{opt_value}", "{opt_label}" }
                }
            }
        }
    }
}

/// A modal overlay with a title and a close button.
#[component]
pub fn Modal(
    title: String,
    #[props(default)] width: u32,
    on_close: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let w = if width > 0 { width } else { 512 };
    rsx! {
        div {
            "class": "modal-overlay",
            onclick: move |e| on_close.call(e),
            div {
                "class": "modal-panel",
                style: "width:{w}px;",
                onclick: move |e| e.stop_propagation(),
                div { style: "display:flex; align-items:center; justify-content:space-between; padding:16px 24px; border-bottom:1px solid {color::NEUTRAL_150}; flex-shrink:0;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "{title}" }
                    button { "class": "btn-icon", onclick: move |e| on_close.call(e), aria_label: "Close".to_string(), Icon { name: "close".to_string(), size: 18 } }
                }
                div { style: "padding:24px;", {children} }
            }
        }
    }
}

/// A sidebar navigation item.
#[component]
pub fn NavItem(
    label: String,
    #[props(default)] icon: String,
    #[props(default)] active: bool,
    #[props(default)] badge: String,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let mut class = "nav-item".to_string();
    if active {
        class.push_str(" nav-item-active");
    }
    rsx! {
        button {
            "class": "{class}",
            onclick: move |e| onclick.call(e),
            icon::Icon { name: "{icon}", size: 18 }
            span { "{label}" }
            if !badge.is_empty() {
                Badge { text: "{badge}", kind: "new".to_string() }
            }
        }
    }
}

/// A simple data table.
#[component]
pub fn Table(columns: Vec<(String, String)>, rows: Vec<Vec<String>>) -> Element {
    rsx! {
        table { "class": "table",
            thead {
                tr {
                    for (_, label) in columns.iter() {
                        th { "class": "table-th", "{label}" }
                    }
                }
            }
            tbody {
                for row in rows.iter() {
                    tr { "class": "table-row",
                        for cell in row.iter() {
                            td { "class": "table-td", "{cell}" }
                        }
                    }
                }
            }
        }
    }
}

/// An empty state with a title, subtitle, and optional action.
#[component]
pub fn EmptyState(
    title: String,
    #[props(default)] subtitle: String,
    #[props(default)] icon: String,
    children: Element,
) -> Element {
    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_800
    );
    let subtitle_style = format!(
        "font-size:{}; color:{}; max-width:360px;",
        typography::BODY_SIZE,
        color::NEUTRAL_600
    );
    rsx! {
        div { "class": "empty-state",
            icon::Icon { name: "{icon}", size: 40, color: color::NEUTRAL_400.to_string() }
            span { style: "{title_style}", "{title}" }
            if !subtitle.is_empty() {
                span { style: "{subtitle_style}", "{subtitle}" }
            }
            {children}
        }
    }
}

/// A toast notification (success / danger / info).
#[component]
pub fn Toast(text: String, kind: String, on_close: EventHandler<()>) -> Element {
    let bg = match kind.as_str() {
        "success" => color::SUCCESS_600,
        "danger" => color::DANGER_600,
        _ => color::NEUTRAL_800,
    };
    rsx! {
        div { style: "display:flex; align-items:center; gap:12px; padding:12px 16px; border-radius:4px; background:{bg}; color:#fff; box-shadow:0 4px 12px rgba(33,33,52,0.2); min-width:240px;",
            span { style: "flex:1; font-size:{typography::BODY_SIZE};", "{text}" }
            button { style: "background:none; border:none; color:#fff; cursor:pointer; font-size:16px;", onclick: move |_| on_close.call(()), "✕" }
        }
    }
}

/// A confirm dialog with a danger action.
#[component]
pub fn ConfirmDialog(
    title: String,
    message: String,
    confirm_label: String,
    on_cancel: EventHandler<()>,
    on_confirm: EventHandler<()>,
) -> Element {
    rsx! {
        Modal { title, width: 512, on_close: move |_| on_cancel.call(()),
            div { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600}; margin-bottom:16px;", "{message}" }
            div { style: "display:flex; justify-content:flex-end; gap:12px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| on_cancel.call(()) }
                Button { label: confirm_label, variant: "danger".to_string(), on_click: move |_| on_confirm.call(()) }
            }
        }
    }
}

/// A horizontal tab bar. `active` selects the focused tab, `on_change` reports
/// the selected index.
#[component]
pub fn Tabs(
    labels: Vec<String>,
    #[props(default)] active: usize,
    on_change: EventHandler<usize>,
) -> Element {
    rsx! {
        div { "class": "tabs", role: "tablist",
            for (idx, label) in labels.iter().enumerate() {
                button {
                    "class": if idx == active { "tab tab-active" } else { "tab" },
                    role: "tab",
                    aria_selected: "{idx == active}",
                    onclick: move |_| on_change.call(idx),
                    "{label}"
                }
            }
        }
    }
}

/// A breadcrumb trail. `crumbs` is a list of `(label, selected)` segments where
/// `selected == true` marks the current (non-interactive) page.
#[component]
pub fn Breadcrumbs(crumbs: Vec<(String, bool)>, on_navigate: EventHandler<usize>) -> Element {
    rsx! {
        nav { "class": "breadcrumb", aria_label: "Breadcrumb",
            for (idx, (label, selected)) in crumbs.iter().enumerate() {
                if *selected {
                    span { "aria_current": "page", "{label}" }
                } else {
                    button { "class": "breadcrumb-link", onclick: move |_| on_navigate.call(idx), "{label}" }
                    span { "class": "breadcrumb-sep", "/" }
                }
            }
        }
    }
}

/// Pagination controls with an info line and prev/next.
#[component]
pub fn Pagination(
    #[props(default)] page: i64,
    #[props(default)] page_count: i64,
    #[props(default)] page_size: i64,
    #[props(default)] total: i64,
    #[props(default)] sizes: Vec<i64>,
    on_page_change: EventHandler<i64>,
    on_page_size_change: EventHandler<i64>,
) -> Element {
    let sizes = if sizes.is_empty() {
        vec![10, 25, 50, 100]
    } else {
        sizes
    };
    rsx! {
        div { "class": "pagination",
            div { "class": "pagination-info",
                span { "Rows per page" }
                select { "class": "select", style: "width:auto; margin-left:8px;",
                    value: "{page_size}",
                    onchange: move |e| {
                        if let Ok(v) = e.value().parse::<i64>() { on_page_size_change.call(v); }
                    },
                    for n in sizes.iter() {
                        option { value: "{n}", "{n}" }
                    }
                }
            }
            div { "class": "pagination-controls",
                span { "class": "pagination-info", "Page {page} of {page_count}" }
                Button { label: "‹ Prev".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), disabled: page <= 1,
                    on_click: move |_| on_page_change.call(page - 1) }
                Button { label: "Next ›".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), disabled: page >= page_count,
                    on_click: move |_| on_page_change.call(page + 1) }
            }
        }
    }
}

/// A lifecycle status indicator (Draft / Published / Modified / Unpublished).
#[component]
pub fn StatusIndicator(status: String) -> Element {
    let (class, label) = match status.as_str() {
        "published" | "P" => ("status-published", "Published"),
        "modified" | "M" => ("status-modified", "Modified"),
        "unpublished" | "U" => ("status-unpublished", "Unpublished"),
        _ => ("status-draft", "Draft"),
    };
    rsx! {
        span { "class": "status {class}",
            span { "class": "badge-dot" }
            "{label}"
        }
    }
}

/// A shimmering skeleton loading block.
#[component]
pub fn Skeleton(#[props(default)] width: String, #[props(default)] height: String) -> Element {
    rsx! {
        div { "class": "skeleton", style: "width:{width}; height:{height};" }
    }
}

/// A small circular spinner used to indicate in-flight work.
#[component]
pub fn Spinner(#[props(default)] size: u32, #[props(default)] color: String) -> Element {
    let sz = if size > 0 { size } else { 20 };
    let c = if color.is_empty() {
        ui::design::tokens::color::PRIMARY_600.to_string()
    } else {
        color
    };
    rsx! {
        div { style: "display:inline-block; width:{sz}px; height:{sz}px; border:2px solid {c}33; border-top-color:{c}; border-radius:50%; animation:fc-spin 0.7s linear infinite;" }
    }
}
