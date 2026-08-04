//! End-to-end integration test for the full admin workflow against the Axum
//! router in-memory (no server process). This duplicates the manual HTTP
//! smoke test so the runtime validation is durable and repeatable.
//!
//! Workflow exercised: register -> login -> unauthenticated admin request
//! rejected (401) -> create content-type via CTB -> create entry -> list ->
//! publish -> public read. Also asserts Strapi's camelCase response shape.

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
        media_storage_dir: std::env::temp_dir().join("ferris-media-test").display().to_string(),
    }
}

async fn setup() -> (axum::Router, Arc<AppState>) {
    let db = connect_sqlite_memory().await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let state = Arc::new(AppState::new(db.clone(), app_config()));
    load_schema_cache(&db, &state.ctx.schema_cache).await.unwrap();
    let _ = state.ctx.init_rbac().await;
    (build_router(state.clone()), state)
}

fn json_request(
    method: &str,
    uri: &str,
    body: serde_json::Value,
    token: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri).header(
        header::CONTENT_TYPE,
        "application/json",
    );
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

#[tokio::test]
async fn full_admin_workflow() {
    let (router, _state) = setup().await;

    // 1. Register the first admin (returns a token).
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"admin@test.dev","password":"StrongPass123!","firstname":"Kai","lastname":"Doe"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(reg.status(), StatusCode::OK);
    let reg_json = body_json(reg).await;
    let token = reg_json["data"]["token"].as_str().expect("token").to_string();
    assert!(!token.is_empty());

    // 2. Unauthenticated admin request must be rejected.
    let anon = router
        .clone()
        .oneshot(empty_request("GET", "/admin/roles", None))
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);

    // 3. Authenticated request is allowed.
    let authed = router
        .clone()
        .oneshot(empty_request("GET", "/admin/roles", Some(&token)))
        .await
        .unwrap();
    assert_eq!(authed.status(), StatusCode::OK);

    // 3b. Roles endpoint returns the seeded roles (data source for the UI).
    let roles_resp = body_json(authed).await;
    let roles = roles_resp["data"].as_array().expect("roles array");
    assert!(!roles.is_empty());
    let editor = roles
        .iter()
        .find(|r| r["code"] == "strapi-editor")
        .expect("editor role seeded");
    let editor_id = editor["id"].as_i64().expect("editor id");

    // 4. Create a content-type via the Content-Type Builder.
    let ct = serde_json::json!({
        "uid": "api::article.article",
        "kind": "collectionType",
        "info": {"singularName":"article","pluralName":"articles","displayName":"Article"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "title": {"type": "string", "required": true},
            "views": {"type": "integer"}
        }
    });
    let create_ct = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas":[ct]}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(create_ct.status(), StatusCode::OK, "CTB apply");

    // 5. Create an entry. Assert the Strapi camelCase response shape.
    let create_entry = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/content-manager/collection-types/api::article.article",
            serde_json::json!({"data":{"title":"Hello World","views":42}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(create_entry.status(), StatusCode::OK);
    let entry_json = body_json(create_entry).await;
    let entry = &entry_json["data"];
    let doc_id = entry["documentId"]
        .as_str()
        .expect("documentId in camelCase response")
        .to_string();
    assert!(!doc_id.is_empty());
    assert_eq!(entry["publicationState"], "draft");

    // 6. List entries.
    let list = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/admin/content-manager/collection-types/api::article.article?pagination[page]=1&pagination[pageSize]=25",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list_json = body_json(list).await;
    assert!(!list_json["data"].as_array().unwrap().is_empty());

    // 7. Publish by documentId.
    let publish = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!(
                "/admin/content-manager/collection-types/api::article.article/{doc_id}/actions/publish"
            ),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::OK, "publish by documentId");

    // 8. Public list endpoint is reachable without auth.
    let public = router
        .clone()
        .oneshot(empty_request("GET", "/api/api::article.article", None))
        .await
        .unwrap();
    assert_eq!(public.status(), StatusCode::OK, "public read");

    // 9. RBAC UI data source: update a role's permissions persists (200).
    let update_perms = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/roles/{editor_id}/permissions"),
            serde_json::json!({ "permissions": [
                { "action": "plugin::content-manager.explorer.read", "subject": "*", "properties": {}, "conditions": [] }
            ] }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(update_perms.status(), StatusCode::OK, "role permissions update");

    // 10. Users UI data source: list + create admin users.
    let users = router
        .clone()
        .oneshot(empty_request("GET", "/admin/users", Some(&token)))
        .await
        .unwrap();
    assert_eq!(users.status(), StatusCode::OK, "users list");

    let create_user = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/users",
            serde_json::json!({
                "email": "author@test.dev",
                "firstname": "An",
                "lastname": "Author",
                "isActive": true
            }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(create_user.status(), StatusCode::OK, "create admin user");
    let created = body_json(create_user).await;
    assert_eq!(created["data"]["email"], "author@test.dev");

    // 11. Media upload + list (Media Library data source).
    let multipart_body = concat!(
        "--XXXX\r\n",
        "Content-Disposition: form-data; name=\"files\"; filename=\"cat.png\"\r\n",
        "Content-Type: image/png\r\n\r\n",
        "fakepngbytes\r\n",
        "--XXXX--\r\n",
    );
    let upload = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/upload/files")
                .header(header::CONTENT_TYPE, "multipart/form-data; boundary=XXXX")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(multipart_body.to_string()))
                .expect("upload request"),
        )
        .await
        .unwrap();
    assert_eq!(upload.status(), StatusCode::OK, "media upload");
    let upload_json = body_json(upload).await;
    let uploaded = &upload_json["data"][0];
    assert_eq!(uploaded["name"], "cat.png");
    assert!(!uploaded["url"].as_str().unwrap_or("").is_empty());

    let media = router
        .clone()
        .oneshot(empty_request("GET", "/admin/upload/files", Some(&token)))
        .await
        .unwrap();
    assert_eq!(media.status(), StatusCode::OK, "media list");
    let media_json = body_json(media).await;
    assert!(!media_json["data"].as_array().unwrap().is_empty());
}
