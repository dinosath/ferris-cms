//! End-to-end Import & Export API tests against a self-hosted server.

use e2e::harness::E2eHarness;
use serde_json::{json, Value};

const EMAIL: &str = "ie@ferriscms.test";
const PASSWORD: &str = "StrongPass1";
const PRODUCT_UID: &str = "api::product.product";

/// Boot a stack, register the super admin, and create a `product` content type.
async fn setup(harness: &E2eHarness) -> anyhow::Result<(reqwest::Client, String)> {
    let base = harness.server_url();
    let client = reqwest::Client::new();
    let register = client
        .post(format!("{base}/admin/register-admin"))
        .json(&json!({"email": EMAIL, "password": PASSWORD}))
        .send()
        .await?;
    anyhow::ensure!(register.status().is_success(), "register failed");
    let reg: Value = register.json().await?;
    let token = reg["data"]["token"].as_str().unwrap_or("").to_string();

    let ct = json!({
        "uid": PRODUCT_UID,
        "kind": "collectionType",
        "info": {"singularName":"product","pluralName":"products","displayName":"Product"},
        "options": {"draftAndPublish": true},
        "attributes": {"name": {"type":"string"}, "sku": {"type":"string", "unique": true}, "price": {"type":"decimal"}}
    });
    let apply = client
        .post(format!("{base}/content-type-builder/schema"))
        .bearer_auth(&token)
        .json(&json!({"schemas":[ct]}))
        .send()
        .await?;
    anyhow::ensure!(apply.status().is_success(), "create content type failed");
    Ok((client, token))
}

#[tokio::test(flavor = "multi_thread")]
async fn analyze_import_export_roundtrip() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    // 1) Analyze a CSV.
    let csv = "name,sku,price\nFerris Lager,BEER-001,3.50\nFerris IPA,BEER-002,4.00\n";
    let analyze: Value = client
        .post(format!("{base}/admin/import-export/analyze"))
        .bearer_auth(&token)
        .json(&json!({"files": [{"filename": "products.csv", "content": csv}]}))
        .send()
        .await?
        .json()
        .await?;
    let datasets = &analyze["data"]["datasets"];
    anyhow::ensure!(
        datasets.is_array() && datasets.as_array().unwrap().len() == 1,
        "expected 1 dataset"
    );
    let ds = &datasets[0];
    assert_eq!(ds["recordCount"], 2, "expected 2 records");
    assert_eq!(ds["format"], "csv");

    // 2) Import with an explicit field mapping.
    let mapping = json!([
        {"sourceField": "name", "targetField": "name", "transform": "none", "status": "autoMapped"},
        {"sourceField": "sku", "targetField": "sku", "transform": "none", "status": "autoMapped"},
        {"sourceField": "price", "targetField": "price", "transform": "number", "status": "autoMapped"}
    ]);
    let import: Value = client
        .post(format!("{base}/admin/import-export/import"))
        .bearer_auth(&token)
        .json(&json!({"files": [{
            "filename": "products.csv",
            "dataset": "data",
            "content": csv,
            "uid": PRODUCT_UID,
            "mapping": mapping,
            "mode": "createOnly",
            "importState": "draft",
            "locale": "en"
        }]}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(import["data"]["created"], 2, "expected 2 created");
    assert_eq!(import["data"]["failed"], 0, "expected 0 failed");

    // Verify the entries exist in the Content Manager.
    let list: Value = client
        .get(format!(
            "{base}/admin/content-manager/collection-types/{PRODUCT_UID}"
        ))
        .bearer_auth(&token)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(list["meta"]["pagination"]["total"], 2, "expected 2 entries");

    // 3) Export as JSON and confirm round-trip content.
    let export: Value = client
        .post(format!("{base}/admin/import-export/export"))
        .bearer_auth(&token)
        .json(&json!({"uids": [PRODUCT_UID], "format": "json"}))
        .send()
        .await?
        .json()
        .await?;
    let content = export["data"]["content"].as_str().unwrap_or("");
    assert!(content.contains("Ferris Lager"), "export missing entry");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn import_upsert_updates_existing() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    let csv = "name,sku,price\nFerris Lager,BEER-001,3.50\n";
    let mapping = json!([
        {"sourceField": "name", "targetField": "name", "transform": "none", "status": "autoMapped"},
        {"sourceField": "sku", "targetField": "sku", "transform": "none", "status": "autoMapped"},
        {"sourceField": "price", "targetField": "price", "transform": "number", "status": "autoMapped"}
    ]);
    let run = |price: &str| {
        client
            .post(format!("{base}/admin/import-export/import"))
            .bearer_auth(&token)
            .json(&json!({"files": [{
                "filename": "products.csv",
                "dataset": "data",
                "content": &format!("name,sku,price\nFerris Lager,BEER-001,{price}\n"),
                "uid": PRODUCT_UID,
                "mapping": mapping.clone(),
                "mode": "upsert",
                "matchField": "sku",
                "importState": "published",
                "locale": "en"
            }]}))
    };
    // First import creates.
    let first: Value = run("3.50").send().await?.json().await?;
    assert_eq!(first["data"]["created"], 1, "first import should create");
    // Second import with same sku should update, not duplicate.
    let second: Value = run("9.99").send().await?.json().await?;
    assert_eq!(second["data"]["updated"], 1, "second import should update");

    let list: Value = client
        .get(format!(
            "{base}/admin/content-manager/collection-types/{PRODUCT_UID}"
        ))
        .bearer_auth(&token)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        list["meta"]["pagination"]["total"], 1,
        "upsert must not duplicate"
    );
    // The price should have been updated to 9.99.
    let first_row = &list["data"][0];
    assert_eq!(
        first_row["price"].as_f64().unwrap_or(0.0),
        9.99,
        "price not updated"
    );

    Ok(())
}

/// Analyze + import a JSON array of objects (the format commonly pasted/uploaded).
#[tokio::test(flavor = "multi_thread")]
async fn analyze_import_json() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    let content = r#"[{"name":"Ferris Lager","sku":"BEER-001","price":3.5},{"name":"Ferris IPA","sku":"BEER-002","price":4.0}]"#;
    let analyze: Value = client
        .post(format!("{base}/admin/import-export/analyze"))
        .bearer_auth(&token)
        .json(&json!({"files": [{"filename": "products.json", "content": content}]}))
        .send()
        .await?
        .json()
        .await?;
    let datasets = &analyze["data"]["datasets"];
    anyhow::ensure!(datasets.as_array().map(|a| a.len()) == Some(1), "expected 1 dataset");
    assert_eq!(datasets[0]["recordCount"], 2, "expected 2 records");

    let mapping = json!([
        {"sourceField": "name", "targetField": "name", "transform": "none", "status": "autoMapped"},
        {"sourceField": "sku", "targetField": "sku", "transform": "none", "status": "autoMapped"},
        {"sourceField": "price", "targetField": "price", "transform": "number", "status": "autoMapped"}
    ]);
    let import: Value = client
        .post(format!("{base}/admin/import-export/import"))
        .bearer_auth(&token)
        .json(&json!({"files": [{
            "filename": "products.json",
            "dataset": "products",
            "content": content,
            "uid": PRODUCT_UID,
            "mapping": mapping,
            "mode": "createOnly",
            "importState": "draft",
            "locale": "en"
        }]}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(import["data"]["created"], 2, "expected 2 created");
    assert_eq!(import["data"]["failed"], 0, "expected 0 failed");
    Ok(())
}

/// Analyze + import a YAML document (one dataset from an array).
#[tokio::test(flavor = "multi_thread")]
async fn analyze_import_yaml() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    let content = "- name: Ferris Lager\n  sku: BEER-001\n  price: 3.5\n- name: Ferris IPA\n  sku: BEER-002\n  price: 4.0\n";
    let analyze: Value = client
        .post(format!("{base}/admin/import-export/analyze"))
        .bearer_auth(&token)
        .json(&json!({"files": [{"filename": "products.yaml", "content": content}]}))
        .send()
        .await?
        .json()
        .await?;
    let datasets = &analyze["data"]["datasets"];
    anyhow::ensure!(datasets.as_array().map(|a| a.len()) == Some(1), "expected 1 dataset");
    assert_eq!(datasets[0]["format"], "yaml");

    let mapping = json!([
        {"sourceField": "name", "targetField": "name", "transform": "none", "status": "autoMapped"},
        {"sourceField": "sku", "targetField": "sku", "transform": "none", "status": "autoMapped"},
        {"sourceField": "price", "targetField": "price", "transform": "number", "status": "autoMapped"}
    ]);
    let import: Value = client
        .post(format!("{base}/admin/import-export/import"))
        .bearer_auth(&token)
        .json(&json!({"files": [{
            "filename": "products.yaml",
            "dataset": "products",
            "content": content,
            "uid": PRODUCT_UID,
            "mapping": mapping,
            "mode": "createOnly",
            "importState": "draft",
            "locale": "en"
        }]}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(import["data"]["created"], 2, "expected 2 created");
    assert_eq!(import["data"]["failed"], 0, "expected 0 failed");
    Ok(())
}
