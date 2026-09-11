//! Computed-field performance benchmarks (requirement 13).
//!
//! Seeds 100 / 10,000 (and, with `COMPUTED_PERF_LARGE=1`, 100,000) rows into a
//! table with a computed column and measures insert / query / filtered / sorted
//! latency, comparing **stored** vs **virtual** generated columns.
//!
//! Ignored by default (it is a benchmark, not a correctness test):
//!
//! ```text
//! cargo test -p services --test computed_performance -- --ignored --nocapture
//! COMPUTED_PERF_LARGE=1 cargo test -p services --test computed_performance -- --ignored --nocapture
//! ```

use core_schema::{diff, Schema};
use db::connect_sqlite_memory;
use dynamic_store::ddl::apply_schema_diff;
use dynamic_store::dml::{insert, query_rows};
use sea_orm::{DbBackend, TransactionTrait};
use sea_query::Value;
use std::time::{Duration, Instant};

fn schema_for(table_uid: &str, uid: &str, stored: bool) -> Schema {
    serde_json::from_value(serde_json::json!({
        "uid": uid,
        "kind": "collectionType",
        "info": {
            "singularName": table_uid,
            "pluralName": format!("{table_uid}s"),
            "displayName": table_uid
        },
        "attributes": {
            "quantity": {"type": "integer"},
            "unit_price": {"type": "decimal"},
            "total": {
                "type": "decimal", "computed": true,
                "expression": "quantity * unit_price",
                "stored": stored,
                "dependencies": ["quantity", "unit_price"]
            }
        }
    }))
    .unwrap()
}

async fn seed(db: &sea_orm::DatabaseConnection, schema: &Schema, rows: i64) -> Duration {
    let table = schema.table_name();
    let now = chrono::Utc::now().to_rfc3339();
    let txn = db.begin().await.unwrap();
    let start = Instant::now();
    for i in 0..rows {
        let doc = format!("doc-{i}");
        let values = vec![
            (
                "document_id".to_string(),
                Value::String(Some(doc)),
            ),
            ("locale".to_string(), Value::String(Some("en".to_string()))),
            (
                "publication_state".to_string(),
                Value::String(Some("draft".to_string())),
            ),
            (
                "created_at".to_string(),
                Value::String(Some(now.clone())),
            ),
            (
                "updated_at".to_string(),
                Value::String(Some(now.clone())),
            ),
            ("quantity".to_string(), Value::BigInt(Some(i % 100))),
            (
                "unit_price".to_string(),
                Value::Double(Some((i % 1000) as f64)),
            ),
        ];
        insert(&txn, DbBackend::Sqlite, &table, values).await.unwrap();
    }
    txn.commit().await.unwrap();
    start.elapsed()
}

async fn time_query(
    db: &sea_orm::DatabaseConnection,
    schema: &Schema,
    query: &str,
) -> Duration {
    let params = api_types::QueryParams::parse(query).unwrap();
    let start = Instant::now();
    let (rows, _total) = query_rows(db, DbBackend::Sqlite, schema, &params)
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert!(!rows.is_empty(), "benchmark query returned no rows");
    elapsed
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance benchmark; run with --ignored --nocapture"]
async fn computed_field_performance() {
    let mut sizes = vec![100i64, 10_000];
    if std::env::var("COMPUTED_PERF_LARGE").is_ok() {
        sizes.push(100_000);
    }

    println!(
        "{:<8} {:>7} {:>12} {:>12} {:>14} {:>14}",
        "mode", "rows", "insert(ms)", "query(ms)", "filter(ms)", "sort(ms)"
    );

    for stored in [true, false] {
        for rows in &sizes {
            let mode = if stored { "stored" } else { "virtual" };
            let tag = format!("perf_{mode}_{rows}");
            let schema = schema_for(&tag, &format!("api::perf.{tag}"), stored);
            let db = connect_sqlite_memory().await.unwrap();
            apply_schema_diff(&db, DbBackend::Sqlite, &diff(None, &schema), std::slice::from_ref(&schema))
                .await
                .unwrap();

            let insert_ms = seed(&db, &schema, *rows).await.as_secs_f64() * 1000.0;
            let query_ms = time_query(&db, &schema, "pagination[pageSize]=25").await.as_secs_f64() * 1000.0;
            let filter_ms = time_query(
                &db,
                &schema,
                "filters[total][$gte]=1000&pagination[pageSize]=25",
            )
            .await
            .as_secs_f64()
                * 1000.0;
            let sort_ms = time_query(&db, &schema, "sort[0]=total:desc&pagination[pageSize]=25")
                .await
                .as_secs_f64()
                * 1000.0;

            println!(
                "{:<8} {:>7} {:>12.1} {:>12.2} {:>14.2} {:>14.2}",
                mode, rows, insert_ms, query_ms, filter_ms, sort_ms
            );
        }
    }
}
