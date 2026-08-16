//! Self-contained e2e harness: no containers, no testcontainers.
//!
//! Each test boots its own stack entirely in the local process / machine:
//!
//! - **Database:** a fresh **Turso** database (SQLite-compatible, driven by the
//!   [`turso`](https://crates.io/crates/turso) engine) is created in a temp
//!   directory. The ferriscms server connects to it over the existing SQLite
//!   backend (`sqlite://...`), runs migrations and seeds it, and serves the
//!   app **in-process** on an ephemeral `127.0.0.1` port.
//! - **Browser:** the **Obscura** headless browser
//!   ([`h4ckf0r0day/obscura`](https://github.com/h4ckf0r0day/obscura)) is
//!   launched as a local subprocess (`obscura serve`). It is a drop-in
//!   replacement for headless Chrome and exposes a Chrome DevTools Protocol
//!   (CDP) websocket that Playwright (`playwright-rs`) and Puppeteer connect
//!   to, so no Chrome container is needed.
//!
//! Dropping the harness aborts the in-process server task, kills the Obscura
//! subprocess and removes the temp directory.

use anyhow::{anyhow, Context, Result};
use api_rest::{build_router, AppState};
use db::{connect, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{load_schema_cache, AppConfig};
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

/// A running ferriscms stack: Turso DB + in-process server + Obscura browser.
pub struct E2eHarness {
    server_url: String,
    browser_cdp_url: String,
    server_task: tokio::task::JoinHandle<()>,
    obscura: Child,
    _tmp: tempfile::TempDir,
}

impl E2eHarness {
    /// Boot the full stack. Blocks until the server answers and Obscura's CDP
    /// port is accepting connections.
    pub async fn start() -> Result<Self> {
        let tmp = tempfile::tempdir().context("create e2e temp dir")?;

        // ---- Database: Turso (SQLite-compatible), no container ----
        let db_path = tmp.path().join("ferriscms.db");
        let db_path_str = db_path.to_string_lossy().to_string();
        provision_turso_db(&db_path_str).await?;

        // The ferriscms server talks to Turso through its existing SQLite
        // backend (Turso writes a standard SQLite-compatible file).
        let database_url = format!("sqlite://{db_path_str}");
        let dbh = connect(&database_url)
            .await
            .with_context(|| format!("connect to turso database at {database_url}"))?;
        Migrator::up(&dbh, None)
            .await
            .context("run system migrations")?;
        seed::seed(&dbh).await.context("seed roles + locales")?;

        let config = AppConfig {
            db_driver: "sqlite".into(),
            jwt_secret: "e2e-test-secret".into(),
            jwt_expiry_secs: 30 * 24 * 3600,
            admin_registration_open: true,
            media_storage_dir: tmp.path().join("media").display().to_string(),
        };

        let state = Arc::new(AppState::new(dbh.clone(), config));
        load_schema_cache(&dbh, &state.ctx.schema_cache)
            .await
            .context("load schema cache")?;
        let _ = state.ctx.init_rbac().await;

        let app = build_router(state);

        // Bind to an ephemeral localhost port, then serve in a background task.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind server listener")?;
        let addr = listener.local_addr().context("read server bound address")?;
        let server_url = format!("http://{addr}");

        let server_task = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("in-process ferriscms server error: {e}");
            }
        });

        // ---- Browser: Obscura, no Chrome container ----
        let (obscura, browser_cdp_url) = spawn_obscura()?;
        wait_for_listening(&browser_cdp_url).await?;

        // ---- Server ready ----
        wait_for_server(&server_url).await?;

        Ok(Self {
            server_url,
            browser_cdp_url,
            server_task,
            obscura,
            _tmp: tmp,
        })
    }

    /// Base URL of the ferriscms server under test (e.g. `http://127.0.0.1:XXXX`).
    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// The URL the browser (Obscura) should navigate to; same as `server_url`
    /// because the browser runs locally alongside the server.
    pub fn browser_app_url(&self) -> &str {
        &self.server_url
    }

    /// CDP websocket endpoint of the Obscura browser, for Playwright / Puppeteer
    /// (e.g. `ws://127.0.0.1:XXXX`).
    pub fn browser_cdp_url(&self) -> &str {
        &self.browser_cdp_url
    }
}

impl Drop for E2eHarness {
    fn drop(&mut self) {
        self.server_task.abort();
        // Reap the Obscura child process.
        let _ = self.obscura.kill();
        let _ = self.obscura.wait();
    }
}

/// Create a Turso database file and prove it is a live, writable Turso DB.
async fn provision_turso_db(path: &str) -> Result<()> {
    let db = turso::Builder::new_local(path.as_ref())
        .build()
        .await
        .map_err(|e| anyhow!("open Turso database at {path}: {e}"))?;
    let conn = db
        .connect()
        .map_err(|e| anyhow!("connect to Turso database: {e}"))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _turso_e2e_probe (id INTEGER PRIMARY KEY)",
        (),
    )
    .await
    .map_err(|e| anyhow!("Turso create probe table: {e}"))?;
    conn.execute("INSERT OR IGNORE INTO _turso_e2e_probe (id) VALUES (1)", ())
        .await
        .map_err(|e| anyhow!("Turso insert probe row: {e}"))?;
    // Drop the Turso handle so the ferriscms server can open the file via its
    // SQLite backend without a second engine holding it open.
    drop(conn);
    drop(db);
    Ok(())
}

/// Launch `obscura serve` on an ephemeral port; return the child and its CDP
/// websocket URL.
fn spawn_obscura() -> Result<(Child, String)> {
    // Reserve a free port, then hand it to Obscura.
    let port = {
        let probe = StdTcpListener::bind("127.0.0.1:0").context("probe free port")?;
        probe.local_addr().context("read probe port")?.port()
    };

    let mut cmd = Command::new("obscura");
    cmd.arg("serve").arg("--port").arg(port.to_string());
    // Obscura blocks loopback/private IPs by default (SSRF protection); the
    // in-process ferriscms server runs on 127.0.0.1, so allow private network.
    cmd.arg("--allow-private-network");
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd
        .spawn()
        .context("spawn `obscura serve` (is the `obscura` binary on PATH?)")?;

    Ok((child, format!("ws://127.0.0.1:{port}")))
}

/// Poll until something is listening on the CDP websocket's TCP port.
async fn wait_for_listening(cdp_url: &str) -> Result<()> {
    let addr: SocketAddr = cdp_url
        .trim_start_matches("ws://")
        .parse()
        .with_context(|| format!("parse Obscura CDP address from {cdp_url}"))?;
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(anyhow!("Obscura did not start listening at {cdp_url}"))
}

/// Poll the `/admin/init` endpoint until the in-process server answers.
async fn wait_for_server(base: &str) -> Result<()> {
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client
            .get(format!("{base}/admin/init"))
            .send()
            .await
            .is_ok()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(anyhow!("ferriscms server did not become ready at {base}"))
}
