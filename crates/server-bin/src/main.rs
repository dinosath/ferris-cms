use api_rest::{build_router, AppState};
use axum_conf::Config as AxumConfig;
use db::{connect, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use serde::Deserialize;
use services::{
    bootstrap_admin, load_current_user, load_schema_cache, AppConfig, CurrentUser, ImportConfig,
};
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Clone, Debug, Default, Deserialize)]
struct FerrisConfig {
    #[serde(default)]
    import: ImportConfig,
    #[serde(default)]
    dev: DevConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct DevConfig {
    bootstrap: DevBootstrapConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
struct DevBootstrapConfig {
    /// Import the configured content types and data on development startup.
    enabled: bool,
    /// Path to a content-type bundle, relative to the process directory.
    content_types: String,
    /// Path to the sample data bundle, relative to the process directory.
    data: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct DevContentBundle {
    bootstrap: DevContentBootstrap,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct DevContentBootstrap {
    imports: Vec<DevContentImport>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevContentImport {
    dataset: String,
    uid: String,
    mapping: Vec<api_types::MappingDto>,
    #[serde(default)]
    match_field: Option<String>,
}

impl Default for DevBootstrapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            content_types: String::new(),
            data: String::new(),
        }
    }
}

fn load_config() -> AxumConfig<FerrisConfig> {
    // FERRISCMS_ENV is the documented/make-task setting. Keep RUST_ENV as a
    // compatible fallback for deployments that already use it.
    let configured_environment = std::env::var("FERRISCMS_ENV")
        .or_else(|_| std::env::var("RUST_ENV"))
        .unwrap_or_else(|_| "prod".into());
    let normalized_environment = configured_environment.to_ascii_lowercase();
    let environment = match normalized_environment.as_str() {
        "development" => "dev".to_string(),
        "production" => "prod".to_string(),
        other => other.to_string(),
    };
    match AxumConfig::<FerrisConfig>::from_toml_file(&environment) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("could not load config/{environment}.toml ({error}); using defaults");
            AxumConfig::<FerrisConfig>::default()
                .with_bind_addr("0.0.0.0")
                .with_bind_port(8080)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime_config = load_config();
    runtime_config.setup_tracing();

    // Default to a local SQLite file next to the working directory.
    // `mode=rwc` is required: without it sqlx refuses to create the file.
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://ferriscms.db?mode=rwc".into());

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
        import: runtime_config.app.import.clone(),
    };

    let state = Arc::new(AppState::new(db.clone(), config));

    // Load existing schemas into cache.
    load_schema_cache(&db, &state.ctx.schema_cache).await?;

    // Seed demo workflows on first boot (idempotent).
    match services::seed_demo_workflows(&state.ctx).await {
        Ok(n) if n > 0 => tracing::info!("seeded {n} demo workflows"),
        _ => {}
    }

    // Initialize SeaORM 2.0 RBAC engine with standard roles/permissions.
    tracing::info!("initializing RBAC engine");
    match state.ctx.init_rbac().await {
        Ok(()) => tracing::info!("RBAC engine initialized"),
        Err(e) => tracing::warn!("RBAC init skipped: {e}"),
    }

    // Provision the initial Super Admin from ADMIN_* env vars (if configured).
    // The Helm chart injects a ConfigMap with a generated password, so this
    // auto-creates `admin` on a fresh database instead of forcing a UI
    // registration step.
    match bootstrap_admin(&state.ctx).await {
        Ok(Some(a)) => tracing::info!(
            "provisioned initial Super Admin username={} email={} from environment (manual registration disabled)",
            a.username,
            a.email
        ),
        Ok(None) => {}
        Err(e) => tracing::warn!("admin bootstrap skipped: {e}"),
    }

    if runtime_config.app.dev.bootstrap.enabled {
        run_dev_bootstrap(&state.ctx, &runtime_config.app.dev.bootstrap).await?;
    }

    let app = build_router(state);

    let addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| runtime_config.http.full_bind_addr())
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

async fn first_admin(
    ctx: &services::AppContext,
) -> Result<CurrentUser, Box<dyn std::error::Error>> {
    use db::entities::admin_user;
    use db::sea_orm::{EntityTrait, QueryOrder};

    let user = admin_user::Entity::find()
        .order_by_asc(admin_user::Column::Id)
        .one(&ctx.db)
        .await?
        .ok_or("development bootstrap requires a local admin user")?;
    Ok(load_current_user(&ctx.db, user.id).await?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_config_loads_json_import_settings() {
        let config =
            AxumConfig::<FerrisConfig>::from_toml(include_str!("../../../config/prod.toml"))
                .unwrap();
        assert!(config.app.import.json.enabled);
        assert_eq!(config.app.import.json.max_file_bytes, 10 * 1024 * 1024);
        assert_eq!(config.http.full_bind_addr(), "0.0.0.0:8080");
    }

    #[test]
    fn development_config_loads_erp_bootstrap() {
        let config =
            AxumConfig::<FerrisConfig>::from_toml(include_str!("../../../config/dev.toml"))
                .unwrap();
        assert!(config.app.dev.bootstrap.enabled);
        assert_eq!(
            config.app.dev.bootstrap.content_types,
            "examples/content-types/erp.json"
        );
        assert_eq!(
            config.app.dev.bootstrap.data,
            "examples/content-types/erp-sample.json"
        );
    }

    #[tokio::test]
    async fn development_bootstrap_imports_erp_types_and_data() {
        let db = db::connect_sqlite_memory().await.unwrap();
        db::Migrator::up(&db, None).await.unwrap();
        db::seed::seed(&db).await.unwrap();

        let ctx = services::AppContext::new(
            db,
            services::AppConfig {
                db_driver: "sqlite".into(),
                ..Default::default()
            },
        );
        services::provision_admin(&ctx, "dev-admin", "dev-admin@ferriscms.test", "admin")
            .await
            .unwrap();

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bootstrap = DevBootstrapConfig {
            enabled: true,
            content_types: root
                .join("examples/content-types/erp.json")
                .to_string_lossy()
                .into_owned(),
            data: root
                .join("examples/content-types/erp-sample.json")
                .to_string_lossy()
                .into_owned(),
        };
        run_dev_bootstrap(&ctx, &bootstrap).await.unwrap();

        let admin_ctx = ctx.with_user(Some(first_admin(&ctx).await.unwrap()));
        for (uid, expected) in [
            ("api::packaging.packaging", 3),
            ("api::product.product", 3),
            ("api::organization.organization", 2),
        ] {
            let entries = services::cm_list(&admin_ctx, uid, &api_types::QueryParams::default())
                .await
                .unwrap();
            assert_eq!(entries.data.len(), expected, "unexpected rows for {uid}");
        }
    }
}

/// Import the configured content bundle and its optional data bootstrap for
/// local development.
///
/// The import uses stable unique fields and `Upsert`, so restarting a dev
/// server refreshes the examples instead of duplicating them. It runs with
/// the local Super Admin identity because the regular import service enforces
/// the same content-manager permissions as an HTTP import.
async fn run_dev_bootstrap(
    ctx: &services::AppContext,
    config: &DevBootstrapConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let content_types = std::fs::read_to_string(&config.content_types)?;
    let sample_data = std::fs::read_to_string(&config.data)?;
    let admin = first_admin(ctx).await?;
    let import_ctx = ctx.with_user(Some(admin));

    let bundle: serde_json::Value = serde_json::from_str(&content_types)?;
    let bootstrap: DevContentBundle = serde_json::from_value(bundle.clone())?;
    services::ctb_import(&import_ctx, &bundle).await?;

    let filename = std::path::Path::new(&config.data)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bootstrap.json")
        .to_string();
    let response = services::run_import(
        &import_ctx,
        &api_types::ImportRequest {
            files: bootstrap
                .bootstrap
                .imports
                .into_iter()
                .map(|import| api_types::FileImportConfig {
                    filename: filename.clone(),
                    dataset: import.dataset,
                    content: sample_data.clone(),
                    uid: import.uid,
                    mapping: import.mapping,
                    mode: api_types::ImportMode::Upsert,
                    match_field: import.match_field,
                    state_field: None,
                    import_state: api_types::ImportState::Draft,
                    locale_field: None,
                    locale: "en".into(),
                    csv_delimiter: None,
                    csv_has_header: None,
                })
                .collect(),
        },
    )
    .await?;

    if response.failed > 0 {
        return Err(format!(
            "development data bootstrap failed for {} rows: {:?}",
            response.failed, response.errors
        )
        .into());
    }

    tracing::info!(
        created = response.created,
        updated = response.updated,
        "development content bundle and sample data loaded"
    );
    Ok(())
}
