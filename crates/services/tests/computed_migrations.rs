//! Computed-field migration lifecycle tests.
//!
//! Exercises `core-schema::diff` + `dynamic-store::apply_schema_diff` on a real
//! (in-memory SQLite) database: create a table with a generated column, add a
//! computed column, alter a computed expression (incompatible → detach + add),
//! roll the expression back, and remove a computed field (unmap, retained).

use core_schema::{diff, Attribute, Schema};
use db::connect_sqlite_memory;
use dynamic_store::ddl::apply_schema_diff;
use dynamic_store::dml::insert_one;
use sea_orm::DbBackend;

fn schema_from(json: serde_json::Value) -> Schema {
    serde_json::from_value(json).unwrap()
}

fn computed(json: serde_json::Value) -> Attribute {
    serde_json::from_value(json).unwrap()
}

fn num(v: &serde_json::Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(f64::NAN)
}

fn base(attrs: serde_json::Value) -> Schema {
    schema_from(serde_json::json!({
        "uid": "api::sales-order.sales-order",
        "kind": "collectionType",
        "info": {"singularName":"sales-order","pluralName":"sales-orders","displayName":"Sales Order"},
        "attributes": attrs
    }))
}

#[tokio::test]
async fn computed_migration_lifecycle() {
    let db = connect_sqlite_memory().await.unwrap();
    let backend = DbBackend::Sqlite;

    // 1. CREATE TABLE with a STORED generated column.
    let s1 = base(serde_json::json!({
        "quantity": {"type": "integer"},
        "unit_price": {"type": "decimal"},
        "total": {
            "type": "decimal", "computed": true,
            "expression": "quantity * unit_price", "stored": true,
            "dependencies": ["quantity", "unit_price"]
        }
    }));
    let d1 = diff(None, &s1);
    apply_schema_diff(&db, backend, &d1, std::slice::from_ref(&s1))
        .await
        .unwrap();
    let row = insert_one(
        &db,
        &s1,
        &serde_json::json!({"quantity":2,"unit_price":5}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(num(&row["total"]), 10.0, "computed on insert: {row}");

    // 2. ADD a computed column (SQLite falls back to VIRTUAL on ALTER).
    let mut s2 = s1.clone();
    s2.attributes.insert(
        "tax".to_string(),
        computed(serde_json::json!({
            "type": "integer", "computed": true,
            "expression": "quantity * 1", "stored": true, "dependencies": ["quantity"]
        })),
    );
    let d2 = diff(Some(&s1), &s2);
    assert_eq!(d2.added_attrs.len(), 1);
    apply_schema_diff(&db, backend, &d2, &[s2.clone()])
        .await
        .unwrap();
    let row2 = insert_one(
        &db,
        &s2,
        &serde_json::json!({"quantity":3,"unit_price":5}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(num(&row2["total"]), 15.0);
    assert_eq!(num(&row2["tax"]), 3.0, "added computed column: {row2}");

    // 3. ALTER a computed expression -> incompatible (detach + add).
    let mut s3 = s2.clone();
    s3.attributes.get_mut("total").unwrap().expression =
        Some("quantity * unit_price * 2".to_string());
    let d3 = diff(Some(&s2), &s3);
    assert_eq!(d3.changed_attrs.len(), 1);
    assert!(
        !d3.changed_attrs[0].compatible,
        "expression change must be incompatible"
    );
    apply_schema_diff(&db, backend, &d3, &[s3.clone()])
        .await
        .unwrap();
    let row3 = insert_one(
        &db,
        &s3,
        &serde_json::json!({"quantity":2,"unit_price":5}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(num(&row3["total"]), 20.0, "altered expression: {row3}");

    // 4. ROLLBACK the expression (re-apply previous schema) — must not collide
    //    with the earlier detached column.
    let d4 = diff(Some(&s3), &s2);
    apply_schema_diff(&db, backend, &d4, &[s2.clone()])
        .await
        .unwrap();
    let row4 = insert_one(
        &db,
        &s2,
        &serde_json::json!({"quantity":2,"unit_price":5}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(num(&row4["total"]), 10.0, "rolled back expression: {row4}");

    // 5. REMOVE a computed field -> unmapped, not hard-dropped; writes still work.
    let mut s5 = s2.clone();
    s5.attributes.shift_remove("tax");
    let d5 = diff(Some(&s2), &s5);
    assert_eq!(d5.removed_attrs, vec!["tax".to_string()]);
    apply_schema_diff(&db, backend, &d5, &[s5.clone()])
        .await
        .unwrap();
    let row5 = insert_one(
        &db,
        &s5,
        &serde_json::json!({"quantity":4,"unit_price":2}),
        None,
    )
    .await
    .unwrap();
    assert_eq!(num(&row5["total"]), 8.0, "after removal: {row5}");
}

/// Writes to computed fields are rejected at the store layer too.
#[tokio::test]
async fn computed_field_write_rejected() {
    let db = connect_sqlite_memory().await.unwrap();
    let backend = DbBackend::Sqlite;
    let s = base(serde_json::json!({
        "quantity": {"type": "integer"},
        "unit_price": {"type": "decimal"},
        "total": {
            "type": "decimal", "computed": true,
            "expression": "quantity * unit_price", "stored": true,
            "dependencies": ["quantity", "unit_price"]
        }
    }));
    apply_schema_diff(&db, backend, &diff(None, &s), std::slice::from_ref(&s))
        .await
        .unwrap();

    let err = insert_one(
        &db,
        &s,
        &serde_json::json!({"quantity":1,"unit_price":2,"total":999}),
        None,
    )
    .await
    .expect_err("computed write must fail");
    assert!(
        format!("{err:?}").contains("computed"),
        "expected computed validation error, got {err:?}"
    );
}
