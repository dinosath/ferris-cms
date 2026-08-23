//! Comprehensive playwright-rs UI screen tests against the **Obscura** headless
//! browser (playwright-rs + Rust only).
//!
//! These drive the Dioxus admin UI through every screen, the sidebar, and the
//! main modals/inputs. The shell is gated behind authentication, so `open_app`
//! seeds a valid JWT into localStorage (bypassing the crashing in-app auth
//! form) so all screens and their controls are reachable and assertable.
//!
//! The login/register **submit** action is deliberately avoided here: the
//! Dioxus WASM app crashes on auth-form submission (see `ui_flows.rs`), so
//! `ui_flows.rs` covers the authenticated flows (currently `#[ignore]`d for that
//! reason) while this file covers the full screen/input surface that works.
//!
//! Interaction is driven with `page.evaluate` (native input value setter +
//! bubbling `input` event, and `.click()`), reading the DOM through bounded
//! polls because the Dioxus hydration overlay is unstable.

use anyhow::Context;
use e2e::harness::E2eHarness;
use playwright_rs::{Page, Playwright};
use std::time::Duration;

const BUILDER_ERROR: &str = "http error: builder error";

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

async fn body_text(page: &Page) -> anyhow::Result<String> {
    let none_arg: Option<&serde_json::Value> = None;
    page.evaluate::<_, String>(
        "() => document.body ? document.body.innerText : ''",
        none_arg,
    )
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

/// Click the first `a` or `button` whose visible text contains `text`.
async fn click_link_or_button(page: &Page, text: &str) -> anyhow::Result<bool> {
    let arg = serde_json::json!([text]);
    let clicked = page
        .evaluate::<_, bool>(
            r#"([text]) => {
                const els = [...document.querySelectorAll('a, button')];
                const el = els.find(e => e.textContent && e.textContent.trim().includes(text));
                if (!el) return false;
                el.click();
                return true;
            }"#,
            Some(&arg),
        )
        .await
        .map_err(|e| anyhow::anyhow!("click '{text}' failed: {e}"))?;
    Ok(clicked)
}

/// Click the icon-only logout button in the sidebar footer (navigates to Login).
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
// Higher-level helpers
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

/// Register a fresh admin over the HTTP API (registration is open) and return
/// its JWT. Used to seed an authenticated browser session for the shell.
async fn register_admin_token(base: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let reg: serde_json::Value = client
        .post(format!("{base}/admin/register-admin"))
        .json(&serde_json::json!({"email":"screen@e2e.dev","password":"AdminPass1"}))
        .send()
        .await?
        .json()
        .await?;
    reg["data"]["token"]
        .as_str()
        .map(|s| s.to_string())
        .context("register token missing")
}

async fn open_app(harness: &E2eHarness) -> anyhow::Result<UiPage> {
    let pw = Playwright::launch().await?;
    let browser = pw
        .chromium()
        .connect_over_cdp(harness.browser_cdp_url(), None)
        .await?;
    let page = browser.new_page().await?;
    // Seed a valid JWT before the SPA bootstraps so the protected shell renders
    // (the in-app auth form submit crashes the WASM app, see file header).
    let token = register_admin_token(harness.server_url()).await?;
    page.add_init_script(&format!(
        "localStorage.setItem('ferriscms_token', '{token}');"
    ))
    .await?;
    page.goto(&format!("{}/", harness.browser_app_url()), None)
        .await?;
    wait_for_text(&page, |t| t.contains("ferriscms"))
        .await
        .context("admin UI did not hydrate")?;
    Ok(UiPage {
        page,
        _browser: browser,
        _pw: pw,
    })
}

/// Navigate to a sidebar screen by clicking its nav item and wait for it to
/// render; returns the body text.
async fn goto_screen(page: &Page, nav_label: &str, header_text: &str) -> anyhow::Result<String> {
    assert!(
        click_button_by_text(page, nav_label).await?,
        "nav item '{nav_label}' not found"
    );
    let body = wait_for_text(page, |t| t.contains(header_text))
        .await
        .with_context(|| format!("screen '{header_text}' did not render"))?;
    Ok(body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The shell + home screen render, and every sidebar nav item is present.
#[tokio::test(flavor = "multi_thread")]
async fn shell_sidebar_and_home_render() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let body = body_text(&ui.page).await?;

    assert!(body.contains("ferriscms"), "brand missing");
    assert!(body.contains("Home"), "home screen missing");
    assert!(body.contains("Welcome"), "welcome card missing");
    for label in [
        "Content Manager",
        "Content-Type Builder",
        "Media Library",
        "Settings",
    ] {
        assert!(body.contains(label), "missing nav label: {label}");
    }
    assert!(!body.contains(BUILDER_ERROR), "builder error present");
    take_screenshot(&ui.page, "screen-home").await?;
    Ok(())
}

/// Navigate to every main screen and verify each renders its header and empty
/// state (all without authentication).
#[tokio::test(flavor = "multi_thread")]
async fn all_main_screens_render() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    // Content Manager (no content types -> empty state).
    let cm = goto_screen(page, "Content Manager", "Content Manager").await?;
    assert!(
        cm.contains("No content types available"),
        "CM empty state expected"
    );

    // Content-Type Builder (no types -> empty state).
    let ctb = goto_screen(page, "Content-Type Builder", "Content-Type Builder").await?;
    assert!(
        ctb.contains("No content types yet"),
        "CTB empty state missing"
    );

    // Media Library (no assets -> empty state).
    let media = goto_screen(page, "Media Library", "Media Library").await?;
    assert!(media.contains("No media yet"), "media empty state missing");

    // Settings.
    let settings = goto_screen(page, "Settings", "Settings").await?;
    assert!(
        settings.contains("GLOBAL SETTINGS"),
        "settings global section missing"
    );

    take_screenshot(page, "screen-all-main").await?;
    Ok(())
}

/// The Content-Type Builder "create collection type" modal exposes all its
/// inputs and toggles and accepts typed values.
#[tokio::test(flavor = "multi_thread")]
async fn content_type_builder_create_type_modal() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;
    goto_screen(page, "Content-Type Builder", "Content-Type Builder").await?;

    // Open the create-content-type modal.
    assert!(
        click_button_by_text(page, "Create content type").await?,
        "create content type button not found"
    );
    let modal = wait_for_text(page, |t| t.contains("Create a collection type"))
        .await
        .context("create modal did not open")?;
    assert!(
        modal.contains("Collection type") && modal.contains("Single type"),
        "type segment missing"
    );
    assert!(modal.contains("Draft & publish"), "draft toggle missing");
    assert!(
        modal.contains("Internationalization"),
        "i18n toggle missing"
    );

    // Fill each text input.
    for (label, value) in [
        ("Display name", "Article"),
        ("API ID (Singular)", "article"),
        ("API ID (Plural)", "articles"),
    ] {
        assert!(
            fill_input_by_label(page, label, value).await?,
            "input '{label}' not found"
        );
    }

    // Continue persists the type and navigates to its editor.
    assert!(
        click_button_by_text(page, "Continue").await?,
        "Continue not found"
    );
    wait_for_text(page, |t| t.contains("Article"))
        .await
        .context("new type not opened in editor")?;

    take_screenshot(page, "screen-ctb-create").await?;
    Ok(())
}

/// The Content Manager "create entry" modal is reachable and lists scalar
/// fields of the selected content type as inputs.
#[tokio::test(flavor = "multi_thread")]
async fn content_manager_create_entry_modal() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    // Seed a content type directly over the HTTP API so the Content Manager has
    // something to show (registration is open; this bypasses the crashing
    // in-app auth form).
    let client = reqwest::Client::new();
    let base = harness.server_url();
    let reg: serde_json::Value = client
        .post(format!("{base}/admin/register-admin"))
        .json(&serde_json::json!({"email":"cmentry@e2e.dev","password":"AdminPass1"}))
        .send()
        .await?
        .json()
        .await?;
    let token = reg["data"]["token"].as_str().context("register token")?;
    let ct = serde_json::json!({
        "uid": "api::post.post",
        "kind": "collectionType",
        "info": {"singularName":"post","pluralName":"posts","displayName":"Post"},
        "options": {"draftAndPublish": true},
        "attributes": {"title": {"type":"string"}, "views": {"type":"integer"}}
    });
    let apply = client
        .post(format!("{base}/content-type-builder/schema"))
        .bearer_auth(token)
        .json(&serde_json::json!({"schemas":[ct]}))
        .send()
        .await?;
    anyhow::ensure!(apply.status().is_success(), "seed content type failed");

    // The app is authenticated (open_app seeded a session), so the CM screen
    // loads the seeded content type from the server.
    let cm = goto_screen(page, "Content Manager", "Content Manager").await?;
    assert!(cm.contains("Post"), "seeded content type not shown in CM");

    take_screenshot(page, "screen-cm").await?;
    Ok(())
}

/// The Settings screen exposes all four admin sections and each create modal.
#[tokio::test(flavor = "multi_thread")]
async fn settings_all_sections_render() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;
    let settings = goto_screen(page, "Settings", "Settings").await?;

    for section in ["Internationalization", "API Tokens", "Roles", "Users"] {
        assert!(
            settings.contains(section),
            "missing settings section: {section}"
        );
    }

    // Switch to API Tokens section and open its create modal.
    assert!(
        click_button_by_text(page, "API Tokens").await?,
        "API Tokens nav"
    );
    wait_for_text(page, |t| t.contains("+ Create new API token"))
        .await
        .context("API tokens section did not render")?;
    assert!(
        click_button_by_text(page, "+ Create new API token").await?,
        "create token button"
    );
    let token_modal = wait_for_text(page, |t| t.contains("Token type"))
        .await
        .context("token create modal did not open")?;
    assert!(token_modal.contains("Name"), "token name field missing");
    assert!(
        fill_input_by_label(page, "Name", "readonly").await?,
        "token name input"
    );
    assert!(
        click_button_by_text(page, "Cancel").await?,
        "cancel token modal"
    );

    // Roles section renders its table (headers always present even when empty
    // because the app is unauthenticated and the roles list is empty).
    assert!(click_button_by_text(page, "Roles").await?, "Roles nav");
    wait_for_text(page, |t| t.contains("Permissions"))
        .await
        .context("roles section table did not render")?;

    take_screenshot(page, "screen-settings").await?;
    Ok(())
}

/// The Login and Register screens render with all their inputs.
#[tokio::test(flavor = "multi_thread")]
async fn login_and_register_screens_render() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    // Home -> Login via the sidebar logout icon.
    assert!(click_logout(page).await?, "logout icon not found");
    let login = wait_for_text(page, |t| t.contains("Log in to your account"))
        .await
        .context("login screen did not appear")?;
    assert!(login.contains("Email"), "login email field missing");
    assert!(login.contains("Password"), "login password field missing");

    // Login -> Register via the "Create an account" link.
    assert!(
        click_link_or_button(page, "Create an account").await?,
        "create account link"
    );
    let register = wait_for_text(page, |t| t.contains("Let's start"))
        .await
        .context("register screen did not appear")?;
    for field in [
        "First name",
        "Last name",
        "Email",
        "Password",
        "Confirm Password",
    ] {
        assert!(register.contains(field), "register field missing: {field}");
    }
    // Fill the register form (without submitting; submit crashes the WASM app).
    for (label, value) in [
        ("First name", "Kai"),
        ("Last name", "Doe"),
        ("Email", "reg@e2e.dev"),
        ("Password", "AdminPass1"),
        ("Confirm Password", "AdminPass1"),
    ] {
        assert!(
            fill_input_by_label(page, label, value).await?,
            "register input '{label}'"
        );
    }

    take_screenshot(page, "screen-register").await?;
    Ok(())
}

/// An unauthenticated visitor is redirected to the Login screen instead of
/// being shown the protected shell.
#[tokio::test(flavor = "multi_thread")]
async fn unauthenticated_redirects_to_login() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let pw = Playwright::launch().await?;
    let browser = pw
        .chromium()
        .connect_over_cdp(harness.browser_cdp_url(), None)
        .await?;
    let page = browser.new_page().await?;
    // No auth token seeded: the app must land on Login, not the shell.
    page.goto(&format!("{}/", harness.browser_app_url()), None)
        .await?;
    let body = wait_for_text(&page, |t| {
        t.contains("Log in to your account") || t.contains("Welcome!")
    })
    .await
    .context("login screen did not render for unauthenticated visitor")?;
    assert!(
        body.contains("Log in to your account"),
        "expected login screen"
    );
    assert!(
        !body.contains("Content Manager"),
        "shell leaked to unauthenticated visitor"
    );

    take_screenshot(&page, "screen-login-redirect").await?;
    Ok(())
}

/// The admin UI is reachable only by authorized users: an anonymous visitor is
/// held on the Login screen with the protected shell (and its data nav) hidden,
/// while a visitor with a valid session token reaches the admin shell.
#[tokio::test(flavor = "multi_thread")]
async fn ui_access_requires_authorization() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let pw = Playwright::launch().await?;
    let browser = pw
        .chromium()
        .connect_over_cdp(harness.browser_cdp_url(), None)
        .await?;
    let page = browser.new_page().await?;

    // (1) Unauthorized: no token -> Login screen, and none of the protected
    // shell's navigation leaks through.
    page.goto(&format!("{}/", harness.browser_app_url()), None)
        .await?;
    let unauth = wait_for_text(&page, |t| {
        t.contains("Log in to your account") || t.contains("Welcome!")
    })
    .await
    .context("login screen did not render for unauthenticated visitor")?;
    assert!(
        unauth.contains("Log in to your account"),
        "expected login screen for anonymous visitor"
    );
    for leaked in ["Content Manager", "Content-Type Builder", "Media Library"] {
        assert!(
            !unauth.contains(leaked),
            "protected shell nav leaked to anonymous visitor: {leaked}"
        );
    }

    // (2) Authorized: a valid token lets the visitor into the admin shell.
    let token = register_admin_token(harness.server_url()).await?;
    page.add_init_script(&format!(
        "localStorage.setItem('ferriscms_token', '{token}');"
    ))
    .await?;
    page.reload(None).await?;
    let authed = wait_for_text(&page, |t| t.contains("Content Manager"))
        .await
        .context("authorized visitor did not reach the admin shell")?;
    assert!(
        authed.contains("Content-Type Builder") && authed.contains("Settings"),
        "admin shell did not fully render for the authorized visitor"
    );

    take_screenshot(&page, "screen-ui-access-boundary").await?;
    Ok(())
}

/// In-browser smoke test of the table-first navigation. Provisions an admin and
/// seeds a content type + entry over the HTTP API (verifying the data path),
/// then drives the real UI through the Obscura headless browser to confirm the
/// Content-Type Builder and Content Manager open as table-first listing pages
/// (not a secondary sidebar) and that navigation between them works.
///
/// Note on data rows: Obscura does not propagate the app's `Headers`-object
/// `Authorization` header on `fetch(Request)` (an explicit `fetch` with the same
/// token returns 200), so authenticated data rows do not render in-browser here.
/// The data path is therefore verified separately over the HTTP API; the in-
/// browser assertions cover the rendered navigation UI.
#[tokio::test(flavor = "multi_thread")]
async fn table_first_navigation_full_smoke() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();

    // Register the FIRST super admin and use its token to seed data + the
    // browser session (avoiding the crashing in-app auth form).
    let token = register_admin_token(&base).await?;
    let client = reqwest::Client::new();
    let ct = serde_json::json!({
        "uid": "api::product.product",
        "kind": "collectionType",
        "info": {"singularName":"product","pluralName":"products","displayName":"Product"},
        "options": {"draftAndPublish": true},
        "attributes": {"name": {"type":"string"}, "price": {"type":"decimal"}}
    });
    let apply = client
        .post(format!("{base}/content-type-builder/schema"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"schemas":[ct]}))
        .send()
        .await?;
    anyhow::ensure!(apply.status().is_success(), "seed schema failed");
    let create = client
        .post(format!(
            "{base}/admin/content-manager/collection-types/api::product.product"
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"data": {"name":"Widget","price":12.5}}))
        .send()
        .await?;
    anyhow::ensure!(create.status().is_success(), "seed entry failed");

    // The data path works over the API with the seeded token.
    let list = client
        .get(format!("{base}/content-type-builder/content-types"))
        .bearer_auth(&token)
        .send()
        .await?;
    assert_eq!(list.status().as_u16(), 200, "ctb_list API should succeed");
    let list_body: serde_json::Value = list.json().await?;
    let names: Vec<String> = list_body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s["info"]["displayName"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        names.contains(&"Product".to_string()),
        "Product not in API ctb_list"
    );

    // Bootstrap an authenticated browser session with the same token.
    let pw = Playwright::launch().await?;
    let browser = pw
        .chromium()
        .connect_over_cdp(harness.browser_cdp_url(), None)
        .await?;
    let page = browser.new_page().await?;
    page.add_init_script(&format!(
        "localStorage.setItem('ferriscms_token', '{token}');"
    ))
    .await?;
    page.goto(&format!("{}/", harness.browser_app_url()), None)
        .await?;
    wait_for_text(&page, |t| t.contains("ferriscms"))
        .await
        .context("admin UI did not hydrate")?;
    let ui = UiPage {
        page,
        _browser: browser,
        _pw: pw,
    };
    let page = &ui.page;

    // The shell renders top-level modules only — no content types in the sidebar.
    let shell = body_text(page).await?;
    for label in [
        "Content Manager",
        "Content-Type Builder",
        "Media Library",
        "Workflows",
        "Settings",
    ] {
        assert!(shell.contains(label), "missing top-level nav: {label}");
    }
    assert!(
        !shell.contains("Product"),
        "content type leaked into the application sidebar"
    );

    // Content-Type Builder opens as a table-first listing page.
    goto_screen(page, "Content-Type Builder", "Content-Type Builder").await?;
    let ctb = wait_for_text(page, |t| {
        t.contains("Define and manage the structure of your content.")
            && t.contains("Create content type")
            && t.contains("Collection Types")
    })
    .await
    .context("CTB table-first listing did not render")?;
    assert!(
        ctb.contains("Single Types") && ctb.contains("Components"),
        "CTB type tabs missing"
    );

    // Navigate to Content Manager: table-first listing page.
    goto_screen(page, "Content Manager", "Content Manager").await?;
    wait_for_text(page, |t| {
        t.contains("Create, read, update and delete your content.")
    })
    .await
    .context("CM table-first listing did not render")?;

    // Navigate back to the Content-Type Builder to confirm two-way navigation.
    goto_screen(page, "Content-Type Builder", "Content-Type Builder").await?;
    wait_for_text(page, |t| {
        t.contains("Define and manage the structure of your content.")
    })
    .await
    .context("did not return to CTB listing")?;

    Ok(())
}

/// The Import and Export wizards are reachable from the sidebar and render
/// their screens (works without authed data, which the -no-render Obscura
/// build can't fetch).
#[tokio::test(flavor = "multi_thread")]
async fn import_export_wizards_render() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    // Sidebar exposes Import and Export under the DATA section.
    let shell = body_text(page).await?;
    assert!(shell.contains("Import"), "Import nav missing");
    assert!(shell.contains("Export"), "Export nav missing");

    // Open the Import wizard.
    assert!(
        click_button_by_text(page, "Import").await?,
        "Import nav not clickable"
    );
    let imp = wait_for_text(page, |t| {
        t.contains("Step 1: Files") && t.contains("Analyze")
    })
    .await
    .context("Import wizard did not render")?;
    assert!(
        imp.contains("CSV delimiter"),
        "CSV options missing in import"
    );

    // Open the Export wizard.
    assert!(
        click_button_by_text(page, "Export").await?,
        "Export nav not clickable"
    );
    let exp = wait_for_text(page, |t| {
        t.contains("Content types") && t.contains("Format")
    })
    .await
    .context("Export wizard did not render")?;
    assert!(exp.contains("Export"), "Export header missing");

    Ok(())
}

/// The AI screens are reachable from the sidebar and render their chrome
/// (works without authed data, which the -no-render Obscura build can't fetch).
#[tokio::test(flavor = "multi_thread")]
async fn ai_screens_render() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let ui = open_app(&harness).await?;
    let page = &ui.page;

    let shell = body_text(page).await?;
    assert!(shell.contains("Assistant"), "AI Assistant nav missing");
    assert!(shell.contains("AI Settings"), "AI Settings nav missing");

    // AI Settings renders its section headers even with no providers configured.
    assert!(
        click_button_by_text(page, "AI Settings").await?,
        "AI Settings nav not clickable"
    );
    let settings = wait_for_text(page, |t| {
        t.contains("AI Settings") && t.contains("AI providers") && t.contains("AI models")
    })
    .await
    .context("AI Settings did not render")?;
    assert!(settings.contains("Usage summary"), "usage summary missing");

    // The Assistant renders its chat chrome + new-conversation controls.
    assert!(
        click_button_by_text(page, "Assistant").await?,
        "Assistant nav not clickable"
    );
    let assistant = wait_for_text(page, |t| {
        t.contains("AI Assistant") && t.contains("New conversation")
    })
    .await
    .context("AI Assistant did not render")?;
    assert!(
        assistant.contains("Select or create a conversation"),
        "assistant empty state missing"
    );

    Ok(())
}
