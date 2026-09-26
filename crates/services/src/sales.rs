//! Sales pricing and quantity calculation over ERP product/customer data.
//!
//! This module is deliberately independent of dynamic-table IDs. It can be
//! used by an invoice/sale command, an API handler, or an import workflow.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
pub struct ProductPricing {
    pub sku: String,
    pub unit_price: f64,
    #[serde(default)]
    pub packaging: HashMap<String, PackagingUnit>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackagingUnit {
    pub units: f64,
    #[serde(default)]
    pub boxes: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CustomerPricing {
    #[serde(default)]
    pub global_discount_percent: f64,
    #[serde(default)]
    pub specific_discounts: HashMap<String, f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleLineInput {
    pub sku: String,
    pub quantity: f64,
    pub unit: String,
    #[serde(default)]
    pub manual_unit_price: Option<f64>,
    #[serde(default)]
    pub manual_discount_percent: Option<f64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalculatedSaleLine {
    pub sku: String,
    pub entered_quantity: f64,
    pub unit: String,
    pub final_quantity: f64,
    pub unit_price: f64,
    pub discount_percent: f64,
    pub gross_amount: f64,
    pub discount_amount: f64,
    pub final_amount: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalculatedSale {
    pub lines: Vec<CalculatedSaleLine>,
    pub gross_amount: f64,
    pub discount_amount: f64,
    pub final_amount: f64,
}

fn money(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Create a sale calculation using base-unit prices and packaging conversions.
/// Discount precedence is line manual, sale manual global, customer-specific,
/// then customer global. Manual prices always override the product price.
pub fn create_sale(
    products: &HashMap<String, ProductPricing>,
    customer: &CustomerPricing,
    lines: &[SaleLineInput],
    manual_global_discount_percent: Option<f64>,
) -> Result<CalculatedSale, String> {
    let mut calculated = Vec::with_capacity(lines.len());
    for input in lines {
        let product = products
            .get(&input.sku)
            .ok_or_else(|| format!("unknown product `{}`", input.sku))?;
        let multiplier = if input.unit == "unit" {
            1.0
        } else {
            product
                .packaging
                .get(&input.unit)
                .ok_or_else(|| {
                    format!("product `{}` has no `{}` conversion", input.sku, input.unit)
                })?
                .units
        };
        let final_quantity = input.quantity * multiplier;
        let unit_price = input.manual_unit_price.unwrap_or(product.unit_price);
        let discount_percent = input
            .manual_discount_percent
            .or(manual_global_discount_percent)
            .or_else(|| customer.specific_discounts.get(&input.sku).copied())
            .unwrap_or(customer.global_discount_percent);
        if !(0.0..=100.0).contains(&discount_percent) {
            return Err(format!(
                "discount must be between 0 and 100 for `{}`",
                input.sku
            ));
        }
        let gross_amount = money(final_quantity * unit_price);
        let discount_amount = money(gross_amount * discount_percent / 100.0);
        calculated.push(CalculatedSaleLine {
            sku: input.sku.clone(),
            entered_quantity: input.quantity,
            unit: input.unit.clone(),
            final_quantity,
            unit_price,
            discount_percent,
            gross_amount,
            discount_amount,
            final_amount: money(gross_amount - discount_amount),
        });
    }
    let gross_amount = money(calculated.iter().map(|line| line.gross_amount).sum());
    let discount_amount = money(calculated.iter().map(|line| line.discount_amount).sum());
    Ok(CalculatedSale {
        lines: calculated,
        gross_amount,
        discount_amount,
        final_amount: money(gross_amount - discount_amount),
    })
}
