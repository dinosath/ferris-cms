//! Register (first-run super admin) screen (design doc §2).

use api_types::admin::RegisterAdminRequest;
use dioxus::prelude::*;
use ui::design::tokens::{color, spacing, typography};

use crate::app::{Route, use_global};
use crate::components::{Button, Card, TextField};

#[derive(Clone, PartialEq)]
enum RegisterOutcome {
    Idle,
    Loading,
    Error(String),
}

#[component]
pub fn Register() -> Element {
    let global = use_global();
    let mut firstname = use_signal(String::new);
    let mut lastname = use_signal(String::new);
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut confirm = use_signal(String::new);
    let mut outcome = use_signal(|| RegisterOutcome::Idle);

    let busy = outcome() == RegisterOutcome::Loading;
    let password_mismatch = !password().is_empty() && password() != confirm();

    let card_width = 456;
    let title_style = format!("font-size:{}; font-weight:600; color:{};", typography::BETA_SIZE, color::NEUTRAL_900);
    let subtitle_style = format!("font-size:{}; color:{}; text-align:center; margin-bottom:{}px;", typography::BODY_SIZE, color::NEUTRAL_600, spacing::SP_7);
    let error_style = format!("width:100%; background:{}; color:{}; border-radius:4px; padding:12px; font-size:{}; margin-bottom:16px;", color::DANGER_100, color::DANGER_700, typography::BODY_SIZE);

    rsx! {
        div {
            style: "min-height:100vh; display:flex; align-items:center; justify-content:center; background:{color::NEUTRAL_100};",
            Card { padding: spacing::SP_9,
                div { style: "width:{card_width}px; display:flex; flex-direction:column; align-items:center;",
                    span { style: "{title_style}", "Welcome!" }
                    span { style: "{subtitle_style}",
                        "Credentials are only used to authenticate in the admin panel. All saved data will be stored in your database."
                    }

                    if let RegisterOutcome::Error(msg) = outcome() {
                        div { style: "{error_style}", "{msg}" }
                    }

                    TextField { value: "{firstname}", label: "First name".to_string(), oninput: move |v| firstname.set(v) }
                    TextField { value: "{lastname}", label: "Last name".to_string(), oninput: move |v| lastname.set(v) }
                    TextField { value: "{email}", label: "Email".to_string(), input_type: "email".to_string(), oninput: move |v| email.set(v) }
                    TextField {
                        value: "{password}", label: "Password".to_string(), input_type: "password".to_string(),
                        helper: "Must be at least 8 characters, 1 uppercase, 1 lowercase, 1 number.".to_string(),
                        oninput: move |v| password.set(v),
                    }
                    TextField {
                        value: "{confirm}", label: "Confirm Password".to_string(), input_type: "password".to_string(),
                        error: if password_mismatch { "Passwords do not match".to_string() } else { String::new() },
                        oninput: move |v| confirm.set(v),
                    }
                    Button {
                        label: "Let's start".to_string(),
                        variant: "primary".to_string(),
                        full_width: true,
                        loading: busy,
                        on_click: move |_| {
                            if password() != confirm() { return; }
                            let mut g = global.clone();
                            let firstname = firstname();
                            let lastname = lastname();
                            let email = email();
                            let password = password();
                            outcome.set(RegisterOutcome::Loading);
                            spawn(async move {
                                let req = RegisterAdminRequest {
                                    email, password,
                                    firstname: Some(firstname),
                                    lastname: Some(lastname),
                                    registration_token: None,
                                };
                                match g.client.auth_register(&req).await {
                                    Ok(r) => {
                                        g.set_token(Some(r.data.token.clone()));
                                        g.route.set(Route::Home);
                                    }
                                    Err(e) => outcome.set(RegisterOutcome::Error(e.to_string())),
                                }
                            });
                        },
                    }
                }
            }
        }
    }
}
