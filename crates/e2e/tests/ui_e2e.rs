//! Playwright UI end-to-end tests against the **Obscura** headless browser.
//!
//! These drive a local Obscura browser (a drop-in replacement for headless
//! Chrome, launched by the harness) over CDP against the in-process ferriscms
//! server, verifying the embedded Dioxus WASM UI loads and that the data
//! screens no longer hit the "http error: builder error" bug (caused by
//! relative API URLs on the web target).
//!
//! No Chrome and no containers are involved: `E2eHarness` boots a fresh Turso
//! database + in-process server and spawns `obscura serve` as the browser.
//!
//! The debug WASM build ships Dioxus devtools, whose hot-reload overlay keeps
//! the page "unstable", so `page.content()` / auto-waiting locators can hang.
//! We therefore read the DOM only through bounded `evaluate` polls.
//!
//! The UI must be reachable at the server root. Point `FERRISCMS_UI_DIR` at a
//! built Dioxus WASM bundle (e.g. `target/dx/ferriscms/release/web`) before
//! running these tests, or use a server binary with the UI embedded.
//!
//! Each test also saves a PNG screenshot of the page it visits, into
//! `target/e2e-screenshots/` (override with `E2E_SCREENSHOT_DIR`).

use anyhow::Context;
use e2e::harness::E2eHarness;
use playwright_rs::{Page, Playwright};
use std::time::Duration;

/// The specific bug this test guards against: the web UI used relative API URLs,
/// so every API call failed with `reqwest` "builder error".
const BUILDER_ERROR: &str = "http error: builder error";

/// Read `document.body.innerText` via `evaluate`.
async fn body_text(page: &Page) -> anyhow::Result<String> {
    let none_arg: Option<&serde_json::Value> = None;
    page.evaluate::<_, String>("() => document.body ? document.body.innerText : ''", none_arg)
        .await
        .map_err(|e| anyhow::anyhow!("evaluate body text failed: {e}"))
}

/// Poll `body_text` until `predicate` returns true or we time out (~40s).
async fn wait_for_text(page: &Page, predicate: impl Fn(&str) -> bool) -> anyhow::Result<String> {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(2000)).await;
        if let Ok(body) = body_text(page).await {
            if predicate(&body) {
                return Ok(body);
            }
        }
    }
    anyhow::bail!("condition not met")
}

/// Save a PNG screenshot of the current page to the screenshots directory.
/// The directory defaults to `target/e2e-screenshots` and can be overridden
/// with the `E2E_SCREENSHOT_DIR` environment variable.
async fn take_screenshot(page: &Page, name: &str) -> anyhow::Result<()> {
    let dir =
        std::env::var("E2E_SCREENSHOT_DIR").unwrap_or_else(|_| "target/e2e-screenshots".to_string());
    std::fs::create_dir_all(&dir).with_context(|| format!("create screenshot dir {dir}"))?;
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    page.screenshot_to_file(&path, None)
        .await
        .with_context(|| format!("capture screenshot {name}"))?;
    tracing::info!("saved screenshot {name} -> {}", path.display());
    Ok(())
}

/// A connected page plus its CDP browser and driver handles.
///
/// The `Playwright` (driver) and `Browser` handles must stay alive for the
/// page's lifetime: dropping them tears down the connection and makes further
/// `evaluate` calls hang. The browser is connected over CDP to the harness's
/// Obscura subprocess, so it is intentionally never closed here.
struct UiPage {
    page: Page,
    _browser: playwright_rs::Browser,
    _pw: Playwright,
}

impl std::ops::Deref for UiPage {
    type Target = Page;
    fn deref(&self) -> &Page {
        &self.page
    }
}

/// Open a fresh page in Obscura and wait until the Dioxus WASM app hydrates.
async fn open_app(harness: &E2eHarness) -> anyhow::Result<UiPage> {
    let pw = Playwright::launch().await?;
    // Obscura speaks CDP exactly like headless Chrome, so Playwright connects
    // to it over the harness's CDP websocket.
    let browser = pw
        .chromium()
        .connect_over_cdp(harness.browser_cdp_url(), None)
        .await?;
    let page = browser.new_page().await?;

    page.goto(&format!("{}/", harness.browser_app_url()), None)
        .await?;
    wait_for_text(&page, |t| t.contains("ferriscms"))
        .await
        .context("admin UI did not hydrate")?;
    Ok(UiPage { page, _browser: browser, _pw: pw })
}

/// The admin UI shell loads and hydrates: brand + home screen render.
#[tokio::test(flavor = "multi_thread")]
async fn admin_ui_loads_and_hydrates() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let body = body_text(&ui.page).await?;

    assert!(body.contains("ferriscms"), "brand missing");
    assert!(body.contains("Welcome"), "home screen welcome missing");
    assert!(!body.contains(BUILDER_ERROR), "builder error present");

    take_screenshot(&ui.page, "admin-home").await?;
    Ok(())
}

/// The Content-Type Builder data screen must not surface the relative-URL
/// builder error (the specific bug being fixed).
#[tokio::test(flavor = "multi_thread")]
async fn content_type_builder_has_no_builder_error() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    // Navigate to the CTB screen via a DOM click on the sidebar nav item.
    let none_arg: Option<&serde_json::Value> = None;
    page.evaluate::<_, ()>(
        r#"() => {
            const items = [...document.querySelectorAll('button')];
            const el = items.find(b => b.textContent && b.textContent.includes('Content-Type Builder'));
            if (el) el.click();
        }"#,
        none_arg,
    )
    .await?;

    // Wait for the CTB screen (header or empty-state prompt).
    wait_for_text(&page, |t| {
        t.contains("Content-Type Builder") || t.contains("Select a content type")
    })
    .await
    .context("CTB screen did not render")?;

    let body = body_text(&page).await?;
    assert!(
        !body.contains(BUILDER_ERROR),
        "page still contains the builder error: {body}"
    );

    take_screenshot(page, "content-type-builder").await?;
    Ok(())
}

/// The app shell chrome (sidebar navigation) renders all nav labels.
#[tokio::test(flavor = "multi_thread")]
async fn sidebar_navigation_renders() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let body = body_text(&ui.page).await?;

    for label in [
        "Content Manager",
        "Content-Type Builder",
        "Media Library",
        "Settings",
    ] {
        assert!(body.contains(label), "missing nav label: {label}");
    }

    take_screenshot(&ui.page, "sidebar").await?;
    Ok(())
}
