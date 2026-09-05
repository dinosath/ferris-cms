//! OpenID Connect (SSO) HTTP surface tests.
//!
//! These cover the OIDC routes that do not require a live identity provider:
//! when OIDC is not configured (the default), `/admin/oidc/status` reports
//! `enabled: false`, kicking off an authorization is rejected, and a callback
//! with no matching authorization state is rejected. Full end-to-end SSO
//! (discovery + token exchange + ID-token verification) is exercised against a
//! real provider in service-level tests / the mock provider harness.

use api_rest::{build_router, AppState};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use db::{connect_sqlite_memory, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{load_schema_cache, AppConfig};
use std::sync::Arc;
use tower::ServiceExt;

async fn app_config() -> AppConfig {
    AppConfig {
        db_driver: "sqlite".into(),
        jwt_secret: "test-secret".into(),
        jwt_expiry_secs: 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::temp_dir()
            .join("ferris-oidc-test")
            .display()
            .to_string(),
    }
}

async fn setup() -> axum::Router {
    let db = connect_sqlite_memory().await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let state = Arc::new(AppState::new(db.clone(), app_config().await));
    load_schema_cache(&db, &state.ctx.schema_cache)
        .await
        .unwrap();
    build_router(state)
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

/// OIDC is disabled by default, and the endpoints refuse to act without a
/// configured provider.
#[tokio::test]
async fn oidc_disabled_when_unconfigured() {
    let router = setup().await;

    let status = router.clone().oneshot(get("/admin/oidc/status")).await.unwrap();
    assert_eq!(status.status(), StatusCode::OK, "status is 200");
    let status_json = body_json(status).await;
    assert_eq!(status_json["data"]["enabled"], serde_json::json!(false));

    // Without a provider configured, we cannot start SSO.
    let authorize = router
        .clone()
        .oneshot(get("/admin/oidc/authorize"))
        .await
        .unwrap();
    assert!(
        authorize.status().is_server_error(),
        "authorize without config is an error, got {}",
        authorize.status()
    );

    // A callback also cannot proceed without a configured provider (the config
    // check happens before any authorization-state lookup).
    let callback = router
        .clone()
        .oneshot(get("/admin/oidc/callback?code=abc&state=xyz"))
        .await
        .unwrap();
    assert!(
        callback.status().is_server_error(),
        "callback without config is an error, got {}",
        callback.status()
    );
}
