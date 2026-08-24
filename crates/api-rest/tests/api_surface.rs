//! API surface integration tests: exercise every route group and the main
//! error/edge paths so the backend reaches high coverage. Unlike
//! `auth_workflow.rs` (happy-path admin workflow), this suite drives the
//! public content API, single-type handlers, CTB get/reserved-names, RBAC role
//! lookup, the SPA fallback, and several 4xx/5xx error branches.

use api_rest::{build_router, AppState};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use core_domain::Uid;
use db::{connect_sqlite_memory, seed, Migrator};
use dynamic_store::dml;
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
            .join("ferris-api-surface")
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

/// Register a fresh admin and return the JWT.
async fn register_admin(router: &axum::Router) -> String {
    let reg = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"surface@test.dev","password":"StrongPass123!"}),
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

/// Create a content type via CTB and return its uid.
async fn create_article(router: &axum::Router, token: &str) -> String {
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
    let resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas":[ct]}),
            Some(token),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "create content type");
    "api::article.article".to_string()
}

#[tokio::test]
async fn public_api_crud_and_errors() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;
    let uid = create_article(&router, &token).await;

    // Public create.
    let created = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/{uid}"),
            serde_json::json!({"data":{"title":"Hello","views":3}}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK, "public create");
    let created_json = body_json(created).await;
    let doc_id = created_json["data"]["documentId"]
        .as_str()
        .expect("documentId")
        .to_string();

    // Public list with query params.
    let list = router
        .clone()
        .oneshot(empty_request(
            "GET",
            &format!("/api/{uid}?pagination[page]=1&pagination[pageSize]=10"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK, "public list");
    let list_json = body_json(list).await;
    assert!(
        list_json["meta"]["pagination"].is_object(),
        "list has pagination meta"
    );

    // Public get by document id.
    let get_one = router
        .clone()
        .oneshot(empty_request("GET", &format!("/api/{uid}/{doc_id}"), None))
        .await
        .unwrap();
    assert_eq!(get_one.status(), StatusCode::OK, "public get");
    let get_json = body_json(get_one).await;
    assert_eq!(get_json["data"]["title"], "Hello");

    // Public update.
    let upd = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/api/{uid}/{doc_id}"),
            serde_json::json!({"data":{"title":"Updated","views":9}}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(upd.status(), StatusCode::OK, "public update");
    assert_eq!(body_json(upd).await["data"]["title"], "Updated");

    // Public delete.
    let del = router
        .clone()
        .oneshot(empty_request(
            "DELETE",
            &format!("/api/{uid}/{doc_id}"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK, "public delete");

    // Get after delete -> not found (404).
    let gone = router
        .clone()
        .oneshot(empty_request("GET", &format!("/api/{uid}/{doc_id}"), None))
        .await
        .unwrap();
    assert_eq!(gone.status(), StatusCode::NOT_FOUND, "deleted entry 404");

    // Unknown content type -> 404.
    let unknown = router
        .clone()
        .oneshot(empty_request("GET", "/api/api::nope.nope", None))
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND, "unknown ct 404");

    // Unauthenticated admin endpoint -> 401 (error.rs Unauthorized branch).
    let anon = router
        .clone()
        .oneshot(empty_request("GET", "/admin/users", None))
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_handlers_and_single_types() {
    let (router, state) = setup().await;
    let token = register_admin(&router).await;

    // Single type get when it does not exist -> 200 with empty data (not found swallowed).
    // First create a single type.
    let single = serde_json::json!({
        "uid": "api::homepage.homepage",
        "kind": "singleType",
        "info": {"singularName":"homepage","pluralName":"homepages","displayName":"Homepage"},
        "options": {"draftAndPublish": true},
        "attributes": {"slug": {"type": "string"}}
    });
    let resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas":[single]}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "create single type");

    // GET single type not yet created -> 200 empty template.
    let single_get = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/admin/content-manager/single-types/api::homepage.homepage",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        single_get.status(),
        StatusCode::OK,
        "single get not found -> 200"
    );
    let single_json = body_json(single_get).await;
    assert!(
        single_json["data"]
            .as_object()
            .map(|m| m.is_empty())
            .unwrap_or(false),
        "empty template"
    );

    // The single-type PUT updates the canonical "default" entry, which must
    // already exist (update_one returns NotFound otherwise). Create it directly
    // so the update path is exercised, mirroring the CM's initial create flow.
    let home_schema = state
        .ctx
        .schema_cache
        .get(&Uid::new("api::homepage.homepage"))
        .expect("homepage schema");
    dml::insert_one(
        &state.ctx.db,
        &home_schema,
        &serde_json::json!({"slug":"initial","documentId":"default"}),
        None,
    )
    .await
    .expect("insert default entry");

    // PUT single type -> update the default entry.
    let single_put = router
        .clone()
        .oneshot(json_request(
            "PUT",
            "/admin/content-manager/single-types/api::homepage.homepage",
            serde_json::json!({"data":{"slug":"welcome"}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(single_put.status(), StatusCode::OK, "single update");
    assert_eq!(body_json(single_put).await["data"]["slug"], "welcome");

    // GET it back now exists.
    let single_get2 = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/admin/content-manager/single-types/api::homepage.homepage",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(single_get2.status(), StatusCode::OK);
    assert_eq!(body_json(single_get2).await["data"]["slug"], "welcome");

    // CTB get by uid.
    let ctb_get = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/content-type-builder/content-types/api::homepage.homepage",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(ctb_get.status(), StatusCode::OK, "ctb get by uid");
    assert_eq!(
        body_json(ctb_get).await["data"]["uid"],
        "api::homepage.homepage"
    );

    // CTB reserved names.
    let reserved = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/content-type-builder/reserved-names",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(reserved.status(), StatusCode::OK, "reserved names");
    let reserved_json = body_json(reserved).await;
    let names = reserved_json["data"].as_array().expect("names array");
    assert!(!names.is_empty(), "reserved names non-empty");

    // CM content-types list.
    let cm_cts = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/admin/content-manager/content-types",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(cm_cts.status(), StatusCode::OK, "cm content types");
    assert!(!body_json(cm_cts).await["data"]
        .as_array()
        .unwrap()
        .is_empty());

    // CTB get unknown uid -> 404.
    let ctb_unknown = router
        .clone()
        .oneshot(empty_request(
            "GET",
            "/content-type-builder/content-types/api::missing.missing",
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        ctb_unknown.status(),
        StatusCode::NOT_FOUND,
        "ctb get unknown 404"
    );

    // RBAC role get by id.
    let roles = body_json(
        router
            .clone()
            .oneshot(empty_request("GET", "/admin/roles", Some(&token)))
            .await
            .unwrap(),
    )
    .await;
    let role_id = roles["data"][0]["id"].as_i64().expect("role id");
    let role = router
        .clone()
        .oneshot(empty_request(
            "GET",
            &format!("/admin/roles/{role_id}"),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(role.status(), StatusCode::OK, "role get");

    // RBAC role get unknown -> 404.
    let role_unknown = router
        .clone()
        .oneshot(empty_request("GET", "/admin/roles/999999", Some(&token)))
        .await
        .unwrap();
    assert_eq!(
        role_unknown.status(),
        StatusCode::NOT_FOUND,
        "unknown role 404"
    );
}

#[tokio::test]
async fn validation_and_bad_requests() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;
    let uid = create_article(&router, &token).await;

    // Invalid content type schema -> 400 ValidationError.
    let bad = serde_json::json!({
        "uid": "api::bad.bad",
        "kind": "collectionType",
        "info": {"singularName":"bad","pluralName":"bads","displayName":"Bad"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "with space": {"type": "string"}
        }
    });
    let bad_resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas":[bad]}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(bad_resp.status(), StatusCode::BAD_REQUEST, "validation 400");
    let bad_json = body_json(bad_resp).await;
    assert_eq!(bad_json["error"]["name"], "ValidationError");

    // Required field missing on create: the API layer validates the payload
    // before it is handled, returning a 400 ValidationError.
    let missing = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            serde_json::json!({"data":{"views":1}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        missing.status(),
        StatusCode::BAD_REQUEST,
        "missing required should return 400"
    );
    let missing_json = body_json(missing).await;
    assert_eq!(missing_json["error"]["name"], "ValidationError");
    let details = missing_json["error"]["details"]["errors"]
        .as_array()
        .unwrap();
    assert!(
        details.iter().any(|e| e["path"] == serde_json::json!(["title"])),
        "missing required should target the title field, got {details:?}"
    );

    // Duplicate UID create -> 409 Conflict.
    let dup = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas":[
                {
                    "uid": "api::article.article",
                    "kind": "collectionType",
                    "info": {"singularName":"article","pluralName":"articles","displayName":"Article"},
                    "options": {"draftAndPublish": true},
                    "attributes": {"title": {"type": "string"}}
                }
            ]}),
            Some(&token),
        ))
        .await
        .unwrap();
    // Applying the same UID twice may be a no-op or conflict; assert it's not 500.
    assert_ne!(
        dup.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "no 500 on re-apply"
    );
}

#[tokio::test]
async fn spa_fallback_and_upload_errors() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    // The embedded UI serves index.html at the root (no FERRISCMS_UI_DIR set).
    let root = router
        .clone()
        .oneshot(empty_request("GET", "/", None))
        .await
        .unwrap();
    assert_eq!(root.status(), StatusCode::OK, "SPA index served");

    // Unmatched admin path falls through to SPA fallback (not a real route).
    let spa = router
        .clone()
        .oneshot(empty_request("GET", "/some/spa/route", None))
        .await
        .unwrap();
    assert_eq!(
        spa.status(),
        StatusCode::OK,
        "SPA fallback for non-API path"
    );

    // Upload with no file -> 400 validation.
    let empty_upload = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/upload/files")
                .header(header::CONTENT_TYPE, "multipart/form-data; boundary=XXXX")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from("--XXXX--\r\n"))
                .expect("empty upload"),
        )
        .await
        .unwrap();
    assert_eq!(
        empty_upload.status(),
        StatusCode::BAD_REQUEST,
        "no file -> 400"
    );
    let up_json = body_json(empty_upload).await;
    assert_eq!(up_json["error"]["name"], "ValidationError");
}

/// Create a content type with required/min/max/pattern constraints and verify
/// the API validates payloads before they are handled.
async fn create_product(router: &axum::Router, token: &str) -> String {
    let ct = serde_json::json!({
        "uid": "api::product.product",
        "kind": "collectionType",
        "info": {"singularName":"product","pluralName":"products","displayName":"Product"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "title": {"type": "string", "required": true},
            "qty":   {"type": "integer", "required": true, "min": 1, "max": 100},
            "sku":   {"type": "string", "regex": "^[A-Z]{2}[0-9]{3}$"},
            "state": {"type": "enumeration", "enum": ["draft", "published"]}
        }
    });
    let resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas":[ct]}),
            Some(token),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "create product content type");
    "api::product.product".to_string()
}

#[tokio::test]
async fn payload_constraints_are_enforced_before_handling() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;
    let uid = create_product(&router, &token).await;
    let base = format!("/admin/content-manager/collection-types/{uid}");

    async fn post(
        router: &axum::Router,
        uri: &str,
        body: serde_json::Value,
        token: &str,
    ) -> (StatusCode, serde_json::Value) {
        let resp = router
            .clone()
            .oneshot(json_request("POST", uri, body, Some(token)))
            .await
            .unwrap();
        let status = resp.status();
        let json = body_json(resp).await;
        (status, json)
    }

    // Valid payload -> 200.
    let (status, body) = post(
        &router,
        &base,
        serde_json::json!({"data":{"title":"Ferris","qty":5,"sku":"AB123","state":"draft"}}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "valid payload accepted");
    let doc_id = body["data"]["documentId"].as_str().unwrap().to_string();

    // Out-of-range qty -> 400 min.
    let (status, body) = post(
        &router,
        &base,
        serde_json::json!({"data":{"title":"Ferris","qty":0,"sku":"AB123","state":"draft"}}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "below min rejected");
    assert_eq!(body["error"]["name"], "ValidationError");
    let details = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(details.iter().any(|e| e["path"] == serde_json::json!(["qty"])));

    // Above max -> 400 max.
    let (status, body) = post(
        &router,
        &base,
        serde_json::json!({"data":{"title":"Ferris","qty":101,"sku":"AB123","state":"draft"}}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "above max rejected");
    let details = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(details.iter().any(|e| e["path"] == serde_json::json!(["qty"])));

    // Pattern violation -> 400 regex.
    let (status, body) = post(
        &router,
        &base,
        serde_json::json!({"data":{"title":"Ferris","qty":5,"sku":"nope","state":"draft"}}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "pattern violation rejected");
    let details = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(details.iter().any(|e| e["path"] == serde_json::json!(["sku"])));

    // Bad enum -> 400 enum.
    let (status, body) = post(
        &router,
        &base,
        serde_json::json!({"data":{"title":"Ferris","qty":5,"sku":"AB123","state":"archived"}}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "bad enum rejected");
    let details = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(details.iter().any(|e| e["path"] == serde_json::json!(["state"])));

    // No record was created by the rejected requests.
    let list = router
        .clone()
        .oneshot(json_request("GET", &base, serde_json::json!({}), Some(&token)))
        .await
        .unwrap();
    let list_json = body_json(list).await;
    let data = list_json["data"].as_array().unwrap();
    assert_eq!(
        data.len(),
        1,
        "only the valid record should exist after rejected writes"
    );

    // Update: partial payload with a violating provided field -> 400, but a
    // partial payload with valid provided fields succeeds.
    let put = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("{base}/{doc_id}"),
            serde_json::json!({"data":{"qty":200}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::BAD_REQUEST, "update above max rejected");

    let put_ok = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("{base}/{doc_id}"),
            serde_json::json!({"data":{"qty":7}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(put_ok.status(), StatusCode::OK, "partial valid update accepted");
    let updated = body_json(put_ok).await;
    assert_eq!(updated["data"]["qty"], serde_json::json!(7));
}

