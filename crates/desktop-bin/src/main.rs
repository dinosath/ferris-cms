//! ferriscms desktop binary — offline-first desktop app with embedded HTTP server.
//!
//! Runs the full backend (SQLite + migrations + seed + RBAC) and exposes the
//! REST API on a local HTTP port so tools, scripts, and the web UI can
//! consume it. No external database or server required.

use api_rest::{build_router, AppState};
use db::{seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{load_schema_cache, AppConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // ---------- database ----------
    let db_path = std::env::var("STRAPI_DB_PATH")
        .unwrap_or_else(|_| "ferriscms-desktop.db".into());
    let database_url = format!("sqlite:{db_path}?mode=rwc");

    tracing::info!("opening database: {db_path}");
    let db = db::connect(&database_url).await?;

    Migrator::up(&db, None).await?;
    seed::seed(&db).await?;

    // ---------- app context ----------
    let config = AppConfig {
        db_driver: "sqlite".into(),
        jwt_secret: std::env::var("JWT_SECRET").unwrap_or_else(|_| "desktop-local-dev".into()),
        jwt_expiry_secs: 30 * 24 * 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::var("MEDIA_STORAGE_DIR").unwrap_or_else(|_| "media".into()),
    };

    let state = Arc::new(AppState::new(db.clone(), config));

    // Load existing schemas into cache.
    load_schema_cache(&db, &state.ctx.schema_cache).await?;

    // Initialize SeaORM 2.0 RBAC engine.
    match state.ctx.init_rbac().await {
        Ok(()) => tracing::info!("RBAC engine initialized"),
        Err(e) => tracing::warn!("RBAC init skipped: {e}"),
    }

    // ---------- HTTP server ----------
    let bind_addr: SocketAddr = std::env::var("STRAPI_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:1338".into())
        .parse()?;

    let app = build_router(state.clone());

    tracing::info!("============================================");
    tracing::info!(" ferriscms desktop ready");
    tracing::info!(" database : {db_path}");
    tracing::info!(" API URL  : http://{bind_addr}");
    tracing::info!("============================================");

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
