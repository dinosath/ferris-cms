//! ferriscms admin UI entrypoint.
//!
//! A multiplatform Dioxus app: the same `App` component compiles to web (WASM)
//! and native desktop. It talks to the Axum backend through `client-core`.

use ferriscms::app::App;

fn main() {
    // Allow overriding the API base URL (used by the desktop build).
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::env::var("FERRISCMS_API_URL");

    // Surface Rust panics in the browser console for diagnosis.
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    dioxus::launch(App);
}
