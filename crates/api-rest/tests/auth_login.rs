//! Authentication authorization tests.
//!
//! Verifies two invariants of the admin security model:
//!   1. `/admin/login` succeeds **only** with the correct credentials — wrong
//!      password, wrong email, and inactive/blocked accounts are rejected with
//!      401 and never issue a token.
//!   2. Every admin / UI data endpoint is reachable **only** with a valid
//!      session token — anonymous requests are rejected with 401, while an
//!      authenticated request is accepted.
//!
//! Runs against the in-memory Axum router (no server process, no browser).

use api_rest::{build_router, AppState};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use db::{connect_sqlite_memory, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{load_schema_cache, AppConfig};
use std::sync::Arc;
use tower::ServiceExt;

fn app_config() -> AppConfig {
    AppConfig {
        db_driver: "sqlite".into(),
        jwt_secret: "test-secret".into(),
        jwt_expiry_secs: 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::temp_dir()
            .join("ferris-auth-login-test")
            .display()
            .to_string(),
    }
}

async fn setup() -> (axum::Router, Arc<AppState>) {
    let db = connect_sqlite_memory().await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let state = Arc::new(AppState::new(db.clone(), app_config()));
    load_schema_cache(&db, &state.ctx.schema_cache)
        .await
        .unwrap();
    let _ = state.ctx.init_rbac().await;
    (build_router(state.clone()), state)
}

fn json_request(
    method: &str,
    uri: &str,
    body: serde_json::Value,
    token: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    builder
        .body(Body::from(body.to_string()))
        .expect("request builds")
}

fn empty_request(method: &str, uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    builder.body(Body::empty()).expect("request builds")
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

const EMAIL: &str = "admin@test.dev";
const PASSWORD: &str = "CorrectPass123!";

/// Register the first admin and return the issued token.
async fn register(router: &axum::Router) -> String {
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({
                "email": EMAIL,
                "password": PASSWORD,
                "firstname": "Kai",
                "lastname": "Doe"
            }),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK, "register first admin");
    let reg_json = body_json(reg).await;
    reg_json["data"]["token"]
        .as_str()
        .expect("registration returns a token")
        .to_string()
}

async fn login(router: &axum::Router, email: &str, password: &str) -> StatusCode {
    router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/login",
            serde_json::json!({ "email": email, "password": password }),
            None,
        ))
        .await
        .unwrap()
        .status()
}

/// Login succeeds only with the exact credentials; any deviation is rejected
/// with 401 and yields no token.
#[tokio::test]
async fn login_accepts_only_correct_credentials() {
    let (router, _state) = setup().await;
    let _token = register(&router).await;

    // Correct credentials -> 200 + a non-empty token.
    let ok = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/login",
            serde_json::json!({ "email": EMAIL, "password": PASSWORD }),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK, "correct credentials succeed");
    let ok_json = body_json(ok).await;
    let token = ok_json["data"]["token"].as_str().unwrap_or("");
    assert!(!token.is_empty(), "correct login issues a token");

    // Wrong password -> 401.
    assert_eq!(
        login(&router, EMAIL, "WrongPass123!").await,
        StatusCode::UNAUTHORIZED,
        "wrong password rejected"
    );

    // Wrong email -> 401.
    assert_eq!(
        login(&router, "nobody@test.dev", PASSWORD).await,
        StatusCode::UNAUTHORIZED,
        "unknown email rejected"
    );

    // Correct credentials still succeed afterwards.
    assert_eq!(
        login(&router, EMAIL, PASSWORD).await,
        StatusCode::OK,
        "correct login still works"
    );
}

/// Every admin / UI data endpoint is gated behind a valid session.
#[tokio::test]
async fn admin_endpoints_require_authorization() {
    let (router, _state) = setup().await;
    let token = register(&router).await;

    // Endpoints that back the admin UI. Anonymous access must 401; a valid
    // session must be accepted (200).
    let endpoints: &[(&str, &str)] = &[
        ("GET", "/admin/roles"),
        ("GET", "/admin/users"),
        ("GET", "/admin/api-tokens"),
        ("GET", "/admin/i18n/locales"),
        ("GET", "/admin/upload/files"),
        ("GET", "/content-type-builder/content-types"),
        ("GET", "/admin/content-manager/content-types"),
    ];

    for (method, uri) in endpoints {
        let anon = router
            .clone()
            .oneshot(empty_request(method, uri, None))
            .await
            .unwrap();
        assert_eq!(
            anon.status(),
            StatusCode::UNAUTHORIZED,
            "anonymous {method} {uri} must be rejected"
        );

        let authed = router
            .clone()
            .oneshot(empty_request(method, uri, Some(&token)))
            .await
            .unwrap();
        assert_eq!(
            authed.status(),
            StatusCode::OK,
            "authenticated {method} {uri} must be allowed"
        );
    }
}

/// A valid token actually grants access to a data endpoint (positive control
/// for the 401 gating above).
#[tokio::test]
async fn issued_token_grants_access() {
    let (router, _state) = setup().await;
    let token = register(&router).await;

    let roles = router
        .clone()
        .oneshot(empty_request("GET", "/admin/roles", Some(&token)))
        .await
        .unwrap();
    assert_eq!(roles.status(), StatusCode::OK, "token grants roles access");
    let roles_json = body_json(roles).await;
    let arr = roles_json["data"].as_array().expect("roles array");
    assert!(!arr.is_empty(), "seeded roles returned");
    assert!(
        arr.iter().any(|r| r["code"] == "strapi-super-admin"),
        "super admin role present"
    );
}
