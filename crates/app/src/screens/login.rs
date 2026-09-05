//! Login screen (design doc §1).

use api_types::admin::LoginRequest;
use dioxus::prelude::*;
use ui::design::tokens::{color, spacing, typography};

use crate::app::{use_global, Route};
use crate::components::{Button, Card, TextField};

/// Whether login is loading, plus the outcome.
#[derive(Clone, PartialEq)]
enum LoginOutcome {
    Idle,
    Loading,
    Error(String),
}

#[component]
pub fn Login() -> Element {
    let global = use_global();
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut outcome = use_signal(|| LoginOutcome::Idle);
    // Set on "Login" click; a `use_effect` below performs the async call. This
    // avoids `spawn` directly inside an event handler, which does not run its
    // async body in the wasm build.
    let mut attempt = use_signal(|| None::<(String, String)>);

    // OIDC/SSO availability, fetched once from the server so the screen can show
    // a "Continue with SSO" action when it is configured.
    let mut oidc_enabled = use_signal(|| false);
    let g_oidc_effect = global.clone();
    use_effect(move || {
        let mut g = g_oidc_effect.clone();
        let mut enabled = oidc_enabled;
        spawn(async move {
            if let Ok(status) = g.client.oidc_status().await {
                enabled.set(status.enabled);
            }
        });
    });

    // Perform login when a submit is requested. Running `spawn` from an effect
    // (rather than an event handler) is what works on the web target.
    let g_login_effect = global.clone();
    use_effect(move || {
        if let Some((em, pw)) = attempt() {
            let mut g = g_login_effect.clone();
            let email = em.clone();
            let password = pw.clone();
            attempt.set(None);
            outcome.set(LoginOutcome::Loading);
            spawn(async move {
                let resp = g.client.auth_login(&LoginRequest { email, password }).await;
                match resp {
                    Ok(r) => {
                        g.set_token(Some(r.data.token.clone()));
                        g.route.set(Route::Home);
                    }
                    Err(e) => outcome.set(LoginOutcome::Error(e.to_string())),
                }
            });
        }
    });

    let busy = outcome() == LoginOutcome::Loading;
    let mut g_register = global.clone();

    let card_width = 456;
    let brand = format!(
        "width:40px;height:40px;border-radius:8px;background:{}; margin-bottom:{}px;",
        color::PRIMARY_600,
        spacing::SP_6
    );
    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::BETA_SIZE,
        color::NEUTRAL_900
    );
    let subtitle_style = format!(
        "font-size:{}; color:{}; margin-bottom:{}px;",
        typography::BODY_SIZE,
        color::NEUTRAL_600,
        spacing::SP_7
    );
    let error_style = format!("width:100%; background:{}; color:{}; border-radius:4px; padding:12px; font-size:{}; margin-bottom:16px;", color::DANGER_100, color::DANGER_700, typography::BODY_SIZE);
    let footer_style = format!(
        "font-size:{}; color:{}; margin-top:16px;",
        typography::PI_SIZE,
        color::NEUTRAL_500
    );

    rsx! {
        div {
            style: "min-height:100vh; display:flex; align-items:center; justify-content:center; background:{color::NEUTRAL_100};",
            Card { padding: spacing::SP_9,
                div { style: "width:{card_width}px; display:flex; flex-direction:column; align-items:center;",
                    div { style: "{brand}" }
                    span { style: "{title_style}", "Welcome!" }
                    span { style: "{subtitle_style}", "Log in to your account" }

                    if let LoginOutcome::Error(msg) = outcome() {
                        div { style: "{error_style}", "{msg}" }
                    }

                    TextField {
                        value: "{email}",
                        label: "Email".to_string(),
                        placeholder: "kai@doe.com".to_string(),
                        input_type: "email".to_string(),
                        oninput: move |v| email.set(v),
                    }
                    TextField {
                        value: "{password}",
                        label: "Password".to_string(),
                        placeholder: "••••••••".to_string(),
                        input_type: "password".to_string(),
                        oninput: move |v| password.set(v),
                    }
                    Button {
                        label: "Login".to_string(),
                        variant: "primary".to_string(),
                        full_width: true,
                        loading: busy,
                        on_click: move |_| {
                            attempt.set(Some((email(), password())));
                        },
                    }
                    if oidc_enabled() {
                        div { style: "width:100%; display:flex; align-items:center; margin:18px 0 8px;",
                            span { style: "flex:1; height:1px; background:{color::NEUTRAL_300};" }
                            span { style: "margin:0 10px; font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "or" }
                            span { style: "flex:1; height:1px; background:{color::NEUTRAL_300};" }
                        }
                        Button {
                            label: "Continue with SSO".to_string(),
                            variant: "secondary".to_string(),
                            full_width: true,
                            on_click: move |_| start_oidc_sso(),
                        }
                    }
                    span { style: "{footer_style}",
                        "New here? ",
                        a { style: "color:{color::PRIMARY_600}; cursor:pointer;",
                            onclick: move |_| g_register.route.set(Route::Register),
                            "Create an account"
                        }
                    }
                }
            }
        }
    }
}

/// Send the user to the OIDC authorization endpoint (web only). The server
/// redirects to the IdP; on success the user returns to `/#oidc_token=...`.
fn start_oidc_sso() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href("/admin/oidc/authorize");
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {}
}
