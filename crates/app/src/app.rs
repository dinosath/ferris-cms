//! Root component, app-state context, and manual (signal-based) routing.

use dioxus::prelude::*;
use dioxus::document::Title;
use std::sync::Arc;

use crate::client::build_client;
use crate::screens;
use crate::theme;
use client_core::Client;

/// Client-side routes. Auth pages are shown when not logged in; everything
/// else renders inside the `Shell`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Login,
    Register,
    Home,
    ContentTypeBuilder,
    ContentManager,
    Media,
    Settings,
}

/// Global app state provided to every screen through the component context.
#[derive(Clone)]
pub struct Global {
    pub client: Arc<Client>,
    pub token: Signal<Option<String>>,
    pub route: Signal<Route>,
    /// (message, kind) toast queue.
    pub toasts: Signal<Vec<(String, String)>>,
}

/// localStorage key holding the JWT so a refresh keeps the session alive.
/// Only referenced on wasm targets (storage access), hence `allow(dead_code)`
/// for native/test builds.
#[allow(dead_code)]
const TOKEN_STORAGE_KEY: &str = "ferriscms_token";

/// Read the persisted JWT (web only). Returns `None` on non-wasm targets.
fn load_persisted_token() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(TOKEN_STORAGE_KEY).ok().flatten())
            .filter(|t| !t.is_empty())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Persist (or clear) the JWT in localStorage (web only). No-op off-wasm.
fn persist_token(token: Option<&str>) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(w) = web_sys::window() {
            if let Ok(Some(storage)) = w.local_storage() {
                match token {
                    Some(t) => {
                        let _ = storage.set_item(TOKEN_STORAGE_KEY, t);
                    }
                    None => {
                        let _ = storage.remove_item(TOKEN_STORAGE_KEY);
                    }
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = token;
    }
}

impl Global {
    pub fn new() -> Self {
        Self {
            client: Arc::new(build_client()),
            token: Signal::new(load_persisted_token()),
            route: Signal::new(Route::Home),
            toasts: Signal::new(Vec::new()),
        }
    }

    /// Set the auth token (updating the signal and the persisted session).
    pub fn set_token(&mut self, token: Option<String>) {
        persist_token(token.as_deref());
        self.token.set(token);
    }

    /// Push a toast notification.
    pub fn toast(&mut self, message: impl Into<String>, kind: &str) {
        self.toasts.write().push((message.into(), kind.to_string()));
    }

    /// True when a JWT is present.
    pub fn authed(&self) -> bool {
        self.token.read().is_some()
    }
}

/// Look up the shared app state.
pub fn use_global() -> Global {
    use_context::<Global>()
}

/// Root component: injects the design-token stylesheet and the app context,
/// then renders either an auth screen or the authenticated shell. Users without
/// a JWT are always shown the login screen (protected routes redirect here).
#[component]
pub fn App() -> Element {
    // Provide the shared app state once; the factory runs lazily on first use.
    use_context_provider(move || Global::new());
    let global = use_context::<Global>();
    let route = global.route;

    // Keep the route signal consistent: an unauthenticated user who somehow
    // reaches a protected route is redirected to Login.
    let g = global.clone();
    let mut r = route;
    use_effect(move || {
        if !g.authed() && r() != Route::Login && r() != Route::Register {
            r.set(Route::Login);
        }
    });

    // Resolve the screen to render without waiting for the effect to land, so
    // unauthenticated users never flash the protected shell.
    let authed = global.authed();
    let effective = match route() {
        Route::Login => Route::Login,
        Route::Register => Route::Register,
        _ if !authed => Route::Login,
        r => r,
    };

    rsx! {
        Title { "ferriscms" }
        style { {theme::token_styles()} }
        div { style: "height:100%; min-height:100vh; background:{theme::neutral_100()};",
            match effective {
                Route::Login => rsx! { screens::login::Login {} },
                Route::Register => rsx! { screens::register::Register {} },
                _ => rsx! { screens::shell::Shell {} },
            }
        }
    }
}
