//! Edge-path integration coverage: drives the validation branches in
//! `core-schema` (many malformed schemas) and the DDL/value branches in
//! `dynamic-store` (relations, components, media, dynamic zones, unique,
//! indexed, uid), plus error-envelope paths.

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
        jwt_secret: "edges-test-secret".into(),
        jwt_expiry_secs: 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::temp_dir().join("ferris-edges").display().to_string(),
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
    builder.body(Body::from(body.to_string())).expect("request builds")
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
            serde_json::json!({"email":"edges@test.dev","password":"StrongPass123!"}),
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

/// Apply a schema set and return its status code (does not assert success).
async fn apply_status(
    router: &axum::Router,
    token: &str,
    schemas: serde_json::Value,
) -> StatusCode {
    router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({"schemas": schemas}),
            Some(token),
        ))
        .await
        .unwrap()
        .status()
}

fn ct(
    uid: &str,
    kind: &str,
    singular: &str,
    plural: &str,
    display: &str,
    attributes: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "uid": uid,
        "kind": kind,
        "info": {"singularName": singular, "pluralName": plural, "displayName": display},
        "options": {"draftAndPublish": true},
        "attributes": attributes
    })
}

#[tokio::test]
async fn validation_error_branches() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    // Each malformed batch must be rejected with a 400 ValidationError,
    // exercising a distinct validation branch in core-schema.
    let cases: Vec<serde_json::Value> = vec![
        // duplicate-uid
        serde_json::json!([
            ct("api::a.a", "collectionType", "a", "as", "A", serde_json::json!({"x": {"type":"string"}})),
            ct("api::a.a", "collectionType", "a2", "a2s", "A2", serde_json::json!({"x": {"type":"string"}}))
        ]),
        // invalid component uid (has "::")
        serde_json::json!([
            ct("api::x.x", "component", "x", "xs", "X", serde_json::json!({"x": {"type":"string"}}))
        ]),
        // missing api id
        serde_json::json!([
            ct("api::m.m", "collectionType", "", "", "M", serde_json::json!({"x": {"type":"string"}}))
        ]),
        // invalid api id (uppercase/space)
        serde_json::json!([
            ct("api::m.m", "collectionType", "Bad Name", "bads", "M", serde_json::json!({"x": {"type":"string"}}))
        ]),
        // reserved api id
        serde_json::json!([
            ct("api::admin.x", "collectionType", "admin", "admins", "M", serde_json::json!({"x": {"type":"string"}}))
        ]),
        // missing display name
        serde_json::json!([
            ct("api::d.d", "collectionType", "d", "ds", "", serde_json::json!({"x": {"type":"string"}}))
        ]),
        // invalid attribute identifier
        serde_json::json!([
            ct("api::i.i", "collectionType", "i", "is", "I", serde_json::json!({"with space": {"type":"string"}}))
        ]),
        // reserved attribute
        serde_json::json!([
            ct("api::r.r", "collectionType", "r", "rs", "R", serde_json::json!({"documentId": {"type":"string"}}))
        ]),
        // invalid regex
        serde_json::json!([
            ct("api::re.re", "collectionType", "re", "res", "Re", serde_json::json!({"x": {"type":"string","regex":"[unclosed"}}))
        ]),
        // minLength > maxLength
        serde_json::json!([
            ct("api::ml.ml", "collectionType", "ml", "mls", "ML", serde_json::json!({"x": {"type":"string","minLength":5,"maxLength":2}}))
        ]),
        // empty enum
        serde_json::json!([
            ct("api::e.e", "collectionType", "e", "es", "E", serde_json::json!({"x": {"type":"enumeration","enum":[]}}))
        ]),
        // invalid enum value
        serde_json::json!([
            ct("api::e.e", "collectionType", "e", "es", "E", serde_json::json!({"x": {"type":"enumeration","enum":["bad-value"]}}))
        ]),
        // duplicate enum value
        serde_json::json!([
            ct("api::e.e", "collectionType", "e", "es", "E", serde_json::json!({"x": {"type":"enumeration","enum":["a","a"]}}))
        ]),
        // uid target field missing
        serde_json::json!([
            ct("api::u.u", "collectionType", "u", "us", "U", serde_json::json!({"x": {"type":"uid","targetField":"nope"}}))
        ]),
        // relation missing kind
        serde_json::json!([
            ct("api::rel.rel", "collectionType", "rel", "rels", "Rel", serde_json::json!({"x": {"type":"relation","target":"api::b.b"}}))
        ]),
        // relation missing target
        serde_json::json!([
            ct("api::rel.rel", "collectionType", "rel", "rels", "Rel", serde_json::json!({"x": {"type":"relation","relation":"oneToOne"}}))
        ]),
        // relation to undefined target
        serde_json::json!([
            ct("api::rel.rel", "collectionType", "rel", "rels", "Rel", serde_json::json!({"x": {"type":"relation","relation":"oneToOne","target":"api::zz.zz"}}))
        ]),
        // relation targeting a component
        serde_json::json!([
            ct("shared.comp", "component", "comp", "comps", "Comp", serde_json::json!({"c": {"type":"string"}})),
            ct("api::rel.rel", "collectionType", "rel", "rels", "Rel", serde_json::json!({"x": {"type":"relation","relation":"oneToOne","target":"shared.comp"}}))
        ]),
        // component undefined
        serde_json::json!([
            ct("api::c.c", "collectionType", "c", "cs", "C", serde_json::json!({"x": {"type":"component","component":"shared.nope"}}))
        ]),
        // component references a non-component
        serde_json::json!([
            ct("api::other.other", "collectionType", "other", "others", "Other", serde_json::json!({"o": {"type":"string"}})),
            ct("api::c.c", "collectionType", "c", "cs", "C", serde_json::json!({"x": {"type":"component","component":"api::other.other"}}))
        ]),
        // component min > max
        serde_json::json!([
            ct("shared.comp", "component", "comp", "comps", "Comp", serde_json::json!({"c": {"type":"string"}})),
            ct("api::c.c", "collectionType", "c", "cs", "C", serde_json::json!({"x": {"type":"component","component":"shared.comp","min":5,"max":2}}))
        ]),
        // empty dynamic zone
        serde_json::json!([
            ct("api::dz.dz", "collectionType", "dz", "dzs", "DZ", serde_json::json!({"x": {"type":"dynamiczone","components":[]}}))
        ]),
        // dynamic zone missing component
        serde_json::json!([
            ct("api::dz.dz", "collectionType", "dz", "dzs", "DZ", serde_json::json!({"x": {"type":"dynamiczone","components":["shared.nope"]}}))
        ]),
        // invalid media allowed type
        serde_json::json!([
            ct("api::med.med", "collectionType", "med", "meds", "Med", serde_json::json!({"x": {"type":"media","allowedTypes":["gifs"]}}))
        ]),
        // duplicate singular api id
        serde_json::json!([
            ct("api::s1.s1", "collectionType", "same", "one", "S1", serde_json::json!({"x": {"type":"string"}})),
            ct("api::s2.s2", "collectionType", "same", "two", "S2", serde_json::json!({"x": {"type":"string"}}))
        ]),
    ];

    for (i, schemas) in cases.into_iter().enumerate() {
        let status = apply_status(&router, &token, schemas).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "case {i} expected 400 validation error"
        );
    }
}

#[tokio::test]
async fn relations_components_media_and_dynamic_zones() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    // Valid schemas: a component, a dynamic zone (uses the component), media,
    // uid/unique fields, and three content types linked by one-to-many and
    // many-to-many relations. Creating these drives DDL create table/join table
    // for a wide range of column types.
    let seo = ct(
        "shared.seo",
        "component",
        "seo",
        "seos",
        "SEO",
        serde_json::json!({"metaTitle": {"type": "string"}}),
    );
    let card = ct(
        "shared.card",
        "component",
        "card",
        "cards",
        "Card",
        serde_json::json!({"heading": {"type": "string"}}),
    );
    // Phase 1: components + content types with scalar/component/media/dynamic-
    // zone fields (no relations yet). Creating these drives DDL for a wide
    // range of column types.
    let author_base = ct(
        "api::author.author",
        "collectionType",
        "author",
        "authors",
        "Author",
        serde_json::json!({"name": {"type": "string"}}),
    );
    let tag_base = ct(
        "api::tag.tag",
        "collectionType",
        "tag",
        "tags",
        "Tag",
        serde_json::json!({"label": {"type": "string"}}),
    );
    let article_base = ct(
        "api::article.article",
        "collectionType",
        "article",
        "articles",
        "Article",
        serde_json::json!({
            "title": {"type": "string", "unique": true, "required": true},
            "slug": {"type": "uid", "targetField": "title"},
            "views": {"type": "integer", "default": 0},
            "seo": {"type": "component", "component": "shared.seo", "repeatable": false},
            "cards": {"type": "component", "component": "shared.card", "repeatable": true},
            "hero": {"type": "media", "multiple": false, "allowedTypes": ["images"]},
            "blocks": {"type": "dynamiczone", "components": ["shared.seo", "shared.card"]}
        }),
    );
    let phase1 = serde_json::json!([seo, card, author_base, tag_base, article_base]);
    let status1 = apply_status(&router, &token, phase1).await;
    assert_eq!(status1, StatusCode::OK, "create base schemas: {status1}");

    // Phase 2: add relations (one-to-many author<->article, many-to-many
    // article<->tag). This drives join-table + inverse-FK DDL.
    let author2 = ct(
        "api::author.author",
        "collectionType",
        "author",
        "authors",
        "Author",
        serde_json::json!({
            "name": {"type": "string"},
            "articles": {"type": "relation", "relation": "oneToMany", "target": "api::article.article", "mappedBy": "author"}
        }),
    );
    let tag2 = ct(
        "api::tag.tag",
        "collectionType",
        "tag",
        "tags",
        "Tag",
        serde_json::json!({
            "label": {"type": "string"},
            "articles": {"type": "relation", "relation": "manyToMany", "target": "api::article.article", "mappedBy": "tags"}
        }),
    );
    let article2 = ct(
        "api::article.article",
        "collectionType",
        "article",
        "articles",
        "Article",
        serde_json::json!({
            "title": {"type": "string", "unique": true, "required": true},
            "slug": {"type": "uid", "targetField": "title"},
            "views": {"type": "integer", "default": 0},
            "author": {"type": "relation", "relation": "manyToOne", "target": "api::author.author", "inversedBy": "articles"},
            "tags": {"type": "relation", "relation": "manyToMany", "target": "api::tag.tag", "inversedBy": "articles"},
            "seo": {"type": "component", "component": "shared.seo", "repeatable": false},
            "cards": {"type": "component", "component": "shared.card", "repeatable": true},
            "hero": {"type": "media", "multiple": false, "allowedTypes": ["images"]},
            "blocks": {"type": "dynamiczone", "components": ["shared.seo", "shared.card"]}
        }),
    );
    let phase2 = serde_json::json!([seo, card, author2, tag2, article2]);
    let status2 = apply_status(&router, &token, phase2).await;
    if status2 != StatusCode::OK {
        let resp = router
            .clone()
            .oneshot(json_request(
                "POST",
                "/content-type-builder/schema",
                serde_json::json!({"schemas": [seo, card, author2, tag2, article2]}),
                Some(&token),
            ))
            .await
            .unwrap();
        eprintln!("PHASE2 BODY: {}", body_json(resp).await);
    }
    assert_eq!(status2, StatusCode::OK, "add relations: {status2}");

    // CRUD an entry with scalar + component/dynamic-zone values (media/relations
    // left unset to avoid requiring pre-uploaded assets).
    let created = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/content-manager/collection-types/api::article.article",
            serde_json::json!({"data": {
                "title": "First",
                "slug": "first",
                "views": 4,
                "seo": {"metaTitle": "Hello"},
                "cards": [{"heading": "A"}, {"heading": "B"}],
                "blocks": [{"__component": "shared.seo", "metaTitle": "B1"}]
            }}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK, "create article: {}", created.status());
    let doc_id = body_json(created).await["data"]["documentId"].as_str().expect("documentId").to_string();

    // Read it back (component + dynamic-zone values round-trip).
    let got = router
        .clone()
        .oneshot(empty_request(
            "GET",
            &format!("/admin/content-manager/collection-types/api::article.article/{doc_id}"),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(got.status(), StatusCode::OK);
    assert_eq!(body_json(got).await["data"]["title"], "First");

    // Enforce uniqueness: a second entry with the same unique title fails.
    let dup = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/content-manager/collection-types/api::article.article",
            serde_json::json!({"data": {"title": "First", "slug": "dup"}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_ne!(dup.status(), StatusCode::OK, "duplicate unique title should not succeed");

    // Delete.
    let del = router
        .clone()
        .oneshot(empty_request(
            "DELETE",
            &format!("/admin/content-manager/collection-types/api::article.article/{doc_id}"),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK, "delete article");
}

#[tokio::test]
async fn schema_mutations_ddl() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    let uid = "api::page.page";
    // Initial schema with scalar columns.
    let base = ct(
        uid,
        "collectionType",
        "page",
        "pages",
        "Page",
        serde_json::json!({
            "title": {"type": "string"},
            "views": {"type": "integer"},
            "published": {"type": "boolean"}
        }),
    );
    assert_eq!(apply_status(&router, &token, serde_json::json!([base])).await, StatusCode::OK);

    // Mutate 1: compatible scalar type changes (string->text, integer->biginteger).
    let m1 = ct(
        uid,
        "collectionType",
        "page",
        "pages",
        "Page",
        serde_json::json!({
            "title": {"type": "text"},
            "views": {"type": "biginteger"},
            "published": {"type": "boolean"},
            "slug": {"type": "uid"}
        }),
    );
    assert_eq!(apply_status(&router, &token, serde_json::json!([m1])).await, StatusCode::OK, "compatible change");

    // Mutate 2: add media + component fields (creates link tables) and drop a
    // boolean column.
    let seo = ct(
        "shared.seo",
        "component",
        "seo",
        "seos",
        "SEO",
        serde_json::json!({"metaTitle": {"type": "string"}}),
    );
    let m2 = ct(
        uid,
        "collectionType",
        "page",
        "pages",
        "Page",
        serde_json::json!({
            "title": {"type": "text"},
            "views": {"type": "biginteger"},
            "slug": {"type": "uid"},
            "hero": {"type": "media", "multiple": false, "allowedTypes": ["images"]},
            "seo": {"type": "component", "component": "shared.seo", "repeatable": false}
        }),
    );
    assert_eq!(
        apply_status(&router, &token, serde_json::json!([seo, m2])).await,
        StatusCode::OK,
        "add media+component"
    );

    // Entries still CRUD on the final schema.
    let created = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            serde_json::json!({"data": {"title": "T", "views": 5, "slug": "t", "seo": {"metaTitle": "M"}}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK, "create on mutated schema: {}", created.status());
    let doc_id = body_json(created).await["data"]["documentId"].as_str().expect("documentId").to_string();

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
}

#[tokio::test]
async fn value_conversion_error_branches() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    let uid = "api::typed.typed";
    let schema = ct(
        uid,
        "collectionType",
        "typed",
        "typeds",
        "Typed",
        serde_json::json!({
            "num": {"type": "integer"},
            "big": {"type": "biginteger"},
            "dec": {"type": "decimal"},
            "dt": {"type": "datetime"},
            "d": {"type": "date"},
            "t": {"type": "time"}
        }),
    );
    assert_eq!(apply_status(&router, &token, serde_json::json!([schema])).await, StatusCode::OK);

    // Each invalid scalar value must be rejected (never 200), exercising the
    // conversion error branches in dynamic-store/value.rs.
    let invalid_values: Vec<serde_json::Value> = vec![
        serde_json::json!({"num": "not-a-number"}),
        serde_json::json!({"big": "nope"}),
        serde_json::json!({"dec": "nope"}),
        serde_json::json!({"dt": "not-a-date"}),
        serde_json::json!({"d": "2024-13-99"}),
        serde_json::json!({"t": "25:99:99"}),
    ];
    for (i, data) in invalid_values.into_iter().enumerate() {
        let resp = router
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/admin/content-manager/collection-types/{uid}"),
                serde_json::json!({"data": data}),
                Some(&token),
            ))
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::OK,
            "case {i} should reject invalid value"
        );
    }
}

#[tokio::test]
async fn error_envelope_and_query_paths() {
    let (router, _state) = setup().await;
    let token = register_admin(&router).await;

    // Validation error envelope includes details.errors (exercised by a bad
    // apply), and error.rs maps it to 400 ValidationError.
    let bad = serde_json::json!([{
        "uid": "api::bad.bad", "kind": "collectionType",
        "info": {"singularName": "bad", "pluralName": "bads", "displayName": "Bad"},
        "options": {"draftAndPublish": true},
        "attributes": {"with space": {"type": "string"}}
    }]);
    let bad_resp = router
        .clone()
        .oneshot(json_request("POST", "/content-type-builder/schema", serde_json::json!({"schemas": bad}), Some(&token)))
        .await
        .unwrap();
    assert_eq!(bad_resp.status(), StatusCode::BAD_REQUEST);
    let bad_json = body_json(bad_resp).await;
    assert_eq!(bad_json["error"]["name"], "ValidationError");
    assert!(bad_json["error"]["details"]["errors"].is_array());

    // 404 for an unknown content type on the public API.
    let nf = router
        .clone()
        .oneshot(empty_request("GET", "/api/api::nope.nope", None))
        .await
        .unwrap();
    assert_eq!(nf.status(), StatusCode::NOT_FOUND);

    // Unauthorized admin access -> 401 (error.rs Unauthorized branch).
    let anon = router
        .clone()
        .oneshot(empty_request("GET", "/admin/users", None))
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);

    // A successful read on an existing type returns the standard envelope.
    let list = router
        .clone()
        .oneshot(empty_request("GET", "/admin/content-manager/content-types", Some(&token)))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    assert!(body_json(list).await["data"].is_array());
}
