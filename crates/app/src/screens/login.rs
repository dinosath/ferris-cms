//! Login screen (design doc §1).

use api_types::admin::LoginRequest;
use dioxus::prelude::*;
use ui::design::tokens::{color, spacing, typography};

use crate::app::{Route, use_global};
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

    let busy = outcome() == LoginOutcome::Loading;
    let g_login = global.clone();
    let mut g_register = global.clone();

    let card_width = 456;
    let brand = format!("width:40px;height:40px;border-radius:8px;background:{}; margin-bottom:{}px;", color::PRIMARY_600, spacing::SP_6);
    let title_style = format!("font-size:{}; font-weight:600; color:{};", typography::BETA_SIZE, color::NEUTRAL_900);
    let subtitle_style = format!("font-size:{}; color:{}; margin-bottom:{}px;", typography::BODY_SIZE, color::NEUTRAL_600, spacing::SP_7);
    let error_style = format!("width:100%; background:{}; color:{}; border-radius:4px; padding:12px; font-size:{}; margin-bottom:16px;", color::DANGER_100, color::DANGER_700, typography::BODY_SIZE);
    let footer_style = format!("font-size:{}; color:{}; margin-top:16px;", typography::PI_SIZE, color::NEUTRAL_500);

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
                            let mut g = g_login.clone();
                            let email = email();
                            let password = password();
                            outcome.set(LoginOutcome::Loading);
                            spawn(async move {
                                let resp = g.client
                                    .auth_login(&LoginRequest { email, password })
                                    .await;
                                match resp {
                                    Ok(r) => {
                                        g.set_token(Some(r.data.token.clone()));
                                        g.route.set(Route::Home);
                                    }
                                    Err(e) => outcome.set(LoginOutcome::Error(e.to_string())),
                                }
                            });
                        },
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
