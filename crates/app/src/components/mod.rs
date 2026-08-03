//! Base Dioxus widgets for the ferriscms admin UI.
//!
//! These mirror the framework-agnostic widget specs in the `ui` crate
//! (`crates/ui/src/widgets`) and are styled from the shared design tokens.

pub mod icon;

pub use icon::Icon;

use dioxus::prelude::*;
use ui::design::tokens::{RADIUS_MD, RADIUS_SM, color, typography};

/// A styled button with several variants.
#[component]
pub fn Button(
    label: String,
    #[props(default)] variant: String,
    #[props(default)] disabled: bool,
    #[props(default)] loading: bool,
    #[props(default)] full_width: bool,
    #[props(default)] on_click: Option<EventHandler<MouseEvent>>,
) -> Element {
    let (bg, fg, border) = match variant.as_str() {
        "secondary" => (color::NEUTRAL_0, color::NEUTRAL_700, color::NEUTRAL_200),
        "danger" => (color::DANGER_600, "#FFFFFF", color::DANGER_600),
        "success" => (color::SUCCESS_600, "#FFFFFF", color::SUCCESS_600),
        "ghost" => ("transparent", color::NEUTRAL_700, "transparent"),
        _ => (color::PRIMARY_600, "#FFFFFF", color::PRIMARY_600),
    };
    let width = if full_width { "width:100%;" } else { "" };
    let opacity = if disabled { "opacity:0.6; cursor:not-allowed;" } else { "" };
    let style = format!(
        "background:{bg}; color:{fg}; border:1px solid {border}; border-radius:{RADIUS_SM}px; padding:8px 16px; font-size:{size}; font-weight:600; {width}{opacity}display:inline-flex; align-items:center; justify-content:center; gap:8px;",
        size = typography::BODY_SIZE
    );
    rsx! {
        button {
            style: "{style}",
            disabled: disabled,
            onclick: move |e| { if let Some(h) = on_click.as_ref() { h.call(e); } },
            if loading {
                span { style: "display:inline-block; width:14px; height:14px; border:2px solid rgba(255,255,255,0.4); border-top-color:currentColor; border-radius:50%; animation:spin 0.8s linear infinite;" }
            }
            "{label}"
        }
    }
}

/// A labelled text input.
#[component]
pub fn TextField(
    value: String,
    #[props(default)] label: String,
    #[props(default)] placeholder: String,
    #[props(default)] input_type: String,
    #[props(default)] error: String,
    #[props(default)] helper: String,
    oninput: EventHandler<String>,
) -> Element {
    let border = if error.is_empty() { color::NEUTRAL_200 } else { color::DANGER_600 };
    let style = format!(
        "width:100%; padding:8px 16px; border:1px solid {border}; border-radius:{RADIUS_SM}px; font-size:{size}; color:{fg}; background:{bg};",
        size = typography::BODY_SIZE,
        fg = color::NEUTRAL_800,
        bg = color::NEUTRAL_0
    );
    let label_style = format!("font-size:{}; font-weight:600; color:{};", typography::LABEL_SIZE, color::NEUTRAL_700);
    let helper_style = format!("font-size:{}; color:{};", typography::PI_SIZE, color::NEUTRAL_500);
    let error_style = format!("font-size:{}; color:{};", typography::PI_SIZE, color::DANGER_700);
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:6px; margin-bottom:16px;",
            if !label.is_empty() {
                label { style: "{label_style}", "{label}" }
            }
            input {
                r#type: "{input_type}",
                value: "{value}",
                placeholder: "{placeholder}",
                style: "{style}",
                oninput: move |e| oninput.call(e.value()),
            }
            if !helper.is_empty() {
                span { style: "{helper_style}", "{helper}" }
            }
            if !error.is_empty() {
                span { style: "{error_style}", "{error}" }
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
    oninput: EventHandler<String>,
) -> Element {
    let style = format!(
        "width:100%; padding:8px 16px; border:1px solid {b}; border-radius:{RADIUS_SM}px; font-size:{size}; color:{fg}; background:{bg}; font-family:inherit; resize:vertical;",
        b = color::NEUTRAL_200,
        size = typography::BODY_SIZE,
        fg = color::NEUTRAL_800,
        bg = color::NEUTRAL_0
    );
    let label_style = format!("font-size:{}; font-weight:600; color:{};", typography::LABEL_SIZE, color::NEUTRAL_700);
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:6px; margin-bottom:16px;",
            if !label.is_empty() {
                label { style: "{label_style}", "{label}" }
            }
            textarea {
                value: "{value}",
                placeholder: "{placeholder}",
                rows: rows,
                style: "{style}",
                oninput: move |e| oninput.call(e.value()),
            }
        }
    }
}

/// A bordered card container.
#[component]
pub fn Card(
    #[props(default)] padding: u32,
    children: Element,
) -> Element {
    let style = format!(
        "background:{bg}; border:1px solid {b}; border-radius:{RADIUS_MD}px; padding:{padding}px; box-shadow:0 1px 4px rgba(33,33,52,0.08);",
        bg = color::NEUTRAL_0,
        b = color::NEUTRAL_150
    );
    rsx! {
        div { style: "{style}", {children} }
    }
}

/// A small status badge.
#[component]
pub fn Badge(
    text: String,
    #[props(default)] kind: String,
) -> Element {
    let (fg, bg) = color::badge_colors(&kind);
    let style = format!(
        "display:inline-block; padding:2px 8px; border-radius:999px; font-size:{size}; font-weight:600; color:{fg}; background:{bg};",
        size = typography::PI_SIZE
    );
    rsx! { span { style: "{style}", "{text}" } }
}

/// A labelled checkbox.
#[component]
pub fn Checkbox(
    checked: bool,
    #[props(default)] label: String,
    onchange: EventHandler<bool>,
) -> Element {
    let style = format!("display:flex; align-items:center; gap:8px; font-size:{}; color:{}; cursor:pointer;", typography::BODY_SIZE, color::NEUTRAL_700);
    rsx! {
        label { style: "{style}",
            input {
                r#type: "checkbox",
                checked: checked,
                onchange: move |e| onchange.call(e.checked()),
            }
            "{label}"
        }
    }
}

/// A labelled toggle switch.
#[component]
pub fn Toggle(
    checked: bool,
    #[props(default)] label: String,
    onchange: EventHandler<bool>,
) -> Element {
    let track = if checked { color::PRIMARY_600 } else { color::NEUTRAL_300 };
    let style = format!(
        "position:relative; width:36px; height:20px; border-radius:999px; background:{track}; transition:background 0.2s; border:none; cursor:pointer;"
    );
    let knob_left = if checked { "16px" } else { "2px" };
    let label_style = format!("font-size:{}; color:{};", typography::BODY_SIZE, color::NEUTRAL_700);
    rsx! {
        div { style: "display:flex; align-items:center; gap:8px;",
            button {
                onclick: move |_| onchange.call(!checked),
                style: "{style}",
                span { style: "position:absolute; top:2px; left:{knob_left}; width:16px; height:16px; border-radius:50%; background:#fff; transition:left 0.2s;" }
            }
            if !label.is_empty() {
                span { style: "{label_style}", "{label}" }
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
    onchange: EventHandler<String>,
) -> Element {
    let label_style = format!("font-size:{}; font-weight:600; color:{};", typography::LABEL_SIZE, color::NEUTRAL_700);
    let select_style = format!(
        "width:100%; padding:8px 16px; border:1px solid {b}; border-radius:{RADIUS_SM}px; font-size:{size}; color:{fg}; background:{bg};",
        b = color::NEUTRAL_200,
        size = typography::BODY_SIZE,
        fg = color::NEUTRAL_800,
        bg = color::NEUTRAL_0
    );
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:6px; margin-bottom:16px;",
            if !label.is_empty() {
                label { style: "{label_style}", "{label}" }
            }
            select {
                value: "{value}",
                style: "{select_style}",
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
    let header_border = color::NEUTRAL_150;
    let title_style = format!("font-size:{}; font-weight:600; color:{};", typography::DELTA_SIZE, color::NEUTRAL_900);
    let close_color = color::NEUTRAL_500;
    rsx! {
        div {
            style: "position:fixed; inset:0; background:rgba(33,33,52,0.5); display:flex; align-items:flex-start; justify-content:center; padding:64px 24px; z-index:100;",
            onclick: move |e| on_close.call(e),
            div {
                style: "background:#fff; border-radius:8px; width:{w}px; max-width:100%; max-height:80vh; overflow:auto; display:flex; flex-direction:column; box-shadow:0 8px 24px rgba(33,33,52,0.2);",
                onclick: move |e| e.stop_propagation(),
                div { style: "display:flex; align-items:center; justify-content:space-between; padding:16px 24px; border-bottom:1px solid {header_border};",
                    span { style: "{title_style}", "{title}" }
                    button { style: "background:none; border:none; color:{close_color}; font-size:20px; cursor:pointer;", onclick: move |e| on_close.call(e), "✕" }
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
    let bg = if active { color::PRIMARY_100 } else { "transparent" };
    let fg = if active { color::PRIMARY_700 } else { color::NEUTRAL_700 };
    let style = format!(
        "display:flex; align-items:center; gap:10px; width:100%; padding:8px 12px; border:none; background:{bg}; color:{fg}; font-size:{size}; font-weight:600; border-radius:{r}px; cursor:pointer; text-align:left;",
        size = typography::BODY_SIZE,
        r = RADIUS_SM
    );
    rsx! {
        button {
            style: "{style}",
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
pub fn Table(
    columns: Vec<(String, String)>,
    rows: Vec<Vec<String>>,
) -> Element {
    let th_style = format!("text-align:left; padding:10px 16px; font-size:{}; font-weight:600; color:{};", typography::LABEL_SIZE, color::NEUTRAL_600);
    let td_style = format!("padding:10px 16px; font-size:{}; color:{};", typography::BODY_SIZE, color::NEUTRAL_800);
    let border = color::NEUTRAL_150;
    rsx! {
        table { style: "width:100%; border-collapse:collapse; background:#fff;",
            thead {
                tr { style: "border-bottom:1px solid {border};",
                    for (_, label) in columns.iter() {
                        th { style: "{th_style}", "{label}" }
                    }
                }
            }
            tbody {
                for row in rows.iter() {
                    tr { style: "border-bottom:1px solid {border};",
                        for cell in row.iter() {
                            td { style: "{td_style}", "{cell}" }
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
    let title_style = format!("font-size:{}; font-weight:600; color:{};", typography::DELTA_SIZE, color::NEUTRAL_800);
    let subtitle_style = format!("font-size:{}; color:{}; max-width:360px;", typography::BODY_SIZE, color::NEUTRAL_600);
    rsx! {
        div { style: "display:flex; flex-direction:column; align-items:center; justify-content:center; gap:12px; padding:48px; text-align:center;",
            icon::Icon { name: "{icon}", size: 40 }
            span { style: "{title_style}", "{title}" }
            if !subtitle.is_empty() {
                span { style: "{subtitle_style}", "{subtitle}" }
            }
            {children}
        }
    }
}
