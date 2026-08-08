//! CRUD REST tests against a self-hosted ferriscms server, using `reqwest`.
//!
//! Exercises the admin Content Manager REST endpoints over HTTP: Create, Read
//! (single + list), Update and Delete of content entries, plus the public
//! read-only API.
//!
//! Each test boots its own stack via [`e2e::harness::E2eHarness`]: a fresh Turso
//! database plus an in-process server. The database is brand new on every run,
//! so the first Super Admin is always registered with FIXED credentials.

use e2e::harness::E2eHarness;
use serde_json::{json, Value};

const EMAIL: &str = "e2e@ferriscms.test";
const PASSWORD: &str = "StrongPass1";
const CT_UID: &str = "api::article.article";

fn bearer(client: &reqwest::Client, method: reqwest::Method, url: String, token: &str) -> reqwest::RequestBuilder {
    client.request(method, url).bearer_auth(token)
}

/// Boot a fresh stack, register the super admin (fresh DB), log in and create
/// the `article` content type. Returns the `reqwest::Client` and a JWT.
async fn setup(harness: &E2eHarness) -> anyhow::Result<(reqwest::Client, String)> {
    let base = harness.server_url();
    let client = reqwest::Client::new();

    // init reports whether an admin already exists (false on a fresh Turso DB).
    let init: Value = client
        .get(format!("{base}/admin/init"))
        .send()
        .await?
        .json()
        .await?;
    if !init["hasAdmin"].as_bool().unwrap_or(false) {
        // Register the first super admin with fixed credentials.
        let register = client
            .post(format!("{base}/admin/register-admin"))
            .json(&json!({
                "email": EMAIL,
                "password": PASSWORD,
                "firstname": "E2E",
                "lastname": "Admin",
            }))
            .send()
            .await?;
        assert!(
            register.status().is_success(),
            "register failed: {}",
            register.text().await?
        );
        let register: Value = register.json().await?;
        assert!(register["data"]["token"].is_string(), "register should return a JWT");
    }

    // Login.
    let login_resp = client
        .post(format!("{base}/admin/login"))
        .json(&json!({ "email": EMAIL, "password": PASSWORD }))
        .send()
        .await?;
    assert!(
        login_resp.status().is_success(),
        "login failed: {}",
        login_resp.text().await?
    );
    let login: Value = login_resp.json().await?;
    let token = login["data"]["token"]
        .as_str()
        .expect("login should return a JWT")
        .to_string();

    // Create the `article` content type via the Content-Type Builder.
    let schema = json!([{
        "uid": CT_UID,
        "kind": "collectionType",
        "info": {
            "singularName": "article",
            "pluralName": "articles",
            "displayName": "Article",
        },
        "options": { "draftAndPublish": true },
        "attributes": {
            "title": { "type": "string", "required": true },
        }
    }]);
    let ctb = bearer(&client, reqwest::Method::POST, format!("{base}/content-type-builder/content-types"), &token)
        .json(&json!({ "schemas": schema }))
        .send()
        .await?;
    assert!(ctb.status().is_success(), "ctb apply failed: {}", ctb.text().await?);

    Ok((client, token))
}

/// Create a content entry and return the response JSON (for `documentId`).
async fn create_entry(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    title: &str,
) -> anyhow::Result<Value> {
    let resp = bearer(
        client,
        reqwest::Method::POST,
        format!("{base}/admin/content-manager/collection-types/{CT_UID}"),
        token,
    )
    .json(&json!({ "data": { "title": title } }))
    .send()
    .await?;
    assert!(resp.status().is_success(), "create entry failed: {}", resp.text().await?);
    Ok(resp.json().await?)
}

/// Read a single entry by documentId.
async fn read_entry(client: &reqwest::Client, base: &str, token: &str, id: &str) -> anyhow::Result<Value> {
    let resp = bearer(
        client,
        reqwest::Method::GET,
        format!("{base}/admin/content-manager/collection-types/{CT_UID}/{id}"),
        token,
    )
    .send()
    .await?;
    assert!(resp.status().is_success(), "read entry failed: {}", resp.text().await?);
    Ok(resp.json().await?)
}

/// List entries (admin content manager).
async fn list_entries(client: &reqwest::Client, base: &str, token: &str) -> anyhow::Result<Vec<Value>> {
    let resp = bearer(
        client,
        reqwest::Method::GET,
        format!("{base}/admin/content-manager/collection-types/{CT_UID}"),
        token,
    )
    .send()
    .await?;
    assert!(resp.status().is_success(), "list entries failed: {}", resp.text().await?);
    let body: Value = resp.json().await?;
    Ok(body["data"].as_array().cloned().unwrap_or_default())
}

#[tokio::test(flavor = "multi_thread")]
async fn crud_create_and_read() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url().to_string();
    let (client, token) = setup(&harness).await?;

    // Create.
    let created = create_entry(&client, &base, &token, "Hello, ferriscms").await?;
    let id = created["data"]["documentId"]
        .as_str()
        .expect("created entry has documentId")
        .to_string();

    // Read single.
    let single = read_entry(&client, &base, &token, &id).await?;
    assert_eq!(single["data"]["title"], json!("Hello, ferriscms"));

    // List includes it (keyed on title, which the entry JSON always carries).
    let list = list_entries(&client, &base, &token).await?;
    assert!(
        list.iter().any(|e| e["title"] == json!("Hello, ferriscms")),
        "created entry not present in list"
    );

    // Public (no-auth) read still works.
    let public_resp = client
        .get(format!("{base}/api/{CT_UID}"))
        .send()
        .await?;
    assert!(public_resp.status().is_success(), "public read failed: {}", public_resp.text().await?);
    let public: Value = public_resp.json().await?;
    assert!(public["data"].is_array(), "public API should return a data array");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn crud_update() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url().to_string();
    let (client, token) = setup(&harness).await?;

    let created = create_entry(&client, &base, &token, "Original title").await?;
    let id = created["data"]["documentId"]
        .as_str()
        .expect("created entry has documentId")
        .to_string();

    // Update the title.
    let update = bearer(
        &client,
        reqwest::Method::PUT,
        format!("{base}/admin/content-manager/collection-types/{CT_UID}/{id}"),
        &token,
    )
    .json(&json!({ "data": { "title": "Updated title" } }))
    .send()
    .await?;
    assert!(update.status().is_success(), "update entry failed: {}", update.text().await?);

    // Read it back and verify the new title persisted.
    let single = read_entry(&client, &base, &token, &id).await?;
    assert_eq!(single["data"]["title"], json!("Updated title"), "update did not persist");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn crud_delete() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url().to_string();
    let (client, token) = setup(&harness).await?;

    let created = create_entry(&client, &base, &token, "To be deleted").await?;
    let id = created["data"]["documentId"]
        .as_str()
        .expect("created entry has documentId")
        .to_string();

    // Delete.
    let delete = bearer(
        &client,
        reqwest::Method::DELETE,
        format!("{base}/admin/content-manager/collection-types/{CT_UID}/{id}"),
        &token,
    )
    .send()
    .await?;
    assert!(delete.status().is_success(), "delete entry failed: {}", delete.text().await?);

    // The list no longer contains it (keyed on title).
    let list = list_entries(&client, &base, &token).await?;
    assert!(
        !list.iter().any(|e| e["title"] == json!("To be deleted")),
        "deleted entry still present in list"
    );

    Ok(())
}
