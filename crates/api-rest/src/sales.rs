//! REST sales endpoint backed by the imported ERP product/customer catalog.

use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use api_types::QueryParams;
use core_domain::{fk_column, relation_join_table};
use dynamic_store::dml;
use sea_orm::DbBackend;
use services::sales::{
    create_sale as calculate_sale, CustomerPricing, PackagingUnit, ProductPricing, SaleLineInput,
};

use crate::{auth::AdminCtx, error::AppError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSaleRequest {
    pub customer_code: String,
    pub lines: Vec<SaleLineInput>,
    pub manual_global_discount_percent: Option<f64>,
}

struct ProductRecord {
    id: i64,
    pricing: ProductPricing,
}

pub async fn create_sale(
    admin: AdminCtx,
    Json(req): Json<CreateSaleRequest>,
) -> Result<Json<Value>, AppError> {
    let products = load_products(&admin).await?;
    let (customer, customer_id) = load_customer(&admin, &req.customer_code).await?;
    let sale = calculate_sale(
        &products
            .iter()
            .map(|(sku, product)| (sku.clone(), product.pricing.clone()))
            .collect(),
        &customer,
        &req.lines,
        req.manual_global_discount_percent,
    )
    .map_err(services::ServiceError::internal)?;
    let invoice = persist_sale(&admin, &req, &sale, &products, customer_id).await?;
    Ok(Json(
        serde_json::json!({ "data": sale, "invoice": invoice }),
    ))
}

async fn load_products(admin: &AdminCtx) -> Result<HashMap<String, ProductRecord>, AppError> {
    let rows = services::cm_list(&admin.0, "api::product.product", &QueryParams::default())
        .await?
        .data;
    let mut products = HashMap::new();
    for row in rows {
        let sku = row
            .get("sku")
            .and_then(Value::as_str)
            .ok_or_else(|| services::ServiceError::validation("product", vec![]))?;
        let packaging = row
            .get("attributes")
            .and_then(|v| v.get("packaging"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(unit, value)| {
                (
                    unit,
                    PackagingUnit {
                        units: value.get("units").and_then(Value::as_f64).unwrap_or(0.0),
                        boxes: value.get("boxes").and_then(Value::as_f64),
                    },
                )
            })
            .collect();
        let unit_price = row
            .get("sale_price")
            .and_then(Value::as_f64)
            .ok_or_else(|| services::ServiceError::validation("product", vec![]))?;
        let id = row
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| services::ServiceError::validation("product", vec![]))?;
        products.insert(
            sku.to_string(),
            ProductRecord {
                id,
                pricing: ProductPricing {
                    sku: sku.to_string(),
                    unit_price,
                    packaging,
                },
            },
        );
    }
    Ok(products)
}

async fn load_customer(admin: &AdminCtx, code: &str) -> Result<(CustomerPricing, i64), AppError> {
    let rows = services::cm_list(
        &admin.0,
        "api::organization.organization",
        &QueryParams::default(),
    )
    .await?
    .data;
    let row = rows
        .into_iter()
        .find(|row| row.get("tax_id").and_then(Value::as_str) == Some(code))
        .ok_or_else(|| services::ServiceError::not_found(format!("customer `{code}` not found")))?;
    let id = row
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| services::ServiceError::validation("customer", vec![]))?;
    Ok((
        CustomerPricing {
            global_discount_percent: row
                .get("global_discount_percent")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            specific_discounts: serde_json::from_value(
                row.get("specific_discounts")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            )
            .map_err(|e| services::ServiceError::validation(e.to_string(), vec![]))?,
        },
        id,
    ))
}

async fn persist_sale(
    admin: &AdminCtx,
    req: &CreateSaleRequest,
    sale: &services::sales::CalculatedSale,
    products: &HashMap<String, ProductRecord>,
    customer_id: i64,
) -> Result<Value, AppError> {
    let document_number = format!("sale-{}", uuid::Uuid::new_v4().simple());
    let invoice = services::cm_create(
        &admin.0,
        "api::sales-invoice.sales-invoice",
        &serde_json::json!({
            "document_number": document_number,
            "document_type": "invoice",
            "status": "draft",
            "currency": "EUR",
            "net_amount": sale.gross_amount,
            "discount_amount": sale.discount_amount,
            "total_amount": sale.final_amount,
            "payment_status": "unpaid"
        }),
    )
    .await
    .map_err(services::ServiceError::from)?;
    let invoice_id = invoice.data["id"]
        .as_i64()
        .ok_or_else(|| services::ServiceError::internal("created invoice has no id"))?;
    let invoice_schema = admin
        .0
        .schema_cache
        .get(&core_domain::Uid::new("api::sales-invoice.sales-invoice"))
        .ok_or_else(|| services::ServiceError::not_found("sales invoice schema"))?;
    dml::update_by_id(
        &admin.0.db,
        DbBackend::Sqlite,
        &invoice_schema.table_name(),
        invoice_id,
        vec![(fk_column("customer"), customer_id.into())],
    )
    .await
    .map_err(services::ServiceError::from)?;

    let line_schema = admin
        .0
        .schema_cache
        .get(&core_domain::Uid::new(
            "api::sales-invoice-line.sales-invoice-line",
        ))
        .ok_or_else(|| services::ServiceError::not_found("sales invoice line schema"))?;
    let line_table = line_schema.table_name();
    let invoice_table = invoice_schema.table_name();
    let join_table = relation_join_table(&invoice_table, "lines");
    let owner_col = "sales_invoice_id";
    let target_col = "sales_invoice_line_id";
    for (position, line) in sale.lines.iter().enumerate() {
        let product = products
            .get(&line.sku)
            .ok_or_else(|| services::ServiceError::not_found(format!("product `{}`", line.sku)))?;
        let created = services::cm_create(
            &admin.0,
            "api::sales-invoice-line.sales-invoice-line",
            &serde_json::json!({
                "quantity": line.final_quantity,
                "unit_price": line.unit_price,
                "price_source": if line.unit_price == product.pricing.unit_price { "default" } else { "manual" },
                "discount_rate": line.discount_percent,
                "line_total": line.final_amount
            }),
        )
        .await
        .map_err(services::ServiceError::from)?;
        let line_id = created.data["id"]
            .as_i64()
            .ok_or_else(|| services::ServiceError::internal("created invoice line has no id"))?;
        dml::update_by_id(
            &admin.0.db,
            DbBackend::Sqlite,
            &line_table,
            line_id,
            vec![
                (fk_column("invoice"), invoice_id.into()),
                (fk_column("product"), product.id.into()),
            ],
        )
        .await
        .map_err(services::ServiceError::from)?;
        dml::insert_link_row(
            &admin.0.db,
            DbBackend::Sqlite,
            &join_table,
            vec![
                (owner_col.into(), invoice_id.into()),
                (target_col.into(), line_id.into()),
                ("lines_order".into(), (position as f64).into()),
            ],
        )
        .await
        .map_err(services::ServiceError::from)?;
    }
    let _ = req;
    Ok(invoice.data)
}
