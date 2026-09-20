//! Regression coverage for schema updates applied through the CTB/import path.

use db::{connect_sqlite_memory, seed, Migrator};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;
use serde_json::{json, Value};
use services::{AppConfig, AppContext, CurrentUser};

fn admin() -> CurrentUser {
    CurrentUser {
        id: 1,
        email: "schema-test@ferriscms.test".into(),
        is_active: true,
        roles: vec!["strapi-super-admin".into()],
    }
}

fn product_schema(required_sku: bool) -> Value {
    json!({
        "uid": "api::schema-product.schema-product",
        "kind": "collectionType",
        "info": {
            "singularName": "schema-product",
            "pluralName": "schema-products",
            "displayName": "Schema Product"
        },
        "attributes": {
            "name": {"type": "string"},
            "sku": {"type": "string", "required": required_sku, "unique": true}
        }
    })
}

#[tokio::test]
async fn required_to_optional_updates_sql_allows_create_and_logs_history() {
    let db = connect_sqlite_memory().await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let ctx = AppContext::new(
        db.clone(),
        AppConfig {
            db_driver: "sqlite".into(),
            ..Default::default()
        },
    )
    .with_user(Some(admin()));

    let required = serde_json::from_value(product_schema(true)).unwrap();
    services::ctb_apply(&ctx, vec![required], vec![])
        .await
        .unwrap();

    let optional = serde_json::from_value(product_schema(false)).unwrap();
    services::ctb_apply(&ctx, vec![optional], vec![])
        .await
        .unwrap();

    // This is the user-visible regression: the updated schema permits the
    // missing value and the physical SQLite column must agree with it.
    let created = services::cm_create(
        &ctx,
        "api::schema-product.schema-product",
        &json!({"name": "No SKU"}),
    )
    .await
    .expect("optional sku should be insertable after schema update");
    assert_eq!(created.data["name"], "No SKU");

    let history_query = Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT from_version, to_version, diff_json FROM schema_change_log \
         WHERE schema_uid = 'api::schema-product.schema-product' ORDER BY id",
        [],
    );
    let rows = db
        .query_all_raw(history_query)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "create and update should both be audited");
    assert_eq!(rows[1].try_get::<i64>("", "from_version").unwrap(), 1);
    assert_eq!(rows[1].try_get::<i64>("", "to_version").unwrap(), 2);
    let diff: Value = rows[1].try_get("", "diff_json").unwrap();
    assert_eq!(diff["changed_attrs"][0]["name"], "sku");
    assert_eq!(diff["changed_attrs"][0]["from"]["required"], true);
    assert!(!diff["changed_attrs"][0]["to"]["required"]
        .as_bool()
        .unwrap_or(false));
}
