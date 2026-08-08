//! Shared e2e harness.
//!
//! The e2e suite is fully self-contained: it does **not** use Docker containers
//! or testcontainers for the database or the browser. Instead it
//!
//! - provisions a local **Turso** database (the SQLite-compatible [`turso`]
//!   engine) in a temp directory, and runs the ferriscms server **in-process**
//!   against it; and
//! - launches **Obscura** ([`h4ckf0r0day/obscura`]) as a local subprocess to
//!   act as the headless browser (a drop-in replacement for headless Chrome),
//!   driven over the Chrome DevTools Protocol by Playwright (`playwright-rs`)
//!   or Puppeteer.
//!
//! See [`harness::E2eHarness`]. Tests boot a stack per test via
//! `E2eHarness::start().await`; no external services are required beyond the
//! `obscura` binary being on `PATH`.

pub mod harness;

/// Base URL of the ferriscms server under test, reachable from the test
/// process (default `http://127.0.0.1:1337`). Only used when the server is
/// managed externally instead of via [`harness::E2eHarness`].
pub fn server_url() -> String {
    std::env::var("FERRISCMS_URL").unwrap_or_else(|_| "http://127.0.0.1:1337".to_string())
}

/// Base URL the browser navigates to (defaults to [`server_url`]). Only used
/// when the server is managed externally instead of via
/// [`harness::E2eHarness`].
pub fn browser_app_url() -> String {
    std::env::var("FERRISCMS_APP_URL").unwrap_or_else(|_| server_url())
}

/// CDP endpoint of the headless browser, reachable from the test process
/// (default `http://127.0.0.1:9222`). Only used when the browser is managed
/// externally instead of via [`harness::E2eHarness`].
pub fn browser_cdp_url() -> String {
    std::env::var("FERRISCMS_BROWSER_URL").unwrap_or_else(|_| "http://127.0.0.1:9222".to_string())
}
