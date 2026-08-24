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
        .json(&json!({"files": [{"filename": "products.json", "content": content}], "preferUid": PRODUCT_UID}))
        .send()
        .await?
        .json()
        .await?;
    let datasets = &analyze["data"]["datasets"];
    anyhow::ensure!(
        datasets.as_array().map(|a| a.len()) == Some(1),
        "expected 1 dataset"
    );
    assert_eq!(datasets[0]["recordCount"], 2, "expected 2 records");
    // The preferred content type is selected and the mapping is pre-filled.
    assert_eq!(
        datasets[0]["detectedContentType"]["uid"], PRODUCT_UID,
        "preferUid not honored"
    );
    assert!(
        datasets[0]["suggestedMapping"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "mapping not suggested"
    );

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
    anyhow::ensure!(
        datasets.as_array().map(|a| a.len()) == Some(1),
        "expected 1 dataset"
    );
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

/// Saved mapping presets are persisted to the DB and round-trip via the API.
#[tokio::test(flavor = "multi_thread")]
async fn mapping_presets_persist() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    let preset = json!({
        "name": "Shopify Products",
        "sourceUid": "shopify",
        "targetUid": PRODUCT_UID,
        "mapping": [
            {"sourceField": "product_name", "targetField": "name", "transform": "none", "status": "autoMapped"}
        ]
    });
    // Save.
    let saved: Value = client
        .post(format!("{base}/admin/import-export/mappings"))
        .bearer_auth(&token)
        .json(&preset)
        .send()
        .await?
        .json()
        .await?;
    let id = saved["data"]["id"].as_i64().expect("preset id");
    assert_eq!(saved["data"]["name"], "Shopify Products");

    // List (persisted).
    let list: Value = client
        .get(format!("{base}/admin/import-export/mappings"))
        .bearer_auth(&token)
        .send()
        .await?
        .json()
        .await?;
    let found = list["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .any(|p| p["id"] == serde_json::json!(id) && p["targetUid"] == PRODUCT_UID)
        })
        .unwrap_or(false);
    assert!(found, "saved preset not listed");

    // Delete.
    let del: Value = client
        .delete(format!("{base}/admin/import-export/mappings/{id}"))
        .bearer_auth(&token)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(del["deleted"], true, "preset not deleted");

    // Empty after delete.
    let list2: Value = client
        .get(format!("{base}/admin/import-export/mappings"))
        .bearer_auth(&token)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        list2["data"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "preset still present"
    );

    Ok(())
}

/// Export supports field projection: selecting fields excludes the others.
#[tokio::test(flavor = "multi_thread")]
async fn export_field_selection() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    // Seed two entries.
    let csv = "name,sku,price\nFerris Lager,BEER-001,3.50\n";
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
    assert_eq!(import["data"]["created"], 1);

    // Export only the `name` and `sku` fields.
    let export: Value = client
        .post(format!("{base}/admin/import-export/export"))
        .bearer_auth(&token)
        .json(&json!({"uids": [PRODUCT_UID], "format": "json", "fields": ["name", "sku"]}))
        .send()
        .await?
        .json()
        .await?;
    let content = export["data"]["content"].as_str().unwrap_or("");
    assert!(content.contains("Ferris Lager"), "name missing");
    assert!(content.contains("BEER-001"), "sku missing");
    assert!(
        !content.contains("3.5") && !content.contains("price"),
        "projection failed: price should be excluded"
    );

    Ok(())
}

/// CSV import honors a custom delimiter and a missing header row (col0..).
#[tokio::test(flavor = "multi_thread")]
async fn import_csv_custom_delimiter_no_header() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    // Semicolon-delimited CSV, no header row.
    let content = "Ferris Lager;BEER-001;3.5\nFerris IPA;BEER-002;4.0\n";
    let mapping = json!([
        {"sourceField": "col0", "targetField": "name", "transform": "none", "status": "autoMapped"},
        {"sourceField": "col1", "targetField": "sku", "transform": "none", "status": "autoMapped"},
        {"sourceField": "col2", "targetField": "price", "transform": "number", "status": "autoMapped"}
    ]);
    let import: Value = client
        .post(format!("{base}/admin/import-export/import"))
        .bearer_auth(&token)
        .json(&json!({"files": [{
            "filename": "products.csv",
            "dataset": "data",
            "content": content,
            "uid": PRODUCT_UID,
            "mapping": mapping,
            "mode": "createOnly",
            "importState": "draft",
            "locale": "en",
            "csvDelimiter": ";",
            "csvHasHeader": false
        }]}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        import["data"]["created"], 2,
        "expected 2 created with custom delimiter"
    );

    // Analyze with the same options yields the synthesized col0/col1/col2 fields.
    let analyze: Value = client
        .post(format!("{base}/admin/import-export/analyze"))
        .bearer_auth(&token)
        .json(&json!({"files": [{"filename": "products.csv", "content": content, "csvDelimiter": ";", "csvHasHeader": false}]}))
        .send()
        .await?
        .json()
        .await?;
    let fields = analyze["data"]["datasets"][0]["schema"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(fields.len(), 3, "expected 3 synthesized columns");
    Ok(())
}

/// Export respects filters (only matching entries are included).
#[tokio::test(flavor = "multi_thread")]
async fn export_respects_filters() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    // Seed three products with different prices.
    let content = "name,sku,price\nCheap,SKU-1,3.5\nMid,SKU-2,4.5\nPremium,SKU-3,5.5\n";
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
    assert_eq!(import["data"]["created"], 3);

    // Export only products with price > 4.
    let export: Value = client
        .post(format!("{base}/admin/import-export/export"))
        .bearer_auth(&token)
        .json(&json!({
            "uids": [PRODUCT_UID],
            "format": "json",
            "fields": ["name", "price"],
            "filters": {"leaf": {"field": "price", "op": "$gt", "values": [4]}}
        }))
        .send()
        .await?
        .json()
        .await?;
    let content = export["data"]["content"].as_str().unwrap_or("");
    assert!(content.contains("Mid"), "Mid missing");
    assert!(content.contains("Premium"), "Premium missing");
    assert!(
        !content.contains("Cheap"),
        "filter failed: Cheap should be excluded"
    );
    assert!(
        !content.contains("SKU-1"),
        "filter failed: SKU-1 should be excluded"
    );

    Ok(())
}

/// Invalid uids yield a clean API error (not a crash / empty success).
#[tokio::test(flavor = "multi_thread")]
async fn invalid_uid_returns_error() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;

    // Export a content type that does not exist -> 4xx.
    let export = client
        .post(format!("{base}/admin/import-export/export"))
        .bearer_auth(&token)
        .json(&json!({"uids": ["api::nonexistent.nonexistent"], "format": "json"}))
        .send()
        .await?;
    assert!(
        export.status().is_client_error(),
        "export of unknown uid should be a client error, got {}",
        export.status()
    );

    // Import into a content type that does not exist -> 4xx.
    let import = client
        .post(format!("{base}/admin/import-export/import"))
        .bearer_auth(&token)
        .json(&json!({"files": [{
            "filename": "x.csv",
            "dataset": "data",
            "content": "name,sku\nA,S1\n",
            "uid": "api::nonexistent.nonexistent",
            "mapping": [],
            "mode": "createOnly",
            "importState": "draft",
            "locale": "en"
        }]}))
        .send()
        .await?;
    assert!(
        import.status().is_client_error(),
        "import into unknown uid should be a client error, got {}",
        import.status()
    );

    Ok(())
}

/// Regression: the read path must return schema ATTRIBUTE names (preserving
/// casing) rather than the physical (lowercased) column names, so content types
/// with capitalized fields (e.g. Name/Price) display their imported values and
/// required-field validation still rejects records missing those fields.
#[tokio::test(flavor = "multi_thread")]
async fn import_preserves_attribute_case_and_validates_required() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url();
    let (client, token) = setup(&harness).await?;
    let uid = "api::stock.stock";

    // Create a content type with CAPITALIZED attributes, Name/Price required.
    let ct = json!({
        "uid": uid,
        "kind": "collectionType",
        "info": {"singularName":"stock","pluralName":"stocks","displayName":"Stock"},
        "options": {"draftAndPublish": true},
        "attributes": {
            "Name": {"type":"string", "required": true},
            "Price": {"type":"float", "required": true},
            "Quantity": {"type":"integer"}
        }
    });
    let apply = client
        .post(format!("{base}/content-type-builder/schema"))
        .bearer_auth(&token)
        .json(&json!({"schemas":[ct]}))
        .send()
        .await?;
    anyhow::ensure!(apply.status().is_success(), "create stock failed");

    // Analyze a JSON with lowercase keys; preferUid must detect the content type
    // and suggest correct mappings to the capitalized target fields.
    let content = r#"[{"name":"Widget","price":9.99,"quantity":3},{"name":"Gadget","price":4.5,"quantity":7}]"#;
    let analyze: Value = client
        .post(format!("{base}/admin/import-export/analyze"))
        .bearer_auth(&token)
        .json(&json!({"files":[{"filename":"s.json","content":content}],"preferUid":uid}))
        .send()
        .await?
        .json()
        .await?;
    let detected = analyze["data"]["datasets"][0]["detectedContentType"]["uid"].as_str().unwrap_or("");
    assert_eq!(detected, uid, "preferUid should win");
    let mapping: Vec<Value> = analyze["data"]["datasets"][0]["suggestedMapping"].as_array().cloned().unwrap_or_default();
    let target_map: Vec<String> = mapping.iter().filter_map(|m| m["targetField"].as_str().map(|s| s.to_string())).collect();
    for t in ["Name", "Price", "Quantity"] {
        assert!(target_map.contains(&t.to_string()), "mapping should target '{t}', got {target_map:?}");
    }

    // Import with the suggested mapping.
    let imp: Value = client
        .post(format!("{base}/admin/import-export/import"))
        .bearer_auth(&token)
        .json(&json!({"files":[{"filename":"s.json","dataset":"data","content":content,"uid":uid,"mapping":mapping,"mode":"createOnly","importState":"draft","locale":"en"}]}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(imp["data"]["created"], 2, "both valid records created");
    assert_eq!(imp["data"]["failed"], 0, "no failures for valid records");

    // The read must expose values under the capitalized attribute names.
    let list: Value = client
        .get(format!("{base}/admin/content-manager/collection-types/{uid}"))
        .bearer_auth(&token)
        .send()
        .await?
        .json()
        .await?;
    let row = list["data"][0].clone();
    assert!(row["Name"].as_str().is_some(), "Name present under capital key");
    assert!(row["Price"].is_number(), "Price present under capital key");
    assert!(
        row["Quantity"].is_number(),
        "integer field Quantity must decode (Postgres INT4 -> i32), got {:?}",
        row["Quantity"]
    );
    // Lowercase keys must NOT be how the API exposes the data anymore.
    assert!(row.get("name").is_none(), "no lowercase 'name' key leaked");

    // Required-field validation: a record missing Name/Price must be rejected.
    let bad_content = r#"[{"quantity":5}]"#;
    let bad_mapping: Vec<Value> = serde_json::from_str(r#"[{"sourceField":"quantity","targetField":"Quantity","transform":"none","status":"autoMapped"}]"#)?;
    let bad: Value = client
        .post(format!("{base}/admin/import-export/import"))
        .bearer_auth(&token)
        .json(&json!({"files":[{"filename":"bad.json","dataset":"data","content":bad_content,"uid":uid,"mapping":bad_mapping,"mode":"createOnly","importState":"draft","locale":"en"}]}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(bad["data"]["created"], 0, "no entry created for missing required fields");
    assert_eq!(bad["data"]["failed"], 1, "record missing required fields rejected");
    let msg = bad["data"]["errors"][0]["message"].as_str().unwrap_or("");
    assert!(msg.contains("required field 'Name'"), "reports missing required Name, got: {msg}");

    Ok(())
}
