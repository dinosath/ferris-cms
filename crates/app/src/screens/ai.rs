//! AI screens: Assistant (chat) and AI Settings (providers/models/usage).

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::use_global;
use crate::components::{
    Badge, Button, Card, ConfirmDialog, Dropdown, EmptyState, Modal, Spinner, Table, TextArea,
    TextField,
};

/// Kinds offered when creating a provider.
const PROVIDER_KINDS: &[(&str, &str)] = &[
    ("openai", "OpenAI-compatible"),
    ("ollama", "Ollama (local)"),
    ("anthropic", "Anthropic"),
    ("gemini", "Google Gemini"),
];

/// localStorage key holding the last active assistant conversation so history
/// is restored across reloads (web only).
#[allow(dead_code)]
const CONV_STORAGE_KEY: &str = "ferriscms_last_ai_conversation";

/// Read the persisted last conversation id (web only). `None` on non-wasm.
#[allow(dead_code)]
fn load_last_conversation() -> Option<i64> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(CONV_STORAGE_KEY).ok().flatten())
            .and_then(|v| v.parse::<i64>().ok())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Persist (or clear) the last conversation id (web only). No-op off-wasm.
#[allow(dead_code)]
fn save_last_conversation(id: Option<i64>) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(w) = web_sys::window() {
            if let Ok(Some(storage)) = w.local_storage() {
                match id {
                    Some(id) => {
                        let _ = storage.set_item(CONV_STORAGE_KEY, &id.to_string());
                    }
                    None => {
                        let _ = storage.remove_item(CONV_STORAGE_KEY);
                    }
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = id;
    }
}

// ===========================================================================
// AI Settings
// ===========================================================================

#[component]
pub fn AiSettings() -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut providers: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut models: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut usage: Signal<serde_json::Value> = use_signal(|| serde_json::json!({}));
    let mut loading = use_signal(|| true);

    let mut show_provider = use_signal(|| false);
    let mut provider_name = use_signal(|| String::new());
    let mut provider_kind = use_signal(|| "openai".to_string());
    let mut base_url = use_signal(|| String::new());
    let mut api_key = use_signal(|| String::new());
    let mut provider_enabled = use_signal(|| true);
    // Provider connection test state.
    let mut conn_status: Signal<Option<(bool, String)>> = use_signal(|| None);
    let mut discovered_models: Signal<Vec<String>> = use_signal(|| Vec::new());
    let mut conn_busy = use_signal(|| false);
    let mut test_req: Signal<Option<api_types::AiProviderCreate>> = use_signal(|| None);

    let mut show_model = use_signal(|| false);
    let mut model_provider = use_signal(|| 0i64);
    let mut model_name = use_signal(|| String::new());
    let mut model_tools = use_signal(|| true);

    let mut provider_req: Signal<Option<(String, String, String, String, bool)>> = use_signal(|| None);
    let mut delete_provider_req: Signal<Option<i64>> = use_signal(|| None);
    let mut model_req: Signal<Option<(i64, String, bool)>> = use_signal(|| None);
    let mut delete_model_req: Signal<Option<i64>> = use_signal(|| None);

    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            let mut ps = providers;
            let mut ms = models;
            let mut us = usage;
            let mut ld = loading;
            spawn(async move {
                if let Ok(v) = client.ai_providers().await {
                    ps.set(v["data"].as_array().cloned().unwrap_or_default());
                }
                if let Ok(v) = client.ai_models(None).await {
                    ms.set(v["data"].as_array().cloned().unwrap_or_default());
                }
                if let Ok(v) = client.ai_usage_summary().await {
                    us.set(v["data"].clone());
                }
                ld.set(false);
            });
        }
    });

    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if let Some((name, kind, url, key, enabled)) = provider_req() {
                provider_req.set(None);
                let client = client.clone();
                let mut g = g.clone();
                let mut ps = providers;
                let mut ms = models;
                let mut ld = loading;
                spawn(async move {
                    let body = api_types::AiProviderCreate {
                        name,
                        kind,
                        base_url: if url.is_empty() { None } else { Some(url) },
                        api_key: if key.is_empty() { None } else { Some(key) },
                        organization: None,
                        enabled,
                        sort_order: None,
                    };
                    match client.ai_provider_create(&body).await {
                        Ok(_) => {
                            if let Ok(v) = client.ai_providers().await {
                                ps.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            // A default model is created automatically with the
                            // provider, so refresh the models table too.
                            if let Ok(v) = client.ai_models(None).await {
                                ms.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast("AI provider saved", "success");
                        }
                        Err(e) => g.toast(format!("Save failed: {e}"), "danger"),
                    }
                    ld.set(false);
                });
            }
        }
    });

    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if let Some(id) = delete_provider_req() {
                delete_provider_req.set(None);
                let client = client.clone();
                let mut g = g.clone();
                let mut ps = providers;
                spawn(async move {
                    match client.ai_provider_delete(id).await {
                        Ok(_) => {
                            if let Ok(v) = client.ai_providers().await {
                                ps.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast("Provider deleted", "success");
                        }
                        Err(e) => g.toast(format!("Delete failed: {e}"), "danger"),
                    }
                });
            }
        }
    });

    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if let Some((pid, name, tools)) = model_req() {
                model_req.set(None);
                let client = client.clone();
                let mut g = g.clone();
                let mut ms = models;
                spawn(async move {
                    let body = api_types::AiModelCreate {
                        provider_id: pid,
                        name,
                        display_name: None,
                        description: None,
                        supports_chat: true,
                        supports_tools: tools,
                        supports_streaming: false,
                        max_input_tokens: None,
                        max_output_tokens: None,
                        enabled: true,
                    };
                    match client.ai_model_create(&body).await {
                        Ok(_) => {
                            if let Ok(v) = client.ai_models(None).await {
                                ms.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast("AI model saved", "success");
                        }
                        Err(e) => g.toast(format!("Save failed: {e}"), "danger"),
                    }
                });
            }
        }
    });

    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if let Some(id) = delete_model_req() {
                delete_model_req.set(None);
                let client = client.clone();
                let mut g = g.clone();
                let mut ms = models;
                spawn(async move {
                    match client.ai_model_delete(id).await {
                        Ok(_) => {
                            if let Ok(v) = client.ai_models(None).await {
                                ms.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            g.toast("Model deleted", "success");
                        }
                        Err(e) => g.toast(format!("Delete failed: {e}"), "danger"),
                    }
                });
            }
        }
    });

    // Test a provider connection (connectivity + discovered models).
    use_effect({
        let client = client.clone();
        move || {
            if let Some(body) = test_req() {
                test_req.set(None);
                let client = client.clone();
                let mut st = conn_status;
                let mut dm = discovered_models;
                let mut busy = conn_busy;
                busy.set(true);
                spawn(async move {
                    match client.ai_test_connection(&body).await {
                        Ok(v) => {
                            let models: Vec<String> = v["data"]["models"]
                                .as_array()
                                .map(|a| a.iter().filter_map(|m| m.as_str().map(String::from)).collect())
                                .unwrap_or_default();
                            dm.set(models);
                            st.set(Some((true, "Connected".to_string())));
                        }
                        Err(e) => {
                            dm.set(Vec::new());
                            st.set(Some((false, format!("Connection failed: {e}"))));
                        }
                    }
                    busy.set(false);
                });
            }
        }
    });

    let provider_rows: Vec<Vec<String>> = providers()
        .iter()
        .map(|p| {
            vec![
                p["name"].as_str().unwrap_or("").to_string(),
                p["kind"].as_str().unwrap_or("").to_string(),
                p["baseUrl"].as_str().unwrap_or("(default)").to_string(),
                if p["enabled"].as_bool().unwrap_or(false) {
                    "Enabled".to_string()
                } else {
                    "Disabled".to_string()
                },
                p["id"].as_i64().unwrap_or(0).to_string(),
            ]
        })
        .collect();

    let model_rows: Vec<Vec<String>> = models()
        .iter()
        .map(|m| {
            vec![
                m["providerId"].as_i64().unwrap_or(0).to_string(),
                m["name"].as_str().unwrap_or("").to_string(),
                if m["supportsTools"].as_bool().unwrap_or(false) {
                    "Tools".to_string()
                } else {
                    "—".to_string()
                },
                if m["enabled"].as_bool().unwrap_or(false) {
                    "Enabled".to_string()
                } else {
                    "Disabled".to_string()
                },
                m["id"].as_i64().unwrap_or(0).to_string(),
            ]
        })
        .collect();

    let provider_delete_buttons: Vec<Element> = providers()
        .iter()
        .map(|p| {
            let id = p["id"].as_i64().unwrap_or(0);
            rsx! {
                div { key: "pd-{id}", style: "display:flex; gap:8px; margin-top:8px;",
                    Button { label: "Delete".to_string(), variant: "danger".to_string(), size: "sm".to_string(), on_click: move |_| delete_provider_req.set(Some(id)) }
                }
            }
        })
        .collect();

    let model_delete_buttons: Vec<Element> = models()
        .iter()
        .map(|m| {
            let id = m["id"].as_i64().unwrap_or(0);
            rsx! {
                div { key: "md-{id}", style: "display:flex; gap:8px; margin-top:8px;",
                    Button { label: "Delete".to_string(), variant: "danger".to_string(), size: "sm".to_string(), on_click: move |_| delete_model_req.set(Some(id)) }
                }
            }
        })
        .collect();

    let provider_conn_color = match conn_status() {
        Some((true, _)) => color::SUCCESS_600.to_string(),
        Some((false, _)) => color::DANGER_600.to_string(),
        None => "inherit".to_string(),
    };

    rsx! {
        div { style: "padding:32px; max-width:1100px; display:flex; flex-direction:column; gap:24px;",
            div { style: "display:flex; flex-direction:column; gap:2px;",
                span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "AI Settings" }
                span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Configure LLM providers, models, and review usage." }
            }

            // Usage summary
            Card { header: "Usage summary".to_string(),
                div { style: "display:flex; gap:32px;",
                    Stat { label: "Requests".to_string(), value: usage()["requests"].as_i64().unwrap_or(0).to_string() }
                    Stat { label: "Input tokens".to_string(), value: usage()["inputTokens"].as_i64().unwrap_or(0).to_string() }
                    Stat { label: "Output tokens".to_string(), value: usage()["outputTokens"].as_i64().unwrap_or(0).to_string() }
                    Stat { label: "Total tokens".to_string(), value: usage()["totalTokens"].as_i64().unwrap_or(0).to_string() }
                }
            }

            // Providers
            Card { header: "AI providers".to_string(),
                div { style: "display:flex; justify-content:flex-end; margin-bottom:12px;",
                    Button { label: "+ New provider".to_string(), on_click: move |_| {
                        provider_name.set(String::new());
                        provider_kind.set("openai".to_string());
                        base_url.set(String::new());
                        api_key.set(String::new());
                        provider_enabled.set(true);
                        show_provider.set(true);
                    } }
                }
                if loading() {
                    Spinner {}
                } else if provider_rows.is_empty() {
                    EmptyState { title: "No AI providers".to_string(), subtitle: "Add a provider to enable AI features.".to_string(), icon: "puzzle".to_string() }
                } else {
                    Table {
                        columns: vec![
                            ("Name".to_string(), "Name".to_string()),
                            ("Kind".to_string(), "Kind".to_string()),
                            ("Base URL".to_string(), "Base URL".to_string()),
                            ("Status".to_string(), "Status".to_string()),
                            ("ID".to_string(), "ID".to_string()),
                        ],
                        rows: provider_rows.clone(),
                    }
                    {provider_delete_buttons.into_iter()}
                }
            }

            // Models
            Card { header: "AI models".to_string(),
                div { style: "display:flex; justify-content:flex-end; margin-bottom:12px;",
                    Button { label: "+ Add model".to_string(), on_click: move |_| {
                        model_provider.set(providers().first().and_then(|p| p["id"].as_i64()).unwrap_or(0));
                        model_name.set(String::new());
                        model_tools.set(true);
                        show_model.set(true);
                    } }
                }
                if model_rows.is_empty() {
                    EmptyState { title: "No models".to_string(), subtitle: "Add a model to one of your providers.".to_string(), icon: "list".to_string() }
                } else {
                    Table {
                        columns: vec![
                            ("Provider".to_string(), "Provider".to_string()),
                            ("Name".to_string(), "Name".to_string()),
                            ("Capabilities".to_string(), "Capabilities".to_string()),
                            ("Status".to_string(), "Status".to_string()),
                            ("ID".to_string(), "ID".to_string()),
                        ],
                        rows: model_rows.clone(),
                    }
                    {model_delete_buttons.into_iter()}
                }
            }

            if show_provider() {
                Modal { title: "New provider".to_string(), width: 600, on_close: move |_| show_provider.set(false),
                    div { style: "display:grid; grid-template-columns:1fr 1fr; gap:16px;",
                        TextField { value: provider_name(), label: "Name".to_string(), placeholder: "OpenAI".to_string(), oninput: move |v| provider_name.set(v) }
                        Dropdown {
                            label: "Kind".to_string(),
                            value: provider_kind(),
                            options: PROVIDER_KINDS.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect(),
                            onchange: move |v: String| provider_kind.set(v),
                        }
                        TextField { value: base_url(), label: "Base URL".to_string(), placeholder: "https://api.openai.com/v1".to_string(), oninput: move |v| base_url.set(v) }
                        TextField { value: api_key(), label: "API key".to_string(), placeholder: "sk-...".to_string(), oninput: move |v| api_key.set(v) }
                    }
                    div { style: "display:flex; align-items:center; gap:12px; margin-top:16px;",
                        Button { label: "Test connection".to_string(), loading: conn_busy(), variant: "secondary".to_string(), on_click: move |_| {
                            conn_status.set(None);
                            discovered_models.set(Vec::new());
                            test_req.set(Some(api_types::AiProviderCreate {
                                name: provider_name(),
                                kind: provider_kind(),
                                base_url: if base_url().is_empty() { None } else { Some(base_url()) },
                                api_key: if api_key().is_empty() { None } else { Some(api_key()) },
                                organization: None,
                                enabled: true,
                                sort_order: None,
                            }));
                        } }
                        if let Some((_, msg)) = conn_status() {
                            span { style: "font-size:13px; color:{provider_conn_color};",
                                "{msg}"
                            }
                        }
                    }
                    if !discovered_models().is_empty() {
                        div { style: "margin-top:14px;",
                            span { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_500};", "Discovered models" }
                            div { style: "display:flex; flex-wrap:wrap; gap:6px; margin-top:8px;",
                                for m in discovered_models().iter() {
                                    Badge { text: m.clone(), kind: "neutral".to_string() }
                                }
                            }
                        }
                    }
                    div { style: "display:flex; justify-content:flex-end; gap:12px; margin-top:20px;",
                        Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_provider.set(false) }
                        Button { label: "Save".to_string(), on_click: move |_| {
                            provider_req.set(Some((provider_name(), provider_kind(), base_url(), api_key(), provider_enabled())));
                            show_provider.set(false);
                        } }
                    }
                }
            }

            if show_model() {
                Modal { title: "New model".to_string(), width: 480, on_close: move |_| show_model.set(false),
                    div { style: "display:flex; flex-direction:column; gap:16px;",
                        Dropdown {
                            label: "Provider".to_string(),
                            value: model_provider().to_string(),
                            options: providers().iter().map(|p| (p["id"].as_i64().unwrap_or(0).to_string(), p["name"].as_str().unwrap_or("").to_string())).collect(),
                            onchange: move |v: String| { if let Ok(n) = v.parse::<i64>() { model_provider.set(n); } }
                        }
                        TextField { value: model_name(), label: "Model name".to_string(), placeholder: "e.g. gpt-4o-mini".to_string(), oninput: move |v| model_name.set(v) }
                        label { style: "display:flex; align-items:center; gap:6px; font-size:13px; color:{color::NEUTRAL_700};",
                            input { r#type: "checkbox", checked: model_tools(), onchange: move |e| model_tools.set(e.checked()) }
                            span { "Supports tools" }
                        }
                    }
                    div { style: "display:flex; justify-content:flex-end; gap:12px; margin-top:20px;",
                        Button { label: "Cancel".to_string(), variant: "secondary".to_string(), on_click: move |_| show_model.set(false) }
                        Button { label: "Save".to_string(), on_click: move |_| {
                            model_req.set(Some((model_provider(), model_name(), model_tools())));
                            show_model.set(false);
                        } }
                    }
                }
            }
        }
    }
}

#[component]
fn Stat(label: String, value: String) -> Element {
    rsx! {
        div { style: "display:flex; flex-direction:column;",
            span { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_500}; margin-bottom:4px;", "{label}" }
            span { style: "font-size:22px; font-weight:600; color:{color::NEUTRAL_900};", "{value}" }
        }
    }
}

// ===========================================================================
// AI Assistant (chat)
// ===========================================================================

#[component]
pub fn AiAssistant() -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut conversations: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut messages: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut models: Signal<Vec<serde_json::Value>> = use_signal(|| vec![]);
    let mut selected: Signal<Option<i64>> = use_signal(|| None);
    let mut title = use_signal(|| String::new());
    let mut model = use_signal(|| String::new());
    let mut privacy = use_signal(|| false);
    let mut input = use_signal(|| String::new());
    let mut busy = use_signal(|| false);
    let mut pending: Signal<Option<serde_json::Value>> = use_signal(|| None);

    let mut create_req = use_signal(|| false);
    let mut send_req: Signal<Option<String>> = use_signal(|| None);
    let mut confirm_req = use_signal(|| false);
    let mut delete_req: Signal<Option<i64>> = use_signal(|| None);

    // Load conversations + models. If a previous conversation was active,
    // restore it so its persisted history is shown again.
    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            let mut cs = conversations;
            let mut ms = models;
            let mut sel = selected;
            spawn(async move {
                if let Ok(v) = client.ai_conversations().await {
                    let arr = v["data"].as_array().cloned().unwrap_or_default();
                    cs.set(arr.clone());
                    if sel().is_none() {
                        if let Some(stored) = load_last_conversation() {
                            if arr.iter().any(|c| c["id"].as_i64() == Some(stored)) {
                                sel.set(Some(stored));
                            }
                        }
                    }
                }
                if let Ok(v) = client.ai_models(None).await {
                    ms.set(v["data"].as_array().cloned().unwrap_or_default());
                }
            });
        }
    });

    // Create conversation.
    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if create_req() {
                create_req.set(false);
                let client = client.clone();
                let mut g = g.clone();
                let mut cs = conversations;
                let mut sel = selected;
                spawn(async move {
                    let body = api_types::AiConversationCreate {
                        title: title(),
                        system_prompt: None,
                        provider_id: None,
                        model: if model().is_empty() { None } else { Some(model()) },
                        privacy_mode: privacy(),
                    };
                    match client.ai_conversation_create(&body).await {
                        Ok(v) => {
                            if let Ok(l) = client.ai_conversations().await {
                                cs.set(l["data"].as_array().cloned().unwrap_or_default());
                            }
                            let id = v["data"]["id"].as_i64();
                            if let Some(id) = id {
                                sel.set(Some(id));
                                if let Ok(m) = client.ai_messages(id).await {
                                    // messages load in the selection effect
                                }
                            }
                            g.toast("Conversation created", "success");
                        }
                        Err(e) => g.toast(format!("Create failed: {e}"), "danger"),
                    }
                });
            }
        }
    });

    // Load messages when a conversation is selected.
    use_effect({
        let client = client.clone();
        move || {
            let sel = selected();
            let client = client.clone();
            let mut msgs = messages;
            if let Some(id) = sel {
                spawn(async move {
                    if let Ok(v) = client.ai_messages(id).await {
                        msgs.set(v["data"].as_array().cloned().unwrap_or_default());
                    }
                });
            } else {
                msgs.set(vec![]);
            }
        }
    });

    // Send a message.
    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if let Some(text) = send_req() {
                send_req.set(None);
                let Some(id) = selected() else { return };
                let client = client.clone();
                let mut g = g.clone();
                let mut msgs = messages;
                let mut busy2 = busy;
                let mut pend = pending;
                spawn(async move {
                    busy2.set(true);
                    // optimistic user bubble
                    let mut m = msgs();
                    m.push(serde_json::json!({ "role": "user", "content": text }));
                    msgs.set(m);
                    match client.ai_send_message(id, &text).await {
                        Ok(v) => {
                            let d = v["data"].clone();
                            // Reload the persisted history so the UI always
                            // reflects what is stored server-side.
                            if let Ok(ms) = client.ai_messages(id).await {
                                msgs.set(ms["data"].as_array().cloned().unwrap_or_default());
                            }
                            let confirm = d["confirmationRequired"].as_array().cloned();
                            if let Some(arr) = confirm {
                                if !arr.is_empty() {
                                    pend.set(Some(serde_json::json!({ "calls": arr })));
                                }
                            }
                        }
                        Err(e) => g.toast(format!("Error: {e}"), "danger"),
                    }
                    busy2.set(false);
                });
            }
        }
    });

    // Confirm pending mutating tool calls.
    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if confirm_req() {
                confirm_req.set(false);
                let Some(id) = selected() else { return };
                let Some(p) = pending() else { return };
                let client = client.clone();
                let mut g = g.clone();
                let mut msgs = messages;
                let mut pend = pending;
                let mut busy2 = busy;
                spawn(async move {
                    busy2.set(true);
                    let calls: Vec<api_types::AiConfirmToolCall> = p["calls"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|c| {
                                    let name = c["name"].as_str()?.to_string();
                                    Some(api_types::AiConfirmToolCall {
                                        id: c["id"].as_str().unwrap_or("").to_string(),
                                        name,
                                        arguments: c["arguments"].clone(),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    match client.ai_confirm_tool_calls(id, calls).await {
                        Ok(_) => {
                            // Reload the persisted history to reflect the
                            // confirmed tool results and final assistant answer.
                            if let Ok(ms) = client.ai_messages(id).await {
                                msgs.set(ms["data"].as_array().cloned().unwrap_or_default());
                            }
                            pend.set(None);
                            g.toast("Action confirmed and applied", "success");
                        }
                        Err(e) => g.toast(format!("Error: {e}"), "danger"),
                    }
                    busy2.set(false);
                });
            }
        }
    });

    // Delete conversation.
    use_effect({
        let client = client.clone();
        let mut g = global.clone();
        move || {
            if let Some(id) = delete_req() {
                delete_req.set(None);
                let client = client.clone();
                let mut g = g.clone();
                let mut cs = conversations;
                let mut sel = selected;
                spawn(async move {
                    match client.ai_conversation_delete(id).await {
                        Ok(_) => {
                            if let Ok(v) = client.ai_conversations().await {
                                cs.set(v["data"].as_array().cloned().unwrap_or_default());
                            }
                            if selected() == Some(id) {
                                sel.set(None);
                                save_last_conversation(None);
                            }
                            g.toast("Conversation deleted", "success");
                        }
                        Err(e) => g.toast(format!("Delete failed: {e}"), "danger"),
                    }
                });
            }
        }
    });

    let model_options: Vec<(String, String)> = models()
        .iter()
        .map(|m| (m["name"].as_str().unwrap_or("").to_string(), m["name"].as_str().unwrap_or("").to_string()))
        .collect();

    let pending_count: usize = pending()
        .as_ref()
        .and_then(|p| p["calls"].as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let confirm_text = format!("The assistant wants to run {pending_count} action(s) that modify content.");

    // Chat history rendered as a table (click a row to open, delete per row).
    let convo_rows: Vec<Element> = conversations()
        .iter()
        .map(|c| {
            let id = c["id"].as_i64().unwrap_or(0);
            let name = c["title"].as_str().unwrap_or("Untitled").to_string();
            let updated = c["updatedAt"].as_str().unwrap_or("").to_string();
            let active = selected() == Some(id);
            let row_style = if active {
                format!("cursor:pointer; background:{};", color::PRIMARY_100)
            } else {
                "cursor:pointer;".to_string()
            };
            rsx! {
                tr { "class": "table-row", style: "{row_style}",
                    onclick: move |_| { selected.set(Some(id)); save_last_conversation(Some(id)); },
                    td { "class": "table-td", "{name}" }
                    td { "class": "table-td", "{updated}" }
                    td { "class": "table-td",
                        Button { label: "Delete".to_string(), variant: "danger".to_string(), size: "sm".to_string(), on_click: move |_| delete_req.set(Some(id)) }
                    }
                }
            }
        })
        .collect();

    let convo_table: Element = rsx! {
        table { "class": "table",
            thead {
                tr {
                    th { "class": "table-th", "Title" }
                    th { "class": "table-th", "Updated" }
                    th { "class": "table-th", "" }
                }
            }
            tbody { {convo_rows.into_iter()} }
        }
    };

    let bubbles: Vec<Element> = messages()
        .iter()
        .map(|m| {
            let role = m["role"].as_str().unwrap_or("").to_string();
            let content = m["content"].as_str().unwrap_or("").to_string();
            let tool_name = m["toolName"].as_str();
            let is_user = role == "user";
            let bg = if is_user { color::PRIMARY_600 } else { color::NEUTRAL_100 };
            let fg = if is_user { "#fff".to_string() } else { color::NEUTRAL_900.to_string() };
            let align = if is_user { "flex-end" } else { "flex-start" };
            rsx! {
                div { style: "display:flex; justify-content:{align}; margin-bottom:12px;",
                    div { style: "max-width:80%; padding:10px 14px; border-radius:10px; background:{bg}; color:{fg}; font-size:14px; white-space:pre-wrap;",
                        if let Some(t) = tool_name { Badge { text: format!("tool: {t}").to_string(), kind: "neutral".to_string() } }
                        span { "{content}" }
                    }
                }
            }
        })
        .collect();

    rsx! {
        div { style: "padding:32px; max-width:1100px;",
            div { style: "display:flex; gap:24px;",
                // Left: conversations
                div { style: "width:260px; flex-shrink:0; display:flex; flex-direction:column; gap:12px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "AI Assistant" }
                    div { style: "display:flex; gap:8px;",
                        TextField { value: title(), label: "New chat".to_string(), placeholder: "Title".to_string(), oninput: move |v| title.set(v) }
                    }
                    if model_options.is_empty() {
                        EmptyState { title: "No models configured".to_string(), subtitle: "Add a provider + model in AI Settings.".to_string(), icon: "puzzle".to_string() }
                    } else {
                        Dropdown {
                            label: "Model".to_string(),
                            value: model(),
                            options: model_options,
                            onchange: move |v: String| model.set(v),
                        }
                    }
                    label { style: "display:flex; align-items:center; gap:6px; font-size:12px; color:{color::NEUTRAL_700}; cursor:pointer;",
                        input { r#type: "checkbox", checked: privacy(), onchange: move |e| privacy.set(e.checked()) }
                        span { "Private mode (don't send history to the model)" }
                    }
                    Button { label: "+ New conversation".to_string(), disabled: model().is_empty(), on_click: move |_| create_req.set(true) }
                    div { style: "display:flex; flex-direction:column; gap:6px;", {convo_table} }
                }
                // Right: chat thread
                div { style: "flex:1; display:flex; flex-direction:column; gap:16px;",
                    if selected().is_none() {
                        EmptyState { title: "Select or create a conversation".to_string(), subtitle: "Ask the assistant to draft content, translate, or query your content types.".to_string(), icon: "text".to_string() }
                    } else {
                        Card { padding: 20,
                            div { style: "max-height:480px; overflow-y:auto; min-height:300px;", {bubbles.into_iter()} }
                        }
                        if pending().is_some() {
                            Card { header: "Confirm actions".to_string(),
                                div { style: "display:flex; align-items:center; gap:12px;",
                                    span { style: "font-size:13px; color:{color::NEUTRAL_700};", "{confirm_text}" }
                                    Button { label: "Confirm".to_string(), loading: busy(), on_click: move |_| confirm_req.set(true) }
                                }
                            }
                        }
                        div { style: "display:flex; gap:8px;",
                            TextArea { value: input(), placeholder: "Type a message…".to_string(), oninput: move |v| input.set(v) }
                            Button { label: "Send".to_string(), loading: busy(), disabled: input().trim().is_empty(), on_click: move |_| { send_req.set(Some(input())); input.set(String::new()); } }
                        }
                    }
                }
            }
        }
    }
}
