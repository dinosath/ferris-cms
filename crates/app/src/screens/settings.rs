//! Settings screen — RBAC Roles management (design doc §8).
//!
//! Lists the seeded admin roles (Super Admin / Editor / Author) and, for each,
//! renders the Strapi-style content-manager permission matrix: the five
//! explorer actions (create / read / update / delete / publish) scoped per
//! content-type. Editing a role toggles the matrix and persists via
//! `PUT /admin/roles/{id}/permissions`.

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::use_global;
use crate::components::{Badge, Button, Card, Dropdown, Modal, NavItem, TextField};

/// The five Strapi content-manager explorer actions, in display order.
const ACTIONS: [(&str, &str); 5] = [
    ("create", "Create"),
    ("read", "Read"),
    ("update", "Update"),
    ("delete", "Delete"),
    ("publish", "Publish"),
];

/// Full action key Strapi uses for the content-manager explorer.
fn action_key(action: &str) -> String {
    format!("plugin::content-manager.explorer.{action}")
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Roles,
    Users,
    ApiTokens,
    Locales,
}

#[component]
pub fn Settings() -> Element {
    let mut section = use_signal(|| Section::Roles);

    let sidebar_style = format!(
        "width:240px; min-width:240px; background:{}; border-right:1px solid {}; display:flex; flex-direction:column;",
        color::NEUTRAL_0, color::NEUTRAL_150
    );
    let header_style = format!(
        "padding:16px; font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let section_label = format!(
        "padding:4px 16px; font-size:{}; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_600
    );

    rsx! {
        div { style: "display:flex; min-height:100vh;",
            div { style: "{sidebar_style}",
                div { style: "{header_style}", "Settings" }
                span { style: "{section_label}", "GLOBAL SETTINGS" }
                NavItem { label: "Internationalization".to_string(), icon: "globe".to_string(), active: section() == Section::Locales, onclick: move |_| section.set(Section::Locales) }
                NavItem { label: "API Tokens".to_string(), icon: "key".to_string(), active: section() == Section::ApiTokens, onclick: move |_| section.set(Section::ApiTokens) }
                span { style: "{section_label}", "ADMINISTRATION PANEL" }
                NavItem { label: "Roles".to_string(), icon: "shield".to_string(), active: section() == Section::Roles, onclick: move |_| section.set(Section::Roles) }
                NavItem { label: "Users".to_string(), icon: "users".to_string(), active: section() == Section::Users, onclick: move |_| section.set(Section::Users) }
            }
            div { style: "flex:1; min-width:0; padding:32px;",
                match section() {
                    Section::Roles => rsx! { RolesSection {} },
                    Section::Users => rsx! { UsersSection {} },
                    Section::ApiTokens => rsx! { ApiTokensSection {} },
                    Section::Locales => rsx! { LocalesSection {} },
                }
            }
        }
    }
}

/// Internationalization locales list + create/delete (design doc §8.5).
#[component]
fn LocalesSection() -> Element {
    let global = use_global();
    let mut locales = use_signal(Vec::<serde_json::Value>::new);
    let mut loaded = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let mut show_create = use_signal(|| false);
    let mut locale_code = use_signal(String::new);

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let g = g_load.clone();
            spawn(async move {
                match g.client.i18n_list().await {
                    Ok(v) => locales.set(
                        v.get("data")
                            .and_then(|d| d.as_array())
                            .cloned()
                            .unwrap_or_default(),
                    ),
                    Err(e) => status.set(Some(format!("Failed to load locales: {e}"))),
                }
            });
        }
    });

    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let th_style = format!(
        "text-align:left; padding:10px 16px; font-size:{}; font-weight:600; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_600
    );
    let border = color::NEUTRAL_150;
    let status_style = format!("padding:12px; margin-bottom:16px; border-radius:4px; background:{}; color:{}; font-size:{};", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);
    let locale_list = locales();
    let g_create = global.clone();

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:16px;",
            div { style: "display:flex; align-items:center; justify-content:space-between;",
                div { style: "{title_style}", "Internationalization" }
                Button { label: "+ Add new locale".to_string(), variant: "primary".to_string(), on_click: move |_| show_create.set(true) }
            }
            if let Some(status) = status() {
                div { style: "{status_style}", "{status}" }
            }
            Card { padding: 0,
                table { style: "width:100%; border-collapse:collapse; background:#fff;",
                    thead {
                        tr { style: "border-bottom:1px solid {border};",
                            th { style: "{th_style}", "Display name" }
                            th { style: "{th_style}", "ISO code" }
                            th { style: "{th_style}", "Default" }
                        }
                    }
                    tbody {
                        for l in locale_list.into_iter() {
                            LocaleRow { locale: l }
                        }
                    }
                }
            }
        }

        if show_create() {
            Modal { title: "Add a new locale".to_string(), width: 640, on_close: move |_| show_create.set(false),
                TextField { value: "{locale_code}", label: "Locale code".to_string(), placeholder: "fr".to_string(), oninput: move |v| locale_code.set(v) }
                div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                    Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_create.set(false) }
                    Button { label: "Add".to_string(), variant: "primary".to_string(), on_click: move |_| {
                        let g = g_create.clone();
                        let req = api_types::admin::CreateLocaleRequest {
                            code: locale_code(),
                            name: None,
                            is_default: Some(false),
                        };
                        show_create.set(false);
                        spawn(async move {
                            if let Ok(v) = g.client.i18n_create(&req).await {
                                if let Some(l) = v.get("data").cloned() {
                                    locales.write().push(l);
                                }
                            }
                        });
                    } }
                }
            }
        }
    }
}

/// A single locale row.
#[component]
fn LocaleRow(locale: serde_json::Value) -> Element {
    let name = locale
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let code = locale
        .get("code")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let is_default = locale
        .get("isDefault")
        .and_then(|n| n.as_bool())
        .unwrap_or(false);
    let border = color::NEUTRAL_150;
    let td_style = format!(
        "padding:10px 16px; font-size:{}; color:{};",
        typography::BODY_SIZE,
        color::NEUTRAL_800
    );
    rsx! {
        tr { style: "border-bottom:1px solid {border};",
            td { style: "{td_style}", "{name}" }
            td { style: "{td_style}", "{code}" }
            td { style: "{td_style}",
                if is_default { Badge { text: "default".to_string(), kind: "published".to_string() } } else { span { "" } }
            }
        }
    }
}

/// API Tokens list + create/delete (design doc §8.4).
#[component]
fn ApiTokensSection() -> Element {
    let global = use_global();
    let mut tokens = use_signal(Vec::<serde_json::Value>::new);
    let mut loaded = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let mut show_create = use_signal(|| false);
    let mut token_name = use_signal(String::new);
    let mut token_type = use_signal(|| "read-only".to_string());

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let g = g_load.clone();
            spawn(async move {
                match g.client.api_tokens_list().await {
                    Ok(v) => tokens.set(
                        v.get("data")
                            .and_then(|d| d.as_array())
                            .cloned()
                            .unwrap_or_default(),
                    ),
                    Err(e) => status.set(Some(format!("Failed to load API tokens: {e}"))),
                }
            });
        }
    });

    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let th_style = format!(
        "text-align:left; padding:10px 16px; font-size:{}; font-weight:600; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_600
    );
    let border = color::NEUTRAL_150;
    let status_style = format!("padding:12px; margin-bottom:16px; border-radius:4px; background:{}; color:{}; font-size:{};", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);
    let token_list = tokens();
    let g_create = global.clone();

    let token_type_options: Vec<(String, String)> = vec![
        ("read-only".to_string(), "Read-only".to_string()),
        ("full-access".to_string(), "Full access".to_string()),
        ("custom".to_string(), "Custom".to_string()),
    ];

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:16px;",
            div { style: "display:flex; align-items:center; justify-content:space-between;",
                div { style: "{title_style}", "API Tokens" }
                Button { label: "+ Create new API token".to_string(), variant: "primary".to_string(), on_click: move |_| show_create.set(true) }
            }
            if let Some(status) = status() {
                div { style: "{status_style}", "{status}" }
            }
            Card { padding: 0,
                table { style: "width:100%; border-collapse:collapse; background:#fff;",
                    thead {
                        tr { style: "border-bottom:1px solid {border};",
                            th { style: "{th_style}", "Name" }
                            th { style: "{th_style}", "Description" }
                            th { style: "{th_style}", "Type" }
                        }
                    }
                    tbody {
                        for t in token_list.into_iter() {
                            ApiTokenRow { token: t }
                        }
                    }
                }
            }
        }

        if show_create() {
            Modal { title: "Create a new API token".to_string(), width: 640, on_close: move |_| show_create.set(false),
                TextField { value: "{token_name}", label: "Name".to_string(), placeholder: "My API token".to_string(), oninput: move |v| token_name.set(v) }
                Dropdown {
                    label: "Token type".to_string(),
                    options: token_type_options,
                    value: "{token_type}",
                    onchange: move |v| token_type.set(v),
                }
                div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                    Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_create.set(false) }
                    Button { label: "Create".to_string(), variant: "primary".to_string(), on_click: move |_| {
                        let g = g_create.clone();
                        let ty = match token_type().as_str() {
                            "full-access" => core_domain::ApiTokenType::FullAccess,
                            "custom" => core_domain::ApiTokenType::Custom,
                            _ => core_domain::ApiTokenType::ReadOnly,
                        };
                        let req = api_types::admin::CreateApiTokenRequest {
                            name: token_name(),
                            description: None,
                            token_type: ty,
                            lifespan: None,
                            permissions: vec![],
                        };
                        show_create.set(false);
                        spawn(async move {
                            if let Ok(v) = g.client.api_token_create(&req).await {
                                if let Some(t) = v.get("data").cloned() {
                                    tokens.write().push(t);
                                }
                            }
                        });
                    } }
                }
            }
        }
    }
}

/// A single API token row.
#[component]
fn ApiTokenRow(token: serde_json::Value) -> Element {
    let name = token
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let desc = token
        .get("description")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let ty = token
        .get("type")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let border = color::NEUTRAL_150;
    let td_style = format!(
        "padding:10px 16px; font-size:{}; color:{};",
        typography::BODY_SIZE,
        color::NEUTRAL_800
    );
    rsx! {
        tr { style: "border-bottom:1px solid {border};",
            td { style: "{td_style}", "{name}" }
            td { style: "{td_style}", "{desc}" }
            td { style: "{td_style}", "{ty}" }
        }
    }
}

/// Users list + invite-new-user modal.
#[component]
fn UsersSection() -> Element {
    let global = use_global();
    let mut users = use_signal(Vec::<serde_json::Value>::new);
    let mut loaded = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let mut show_invite = use_signal(|| false);
    let mut invite_email = use_signal(String::new);
    let mut invite_first = use_signal(String::new);
    let mut invite_last = use_signal(String::new);

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let g = g_load.clone();
            spawn(async move {
                match g.client.users_list().await {
                    Ok(v) => {
                        users.set(
                            v.get("data")
                                .and_then(|d| d.as_array())
                                .cloned()
                                .unwrap_or_default(),
                        );
                    }
                    Err(e) => status.set(Some(format!("Failed to load users: {e}"))),
                }
            });
        }
    });

    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let th_style = format!(
        "text-align:left; padding:10px 16px; font-size:{}; font-weight:600; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_600
    );
    let border = color::NEUTRAL_150;
    let status_style = format!("padding:12px; margin-bottom:16px; border-radius:4px; background:{}; color:{}; font-size:{};", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);
    let user_list = users();
    let g_invite = global.clone();

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:16px;",
            div { style: "display:flex; align-items:center; justify-content:space-between;",
                div { style: "{title_style}", "Users" }
                Button { label: "+ Invite new user".to_string(), variant: "primary".to_string(), on_click: move |_| show_invite.set(true) }
            }
            if let Some(status) = status() {
                div { style: "{status_style}", "{status}" }
            }
            Card { padding: 0,
                table { style: "width:100%; border-collapse:collapse; background:#fff;",
                    thead {
                        tr { style: "border-bottom:1px solid {border};",
                            th { style: "{th_style}", "Email" }
                            th { style: "{th_style}", "First name" }
                            th { style: "{th_style}", "Last name" }
                            th { style: "{th_style}", "Active" }
                        }
                    }
                    tbody {
                        for u in user_list.into_iter() {
                            UserRow { user: u }
                        }
                    }
                }
            }
        }

        if show_invite() {
            Modal { title: "Invite a new user".to_string(), width: 640, on_close: move |_| show_invite.set(false),
                TextField { value: "{invite_email}", label: "Email".to_string(), placeholder: "kai@doe.com".to_string(), oninput: move |v| invite_email.set(v) }
                TextField { value: "{invite_first}", label: "First name".to_string(), placeholder: "Kai".to_string(), oninput: move |v| invite_first.set(v) }
                TextField { value: "{invite_last}", label: "Last name".to_string(), placeholder: "Doe".to_string(), oninput: move |v| invite_last.set(v) }
                div { style: "display:flex; justify-content:flex-end; gap:12px; padding-top:8px;",
                    Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_invite.set(false) }
                    Button { label: "Invite".to_string(), variant: "primary".to_string(), on_click: move |_| {
                        let g = g_invite.clone();
                        let req = api_types::admin::CreateAdminUserRequest {
                            email: invite_email(),
                            firstname: Some(invite_first()),
                            lastname: Some(invite_last()),
                            password: Some("TemporaryPass123!".to_string()),
                            roles: vec![],
                            is_active: Some(true),
                        };
                        show_invite.set(false);
                        spawn(async move {
                            if let Ok(v) = g.client.user_create(&req).await {
                                if let Ok(u) = serde_json::from_value(v.get("data").cloned().unwrap_or(serde_json::Value::Null)) {
                                    users.write().push(u);
                                }
                            }
                        });
                    } }
                }
            }
        }
    }
}

/// A single user row.
#[component]
fn UserRow(user: serde_json::Value) -> Element {
    let email = user
        .get("email")
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_string();
    let first = user
        .get("firstname")
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_string();
    let last = user
        .get("lastname")
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_string();
    let active = user
        .get("isActive")
        .and_then(|a| a.as_bool())
        .unwrap_or(false);
    let border = color::NEUTRAL_150;
    let td_style = format!(
        "padding:10px 16px; font-size:{}; color:{};",
        typography::BODY_SIZE,
        color::NEUTRAL_800
    );
    rsx! {
        tr { style: "border-bottom:1px solid {border};",
            td { style: "{td_style}", "{email}" }
            td { style: "{td_style}", "{first}" }
            td { style: "{td_style}", "{last}" }
            td { style: "{td_style}",
                Badge { text: if active { "Active".to_string() } else { "Inactive".to_string() }, kind: if active { "published".to_string() } else { "draft".to_string() } }
            }
        }
    }
}

/// Roles list + per-role permission matrix.
#[component]
fn RolesSection() -> Element {
    let global = use_global();
    let mut roles = use_signal(Vec::<serde_json::Value>::new);
    let mut loaded = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let mut editing_role = use_signal(|| None::<serde_json::Value>);
    let mut matrices = use_signal(
        std::collections::HashMap::<String, std::collections::HashMap<String, bool>>::new,
    );

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let g = g_load.clone();
            spawn(async move {
                match g.client.roles_list().await {
                    Ok(v) => {
                        let list: Vec<serde_json::Value> = v
                            .get("data")
                            .and_then(|d| d.as_array())
                            .cloned()
                            .unwrap_or_default();
                        // Prefill each role's matrix based on role code.
                        let mut m = std::collections::HashMap::<
                            String,
                            std::collections::HashMap<String, bool>,
                        >::new();
                        for r in &list {
                            let code = r
                                .get("code")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string();
                            let mut actions = std::collections::HashMap::<String, bool>::new();
                            for (act, _) in ACTIONS {
                                actions.insert(act.to_string(), matrix_default(&code, act));
                            }
                            m.insert(
                                r.get("id").map(|i| i.to_string()).unwrap_or_default(),
                                actions,
                            );
                        }
                        matrices.set(m);
                        roles.set(list);
                    }
                    Err(e) => status.set(Some(format!("Failed to load roles: {e}"))),
                }
            });
        }
    });

    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let th_style = format!(
        "text-align:left; padding:10px 16px; font-size:{}; font-weight:600; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_600
    );
    let border = color::NEUTRAL_150;
    let status_style = format!("padding:12px; margin-bottom:16px; border-radius:4px; background:{}; color:{}; font-size:{};", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);

    let role_list = roles();

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:16px;",
            div { style: "{title_style}", "Roles" }
            if let Some(status) = status() {
                div { style: "{status_style}", "{status}" }
            }
            Card { padding: 0,
                table { style: "width:100%; border-collapse:collapse; background:#fff;",
                    thead {
                        tr { style: "border-bottom:1px solid {border};",
                            th { style: "{th_style}", "Name" }
                            th { style: "{th_style}", "Description" }
                            th { style: "{th_style}", "Permissions" }
                        }
                    }
                    tbody {
                        for role in role_list.into_iter() {
                            RoleRow {
                                role,
                                on_configure: move |role| editing_role.set(Some(role)),
                            }
                        }
                    }
                }
            }
        }

        if let Some(role) = editing_role() {
            RoleEditorModal {
                role,
                matrix: matrices(),
                on_close: move |_| editing_role.set(None),
                on_change: move |(rid, act, on): (String, String, bool)| {
                    matrices.write().entry(rid).or_default().insert(act, on);
                },
            }
        }
    }
}

/// Default permission matrix for a role code (mirrors the backend grant set).
fn matrix_default(code: &str, action: &str) -> bool {
    match code {
        "strapi-super-admin" => true,
        "strapi-editor" => true,
        "strapi-author" => matches!(action, "create" | "read" | "update"),
        _ => action == "read",
    }
}

/// A single role row in the roles table.
#[component]
fn RoleRow(role: serde_json::Value, on_configure: EventHandler<serde_json::Value>) -> Element {
    let name = role
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let desc = role
        .get("description")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let border = color::NEUTRAL_150;
    let td_style = format!(
        "padding:10px 16px; font-size:{}; color:{};",
        typography::BODY_SIZE,
        color::NEUTRAL_800
    );
    rsx! {
        tr { style: "border-bottom:1px solid {border};",
            td { style: "{td_style}", "{name}" }
            td { style: "{td_style}", "{desc}" }
            td { style: "{td_style}",
                Button { label: "Configure".to_string(), variant: "secondary".to_string(),
                    on_click: move |_| on_configure.call(role.clone()),
                }
            }
        }
    }
}

/// A single permission-matrix row (one explorer action).
#[component]
fn PermissionRow(
    action: String,
    rid: String,
    matrix: std::collections::HashMap<String, std::collections::HashMap<String, bool>>,
    on_change: EventHandler<(String, String, bool)>,
) -> Element {
    let checked = matrix
        .get(&rid)
        .and_then(|m| m.get(&action))
        .copied()
        .unwrap_or(false);
    let border = color::NEUTRAL_150;
    let td_style = format!(
        "padding:8px 12px; font-size:{}; color:{};",
        typography::BODY_SIZE,
        color::NEUTRAL_800
    );
    let row_rid = rid.clone();
    rsx! {
        tr { style: "border-bottom:1px solid {border};",
            td { style: "{td_style}", "{action}" }
            td { style: "{td_style}",
                input { r#type: "checkbox", checked: checked,
                    onchange: move |e| on_change.call((row_rid.clone(), action.clone(), e.checked())),
                }
            }
        }
    }
}

/// Permission-matrix editing modal for one role.
#[component]
fn RoleEditorModal(
    role: serde_json::Value,
    matrix: std::collections::HashMap<String, std::collections::HashMap<String, bool>>,
    on_close: EventHandler<()>,
    on_change: EventHandler<(String, String, bool)>,
) -> Element {
    let global = use_global();
    let rid = role.get("id").map(|i| i.to_string()).unwrap_or_default();
    let name = role
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let th_style = format!(
        "padding:8px 12px; font-size:{}; font-weight:600; color:{};",
        typography::LABEL_SIZE,
        color::NEUTRAL_600
    );
    let border = color::NEUTRAL_150;
    let actions = ACTIONS.map(|(k, _)| k.to_string()).to_vec();
    let save_rid = rid.clone();
    let g_save = global.clone();
    rsx! {
        Modal { title: format!("Permissions — {name}"), width: 640, on_close: move |_| on_close.call(()),
            table { style: "width:100%; border-collapse:collapse; margin-bottom:16px;",
                thead {
                    tr { style: "border-bottom:1px solid {border};",
                        th { style: "{th_style}", "Action" }
                        th { style: "{th_style}", "All content types" }
                    }
                }
                tbody {
                    for act in actions.into_iter() {
                        PermissionRow {
                            action: act,
                            rid: rid.clone(),
                            matrix: matrix.clone(),
                            on_change: on_change.clone(),
                        }
                    }
                }
            }
            div { style: "display:flex; justify-content:flex-end; gap:12px;",
                Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| on_close.call(()) }
                Button { label: "Save".to_string(), variant: "primary".to_string(),
                    on_click: move |_| {
                        let g = g_save.clone();
                        let rid = save_rid.clone();
                        let m = matrix.clone();
                        spawn(async move {
                            let perms: Vec<serde_json::Value> = m.get(&rid)
                                .map(|m| m.iter().filter(|(_, v)| **v).map(|(act, _)| serde_json::json!({
                                    "action": action_key(act),
                                    "subject": "*",
                                    "properties": {},
                                    "conditions": [],
                                })).collect())
                                .unwrap_or_default();
                            if let Ok(id) = rid.parse::<i64>() {
                                let _ = g.client.role_update_permissions(id, &serde_json::Value::Array(perms)).await;
                            }
                            on_close.call(());
                        });
                    }
                }
            }
        }
    }
}
