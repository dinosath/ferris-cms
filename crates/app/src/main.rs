//! ferriscms admin UI entrypoint.
//!
//! A multiplatform Dioxus app: the same `App` component compiles to web (WASM)
//! and native desktop. It talks to the Axum backend through `client-core`.

use ferriscms::app::App;

fn main() {
    // Allow overriding the API base URL (used by the desktop build).
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::env::var("FERRISCMS_API_URL");

    dioxus::launch(App);
}
