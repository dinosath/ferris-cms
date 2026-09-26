//! Computed-field acceptance test against a **real PostgreSQL** database.
//!
//! PostgreSQL is the production database and only supports `STORED` generated
//! columns, so this is the true acceptance boundary for computed fields (the
//! SQLite tests cover the rest). It drives the real service API
//! (`ctb_apply` → `cm_create`/`cm_update`/`cm_list`), which runs real DDL and
//! DML against Postgres.
//!
//! Ignored by default; requires a reachable, disposable Postgres:
//!
//! ```text
//! TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:55432/ferriscms \
//!   cargo test -p services --test computed_postgres -- --ignored --nocapture
//! ```

use api_types::QueryParams;
use db::{connect, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{AppConfig, AppContext, CurrentUser};

fn num(v: &serde_json::Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(f64::NAN)
}

fn super_admin_ctx() -> CurrentUser {
    CurrentUser {
        id: 0,
        email: "acceptance@test.dev".into(),
        is_active: true,
        roles: vec!["strapi-super-admin".to_string()],
    }
}

#[ignore = "requires TEST_POSTGRES_URL (real PostgreSQL)"]
#[tokio::test]
async fn computed_fields_on_real_postgres() {
    let url = match std::env::var("TEST_POSTGRES_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("TEST_POSTGRES_URL not set; skipping Postgres acceptance test");
            return;
        }
    };

    let db = connect(&url).await.expect("connect to postgres");
    Migrator::up(&db, None).await.expect("migrate");
    seed::seed(&db).await.expect("seed");

    let config = AppConfig {
        db_driver: "postgres".into(),
        ..Default::default()
    };
    let ctx = AppContext::new(db.clone(), config).with_user(Some(super_admin_ctx()));
    // The server treats RBAC init failures as a warning; mirror that here.
    if let Err(e) = ctx.init_rbac().await {
        eprintln!("rbac init skipped: {e}");
    }

    // Unique names so the test can be re-run against the same database.
    let suffix = chrono::Utc::now().timestamp_millis();
    let name = format!("so{suffix}");
    let uid = format!("api::{name}.{name}");

    let schema: core_schema::Schema = serde_json::from_value(serde_json::json!({
        "uid": uid,
        "kind": "collectionType",
        "info": {
            "singularName": name,
            "pluralName": format!("{name}s"),
            "displayName": "Sales Order"
        },
        "attributes": {
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
    }))
    .expect("schema json");

    services::ctb_apply(&ctx, vec![schema.clone()], vec![])
        .await
        .expect("apply schema on postgres (STORED generated columns)");

    // Create + read database-derived values.
    let created = services::cm_create(
        &ctx,
        &uid,
        &serde_json::json!({"quantity": 10, "unit_price": 100, "discount": 10}),
    )
    .await
    .expect("create");
    let d = &created.data;
    assert_eq!(num(&d["subtotal"]), 1000.0, "pg subtotal: {d}");
    assert_eq!(num(&d["discount_amount"]), 100.0, "pg discount_amount: {d}");
    assert_eq!(num(&d["total_amount"]), 900.0, "pg total_amount: {d}");
    let doc_id = d["documentId"].as_str().unwrap().to_string();

    // Update -> recompute.
    let updated = services::cm_update(&ctx, &uid, &doc_id, &serde_json::json!({"quantity": 20}))
        .await
        .expect("update");
    assert_eq!(num(&updated.data["subtotal"]), 2000.0);
    assert_eq!(num(&updated.data["total_amount"]), 1800.0);

    // Filter + sort by computed fields.
    let filtered = services::cm_list(
        &ctx,
        &uid,
        &QueryParams::parse("filters[total_amount][$gte]=1000&pagination[pageSize]=10").unwrap(),
    )
    .await
    .expect("filter");
    assert_eq!(
        filtered.data.len(),
        1,
        "filter on computed column: {filtered:?}"
    );
    assert_eq!(num(&filtered.data[0]["total_amount"]), 1800.0);

    let sorted = services::cm_list(
        &ctx,
        &uid,
        &QueryParams::parse("sort[0]=total_amount:desc&pagination[pageSize]=10").unwrap(),
    )
    .await
    .expect("sort");
    assert_eq!(num(&sorted.data[0]["total_amount"]), 1800.0);

    // Writing a computed field is rejected on Postgres too.
    let bad = services::cm_create(
        &ctx,
        &uid,
        &serde_json::json!({"quantity": 1, "unit_price": 1, "total_amount": 999}),
    )
    .await;
    assert!(bad.is_err(), "computed write must be rejected: {bad:?}");

    // CRM: string concatenation and integer division generated columns.
    let cname = format!("cu{suffix}");
    let cuid = format!("api::{cname}.{cname}");
    let cschema: core_schema::Schema = serde_json::from_value(serde_json::json!({
        "uid": cuid,
        "kind": "collectionType",
        "info": {"singularName": cname, "pluralName": format!("{cname}s"), "displayName": "Customer"},
        "attributes": {
            "first_name": {"type": "string"},
            "last_name": {"type": "string"},
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
    }))
    .unwrap();
    services::ctb_apply(&ctx, vec![cschema], vec![])
        .await
        .expect("apply CRM schema on postgres (concat + division)");
    let customer = services::cm_create(
        &ctx,
        &cuid,
        &serde_json::json!({"first_name": "John", "last_name": "Smith", "deals_won": 8, "deals_lost": 2}),
    )
    .await
    .expect("create customer");
    assert_eq!(
        customer.data["full_name"], "John Smith",
        "pg full_name: {:?}",
        customer.data
    );
    assert_eq!(
        num(&customer.data["success_rate"]),
        80.0,
        "pg success_rate: {:?}",
        customer.data
    );

    // A VIRTUAL request is upgraded to STORED on Postgres (its only mode) and
    // must still work.
    let vname = format!("vi{suffix}");
    let vuid = format!("api::{vname}.{vname}");
    let vschema: core_schema::Schema = serde_json::from_value(serde_json::json!({
        "uid": vuid,
        "kind": "collectionType",
        "info": {"singularName": vname, "pluralName": format!("{vname}s"), "displayName": "Virtual Test"},
        "attributes": {
            "a": {"type": "integer"},
            "b": {"type": "integer"},
            "sum": {"type": "integer", "computed": true, "expression": "a + b", "stored": false, "dependencies": ["a", "b"]}
        }
    }))
    .unwrap();
    services::ctb_apply(&ctx, vec![vschema], vec![])
        .await
        .expect("apply virtual-requested schema on postgres");
    let vrow = services::cm_create(&ctx, &vuid, &serde_json::json!({"a": 2, "b": 3}))
        .await
        .expect("create on virtual-upgraded schema");
    assert_eq!(
        num(&vrow.data["sum"]),
        5.0,
        "postgres upgraded to STORED: {vrow:?}"
    );
}
