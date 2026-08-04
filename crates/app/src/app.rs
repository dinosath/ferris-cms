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

impl Global {
    pub fn new() -> Self {
        Self {
            client: Arc::new(build_client()),
            token: Signal::new(None),
            route: Signal::new(Route::Home),
            toasts: Signal::new(Vec::new()),
        }
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
/// then renders either an auth screen or the authenticated shell.
#[component]
pub fn App() -> Element {
    // Provide the shared app state once; the factory runs lazily on first use.
    use_context_provider(move || Global::new());
    let global = use_context::<Global>();
    let route = global.route;

    rsx! {
        Title { "ferriscms" }
        style { {theme::token_styles()} }
        div { style: "height:100%; min-height:100vh; background:{theme::neutral_100()};",
            match route() {
                Route::Login => rsx! { screens::login::Login {} },
                Route::Register => rsx! { screens::register::Register {} },
                _ => rsx! { screens::shell::Shell {} },
            }
        }
    }
}
