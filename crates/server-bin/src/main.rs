//! ferriscms server binary — online Axum server (design Part II §1).
//!
//! Starts the REST API + admin endpoints on the configured port, and serves
//! the embedded Dioxus WASM admin UI at the site root.
//! Uses PostgreSQL (or SQLite).

use api_rest::{build_router, AppState};
use db::{connect, seed, Migrator};
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

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/ferriscms".into());

    tracing::info!("connecting to database: {database_url}");
    let db = connect(&database_url).await?;

    // Run system migrations.
    Migrator::up(&db, None).await?;

    // Seed roles + locales.
    seed::seed(&db).await?;

    // Build app context.
    let config = AppConfig {
        db_driver: if database_url.contains("postgres") {
            "postgres".into()
        } else {
            "sqlite".into()
        },
        jwt_secret: std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "change-me-in-production".into()),
        jwt_expiry_secs: 30 * 24 * 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::var("MEDIA_STORAGE_DIR").unwrap_or_else(|_| "media".into()),
    };

    let state = Arc::new(AppState::new(db.clone(), config));

    // Load existing schemas into cache.
    load_schema_cache(&db, &state.ctx.schema_cache).await?;

    // Initialize SeaORM 2.0 RBAC engine with standard roles/permissions.
    tracing::info!("initializing RBAC engine");
    match state.ctx.init_rbac().await {
        Ok(()) => tracing::info!("RBAC engine initialized"),
        Err(e) => tracing::warn!("RBAC init skipped: {e}"),
    }

    let app = build_router(state);

    let addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:1337".into())
        .parse()?;

    // Optional HTTPS: when TLS_CERT_FILE and TLS_KEY_FILE point at a cert/key
    // (e.g. from Let's Encrypt), serve TLS on BIND_ADDR. Otherwise serve plain
    // HTTP.
    let cert_file = std::env::var("TLS_CERT_FILE").ok();
    let key_file = std::env::var("TLS_KEY_FILE").ok();
    if let (Some(cert), Some(key)) = (cert_file, key_file) {
        tracing::info!("ferriscms server serving HTTPS (TLS) on {addr}");
        serve_tls(addr, app, &cert, &key).await?;
    } else {
        tracing::info!("ferriscms server listening on {addr}");
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
    }

    Ok(())
}

/// Serve the app router over TLS using an X.509 cert chain + private key in PEM
/// (rustls-pemfile) with the rustls ring crypto provider.
async fn serve_tls(
    addr: SocketAddr,
    app: axum::Router,
    cert_path: &str,
    key_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // rustls 0.23 requires a crypto provider; ring is enabled as a feature.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;

    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
