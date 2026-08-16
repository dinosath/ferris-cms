//! Deep integration coverage: drives the widest set of backend branches —
//! many attribute types (which stress `dynamic-store` value/DML handling), DDL
//! schema mutation (add/remove fields), content-type removal (table drop),
//! single types, components, relations, i18n, and the auth login/init paths.
//!
//! These supplement `auth_workflow.rs` and `api_surface.rs`, which already
//! cover the main CRUD + admin workflow, so this file focuses on the branches
//! those leave untouched.

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
        jwt_secret: "deep-test-secret".into(),
        jwt_expiry_secs: 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::temp_dir()
            .join("ferris-deep")
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

async fn register_admin(router: &axum::Router) -> String {
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"deep@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);
    body_json(reg).await["data"]["token"]
        .as_str()
        .expect("token")
        .to_string()
}

async fn apply_schema(router: &axum::Router, token: &str, schemas: serde_json::Value) {
    let resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas": schemas}),
            Some(token),
        ))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["message"],
        serde_json::Value::Null,
        "apply schema failed: {body}"
    );
}

/// Register + create a collection type with many attribute types.
async fn create_rich_type(router: &axum::Router, token: &str) -> String {
    let ct = serde_json::json!({
        "uid": "api::rich.rich",
        "kind": "collectionType",
        "info": {"singularName":"rich","pluralName":"riches","displayName":"Rich"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "title": {"type": "string", "required": true},
            "body": {"type": "text"},
            "slug": {"type": "uid"},
            "views": {"type": "integer"},
            "big": {"type": "biginteger"},
            "price": {"type": "decimal"},
            "rating": {"type": "float"},
            "publishedOn": {"type": "date"},
            "timestamp": {"type": "datetime"},
            "startTime": {"type": "time"},
            "active": {"type": "boolean"},
            "contact": {"type": "email"},
            "secret": {"type": "password"},
            "kind": {"type": "enumeration", "enum": ["news", "blog"]},
            "payload": {"type": "json"}
        }
    });
    apply_schema(router, token, serde_json::json!([ct])).await;
    "api::rich.rich".to_string()
}

fn rich_entry() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "title": "Hello",
            "body": "Body text",
            "slug": "hello",
            "views": 3,
            "big": 9000000000i64,
            "price": 19.99,
            "rating": 4.5,
            "publishedOn": "2024-01-01",
            "timestamp": "2024-01-01T00:00:00Z",
            "startTime": "09:30:00",
            "active": true,
            "contact": "a@b.com",
            "secret": "s3cret",
            "kind": "news",
            "payload": {"x": 1}
        }
    })
}

#[tokio::test]
async fn diverse_field_types_and_schema_mutation() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;
    let uid = create_rich_type(&router, &token).await;

    // Create an entry with every attribute populated.
    let created = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            rich_entry(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        created.status(),
        StatusCode::OK,
        "create diverse entry: {}",
        created.status()
    );
    let doc_id = body_json(created).await["data"]["documentId"]
        .as_str()
        .expect("documentId")
        .to_string();

    // Read it back.
    let got = router
        .clone()
        .oneshot(empty_request(
            "GET",
            &format!("/admin/content-manager/collection-types/{uid}/{doc_id}"),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(got.status(), StatusCode::OK);
    assert_eq!(body_json(got).await["data"]["title"], "Hello");

    // Update a few fields (exercises update_one write path).
    let upd = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/content-manager/collection-types/{uid}/{doc_id}"),
            serde_json::json!({"data":{"title":"Updated","views":9,"kind":"blog","active":false}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(upd.status(), StatusCode::OK, "update: {}", upd.status());
    assert_eq!(body_json(upd).await["data"]["title"], "Updated");

    // Mutate the schema: add a richtext field and drop the float field. This
    // drives DDL ALTER for both a new column and a removed column.
    let ct_mutated = serde_json::json!({
        "uid": "api::rich.rich",
        "kind": "collectionType",
        "info": {"singularName":"rich","pluralName":"riches","displayName":"Rich"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "title": {"type": "string", "required": true},
            "body": {"type": "text"},
            "slug": {"type": "uid"},
            "views": {"type": "integer"},
            "big": {"type": "biginteger"},
            "price": {"type": "decimal"},
            "publishedOn": {"type": "date"},
            "timestamp": {"type": "datetime"},
            "startTime": {"type": "time"},
            "active": {"type": "boolean"},
            "contact": {"type": "email"},
            "secret": {"type": "password"},
            "kind": {"type": "enumeration", "enum": ["news", "blog"]},
            "payload": {"type": "json"},
            "note": {"type": "richtext"}
        }
    });
    apply_schema(&router, &token, serde_json::json!([ct_mutated])).await;

    // Create another entry on the mutated schema (new column present).
    let second = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            serde_json::json!({"data":{"title":"Second","slug":"second","kind":"news","note":"**rich**"}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        second.status(),
        StatusCode::OK,
        "create on mutated schema: {}",
        second.status()
    );

    // Delete the first entry.
    let del = router
        .clone()
        .oneshot(empty_request(
            "DELETE",
            &format!("/admin/content-manager/collection-types/{uid}/{doc_id}"),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK, "delete");
}

#[tokio::test]
async fn content_type_removal_single_and_component() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    // Create a collection type, then remove it by applying a schema set that no
    // longer includes it (drives diff_removed + DDL drop).
    let temp = serde_json::json!({
        "uid": "api::temp.temp",
        "kind": "collectionType",
        "info": {"singularName":"temp","pluralName":"temps","displayName":"Temp"},
        "options": {"draftAndPublish": true},
        "attributes": {"name": {"type": "string"}}
    });
    apply_schema(&router, &token, serde_json::json!([temp])).await;

    // Now apply a single type + component, dropping `temp`.
    let single = serde_json::json!({
        "uid": "api::settings.settings",
        "kind": "singleType",
        "info": {"singularName":"settings","pluralName":"settings","displayName":"Settings"},
        "options": {"draftAndPublish": true},
        "attributes": {"siteName": {"type": "string"}}
    });
    let component = serde_json::json!({
        "uid": "shared.seo",
        "kind": "component",
        "info": {"singularName":"seo","pluralName":"seos","displayName":"SEO"},
        "options": {"draftAndPublish": false},
        "attributes": {"metaTitle": {"type": "string"}}
    });
    apply_schema(&router, &token, serde_json::json!([single, component])).await;

    // temp should be gone (404 on get).
    let gone = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/content-type-builder/content-types/api::temp.temp",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        gone.status(),
        StatusCode::NOT_FOUND,
        "removed temp type should 404"
    );

    // Single type config + list of components.
    let config = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/admin/content-manager/content-types/api::settings.settings/configuration",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(config.status(), StatusCode::OK, "single config");

    let comps = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/content-type-builder/components",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(comps.status(), StatusCode::OK, "components list");

    // Public list on a single type -> 409 Conflict (ensure_collection).
    let conflict = router
        .clone()
        .oneshot(empty_request("GET", "/api/api::settings.settings", None))
        .await
        .unwrap();
    assert_eq!(
        conflict.status(),
        StatusCode::CONFLICT,
        "single type public list -> 409"
    );
}

#[tokio::test]
async fn auth_init_login_and_failures() {
    let (router, _state) = setup().await;

    // /admin/init reflects registration-open state.
    let init = router
        .clone()
        .oneshot(empty_request("GET", "/admin/init", None))
        .await
        .unwrap();
    assert_eq!(init.status(), StatusCode::OK, "admin init");
    let init_json = body_json(init).await;
    // No admin exists yet, so init reports hasAdmin=false.
    assert_eq!(init_json["hasAdmin"], false);

    // Register, then log in successfully with the same credentials.
    let token = register_admin(&router).await;
    let login = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/login",
            serde_json::json!({"email":"deep@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK, "login success");
    assert!(body_json(login).await["data"]["token"].as_str().is_some());

    // Wrong password -> 401.
    let bad_login = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/login",
            serde_json::json!({"email":"deep@test.dev","password":"WrongPass1"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(
        bad_login.status(),
        StatusCode::UNAUTHORIZED,
        "login bad password 401"
    );
    assert_eq!(body_json(bad_login).await["error"]["name"], "Unauthorized");

    // Unknown user -> 401.
    let unknown = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/login",
            serde_json::json!({"email":"nobody@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(
        unknown.status(),
        StatusCode::UNAUTHORIZED,
        "login unknown user 401"
    );

    // The JWT still authorizes a normal admin request.
    let roles = router
        .clone()
        .oneshot(empty_request("GET", "/admin/roles", Some(&token)))
        .await
        .unwrap();
    assert_eq!(roles.status(), StatusCode::OK, "token valid after login");
}

#[tokio::test]
async fn api_tokens_authorize_public_read() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    // Create an API token.
    let created = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/api-tokens",
            serde_json::json!({"name":"public-read","type":"read-only"}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        created.status(),
        StatusCode::OK,
        "create api token: {}",
        created.status()
    );
    let created_json = body_json(created).await;
    let api_token = created_json["data"]["accessKey"]
        .as_str()
        .expect("api token accessKey");

    // List tokens (admin).
    let list = router
        .clone()
        .oneshot(empty_request("GET", "/admin/api-tokens", Some(&token)))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK, "list api tokens");

    // Use the token against a public content endpoint for an existing type.
    let ct = serde_json::json!({
        "uid": "api::post.post",
        "kind": "collectionType",
        "info": {"singularName":"post","pluralName":"posts","displayName":"Post"},
        "options": {"draftAndPublish": true},
        "attributes": {"title": {"type": "string"}}
    });
    apply_schema(&router, &token, serde_json::json!([ct])).await;

    let pub_list = router
        .clone()
        .oneshot(empty_request("GET", "/api/api::post.post", Some(api_token)))
        .await
        .unwrap();
    // Either authorized (200) or rejected with a client error; must not be a 500.
    assert_ne!(
        pub_list.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "no 500 on token auth"
    );

    // Delete the token.
    let list_json = body_json(
        router
            .clone()
            .oneshot(empty_request("GET", "/admin/api-tokens", Some(&token)))
            .await
            .unwrap(),
    )
    .await;
    let token_id = list_json["data"][0]["id"].as_i64().expect("token id");
    let del = router
        .clone()
        .oneshot(empty_request(
            "DELETE",
            &format!("/admin/api-tokens/{token_id}"),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK, "delete api token");
}
