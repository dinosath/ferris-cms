//! Backend REST API end-to-end tests against a self-hosted ferriscms server.
//!
//! Exercises the admin workflow over HTTP: init → (register super admin on a
//! fresh Turso database) → login (JWT) → create a content type → list it →
//! create an entry → read it back via the public API.
//!
//! Each test boots its own stack via [`e2e::harness::E2eHarness`]: a fresh Turso
//! database plus an in-process server. The database is brand new on every run,
//! so the first Super Admin is always registered with FIXED credentials.

use e2e::harness::E2eHarness;
use serde_json::{json, Value};

const EMAIL: &str = "e2e@ferriscms.test";
const PASSWORD: &str = "StrongPass1";

fn bearer(client: &reqwest::Client, method: reqwest::Method, url: String, token: &str) -> reqwest::RequestBuilder {
    client.request(method, url).bearer_auth(token)
}

#[tokio::test(flavor = "multi_thread")]
async fn init_register_login_and_crud_work_end_to_end() -> anyhow::Result<()> {
    let harness = E2eHarness::start().await?;
    let base = harness.server_url().to_string();
    let client = reqwest::Client::new();

    // init reports whether an admin already exists (false on a fresh Turso DB).
    let init: Value = client
        .get(format!("{base}/admin/init"))
        .send()
        .await?
        .json()
        .await?;
    let has_admin = init["hasAdmin"].as_bool().unwrap_or(false);

    if !has_admin {
        // First run on the fresh Turso database: register the first super admin.
        let register_resp = client
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
            register_resp.status().is_success(),
            "register failed: {}",
            register_resp.text().await?
        );
        let register: Value = register_resp.json().await?;
        assert!(
            register["data"]["token"].is_string(),
            "register response should include a JWT"
        );

        // init now reports an admin exists.
        let init2: Value = client
            .get(format!("{base}/admin/init"))
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(init2["hasAdmin"], json!(true));
    }

    // Login with the fixed credentials.
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
        .expect("login response should include a JWT")
        .to_string();

    // Create a content type via the Content-Type Builder.
    let schema = json!([{
        "uid": "api::article.article",
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

    // List content types (requires auth).
    let list = bearer(&client, reqwest::Method::GET, format!("{base}/content-type-builder/content-types"), &token)
        .send()
        .await?;
    assert!(list.status().is_success(), "ctb list failed");
    let list: Value = list.json().await?;
    assert!(
        list["data"].as_array().is_some_and(|a| a.iter().any(|s| s["uid"] == json!("api::article.article"))),
        "article content type not present after create"
    );

    // Create an entry in the content manager.
    let create = bearer(&client, reqwest::Method::POST, format!("{base}/admin/content-manager/collection-types/api::article.article"), &token)
        .json(&json!({ "data": { "title": "Hello, ferriscms" } }))
        .send()
        .await?;
    assert!(create.status().is_success(), "create entry failed: {}", create.text().await?);

    // Read it back via the public API (no auth).
    let public = client
        .get(format!("{base}/api/api::article.article"))
        .send()
        .await?;
    assert!(public.status().is_success(), "public read failed");
    let public: Value = public.json().await?;
    let title = public["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|e| e["title"].as_str())
        .unwrap_or_default();
    assert_eq!(title, "Hello, ferriscms");

    Ok(())
}
