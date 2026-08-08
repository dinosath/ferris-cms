//! Shared e2e harness.
//!
//! The e2e suite does NOT start containers itself. It expects a ferriscms
//! server and a headless Chrome (CDP) to already be running, and reads their
//! endpoints from environment variables:
//!
//! - `FERRISCMS_URL`  — base URL of the ferriscms server, reachable from the
//!   test process (default `http://127.0.0.1:1337`).
//! - `FERRISCMS_APP_URL` — base URL the browser should navigate to. Because the
//!   browser runs inside its own container, this must be a host reachable from
//!   there (e.g. the compose service name `http://server:1337`), not the test
//!   process's localhost. Defaults to `FERRISCMS_URL`.
//! - `FERRISCMS_BROWSER_URL` — CDP endpoint of the Chrome container, reachable
//!   from the test process (default `http://127.0.0.1:9222`).
//!
//! Start those with the bundled `docker-compose.e2e.yml`:
//!   docker compose -f docker-compose.e2e.yml up -d

/// Base URL of the ferriscms server, reachable from the test process.
pub fn server_url() -> String {
    std::env::var("FERRISCMS_URL").unwrap_or_else(|_| "http://127.0.0.1:1337".to_string())
}

/// Base URL the browser navigates to (reachable from inside the browser
/// container). Defaults to `server_url()`.
pub fn browser_app_url() -> String {
    std::env::var("FERRISCMS_APP_URL").unwrap_or_else(|_| server_url())
}

/// CDP endpoint of the Chrome container, reachable from the test process.
pub fn browser_cdp_url() -> String {
    std::env::var("FERRISCMS_BROWSER_URL").unwrap_or_else(|_| "http://127.0.0.1:9222".to_string())
}
