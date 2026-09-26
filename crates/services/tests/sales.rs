use std::collections::HashMap;

use api_types::{
    FileImportConfig, ImportMode, ImportState, MappingDto, MappingStatus, TransformKind,
};
use db::{connect_sqlite_memory, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use serde_json::Value;
use services::{AppConfig, AppContext, CurrentUser};

use services::sales::{create_sale, CustomerPricing, PackagingUnit, ProductPricing, SaleLineInput};

fn admin() -> CurrentUser {
    CurrentUser {
        id: 1,
        email: "sales-test@ferriscms.test".into(),
        is_active: true,
        roles: vec!["strapi-super-admin".into()],
    }
}

fn mapping(source: &str, target: &str) -> MappingDto {
    MappingDto {
        source_field: source.into(),
        target_field: Some(target.into()),
        transform: TransformKind::None,
        status: MappingStatus::AutoMapped,
        confidence: 1.0,
    }
}

fn import_config(
    dataset: &str,
    uid: &str,
    mappings: Vec<MappingDto>,
    sample: &str,
) -> FileImportConfig {
    FileImportConfig {
        filename: "erp-sample.json".into(),
        dataset: dataset.into(),
        content: sample.into(),
        uid: uid.into(),
        mapping: mappings,
        mode: ImportMode::CreateOnly,
        match_field: None,
        state_field: None,
        import_state: ImportState::Draft,
        locale_field: None,
        locale: "en".into(),
        csv_delimiter: None,
        csv_has_header: None,
    }
}

async fn setup_imported_sample() -> AppContext {
    let db = connect_sqlite_memory().await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let ctx = AppContext::new(
        db,
        AppConfig {
            db_driver: "sqlite".into(),
            ..Default::default()
        },
    )
    .with_user(Some(admin()));

    let erp: Value =
        serde_json::from_str(include_str!("../../../examples/content-types/erp.json")).unwrap();
    services::ctb_import(&ctx, &erp).await.unwrap();

    let sample = include_str!("../../../examples/content-types/erp-sample.json");
    let products = import_config(
        "products",
        "api::product.product",
        vec![
            mapping("Sku", "sku"),
            mapping("Name", "name"),
            mapping("Unit", "unit"),
            mapping("Price", "sale_price"),
            mapping("Attributes", "attributes"),
        ],
        sample,
    );
    let customers = import_config(
        "customers",
        "api::organization.organization",
        vec![
            mapping("Code", "tax_id"),
            mapping("Name", "legal_name"),
            mapping("GlobalDiscountPercent", "global_discount_percent"),
            mapping("SpecificDiscounts", "specific_discounts"),
        ],
        sample,
    );
    let response = services::run_import(
        &ctx,
        &api_types::ImportRequest {
            files: vec![products, customers],
        },
    )
    .await
    .unwrap();
    assert_eq!(response.created, 5, "import errors: {:?}", response.errors);
    assert_eq!(response.failed, 0, "import errors: {:?}", response.errors);
    ctx
}

async fn imported_prices(ctx: &AppContext) -> HashMap<String, ProductPricing> {
    let rows = services::cm_list(
        ctx,
        "api::product.product",
        &api_types::QueryParams::default(),
    )
    .await
    .unwrap()
    .data;
    rows.into_iter()
        .map(|row| {
            let packaging = row["attributes"]["packaging"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(unit, value)| {
                    (
                        unit.clone(),
                        PackagingUnit {
                            units: value["units"].as_f64().unwrap(),
                            boxes: value.get("boxes").and_then(Value::as_f64),
                        },
                    )
                })
                .collect();
            (
                row["sku"].as_str().unwrap().to_string(),
                ProductPricing {
                    sku: row["sku"].as_str().unwrap().to_string(),
                    unit_price: row["sale_price"].as_f64().unwrap(),
                    packaging,
                },
            )
        })
        .collect()
}

async fn imported_customer(ctx: &AppContext, code: &str) -> CustomerPricing {
    let rows = services::cm_list(
        ctx,
        "api::organization.organization",
        &api_types::QueryParams::default(),
    )
    .await
    .unwrap()
    .data;
    let row = rows
        .into_iter()
        .find(|row| row["tax_id"] == code)
        .unwrap_or_else(|| panic!("customer {code} was not imported"));
    CustomerPricing {
        global_discount_percent: row["global_discount_percent"].as_f64().unwrap_or(0.0),
        specific_discounts: serde_json::from_value(row["specific_discounts"].clone()).unwrap(),
    }
}

#[tokio::test]
async fn imported_sample_sales_apply_quantities_prices_and_customer_discounts() {
    let ctx = setup_imported_sample().await;
    let products = imported_prices(&ctx).await;

    let global = imported_customer(&ctx, "GLOBAL10").await;
    let sale = create_sale(
        &products,
        &global,
        &[
            SaleLineInput {
                sku: "PRODUCT-1".into(),
                quantity: 2.0,
                unit: "pallet".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
            SaleLineInput {
                sku: "PRODUCT-2".into(),
                quantity: 3.0,
                unit: "box".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
            SaleLineInput {
                sku: "PRODUCT-C".into(),
                quantity: 5.0,
                unit: "box".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
        ],
        None,
    )
    .unwrap();

    assert_eq!(sale.lines[0].final_quantity, 2.0 * 1400.0);
    assert_eq!(sale.lines[1].final_quantity, 3.0 * 20.0);
    assert_eq!(sale.lines[2].final_quantity, 5.0 * 24.0);
    assert_eq!(sale.gross_amount, 29_710.0);
    assert_eq!(sale.discount_amount, 2_971.0);
    assert_eq!(sale.final_amount, 26_739.0);
}

#[tokio::test]
async fn imported_sample_sales_apply_specific_and_manual_price_discounts() {
    let ctx = setup_imported_sample().await;
    let products = imported_prices(&ctx).await;
    let customer = imported_customer(&ctx, "PRODUCT15").await;

    let specific = create_sale(
        &products,
        &customer,
        &[
            SaleLineInput {
                sku: "PRODUCT-1".into(),
                quantity: 1.0,
                unit: "box".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
            SaleLineInput {
                sku: "PRODUCT-2".into(),
                quantity: 2.0,
                unit: "pallet".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
            SaleLineInput {
                sku: "PRODUCT-C".into(),
                quantity: 4.0,
                unit: "box".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
        ],
        None,
    )
    .unwrap();
    assert_eq!(specific.lines[0].discount_percent, 15.0);
    assert_eq!(specific.lines[1].discount_percent, 0.0);
    assert_eq!(specific.gross_amount, 39_468.0);
    assert_eq!(specific.discount_amount, 30.0);
    assert_eq!(specific.final_amount, 39_438.0);

    let manual = create_sale(
        &products,
        &customer,
        &[
            SaleLineInput {
                sku: "PRODUCT-1".into(),
                quantity: 2.0,
                unit: "unit".into(),
                manual_unit_price: Some(9.0),
                manual_discount_percent: None,
            },
            SaleLineInput {
                sku: "PRODUCT-2".into(),
                quantity: 1.0,
                unit: "box".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
            SaleLineInput {
                sku: "PRODUCT-C".into(),
                quantity: 1.0,
                unit: "box".into(),
                manual_unit_price: None,
                manual_discount_percent: None,
            },
        ],
        Some(5.0),
    )
    .unwrap();
    assert_eq!(manual.lines[0].unit_price, 9.0);
    assert_eq!(manual.lines[0].discount_percent, 5.0);
    assert_eq!(manual.final_amount, 437.0);

    let manual_line_discount = create_sale(
        &products,
        &customer,
        &[SaleLineInput {
            sku: "PRODUCT-1".into(),
            quantity: 1.0,
            unit: "box".into(),
            manual_unit_price: Some(11.0),
            manual_discount_percent: Some(25.0),
        }],
        Some(5.0),
    )
    .unwrap();
    assert_eq!(manual_line_discount.lines[0].discount_percent, 25.0);
    assert_eq!(manual_line_discount.lines[0].gross_amount, 220.0);
    assert_eq!(manual_line_discount.lines[0].discount_amount, 55.0);
    assert_eq!(manual_line_discount.final_amount, 165.0);
}
