//! REST-level coverage for the imported ERP sales scenarios.

use api_rest::{build_router, AppState};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use db::{connect_sqlite_memory, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use serde_json::Value;
use services::{load_schema_cache, AppConfig};
use std::sync::Arc;
use tower::ServiceExt;

fn config() -> AppConfig {
    AppConfig {
        db_driver: "sqlite".into(),
        jwt_secret: "sales-rest-test-secret".into(),
        jwt_expiry_secs: 3600,
        admin_registration_open: true,
        media_storage_dir: std::env::temp_dir()
            .join("ferris-sales-rest")
            .display()
            .to_string(),
    }
}

async fn setup() -> axum::Router {
    let db = connect_sqlite_memory().await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let state = Arc::new(AppState::new(db.clone(), config()));
    load_schema_cache(&db, &state.ctx.schema_cache)
        .await
        .unwrap();
    let _ = state.ctx.init_rbac().await;
    build_router(state)
}

fn request(method: &str, uri: &str, body: Value, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn register(router: &axum::Router) -> String {
    let response = router
        .clone()
        .oneshot(request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({
                "email": "sales-rest@test.dev",
                "password": "StrongPass123!"
            }),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json(response).await["data"]["token"]
        .as_str()
        .unwrap()
        .to_string()
}

fn mapping(source: &str, target: &str) -> Value {
    serde_json::json!({
        "sourceField": source,
        "targetField": target,
        "transform": "none",
        "status": "autoMapped",
        "confidence": 1.0
    })
}

fn import_file(dataset: &str, uid: &str, mappings: Vec<Value>, sample: &str) -> Value {
    serde_json::json!({
        "filename": "erp-sample.json",
        "dataset": dataset,
        "content": sample,
        "uid": uid,
        "mapping": mappings,
        "mode": "createOnly",
        "importState": "draft",
        "locale": "en"
    })
}

async fn import_sample(router: &axum::Router, token: &str) {
    let erp: Value =
        serde_json::from_str(include_str!("../../../examples/content-types/erp.json")).unwrap();
    let schema = router
        .clone()
        .oneshot(request(
            "POST",
            "/content-type-builder/bulk-import",
            erp,
            Some(token),
        ))
        .await
        .unwrap();
    assert_eq!(schema.status(), StatusCode::OK);

    let sample = include_str!("../../../examples/content-types/erp-sample.json");
    let import = router
        .clone()
        .oneshot(request(
            "POST",
            "/admin/import-export/import",
            serde_json::json!({
                "files": [
                    import_file("products", "api::product.product", vec![
                        mapping("Sku", "sku"), mapping("Name", "name"),
                        mapping("Unit", "unit"), mapping("Price", "sale_price"),
                        mapping("Attributes", "attributes")
                    ], sample),
                    import_file("customers", "api::organization.organization", vec![
                        mapping("Code", "tax_id"), mapping("Name", "legal_name"),
                        mapping("GlobalDiscountPercent", "global_discount_percent"),
                        mapping("SpecificDiscounts", "specific_discounts")
                    ], sample)
                ]
            }),
            Some(token),
        ))
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK);
    let body = json(import).await;
    assert_eq!(body["data"]["created"], 5);
    assert_eq!(body["data"]["failed"], 0);
}

#[tokio::test]
async fn rest_import_and_global_discount_sale_match_business_totals() {
    let router = setup().await;
    let token = register(&router).await;
    import_sample(&router, &token).await;
    let products = router
        .clone()
        .oneshot(request(
            "GET",
            "/admin/content-manager/collection-types/api::product.product",
            Value::Null,
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(products.status(), StatusCode::OK);
    assert_eq!(json(products).await["data"].as_array().unwrap().len(), 3);
    let unauthenticated = router
        .clone()
        .oneshot(request(
            "POST",
            "/admin/sales",
            serde_json::json!({"customerCode":"GLOBAL10","lines":[]}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let sale = router
        .clone()
        .oneshot(request(
            "POST",
            "/admin/sales",
            serde_json::json!({
                "customerCode":"GLOBAL10",
                "lines":[
                    {"sku":"PRODUCT-1","quantity":2,"unit":"pallet"},
                    {"sku":"PRODUCT-2","quantity":3,"unit":"box"},
                    {"sku":"PRODUCT-C","quantity":5,"unit":"box"}
                ]
            }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(sale.status(), StatusCode::OK);
    let response = json(sale).await;
    let invoice = response["invoice"].clone();
    let sale = response["data"].clone();
    assert_eq!(sale["lines"][0]["finalQuantity"], 2800.0);
    assert_eq!(sale["lines"][1]["finalQuantity"], 60.0);
    assert_eq!(sale["lines"][2]["finalQuantity"], 120.0);
    assert_eq!(sale["grossAmount"], 29_710.0);
    assert_eq!(sale["discountAmount"], 2_971.0);
    assert_eq!(sale["finalAmount"], 26_739.0);
    let invoice_id = invoice["documentId"].as_str().unwrap();
    let stored_invoice = router
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/admin/content-manager/collection-types/api::sales-invoice.sales-invoice/{invoice_id}"
            ),
            Value::Null,
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(stored_invoice.status(), StatusCode::OK);
    let stored_invoice = json(stored_invoice).await["data"].clone();
    assert_eq!(stored_invoice["total_amount"], 26_739.0);
    assert_eq!(stored_invoice["discount_amount"], 2_971.0);
    let stored_lines = router
        .clone()
        .oneshot(request(
            "GET",
            "/admin/content-manager/collection-types/api::sales-invoice-line.sales-invoice-line",
            Value::Null,
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(stored_lines.status(), StatusCode::OK);
    assert_eq!(
        json(stored_lines).await["data"].as_array().unwrap().len(),
        3
    );
}

#[tokio::test]
async fn rest_import_and_manual_price_discount_sales_match_business_totals() {
    let router = setup().await;
    let token = register(&router).await;
    import_sample(&router, &token).await;
    let specific_sale = router
        .clone()
        .oneshot(request(
            "POST",
            "/admin/sales",
            serde_json::json!({
                "customerCode":"PRODUCT15",
                "lines":[
                    {"sku":"PRODUCT-1","quantity":1,"unit":"box"},
                    {"sku":"PRODUCT-2","quantity":2,"unit":"pallet"},
                    {"sku":"PRODUCT-C","quantity":4,"unit":"box"}
                ]
            }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(specific_sale.status(), StatusCode::OK);
    let specific_sale = json(specific_sale).await["data"].clone();
    assert_eq!(specific_sale["grossAmount"], 39_468.0);
    assert_eq!(specific_sale["discountAmount"], 30.0);
    assert_eq!(specific_sale["finalAmount"], 39_438.0);

    let manual_global = router
        .clone()
        .oneshot(request(
            "POST",
            "/admin/sales",
            serde_json::json!({
                "customerCode":"PRODUCT15","manualGlobalDiscountPercent":5,
                "lines":[
                    {"sku":"PRODUCT-1","quantity":2,"unit":"unit","manualUnitPrice":9},
                    {"sku":"PRODUCT-2","quantity":1,"unit":"box"},
                    {"sku":"PRODUCT-C","quantity":1,"unit":"box"}
                ]
            }),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(manual_global.status(), StatusCode::OK);
    assert_eq!(json(manual_global).await["data"]["finalAmount"], 437.0);

    let manual_line = router.clone().oneshot(request("POST", "/admin/sales", serde_json::json!({
        "customerCode":"PRODUCT15","manualGlobalDiscountPercent":5,
        "lines":[{"sku":"PRODUCT-1","quantity":1,"unit":"box","manualUnitPrice":11,"manualDiscountPercent":25}]
    }), Some(&token))).await.unwrap();
    assert_eq!(manual_line.status(), StatusCode::OK);
    let manual_line = json(manual_line).await["data"].clone();
    assert_eq!(manual_line["lines"][0]["discountPercent"], 25.0);
    assert_eq!(manual_line["finalAmount"], 165.0);
}
