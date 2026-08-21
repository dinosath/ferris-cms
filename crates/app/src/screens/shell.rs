//! App shell — global sidebar + routed main content area (design doc §3).

use dioxus::prelude::*;
use ui::design::tokens::{color, typography, SIDEBAR_WIDTH};

use crate::app::{use_global, Route};
use crate::components::{Breadcrumbs, Icon, NavItem, Toast};
use crate::screens::{
    content_manager, content_type_builder, credentials, executions, home, media, settings, workflow_editor,
    workflows,
};

#[component]
pub fn Shell() -> Element {
    let global = use_global();
    let route = global.route;
    let mut toasts = global.toasts;

    rsx! {
        div { style: "display:flex; min-height:100vh;",
            Sidebar {}
            div { style: "flex:1; min-width:0;",
                TopBar {}
                main { style: "min-width:0;",
                    match route() {
                        Route::Home => rsx! { home::Home {} },
                        Route::ContentTypeBuilder => rsx! { content_type_builder::ContentTypeBuilder {} },
                        Route::ContentTypeBuilderEditor(uid) => rsx! { content_type_builder::ContentTypeBuilderEditor { uid } },
                        Route::ContentManager => rsx! { content_manager::ContentManager {} },
                        Route::ContentManagerEntries(uid) => rsx! { content_manager::ContentManagerEntries { uid } },
                        Route::ContentManagerEntry { uid, document_id } => rsx! { content_manager::ContentManagerEntry { uid, document_id } },
                        Route::Media => rsx! { media::MediaLibrary {} },
                        Route::Workflows => rsx! { workflows::Workflows {} },
                        Route::WorkflowEditor(id) => rsx! { workflow_editor::WorkflowEditor { workflow_id: id } },
                        Route::WorkflowExecutions => rsx! { executions::Executions {} },
                        Route::Execution(id) => rsx! { executions::ExecutionDetail { execution_id: id } },
                        Route::Credentials => rsx! { credentials::Credentials {} },
                        Route::Settings => rsx! { settings::Settings {} },
                        _ => rsx! { home::Home {} },
                    }
                }
            }
        }
        div { style: "position:fixed; top:16px; right:16px; z-index:1000; display:flex; flex-direction:column; gap:8px;",
            for (idx, (msg, kind)) in toasts().into_iter().enumerate() {
                Toast {
                    text: msg,
                    kind,
                    on_close: move |_| {
                        let mut t = toasts();
                        if idx < t.len() { t.remove(idx); }
                        toasts.set(t);
                    },
                }
            }
        }
    }
}

/// A slim Strapi-style breadcrumb strip rendered above every authenticated
/// screen, giving a persistent sense of place in the admin hierarchy. Nested
/// routes (editor / entries / entry) resolve the content-type display name from
/// the shared `ct_names` registry to render "Home / Content Manager / Products".
#[component]
fn TopBar() -> Element {
    let global = use_global();
    let mut route = global.route;
    let ct_names_signal = global.ct_names;
    let ct_names = ct_names_signal();

    let mut crumbs: Vec<(String, bool)> = Vec::new();
    let mut nav: Vec<Route> = Vec::new();
    match route() {
        Route::ContentManager => {
            crumbs = vec![("Home".into(), false), ("Content Manager".into(), true)];
            nav = vec![Route::Home];
        }
        Route::ContentManagerEntries(uid) => {
            let name = ct_name(&ct_names, &uid);
            crumbs = vec![
                ("Home".into(), false),
                ("Content Manager".into(), false),
                (name, true),
            ];
            nav = vec![Route::Home, Route::ContentManager];
        }
        Route::ContentManagerEntry { uid, .. } => {
            let name = ct_name(&ct_names, &uid);
            crumbs = vec![
                ("Home".into(), false),
                ("Content Manager".into(), false),
                (name, true),
            ];
            nav = vec![Route::Home, Route::ContentManager];
        }
        Route::ContentTypeBuilder => {
            crumbs = vec![("Home".into(), false), ("Content-Type Builder".into(), true)];
            nav = vec![Route::Home];
        }
        Route::ContentTypeBuilderEditor(uid) => {
            let name = ct_name(&ct_names, &uid);
            crumbs = vec![
                ("Home".into(), false),
                ("Content-Type Builder".into(), false),
                (name, true),
            ];
            nav = vec![Route::Home, Route::ContentTypeBuilder];
        }
        Route::Media => {
            crumbs = vec![("Home".into(), false), ("Media Library".into(), true)];
            nav = vec![Route::Home];
        }
        Route::Workflows => {
            crumbs = vec![("Home".into(), false), ("Workflows".into(), true)];
            nav = vec![Route::Home];
        }
        Route::WorkflowEditor(_) => {
            crumbs = vec![("Home".into(), false), ("Workflow Editor".into(), true)];
            nav = vec![Route::Home];
        }
        Route::WorkflowExecutions => {
            crumbs = vec![("Home".into(), false), ("Workflow Executions".into(), true)];
            nav = vec![Route::Home];
        }
        Route::Execution(_) => {
            crumbs = vec![("Home".into(), false), ("Execution".into(), true)];
            nav = vec![Route::Home];
        }
        Route::Credentials => {
            crumbs = vec![("Home".into(), false), ("API / Integrations".into(), true)];
            nav = vec![Route::Home];
        }
        Route::Settings => {
            crumbs = vec![("Home".into(), false), ("Settings".into(), true)];
            nav = vec![Route::Home];
        }
        _ => {
            crumbs = vec![("Home".into(), false), ("Home".into(), true)];
            nav = vec![Route::Home];
        }
    }

    let bar_style = format!(
        "display:flex; align-items:center; height:56px; padding:0 32px; border-bottom:1px solid {}; background:{}; position:sticky; top:0; z-index:50;",
        color::NEUTRAL_150, color::NEUTRAL_0
    );
    rsx! {
        div { style: "{bar_style}",
            Breadcrumbs {
                crumbs,
                on_navigate: move |idx: usize| {
                    if let Some(r) = nav.get(idx).cloned() {
                        route.set(r);
                    }
                },
            }
        }
    }
}

/// Resolve a content type's display name from the shared (uid, name) registry.
fn ct_name(registry: &[(String, String)], uid: &str) -> String {
    registry
        .iter()
        .find(|(u, _)| u == uid)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| uid.to_string())
}

#[component]
fn Sidebar() -> Element {
    let global = use_global();
    let route = global.route;
    let active = |r: Route| route() == r;
    let mut g_cm = global.clone();
    let mut g_ctb = global.clone();
    let mut g_media = global.clone();
    let mut g_wf = global.clone();
    let mut g_exec = global.clone();
    let mut g_cred = global.clone();
    let mut g_settings = global.clone();
    let mut g_logout = global.clone();

    let sidebar_style = format!(
        "position:sticky; top:0; height:100vh; width:{SIDEBAR_WIDTH}px; min-width:{SIDEBAR_WIDTH}px; background:{}; border-right:1px solid {}; display:flex; flex-direction:column;",
        color::NEUTRAL_0,
        color::NEUTRAL_150
    );
    let brand_style = format!(
        "display:flex; align-items:center; gap:8px; padding:8px 16px; height:56px; border-bottom:1px solid {};",
        color::NEUTRAL_150
    );
    let brand_text = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::EPSILON_SIZE,
        color::NEUTRAL_900
    );
    let nav_style = "padding:12px; display:flex; flex-direction:column; gap:4px;";
    let section_label = format!(
        "padding:4px 16px; font-size:{}; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_500
    );
    let footer_style = format!(
        "display:flex; align-items:center; gap:10px; padding:8px 16px; height:56px; border-top:1px solid {};",
        color::NEUTRAL_150
    );
    let avatar_style = format!(
        "width:32px;height:32px;border-radius:50%;background:{}; display:flex; align-items:center; justify-content:center; color:{}; font-size:{}; font-weight:600;",
        color::PRIMARY_100, color::PRIMARY_600, typography::PI_SIZE
    );
    let user_style = format!(
        "flex:1; font-size:{}; color:{};",
        typography::BODY_SIZE,
        color::NEUTRAL_800
    );
    let logout_style = format!(
        "background:none; border:none; color:{}; cursor:pointer; display:flex; align-items:center;",
        color::NEUTRAL_500
    );

    rsx! {
        aside { style: "{sidebar_style}",
            div { style: "{brand_style}",
                div { style: "width:28px;height:28px;border-radius:6px;background:{color::PRIMARY_600};" }
                span { style: "{brand_text}", "ferriscms" }
            }
            div { style: "{nav_style}",
                NavItem { label: "Content Manager".to_string(), icon: "stack".to_string(), active: active(Route::ContentManager), onclick: move |_| g_cm.route.set(Route::ContentManager) }
                NavItem { label: "Content-Type Builder".to_string(), icon: "grid".to_string(), active: active(Route::ContentTypeBuilder), onclick: move |_| g_ctb.route.set(Route::ContentTypeBuilder) }
                NavItem { label: "Media Library".to_string(), icon: "image".to_string(), active: active(Route::Media), onclick: move |_| g_media.route.set(Route::Media) }
            }
            div { style: "margin:8px 16px; height:1px; background:{color::NEUTRAL_150};" }
            span { style: "{section_label}", "AUTOMATION" }
            div { style: "padding:0 12px 12px; display:flex; flex-direction:column; gap:4px;",
                NavItem { label: "Workflows".to_string(), icon: "layers".to_string(), active: active(Route::Workflows) || matches!(route(), Route::WorkflowEditor(_)), onclick: move |_| g_wf.route.set(Route::Workflows) }
                NavItem { label: "Executions".to_string(), icon: "list".to_string(), active: active(Route::WorkflowExecutions) || matches!(route(), Route::Execution(_)), onclick: move |_| g_exec.route.set(Route::WorkflowExecutions) }
                NavItem { label: "API / Integrations".to_string(), icon: "key".to_string(), active: active(Route::Credentials), onclick: move |_| g_cred.route.set(Route::Credentials) }
            }
            div { style: "margin:8px 16px; height:1px; background:{color::NEUTRAL_150};" }
            span { style: "{section_label}", "GENERAL" }
            div { style: "padding:0 12px 12px; display:flex; flex-direction:column; gap:4px;",
                NavItem { label: "Settings".to_string(), icon: "cog".to_string(), active: active(Route::Settings), onclick: move |_| g_settings.route.set(Route::Settings) }
            }
            div { style: "flex:1;" }
            div { style: "{footer_style}",
                div { style: "{avatar_style}", "AD" }
                span { style: "{user_style}", "Admin" }
                button {
                    style: "{logout_style}",
                    onclick: move |_| {
                        g_logout.set_token(None);
                        g_logout.route.set(Route::Login);
                    },
                    Icon { name: "external_link".to_string(), size: 18 }
                }
            }
        }
    }
}
