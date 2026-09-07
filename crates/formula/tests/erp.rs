//! ERP integration tests for the Formula subsystem.
//!
//! Runs against a real SQLite database through Ferris's dynamic-store DDL and
//! the actual sea-query query path. Verifies the ERP chain:
//!
//! ```text
//! SaleLine.net_price, final_price, margin
//! Sale.subtotal = SUM(lines.net_price)
//! Customer.total_revenue = SUM(sales.total)   (nested aggregate)
//! Customer.outstanding  = SUM(sales.balance WHERE sales.status = "OPEN")
//! ```
//!
//! Computed fields are virtual: they never become physical columns and are
//! produced by the sea-query compiler in a single correlated query per host
//! (no N+1 per row).

use core_domain::{ContentTypeKind, FieldType, RelationKind, Uid};
use core_schema::{
    Attribute, ExecutionStrategy, FormulaConfig, FormulaType, Schema, SchemaInfo,
};
use dynamic_store::base_columns as base;
use dynamic_store::{ddl, dml};
use formula::sql::build_expr;
use indexmap::IndexMap;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend};
use sea_query::{Alias, Query};
use std::collections::BTreeMap;

fn schema(uid: &str, singular: &str, attrs: Vec<(&str, Attribute)>) -> Schema {
    Schema {
        uid: Uid::new(uid),
        kind: ContentTypeKind::CollectionType,
        collection_name: None,
        info: SchemaInfo {
            singular_name: singular.into(),
            plural_name: format!("{singular}s"),
            display_name: singular.into(),
            description: None,
            icon: None,
        },
        options: Default::default(),
        plugin_options: None,
        attributes: attrs
            .into_iter()
            .map(|(n, a)| (n.to_string(), a))
            .collect::<IndexMap<_, _>>(),
        metadata: None,
    }
}

fn num() -> Attribute {
    Attribute::new(FieldType::Decimal)
}
fn int() -> Attribute {
    Attribute::new(FieldType::Integer)
}
fn str_attr() -> Attribute {
    Attribute::new(FieldType::String)
}
fn date() -> Attribute {
    Attribute::new(FieldType::Date)
}

fn formula(expression: &str, ret: FormulaType) -> Attribute {
    Attribute {
        attr_type: FieldType::Decimal,
        formula: Some(FormulaConfig {
            expression: expression.into(),
            return_type: ret,
            execution_strategy: ExecutionStrategy::Computed,
            dependencies: vec![],
        }),
        ..Default::default()
    }
}

fn relation(kind: RelationKind, target: &str, mapped_by: Option<&str>) -> Attribute {
    Attribute {
        attr_type: FieldType::Relation,
        relation: Some(kind),
        target: Some(Uid::new(target)),
        mapped_by: mapped_by.map(|s| s.to_string()),
        ..Default::default()
    }
}

/// Build the ERP schema registry:
/// Customer [Product] [Order] Sale SaleLine
fn erp_schemas() -> Vec<Schema> {
    // Product: id, name, cost
    let product = schema(
        "api::product.product",
        "product",
        vec![("name", str_attr()), ("cost", num())],
    );
    // Order: id, customer (manyToOne), status
    let order = schema(
        "api::order.order",
        "order",
        vec![
            ("status", str_attr()),
            ("customer", relation(RelationKind::ManyToOne, "api::customer.customer", None)),
        ],
    );
    // Sale: id, customer, order, discount, vat_rate, balance, status, due_date
    let sale = schema(
        "api::sale.sale",
        "sale",
        vec![
            ("discount", num()),
            ("vat_rate", int()),
            ("balance", num()),
            ("status", str_attr()),
            ("due_date", date()),
            ("customer", relation(RelationKind::ManyToOne, "api::customer.customer", None)),
            (
                "lines",
                relation(
                    RelationKind::OneToMany,
                    "api::sale-line.sale_line",
                    Some("sale"),
                ),
            ),
            ("subtotal", formula("SUM(lines.net_price)", FormulaType::Decimal)),
            ("total", formula("SUM(lines.final_price)", FormulaType::Decimal)),
        ],
    );
    // SaleLine: id, sale, product, quantity, unit_price, discount
    let sale_line = schema(
        "api::sale-line.sale_line",
        "saleLine",
        vec![
            ("quantity", int()),
            ("unit_price", num()),
            ("discount", num()),
            ("sale", relation(RelationKind::ManyToOne, "api::sale.sale", None)),
            ("product", relation(RelationKind::ManyToOne, "api::product.product", None)),
            (
                "net_price",
                formula(
                    "(quantity * unit_price) - COALESCE(discount, 0)",
                    FormulaType::Decimal,
                ),
            ),
            (
                "final_price",
                formula("net_price + (net_price * 0)", FormulaType::Decimal),
            ),
            (
                "margin",
                formula("net_price - (quantity * product.cost)", FormulaType::Decimal),
            ),
        ],
    );
    // Customer: id, name
    let customer = schema(
        "api::customer.customer",
        "customer",
        vec![
            ("name", str_attr()),
            (
                "sales",
                relation(RelationKind::OneToMany, "api::sale.sale", Some("customer")),
            ),
            (
                "total_revenue",
                formula("SUM(sales.total)", FormulaType::Decimal),
            ),
            (
                "outstanding",
                formula(
                    "SUM(sales.balance WHERE sales.status = \"OPEN\")",
                    FormulaType::Decimal,
                ),
            ),
        ],
    );
    vec![customer, sale_line, sale, product, order]
}

async fn setup() -> (DatabaseConnection, Vec<Schema>) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let backend = DbBackend::Sqlite;
    let all = erp_schemas();
    // two-phase: host tables first, then aux (inverse FK columns / links)
    for s in &all {
        let d = core_schema::diff(None, s);
        ddl::apply_schema_diff(&db, backend, &d, &all).await.unwrap();
    }
    for s in &all {
        ddl::apply_aux(&db, backend, s, &all).await.unwrap();
    }
    (db, all)
}

fn host_by_uid<'a>(all: &'a [Schema], uid: &str) -> &'a Schema {
    all.iter().find(|s| s.uid.as_str() == uid).expect("schema")
}

fn table_of(uid: &str) -> &'static str {
    match uid {
        "api::sale.sale" => "ct_sales",
        "api::sale-line.sale_line" => "ct_sale_lines",
        "api::product.product" => "ct_products",
        "api::customer.customer" => "ct_customers",
        "api::order.order" => "ct_orders",
        other => panic!("unknown table for {other}"),
    }
}

// ---- insert helpers ---------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn insert_sale(
    db: &DatabaseConnection,
    backend: DbBackend,
    customer_id: i64,
    balance: f64,
    status: &str,
    due_date: &str,
    vat_rate: i64,
) -> i64 {
    let vals: Vec<(String, sea_orm::sea_query::Value)> = vec![
        (base::DOCUMENT_ID.into(), format!("sale-{customer_id}-{status}-{balance}").into()),
        (base::PUBLICATION_STATE.into(), "published".into()),
        (base::CREATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::UPDATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::SYNC_VERSION.into(), 1_i64.into()),
        ("customer_id".into(), customer_id.into()),
        ("balance".into(), sea_orm::sea_query::Value::Double(Some(balance))),
        ("status".into(), status.into()),
        ("due_date".into(), due_date.into()),
        ("vat_rate".into(), vat_rate.into()),
        ("discount".into(), sea_orm::sea_query::Value::Double(Some(0.0))),
    ];
    dml::insert(db, backend, &table_of("api::sale.sale"), vals).await.unwrap()
}

async fn insert_sale_line(
    db: &DatabaseConnection,
    backend: DbBackend,
    sale_id: i64,
    product_id: i64,
    quantity: i64,
    unit_price: f64,
    discount: Option<f64>,
) -> i64 {
    let vals: Vec<(String, sea_orm::sea_query::Value)> = vec![
        (base::DOCUMENT_ID.into(), format!("sl-{sale_id}-{quantity}").into()),
        (base::PUBLICATION_STATE.into(), "published".into()),
        (base::CREATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::UPDATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::SYNC_VERSION.into(), 1_i64.into()),
        ("sale_id".into(), sale_id.into()),
        ("product_id".into(), product_id.into()),
        ("quantity".into(), quantity.into()),
        ("unit_price".into(), sea_orm::sea_query::Value::Double(Some(unit_price))),
        (
            "discount".into(),
            discount
                .map(|d| sea_orm::sea_query::Value::Double(Some(d)))
                .unwrap_or(sea_orm::sea_query::Value::Double(None)),
        ),
    ];
    dml::insert(db, backend, &table_of("api::sale-line.sale_line"), vals).await.unwrap()
}

async fn insert_product(
    db: &DatabaseConnection,
    backend: DbBackend,
    name: &str,
    cost: f64,
) -> i64 {
    let vals: Vec<(String, sea_orm::sea_query::Value)> = vec![
        (base::DOCUMENT_ID.into(), format!("p-{name}").into()),
        (base::PUBLICATION_STATE.into(), "published".into()),
        (base::CREATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::UPDATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::SYNC_VERSION.into(), 1_i64.into()),
        ("name".into(), name.into()),
        ("cost".into(), sea_orm::sea_query::Value::Double(Some(cost))),
    ];
    dml::insert(db, backend, &table_of("api::product.product"), vals).await.unwrap()
}

async fn insert_customer(db: &DatabaseConnection, backend: DbBackend, name: &str) -> i64 {
    let vals: Vec<(String, sea_orm::sea_query::Value)> = vec![
        (base::DOCUMENT_ID.into(), format!("c-{name}").into()),
        (base::PUBLICATION_STATE.into(), "published".into()),
        (base::CREATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::UPDATED_AT.into(), "2024-01-01T00:00:00Z".into()),
        (base::SYNC_VERSION.into(), 1_i64.into()),
        ("name".into(), name.into()),
    ];
    dml::insert(db, backend, &table_of("api::customer.customer"), vals).await.unwrap()
}

// ---- query helper -----------------------------------------------------------

/// Compute a formula field across every row of its host schema, returning
/// `(row_id -> value)`. The formula is compiled to SQL and appended as a
/// computed column; the whole set is fetched in ONE query.
async fn compute_column(
    db: &DatabaseConnection,
    all: &[Schema],
    host: &Schema,
    field: &str,
) -> BTreeMap<i64, Option<f64>> {
    let compiled = build_expr(all, host, field).expect("compiles");
    let table = host.table_name();
    let mut sel = Query::select();
    sel.column(Alias::new(base::ID))
        .from(Alias::new(&table))
        .expr_as(compiled.sql, Alias::new(field));
    let rows = db.query_all(&sel).await.unwrap();
    let mut out = BTreeMap::new();
    for r in &rows {
        let id: i64 = r.try_get("", base::ID).unwrap();
        let v: Option<f64> = r.try_get("", field).unwrap_or(None);
        out.insert(id, v);
    }
    out
}

fn approx(a: Option<f64>, b: f64) -> bool {
    match a {
        None => false,
        Some(x) => (x - b).abs() < 1e-6,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test1_and_4_line_net_price_and_margin() {
    let (db, all) = setup().await;
    let backend = DbBackend::Sqlite;

    // Two products with different costs.
    let cheap = insert_product(&db, backend, "A", 5.00).await;
    let expensive = insert_product(&db, backend, "B", 2.00).await;

    // Customers A, B, C each with a line at different unit prices for product A.
    let cust_a = insert_customer(&db, backend, "A").await;
    let cust_b = insert_customer(&db, backend, "B").await;
    let cust_c = insert_customer(&db, backend, "C").await;

    for (cust, up, prod, discount) in [
        (cust_a, 10.0, cheap, None),
        (cust_b, 8.0, cheap, None),
        (cust_c, 6.5, cheap, None),
    ] {
        let sid = insert_sale(&db, backend, cust, 0.0, "OPEN", "2020-01-01", 24).await;
        insert_sale_line(&db, backend, sid, prod, 10, up, discount).await;
        // Also one line on expensive product with discount.
        insert_sale_line(&db, backend, sid, expensive, 1, 10.0, Some(0.0)).await;
    }

    let host = host_by_uid(&all, "api::sale-line.sale_line");

    // net_price for the 10x lines.
    let net = compute_column(&db, &all, host, "net_price").await;
    for v in net.values() {
        // Each SaleLine has either net 100/80/65 (10 units) or 10.
        assert!(approx(*v, 100.0) || approx(*v, 80.0) || approx(*v, 65.0) || approx(*v, 10.0));
    }

    // margin = net_price - quantity * product.cost for the 10-unit cheap lines.
    let margin = compute_column(&db, &all, host, "margin").await;
    // Expected: A 100 - 50 = 50, B 80-50 = 30, C 65-50 = 15.
    let values: Vec<f64> = margin.values().flatten().copied().collect();
    assert!(values.contains(&50.0), "{values:?}");
    assert!(values.contains(&30.0), "{values:?}");
    assert!(values.contains(&15.0), "{values:?}");
}

#[tokio::test]
async fn test5_subtotal_and_nested_revenue_with_correlation() {
    let (db, all) = setup().await;
    let backend = DbBackend::Sqlite;

    let product = insert_product(&db, backend, "A", 5.00).await;

    // Customer A: two sales.
    let cust_a = insert_customer(&db, backend, "A").await;
    // Sale 1: lines 10x8 (disc 5 -> net 75) and 2x20 (net 40) -> subtotal 115
    let s1 = insert_sale(&db, backend, cust_a, 42.60, "OPEN", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s1, product, 10, 8.0, Some(5.0)).await;
    insert_sale_line(&db, backend, s1, product, 2, 20.0, None).await;
    // Sale 2: 1 line 10x25 -> net 250
    let s2 = insert_sale(&db, backend, cust_a, 0.0, "PAID", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s2, product, 10, 25.0, None).await;

    // Customer B: one sale -> 10x7.5 = 75
    let cust_b = insert_customer(&db, backend, "B").await;
    let s3 = insert_sale(&db, backend, cust_b, 75.0, "OPEN", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s3, product, 10, 7.5, None).await;

    // Sale.subtotal = SUM(lines.net_price)
    let sale_host = host_by_uid(&all, "api::sale.sale");
    let subtotals = compute_column(&db, &all, sale_host, "subtotal").await;
    // s1 = 75 + 40 = 115; s2 = 250; s3 = 75.
    let sub_vals: Vec<f64> = subtotals.values().flatten().copied().collect();
    assert!(sub_vals.contains(&115.0), "{sub_vals:?}");
    assert!(sub_vals.contains(&250.0), "{sub_vals:?}");
    assert!(sub_vals.contains(&75.0), "{sub_vals:?}");

    // Customer.total_revenue = SUM(sales.total)  where total=SUM(lines.final_price).
    // final_price = net_price + 0 = net_price, so revenue == SUM(subtotals).
    let cust_host = host_by_uid(&all, "api::customer.customer");
    let revenue = compute_column(&db, &all, cust_host, "total_revenue").await;
    let rev_a = revenue.get(&cust_a).copied().flatten().unwrap();
    let rev_b = revenue.get(&cust_b).copied().flatten().unwrap();
    assert!(approx(Some(rev_a), 365.0), "customer A revenue {rev_a}");
    assert!(approx(Some(rev_b), 75.0), "customer B revenue {rev_b}");
    // Crucial: A's revenue is NOT the sum of B's sales (correlation).
    assert_ne!(rev_a, rev_b);
}

#[tokio::test]
async fn test9_outstanding_filtered_aggregate() {
    let (db, all) = setup().await;
    let backend = DbBackend::Sqlite;
    let product = insert_product(&db, backend, "A", 5.00).await;

    // Customer with Sale1 OPEN balance 42.60, Sale2 PAID balance 0, Sale3 OPEN 75.
    let cust = insert_customer(&db, backend, "X").await;
    let s1 = insert_sale(&db, backend, cust, 42.60, "OPEN", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s1, product, 1, 10.0, None).await;
    let s2 = insert_sale(&db, backend, cust, 0.0, "PAID", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s2, product, 1, 10.0, None).await;
    let s3 = insert_sale(&db, backend, cust, 75.0, "OPEN", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s3, product, 1, 10.0, None).await;

    let cust_host = host_by_uid(&all, "api::customer.customer");
    let out = compute_column(&db, &all, cust_host, "outstanding").await;
    let v = out.get(&cust).copied().flatten().unwrap();
    assert!(approx(Some(v), 117.60), "outstanding {v}");
}

#[tokio::test]
async fn test_null_coalesce_equals_zero() {
    let (db, all) = setup().await;
    let backend = DbBackend::Sqlite;
    let product = insert_product(&db, backend, "A", 5.00).await;
    let cust = insert_customer(&db, backend, "Z").await;
    // Two sales, identical line except discount: NULL vs 0.
    let s_null = insert_sale(&db, backend, cust, 0.0, "OPEN", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s_null, product, 10, 8.0, None).await;
    let s_zero = insert_sale(&db, backend, cust, 0.0, "OPEN", "2020-01-01", 24).await;
    insert_sale_line(&db, backend, s_zero, product, 10, 8.0, Some(0.0)).await;

    let line_host = host_by_uid(&all, "api::sale-line.sale_line");
    let net = compute_column(&db, &all, line_host, "net_price").await;
    let vals: Vec<f64> = net.values().flatten().copied().collect();
    assert_eq!(vals.len(), 2);
    for v in vals {
        assert!(approx(Some(v), 80.0), "net price {v}");
    }
}
