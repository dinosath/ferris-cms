//! Computed/generated field support — end-to-end integration tests.
//!
//! Exercises the full stack (Content-Type Builder → DDL → REST CRUD) with the
//! ERP and CRM business scenarios from the computed-fields spec:
//!
//! - ERP `SalesOrder`: chained computed columns
//!   `subtotal = quantity * unit_price`,
//!   `discount_amount = subtotal * discount / 100`,
//!   `total_amount = subtotal - discount_amount`.
//! - CRM `Customer`:
//!   `full_name = first_name || ' ' || last_name`,
//!   `success_rate = (deals_won * 100) / (deals_won + deals_lost)`.
//!
//! Asserts database-derived values, recomputation on update, filtering/sorting
//! by computed fields, and rejection of writes to computed fields.

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
        jwt_secret: "computed-test-secret".into(),
        jwt_expiry_secs: 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::temp_dir()
            .join("ferris-computed")
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
    let _ = state.ctx.init_rbac().await;
    build_router(state)
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
    builder.body(Body::from(body.to_string())).unwrap()
}

fn get(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
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
            serde_json::json!({"email":"computed@test.dev","password":"StrongPass123!"}),
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

/// Apply a schema batch, asserting success.
async fn apply_schema(router: &axum::Router, token: &str, schemas: serde_json::Value) {
    let resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({ "schemas": schemas }),
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

/// Apply a schema batch expecting it to be rejected (validation error).
async fn apply_schema_expect_error(
    router: &axum::Router,
    token: &str,
    schemas: serde_json::Value,
) -> serde_json::Value {
    let resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/content-type-builder/schema",
            serde_json::json!({ "schemas": schemas }),
            Some(token),
        ))
        .await
        .unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "expected 400 schema validation error: {body}"
    );
    body
}

fn num(v: &serde_json::Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(f64::NAN)
}

async fn create_entry(
    router: &axum::Router,
    token: &str,
    uid: &str,
    data: serde_json::Value,
) -> serde_json::Value {
    let resp = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            serde_json::json!({ "data": data }),
            Some(token),
        ))
        .await
        .unwrap();
    let status = resp.status();
    let body = body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "create entry failed: {body}");
    body
}

async fn get_entry(
    router: &axum::Router,
    token: &str,
    uid: &str,
    document_id: &str,
) -> serde_json::Value {
    let resp = router
        .clone()
        .oneshot(get(
            &format!("/admin/content-manager/collection-types/{uid}/{document_id}"),
            Some(token),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_json(resp).await
}

/// ERP scenario.
#[tokio::test]
async fn erp_sales_order_computed_fields() {
    let router = setup().await;
    let token = register_admin(&router).await;

    let ct = serde_json::json!({
        "uid": "api::sales-order.sales-order",
        "kind": "collectionType",
        "info": {"singularName":"sales-order","pluralName":"sales-orders","displayName":"Sales Order"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "customer_name": {"type": "string"},
            "quantity": {"type": "integer"},
            "unit_price": {"type": "decimal"},
            "discount": {"type": "integer"},
            "subtotal": {
                "type": "decimal", "computed": true,
                "expression": "quantity * unit_price", "stored": true,
                "dependencies": ["quantity", "unit_price"]
            },
            "discount_amount": {
                "type": "decimal", "computed": true,
                "expression": "subtotal * discount / 100", "stored": true,
                "dependencies": ["subtotal", "discount"]
            },
            "total_amount": {
                "type": "decimal", "computed": true,
                "expression": "subtotal - discount_amount", "stored": true,
                "dependencies": ["subtotal", "discount_amount"]
            }
        }
    });
    apply_schema(&router, &token, serde_json::json!([ct])).await;
    let uid = "api::sales-order.sales-order";

    // 1-4. Create order and verify all computed values.
    let created = create_entry(
        &router,
        &token,
        uid,
        serde_json::json!({"customer_name":"Acme","quantity":10,"unit_price":100,"discount":10}),
    )
    .await;
    let d = &created["data"];
    assert_eq!(num(&d["subtotal"]), 1000.0, "subtotal: {d}");
    assert_eq!(num(&d["discount_amount"]), 100.0, "discount_amount: {d}");
    assert_eq!(num(&d["total_amount"]), 900.0, "total_amount: {d}");
    let doc_id = d["documentId"].as_str().expect("documentId").to_string();

    // 5-6. Update quantity -> all computed values recalculate.
    let upd = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/content-manager/collection-types/{uid}/{doc_id}"),
            serde_json::json!({"data":{"quantity":20}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(upd.status(), StatusCode::OK);
    let upd = body_json(upd).await;
    assert_eq!(num(&upd["data"]["subtotal"]), 2000.0, "subtotal after update");
    assert_eq!(num(&upd["data"]["discount_amount"]), 200.0);
    assert_eq!(num(&upd["data"]["total_amount"]), 1800.0);

    // 7. REST GET returns computed values.
    let got = get_entry(&router, &token, uid, &doc_id).await;
    assert_eq!(num(&got["data"]["total_amount"]), 1800.0);

    // Second order for sorting/filtering.
    create_entry(
        &router,
        &token,
        uid,
        serde_json::json!({"customer_name":"Globex","quantity":2,"unit_price":50,"discount":0}),
    )
    .await;

    // 8. Sorting by a computed field (descending) puts 1800 first.
    let sorted = router
        .clone()
        .oneshot(get(
            &format!("/admin/content-manager/collection-types/{uid}?sort[0]=total_amount:desc&pagination[pageSize]=10"),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(sorted.status(), StatusCode::OK);
    let sorted = body_json(sorted).await;
    let rows = sorted["data"].as_array().expect("list data");
    assert_eq!(rows.len(), 2, "two orders: {sorted}");
    assert_eq!(num(&rows[0]["total_amount"]), 1800.0, "sorted desc: {sorted}");

    // 9. Filtering by a computed field.
    let filtered = router
        .clone()
        .oneshot(get(
            &format!("/admin/content-manager/collection-types/{uid}?filters[total_amount][$gte]=1000&pagination[pageSize]=10"),
            Some(&token),
        ))
        .await
        .unwrap();
    let filtered = body_json(filtered).await;
    let rows = filtered["data"].as_array().expect("filtered data");
    assert_eq!(rows.len(), 1, "only the 1800 order matches: {filtered}");
    assert_eq!(num(&rows[0]["total_amount"]), 1800.0);

    // Writing a computed field is rejected.
    let bad = router
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            serde_json::json!({"data":{"quantity":10,"unit_price":5,"total_amount":999999}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(
        bad.status(),
        StatusCode::BAD_REQUEST,
        "writing a computed field must be rejected"
    );
}

/// CRM scenario.
#[tokio::test]
async fn crm_customer_computed_fields() {
    let router = setup().await;
    let token = register_admin(&router).await;

    let ct = serde_json::json!({
        "uid": "api::customer.customer",
        "kind": "collectionType",
        "info": {"singularName":"customer","pluralName":"customers","displayName":"Customer"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "first_name": {"type": "string"},
            "last_name": {"type": "string"},
            "company": {"type": "string"},
            "deals_won": {"type": "integer"},
            "deals_lost": {"type": "integer"},
            "full_name": {
                "type": "text", "computed": true,
                "expression": "first_name || ' ' || last_name", "stored": true,
                "dependencies": ["first_name", "last_name"]
            },
            "success_rate": {
                "type": "integer", "computed": true,
                "expression": "(deals_won * 100) / (deals_won + deals_lost)", "stored": true,
                "dependencies": ["deals_won", "deals_lost"]
            }
        }
    });
    apply_schema(&router, &token, serde_json::json!([ct])).await;
    let uid = "api::customer.customer";

    // 1-3. Create and verify full_name + success_rate.
    let created = create_entry(
        &router,
        &token,
        uid,
        serde_json::json!({
            "first_name":"John","last_name":"Smith","company":"Acme",
            "deals_won":8,"deals_lost":2
        }),
    )
    .await;
    let d = &created["data"];
    assert_eq!(d["full_name"], "John Smith", "full_name: {d}");
    assert_eq!(num(&d["success_rate"]), 80.0, "success_rate: {d}");
    let doc_id = d["documentId"].as_str().unwrap().to_string();

    // 4-5. Update deal counts -> recomputation.
    let upd = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/admin/content-manager/collection-types/{uid}/{doc_id}"),
            serde_json::json!({"data":{"deals_won":9,"deals_lost":1}}),
            Some(&token),
        ))
        .await
        .unwrap();
    let upd = body_json(upd).await;
    assert_eq!(num(&upd["data"]["success_rate"]), 90.0, "recomputed: {upd}");

    // Another customer for filtering/sorting.
    create_entry(
        &router,
        &token,
        uid,
        serde_json::json!({"first_name":"Alice","last_name":"Brown","deals_won":2,"deals_lost":8}),
    )
    .await;

    // 6. Query by success_rate.
    let by_rate = router
        .clone()
        .oneshot(get(
            &format!("/admin/content-manager/collection-types/{uid}?filters[success_rate][$eq]=90&pagination[pageSize]=10"),
            Some(&token),
        ))
        .await
        .unwrap();
    let by_rate = body_json(by_rate).await;
    let rows = by_rate["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "one customer at 90%: {by_rate}");
    assert_eq!(rows[0]["full_name"], "John Smith");

    // 7. Sort by full_name ascending -> Alice Brown first.
    let sorted = router
        .clone()
        .oneshot(get(
            &format!("/admin/content-manager/collection-types/{uid}?sort[0]=full_name:asc&pagination[pageSize]=10"),
            Some(&token),
        ))
        .await
        .unwrap();
    let sorted = body_json(sorted).await;
    let rows = sorted["data"].as_array().unwrap();
    assert_eq!(rows[0]["full_name"], "Alice Brown", "sorted by full_name: {sorted}");
}

/// Invalid computed-field definitions are rejected by the CTB.
#[tokio::test]
async fn computed_field_validation_errors() {
    let router = setup().await;
    let token = register_admin(&router).await;

    // required + default are not allowed on computed fields.
    let bad = serde_json::json!({
        "uid": "api::bad.bad",
        "kind": "collectionType",
        "info": {"singularName":"bad","pluralName":"bads","displayName":"Bad"},
        "attributes": {
            "a": {"type": "decimal"},
            "b": {"type": "decimal"},
            "total": {"type": "decimal", "computed": true, "expression": "a + b", "required": true, "default": 0}
        }
    });
    let body = apply_schema_expect_error(&router, &token, serde_json::json!([bad])).await;
    let text = body.to_string();
    assert!(
        text.contains("computed fields cannot be required")
            && text.contains("computed fields cannot have a default value"),
        "expected computed validation errors, got {body}"
    );

    // Circular dependency is rejected.
    let cyclic = serde_json::json!({
        "uid": "api::cyclic.cyclic",
        "kind": "collectionType",
        "info": {"singularName":"cyclic","pluralName":"cyclics","displayName":"Cyclic"},
        "attributes": {
            "a": {"type": "decimal", "computed": true, "expression": "b + 1", "dependencies": ["b"]},
            "b": {"type": "decimal", "computed": true, "expression": "a + 1", "dependencies": ["a"]}
        }
    });
    let body = apply_schema_expect_error(&router, &token, serde_json::json!([cyclic])).await;
    assert!(
        body.to_string().contains("circular dependency"),
        "expected circular dependency error, got {body}"
    );
}
