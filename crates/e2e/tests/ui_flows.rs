//! Playwright UI flow tests (playwright-rs + Rust only) against the **Obscura**
//! headless browser.
//!
//! Where `ui_e2e.rs` is a set of load/smoke checks, this file drives actual
//! *flows* through the Dioxus admin UI: first-run super-admin registration,
//! logout + login, and creating a collection type through the Content-Type
//! Builder and confirming it appears in the Content Manager.
//!
//! Like `ui_e2e.rs`, the debug WASM build ships Dioxus devtools whose
//! hot-reload overlay makes the page "unstable", so auto-waiting locators can
//! hang. We therefore read the DOM only through bounded `evaluate` polls and
//! drive all interaction via `evaluate` (setting input values through the
//! native `HTMLInputElement` value setter and dispatching a bubbling `input`
//! event, and `.click()`ing buttons).
//!
//! Requires `obscura` on PATH, Node.js 18+ on PATH, and a built Dioxus WASM UI
//! (see the crate README). Each test saves a screenshot to
//! `target/e2e-screenshots/` (override with `E2E_SCREENSHOT_DIR`).

use anyhow::Context;
use e2e::harness::E2eHarness;
use playwright_rs::{Page, Playwright};
use std::time::Duration;

const BUILDER_ERROR: &str = "http error: builder error";

// ---------------------------------------------------------------------------
// Low-level evaluate helpers (bounded, non-auto-waiting)
// ---------------------------------------------------------------------------

async fn body_text(page: &Page) -> anyhow::Result<String> {
    let none_arg: Option<&serde_json::Value> = None;
    page.evaluate::<_, String>("() => document.body ? document.body.innerText : ''", none_arg)
        .await
        .map_err(|e| anyhow::anyhow!("evaluate body text failed: {e}"))
}

async fn wait_for_text(page: &Page, predicate: impl Fn(&str) -> bool) -> anyhow::Result<String> {
    for _ in 0..25 {
        tokio::time::sleep(Duration::from_millis(2000)).await;
        if let Ok(body) = body_text(page).await {
            if predicate(&body) {
                return Ok(body);
            }
        }
    }
    anyhow::bail!("condition not met within timeout")
}

/// Set the value of an `<input>` whose preceding sibling `<label>` contains
/// `label`, dispatching a bubbling `input` event so Dioxus's `oninput` fires.
/// Returns true if an input was found and updated.
async fn fill_input_by_label(page: &Page, label: &str, value: &str) -> anyhow::Result<bool> {
    let arg = serde_json::json!([label, value]);
    let found = page
        .evaluate::<_, bool>(
            r#"([label, value]) => {
                const inputs = [...document.querySelectorAll('input')];
                const el = inputs.find(i => {
                    const prev = i.previousElementSibling;
                    return prev && prev.tagName === 'LABEL' && prev.textContent.trim().includes(label);
                });
                if (!el) return false;
                const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
                setter.call(el, value);
                el.dispatchEvent(new Event('input', { bubbles: true }));
                return true;
            }"#,
            Some(&arg),
        )
        .await
        .map_err(|e| anyhow::anyhow!("fill input '{label}' failed: {e}"))?;
    Ok(found)
}

/// Click the first `<button>` whose visible text contains `text`.
async fn click_button_by_text(page: &Page, text: &str) -> anyhow::Result<bool> {
    let arg = serde_json::json!([text]);
    let clicked = page
        .evaluate::<_, bool>(
            r#"([text]) => {
                const btns = [...document.querySelectorAll('button')];
                const el = btns.find(b => b.textContent && b.textContent.trim().includes(text));
                if (!el) return false;
                el.click();
                return true;
            }"#,
            Some(&arg),
        )
        .await
        .map_err(|e| anyhow::anyhow!("click button '{text}' failed: {e}"))?;
    Ok(clicked)
}

/// Click the icon-only logout button in the sidebar footer.
async fn click_logout(page: &Page) -> anyhow::Result<bool> {
    let clicked = page
        .evaluate::<serde_json::Value, bool>(
            r#"() => {
                const aside = document.querySelector('aside');
                if (!aside) return false;
                const btns = [...aside.querySelectorAll('button')];
                const el = btns.find(b => !(b.textContent && b.textContent.trim().length > 0) && b.querySelector('svg'));
                if (!el) return false;
                el.click();
                return true;
            }"#,
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("click logout failed: {e}"))?;
    Ok(clicked)
}

async fn take_screenshot(page: &Page, name: &str) -> anyhow::Result<()> {
    let dir = std::env::var("E2E_SCREENSHOT_DIR")
        .unwrap_or_else(|_| "target/e2e-screenshots".to_string());
    std::fs::create_dir_all(&dir).with_context(|| format!("create screenshot dir {dir}"))?;
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    page.screenshot_to_file(&path, None)
        .await
        .with_context(|| format!("capture screenshot {name}"))?;
    tracing::info!("saved screenshot {name} -> {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Higher-level flow helpers
// ---------------------------------------------------------------------------

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

async fn open_app(harness: &E2eHarness) -> anyhow::Result<UiPage> {
    let pw = Playwright::launch().await?;
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

/// Fill and submit the first-run super-admin registration form, then wait for
/// the app shell (sidebar nav) to appear.
async fn register_super_admin(page: &Page, email: &str) -> anyhow::Result<()> {
    for (label, value) in [
        ("First name", "Kai"),
        ("Last name", "Doe"),
        ("Email", email),
        ("Password", "AdminPass1"),
        ("Confirm Password", "AdminPass1"),
    ] {
        assert!(
            fill_input_by_label(page, label, value).await?,
            "register input '{label}' not found"
        );
    }
    assert!(
        click_button_by_text(page, "Let's start").await?,
        "'Let's start' button not found"
    );
    wait_for_text(page, |t| t.contains("Content Manager") && t.contains("Settings"))
        .await
        .context("app shell did not render after registration")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Full first-run registration flow lands in the admin shell.
#[tokio::test(flavor = "multi_thread")]
async fn register_super_admin_via_ui() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    // We should be on the registration screen first.
    let body = body_text(page).await?;
    assert!(body.contains("Let's start"), "register screen expected");

    register_super_admin(page, "reg@e2e.dev").await?;

    let after = body_text(page).await?;
    assert!(after.contains("ferriscms"), "brand missing after register");
    assert!(
        after.contains("Content Manager") && after.contains("Settings"),
        "shell nav missing after register"
    );
    assert!(!after.contains(BUILDER_ERROR), "builder error present");

    take_screenshot(page, "ui-registered-shell").await?;
    Ok(())
}

/// After registration, logging out returns to the login screen and logging
/// back in restores the shell.
#[tokio::test(flavor = "multi_thread")]
async fn logout_then_login_via_ui() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    register_super_admin(page, "logout@e2e.dev").await?;

    // Log out via the sidebar footer icon button.
    assert!(click_logout(page).await?, "logout button not found");
    wait_for_text(page, |t| t.contains("Log in to your account"))
        .await
        .context("login screen did not appear after logout")?;

    // Log back in.
    assert!(
        fill_input_by_label(page, "Email", "logout@e2e.dev").await?,
        "login email input not found"
    );
    assert!(
        fill_input_by_label(page, "Password", "AdminPass1").await?,
        "login password input not found"
    );
    assert!(click_button_by_text(page, "Login").await?, "login button not found");
    wait_for_text(page, |t| t.contains("Content Manager") && t.contains("Settings"))
        .await
        .context("shell did not restore after login")?;

    take_screenshot(page, "ui-relogged-shell").await?;
    Ok(())
}

/// Create a collection type through the Content-Type Builder and confirm it
/// shows up in the Content Manager afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn create_collection_type_via_ctb() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    register_super_admin(page, "ct@e2e.dev").await?;

    // Navigate to the Content-Type Builder.
    assert!(
        click_button_by_text(page, "Content-Type Builder").await?,
        "CTB nav not found"
    );
    wait_for_text(page, |t| {
        t.contains("Content-Type Builder") || t.contains("Select a content type")
    })
    .await
    .context("CTB screen did not render")?;

    // Open the "create collection type" modal.
    assert!(
        click_button_by_text(page, "+ Create new collection type").await?,
        "create collection type button not found"
    );
    wait_for_text(page, |t| t.contains("Create a collection type"))
        .await
        .context("create modal did not open")?;

    // Fill the schema identity fields and continue.
    assert!(
        fill_input_by_label(page, "Display name", "Article").await?,
        "display name input not found"
    );
    assert!(
        fill_input_by_label(page, "API ID (Singular)", "article").await?,
        "singular input not found"
    );
    assert!(
        fill_input_by_label(page, "API ID (Plural)", "articles").await?,
        "plural input not found"
    );
    assert!(click_button_by_text(page, "Continue").await?, "Continue not found");

    // The new type is now selected in the editor; save it to the backend.
    wait_for_text(page, |t| t.contains("Article"))
        .await
        .context("new type not selected in editor")?;
    assert!(click_button_by_text(page, "Save").await?, "Save button not found");
    wait_for_text(page, |t| t.contains("Saved"))
        .await
        .context("schema did not save")?;

    // Confirm the saved schema is applied server-side: it should appear in the
    // Content Manager's content-type list after navigating there.
    assert!(
        click_button_by_text(page, "Content Manager").await?,
        "Content Manager nav not found"
    );
    wait_for_text(page, |t| t.contains("Article"))
        .await
        .context("Article type not listed in Content Manager")?;

    let body = body_text(page).await?;
    assert!(!body.contains(BUILDER_ERROR), "builder error present: {body}");
    take_screenshot(page, "ui-created-article").await?;
    Ok(())
}
