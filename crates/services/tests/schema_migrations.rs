//! Regression coverage for schema updates applied through the CTB/import path.

use api_types::{
    FileImportConfig, ImportMode, ImportState, MappingDto, MappingStatus, TransformKind,
};
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
    let rows = db.query_all_raw(history_query).await.unwrap();
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

#[tokio::test]
async fn removed_required_field_is_retained_but_no_longer_not_null() {
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

    let without_sku = json!({
        "uid": "api::schema-product.schema-product",
        "kind": "collectionType",
        "info": {
            "singularName": "schema-product",
            "pluralName": "schema-products",
            "displayName": "Schema Product"
        },
        "attributes": {
            "name": {"type": "string"}
        }
    });
    let without_sku = serde_json::from_value(without_sku).unwrap();
    services::ctb_apply(&ctx, vec![without_sku], vec![])
        .await
        .unwrap();

    let created = services::cm_create(
        &ctx,
        "api::schema-product.schema-product",
        &json!({"name": "No SKU after schema import"}),
    )
    .await
    .expect("a removed required field must not block new records");
    assert_eq!(created.data["name"], "No SKU after schema import");
}

#[tokio::test]
async fn importing_erp_schema_allows_name_only_product_import() {
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

    // Reproduce the existing database state: sku used to be a required
    // string, while the ERP fixture defines it as an optional uid.
    let legacy = serde_json::from_value(product_schema(true)).unwrap();
    services::ctb_apply(&ctx, vec![legacy], vec![])
        .await
        .unwrap();

    let erp: Value =
        serde_json::from_str(include_str!("../../../examples/content-types/erp.json")).unwrap();
    services::ctb_import(&ctx, &erp).await.unwrap();

    let sample = r#"[{"Name":"Μπύρα"}]"#;
    let cfg = FileImportConfig {
        filename: "DataExport-Items.json".into(),
        dataset: "data".into(),
        content: sample.into(),
        uid: "api::product.product".into(),
        mapping: vec![MappingDto {
            source_field: "Name".into(),
            target_field: Some("name".into()),
            transform: TransformKind::None,
            status: MappingStatus::AutoMapped,
            confidence: 1.0,
        }],
        mode: ImportMode::CreateOnly,
        match_field: None,
        state_field: None,
        import_state: ImportState::Draft,
        locale_field: None,
        locale: "en".into(),
        csv_delimiter: None,
        csv_has_header: None,
    };

    let response = services::run_import(&ctx, &api_types::ImportRequest { files: vec![cfg] })
        .await
        .unwrap();
    assert_eq!(
        response.created, 1,
        "ERP schema should allow a name-only product"
    );
    assert_eq!(
        response.failed, 0,
        "unexpected import errors: {:?}",
        response.errors
    );
}

#[tokio::test]
async fn erp_product_unit_has_unit_box_and_pallet_defaults() {
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

    let product = services::ctb_get(&ctx, "api::product.product")
        .await
        .unwrap();
    let unit = &product.attributes["unit"];
    assert_eq!(unit.attr_type, core_domain::FieldType::Enumeration);
    assert_eq!(unit.enum_values, ["unit", "box", "pallet"]);
    assert_eq!(unit.default, Some(json!("unit")));

    let created = services::cm_create(
        &ctx,
        "api::product.product",
        &json!({"name": "Default unit product"}),
    )
    .await
    .unwrap();
    assert_eq!(created.data["unit"], "unit");
}
