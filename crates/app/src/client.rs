//! Thin typed wrapper over `client-core` so screens never touch transports.

use client_core::{Client, HttpTransport};
use std::sync::Arc;

/// Resolve the API base URL for the current platform.
///
/// - Web: empty string → same-origin requests against the Axum server.
/// - Desktop: the embedded server address, overridable via `FERRISCMS_API_URL`.
pub fn api_base_url() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = "";
        String::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("FERRISCMS_API_URL").unwrap_or_else(|_| "http://127.0.0.1:1337".to_string())
    }
}

/// Build a `client-core::Client` wired to the Axum backend.
pub fn build_client() -> Client {
    let transport = Arc::new(HttpTransport::new(api_base_url()));
    Client::new(transport)
}
