//! REST/e2e coverage for the ERP Sales Invoice JSON content types.
//!
//! These tests deliberately use only the generic Content Manager REST API.
//! There is no sales-specific REST service: the JSON content type owns the
//! line calculations and relations.

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
        import: Default::default(),
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
            serde_json::json!({"email":"sales-rest@test.dev","password":"StrongPass123!"}),
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
    serde_json::json!({"sourceField":source,"targetField":target,"transform":"none","status":"autoMapped","confidence":1.0})
}

fn import_file(dataset: &str, uid: &str, mappings: Vec<Value>) -> Value {
    serde_json::json!({
        "filename":"erp-sample.json", "dataset":dataset,
        "content":include_str!("../../../examples/content-types/erp-sample.json"),
        "uid":uid, "mapping":mappings, "mode":"createOnly", "importState":"draft", "locale":"en"
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
    let import = router.clone().oneshot(request("POST", "/admin/import-export/import", serde_json::json!({
        "files":[
            import_file("packagings", "api::packaging.packaging", vec![mapping("Code","code"),mapping("Name","name"),mapping("Units","units")]),
            import_file("products", "api::product.product", vec![mapping("Sku","sku"),mapping("Name","name"),mapping("Price","sale_price"),mapping("Packaging","packaging")]),
            import_file("customers", "api::organization.organization", vec![mapping("Code","tax_id"),mapping("Name","legal_name"),mapping("GlobalDiscountPercent","global_discount_percent"),mapping("SpecificPrices","specific_prices")])
        ]
    }), Some(token))).await.unwrap();
    assert_eq!(import.status(), StatusCode::OK);
    let body = json(import).await;
    assert_eq!(body["data"]["created"], 8);
    assert_eq!(body["data"]["failed"], 0);
}

async fn list_entry(
    router: &axum::Router,
    uid: &str,
    token: &str,
    field: &str,
    value: &str,
) -> Value {
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/admin/content-manager/collection-types/{uid}"),
            Value::Null,
            Some(token),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json(response).await["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry[field] == value)
        .cloned()
        .unwrap()
}

#[tokio::test]
async fn rest_imports_erp_and_creates_sales_invoice_graph_with_generated_line_amounts() {
    let router = setup().await;
    let token = register(&router).await;
    import_sample(&router, &token).await;

    let product = list_entry(&router, "api::product.product", &token, "sku", "PRODUCT-1").await;
    let packaging = list_entry(&router, "api::packaging.packaging", &token, "code", "box").await;
    let customer = list_entry(
        &router,
        "api::organization.organization",
        &token,
        "tax_id",
        "PRODUCT15",
    )
    .await;

    let invoice = router.clone().oneshot(request("POST", "/admin/content-manager/collection-types/api::sales-invoice.sales-invoice", serde_json::json!({"data":{
        "document_number":"REST-INV-1","status":"draft","currency":"EUR","payment_status":"unpaid",
        "customer":{"documentId":customer["documentId"]},
        "pre_discount_amount":50,"discount_amount":5,"net_amount":45,"charges_amount":2,"vat_amount":12,"tax_amount":1,"total_amount":60,"withholding_amount":3,"payable_amount":57
    }}), Some(&token))).await.unwrap();
    assert_eq!(invoice.status(), StatusCode::OK);
    let invoice = json(invoice).await["data"].clone();

    let line = router.clone().oneshot(request("POST", "/admin/content-manager/collection-types/api::sales-invoice-line.sales-invoice-line", serde_json::json!({"data":{
        "invoice":{"documentId":invoice["documentId"]},"product":{"documentId":product["documentId"]},"packaging":{"documentId":packaging["documentId"]},
        "quantity":2,"unit_price":50,"base_unit_price":2.5,"discount_rate":10,"price_source":"customer"
    }}), Some(&token))).await.unwrap();
    assert_eq!(line.status(), StatusCode::OK);
    let line = json(line).await["data"].clone();
    assert_eq!(line["pre_discount_amount"], 100.0);
    assert_eq!(line["discount_amount"], 10.0);
    assert_eq!(line["net_amount"], 90.0);
    assert_eq!(line["line_total"], 90.0);

    let attached = router
        .clone()
        .oneshot(request(
            "PUT",
            &format!(
                "/admin/content-manager/collection-types/api::sales-invoice.sales-invoice/{}",
                invoice["documentId"].as_str().unwrap()
            ),
            serde_json::json!({"data":{"lines":[{"documentId":line["documentId"]}]}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(attached.status(), StatusCode::OK);
    let detail = router
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/admin/content-manager/collection-types/api::sales-invoice.sales-invoice/{}",
                invoice["documentId"].as_str().unwrap()
            ),
            Value::Null,
            Some(&token),
        ))
        .await
        .unwrap();
    let detail = json(detail).await["data"].clone();
    assert_eq!(detail["lines"].as_array().unwrap().len(), 1);
    assert!(detail["customer"]["tax_id"].is_string());
    let line_detail = router
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/admin/content-manager/collection-types/api::sales-invoice-line.sales-invoice-line/{}",
                line["documentId"].as_str().unwrap()
            ),
            Value::Null,
            Some(&token),
        ))
        .await
        .unwrap();
    let line_detail = json(line_detail).await["data"].clone();
    assert!(line_detail["product"]["sku"].is_string());
    assert!(line_detail["invoice"]["document_number"].is_string());
}

#[tokio::test]
async fn rest_content_type_recomputes_editable_line_values_and_rejects_generated_writes() {
    let router = setup().await;
    let token = register(&router).await;
    import_sample(&router, &token).await;
    let product = list_entry(&router, "api::product.product", &token, "sku", "PRODUCT-1").await;
    let line = router.clone().oneshot(request("POST", "/admin/content-manager/collection-types/api::sales-invoice-line.sales-invoice-line", serde_json::json!({"data":{
        "product":{"documentId":product["documentId"]},"quantity":3,"unit_price":9,"discount_rate":25
    }}), Some(&token))).await.unwrap();
    assert_eq!(line.status(), StatusCode::OK);
    let line = json(line).await["data"].clone();
    assert_eq!(line["pre_discount_amount"], 27.0);
    assert_eq!(line["discount_amount"], 6.75);
    assert_eq!(line["net_amount"], 20.25);

    let updated = router.clone().oneshot(request("PUT", &format!("/admin/content-manager/collection-types/api::sales-invoice-line.sales-invoice-line/{}", line["documentId"].as_str().unwrap()), serde_json::json!({"data":{"quantity":4,"unit_price":10,"discount_rate":0}}), Some(&token))).await.unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = json(updated).await["data"].clone();
    assert_eq!(updated["pre_discount_amount"], 40.0);
    assert_eq!(updated["line_total"], 40.0);

    let rejected = router.clone().oneshot(request("PUT", &format!("/admin/content-manager/collection-types/api::sales-invoice-line.sales-invoice-line/{}", line["documentId"].as_str().unwrap()), serde_json::json!({"data":{"line_total":999}}), Some(&token))).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
}
