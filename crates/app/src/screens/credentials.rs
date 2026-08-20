//! API / Integrations — credential management screen.
//!
//! Credentials are stored encrypted on the server and never returned to the
//! client. This screen lists them, creates new ones (type + a JSON data blob),
//! and deletes them. They are referenced by integration nodes in workflows.

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::use_global;
use crate::components::{Badge, Button, Card, ConfirmDialog, EmptyState, IconButton, Modal, Spinner, TextArea, TextField};

/// The API / Integrations (credential management) screen.
#[component]
pub fn Credentials() -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut items: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut types: Signal<Vec<(String, String)>> = use_signal(|| vec![]);
    let mut loading = use_signal(|| true);
    let mut show_create = use_signal(|| false);
    let mut creating = use_signal(|| false);
    let mut name = use_signal(|| String::new());
    let mut credential_type = use_signal(|| String::new());
    let mut data_json = use_signal(|| String::from("{\n  \"headerName\": \"Authorization\",\n  \"headerValue\": \"Bearer ...\"\n}"));
    let mut to_delete: Signal<Option<i64>> = use_signal(|| None);

    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            spawn(async move {
                if let Ok(v) = client.credential_list().await {
                    items.set(v["data"].as_array().cloned().unwrap_or_default());
                }
                if let Ok(t) = client.credential_types().await {
                    let opts = t["data"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| {
                            let key = x[0].as_str().unwrap_or("").to_string();
                            let label = x[1].as_str().unwrap_or("").to_string();
                            if key.is_empty() { None } else { Some((key, label)) }
                        }).collect())
                        .unwrap_or_default();
                    types.set(opts);
                    if let Some((k, _)) = types().first() {
                        credential_type.set(k.clone());
                    }
                }
                loading.set(false);
            });
        }
    });

    let g_modal = global.clone();
    let g_delete = global.clone();
    let client_modal = client.clone();
    let client_delete = client.clone();

    let mut rows: Vec<Element> = Vec::new();
    for item in items().iter() {
        let id = item["id"].as_i64().unwrap_or(0);
        let cname = item["name"].as_str().unwrap_or("").to_string();
        let ctype = item["credentialType"].as_str().unwrap_or("").to_string();
        let mut del = to_delete;
        rows.push(rsx! {
            tr { style: "border-bottom:1px solid {color::NEUTRAL_150};",
                td { style: "padding:12px 16px; font-size:14px; color:{color::NEUTRAL_900}; font-weight:600;", "{cname}" }
                td { style: "padding:12px 16px;", Badge { text: ctype.clone(), kind: "neutral".to_string() } }
                td { style: "padding:12px 16px; font-size:13px; color:{color::NEUTRAL_600};", "🔒 Encrypted" }
                td { style: "padding:12px 16px;",
                    IconButton { name: "trash".to_string(), variant: "danger".to_string(), aria_label: "Delete".to_string(), on_click: move |_| del.set(Some(id)) }
                }
            }
        });
    }

    rsx! {
        div { style: "padding:32px; max-width:1000px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:24px;",
                div { style: "display:flex; flex-direction:column; gap:4px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "API / Integrations" }
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Manage credentials used by integration nodes. Values are encrypted at rest and never exposed." }
                }
                Button { label: "Add Credential".to_string(), on_click: move |_| show_create.set(true) }
            }

            if loading() {
                div { style: "display:flex; justify-content:center; padding:48px;", Spinner { size: 28 } }
            } else if items().is_empty() {
                EmptyState {
                    title: "No credentials yet".to_string(),
                    subtitle: "Add an API key or header credential to use HTTP / integration nodes in your workflows.".to_string(),
                    icon: "key".to_string(),
                    Button { label: "Add Credential".to_string(), on_click: move |_| show_create.set(true) }
                }
            } else {
                Card {
                    header: format!("{} credentials", items().len()),
                    table { style: "width:100%; border-collapse:collapse;",
                        thead {
                            tr {
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Name" }
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Type" }
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Storage" }
                                th { style: "text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600}; background:{color::NEUTRAL_100}; border-bottom:1px solid {color::NEUTRAL_150};", "Actions" }
                            }
                        }
                        tbody { {rows.into_iter()} }
                    }
                }
            }
        }

        if show_create() {
            Modal {
                title: "Add Credential".to_string(),
                on_close: move |_| show_create.set(false),
                div { style: "display:flex; flex-direction:column; gap:16px;",
                    TextField {
                        value: name(),
                        label: "Name".to_string(),
                        placeholder: "e.g. Stripe API".to_string(),
                        oninput: move |v| name.set(v),
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        label { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_700};", "Type" }
                        select { style: "width:100%; height:40px; padding:0 14px; border:1px solid {color::NEUTRAL_200}; border-radius:4px; font-size:14px; color:{color::NEUTRAL_800}; background:#fff;",
                            value: credential_type(),
                            onchange: move |e| credential_type.set(e.value()),
                            for (k, l) in types() {
                                option { value: "{k}", "{l}" }
                            }
                        }
                    }
                    TextArea {
                        value: data_json(),
                        label: "Credential data (JSON)".to_string(),
                        rows: 6,
                        hint: "e.g. {\"headerName\":\"Authorization\",\"headerValue\":\"Bearer <token>\"}".to_string(),
                        oninput: move |v| data_json.set(v),
                    }
                    div { style: "display:flex; justify-content:flex-end; gap:12px;",
                        Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_create.set(false) }
                        Button {
                            label: "Save".to_string(),
                            loading: creating(),
                            disabled: name().trim().is_empty() || credential_type().is_empty(),
                            on_click: move |_| {
                                let cname = name();
                                let ctype = credential_type();
                                let data = serde_json::from_str::<serde_json::Value>(&data_json()).unwrap_or(serde_json::json!({}));
                                let client = client_modal.clone();
                                let mut g = g_modal.clone();
                                let mut creating2 = creating;
                                let mut its = items;
                                let mut show = show_create;
                                spawn(async move {
                                    creating2.set(true);
                                    match client.credential_create(&cname, &ctype, &data).await {
                                        Ok(_) => {
                                            if let Ok(v) = client.credential_list().await {
                                                its.set(v["data"].as_array().cloned().unwrap_or_default());
                                            }
                                            g.toast("Credential saved", "success");
                                        }
                                        Err(e) => g.toast(format!("Save failed: {e}"), "danger"),
                                    }
                                    creating2.set(false);
                                    show.set(false);
                                });
                            }
                        }
                    }
                }
            }
        }

        if let Some(id) = to_delete() {
            ConfirmDialog {
                title: "Delete Credential".to_string(),
                message: "Delete this credential? Workflow nodes using it will no longer be able to authenticate.".to_string(),
                confirm_label: "Delete".to_string(),
                on_cancel: move |_| to_delete.set(None),
                on_confirm: move |_| {
                    let client = client_delete.clone();
                    let mut g = g_delete.clone();
                    let mut its = items;
                    spawn(async move {
                        if client.credential_delete(id).await.is_ok() {
                            if let Ok(v) = client.credential_list().await {
                                its.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast("Credential deleted", "success");
                        }
                        to_delete.set(None);
                    });
                },
            }
        }
    }
}
