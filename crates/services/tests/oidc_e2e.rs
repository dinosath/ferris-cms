//! End-to-end OIDC SSO integration test.
//!
//! Runs a minimal in-process OpenID Provider (discovery + token endpoints) on a
//! blocking TCP server. It signs `id_token`s with HS256 using the configured
//! client secret (a valid OIDC "symmetric signing" configuration), then drives
//! ferriscms's own `oidc_authorize_url` → `oidc_login` flow against it to prove
//! that discovery, PKCE token exchange and ID-token claim verification all work
//! end to end.

use chrono::Utc;
use sea_orm_migration::MigratorTrait;
use services::{AppConfig, AppContext, OidcConfig};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct MockIdp {
    issuer: String,
    client_id: String,
    secret: String,
    /// nonce to embed in the issued id_token (set by the test from the
    /// authorize URL before the callback happens).
    nonce: Arc<Mutex<Option<String>>>,
}

fn http_response(stream: &mut std::net::TcpStream, status: &str, content_type: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn sign_id_token(idp: &MockIdp) -> String {
    let now = Utc::now().timestamp();
    let nonce = idp
        .nonce
        .lock()
        .map(|g| g.clone().unwrap_or_default())
        .unwrap_or_default();
    let claims = serde_json::json!({
        "iss": idp.issuer,
        "sub": "sso-user-1",
        "aud": idp.client_id,
        "email": "sso@corp.test",
        "name": "SSO User",
        "preferred_username": "sso",
        "nonce": nonce,
        "iat": now,
        "exp": now + 3600,
    });
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(idp.secret.as_bytes()),
    )
    .expect("sign id_token")
}

/// Serve the mock IdP: discovery + jwks + token endpoints. One request per
/// connection; responds and closes.
fn serve_mock_idp(listener: TcpListener, idp: MockIdp) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 16384];
        let n = match stream.read(&mut buf) {
            Ok(n) if n > 0 => n,
            _ => continue,
        };
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        let first_line = request.lines().next().unwrap_or("");
        let mut parts = first_line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");

        let (status, ctype, body): (&str, &str, String) = match (method, path) {
            ("GET", "/.well-known/openid-configuration") => (
                "200 OK",
                "application/json",
                serde_json::json!({
                    "issuer": idp.issuer,
                    "authorization_endpoint": format!("{}/authorize", idp.issuer),
                    "token_endpoint": format!("{}/token", idp.issuer),
                    "jwks_uri": format!("{}/jwks", idp.issuer),
                    "response_types_supported": ["code"],
                    "subject_types_supported": ["public"],
                    "id_token_signing_alg_values_supported": ["HS256"],
                    "claims_supported": ["sub", "iss", "aud", "email", "name", "preferred_username", "exp", "nonce"],
                })
                .to_string(),
            ),
            ("GET", "/jwks") => (
                "200 OK",
                "application/json",
                serde_json::json!({ "keys": [] }).to_string(),
            ),
            ("POST", "/token") => (
                "200 OK",
                "application/json",
                serde_json::json!({
                    "access_token": "mock-access-token",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "id_token": sign_id_token(&idp),
                })
                .to_string(),
            ),
            _ => ("404 Not Found", "text/plain", "not found".to_string()),
        };

        http_response(&mut stream, status, ctype, &body);
    }
}

fn query_value<'a>(url: &'a url::Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// Full SSO: discover the mock IdP, build an authorize URL, then complete the
/// callback with the mock's signed id_token and auto-provision an admin.
#[tokio::test(flavor = "multi_thread")]
async fn full_oidc_sso_flow_roundtrip() {
    // Bind + start the mock IdP on an ephemeral port (blocking thread).
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock idp");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let base = format!("http://{addr}");
    let secret = "test-client-secret".to_string();
    let client_id = "ferriscms".to_string();
    let nonce = Arc::new(Mutex::new(None));
    let idp = MockIdp {
        issuer: base.clone(),
        client_id: client_id.clone(),
        secret: secret.clone(),
        nonce: nonce.clone(),
    };
    let _server = thread::spawn(move || serve_mock_idp(listener, idp));
    // Let the accepting thread settle.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // App context pointing at the mock IdP, with auto-provision enabled.
    let db = db::connect_sqlite_memory().await.expect("db");
    db::migration::Migrator::up(&db, None).await.expect("migrate");
    db::seed::seed(&db).await.expect("seed");
    let oidc = OidcConfig {
        issuer: base.clone(),
        client_id: client_id.clone(),
        client_secret: secret.clone(),
        redirect_uri: format!("{base}/cb"),
        scopes: vec!["openid".into(), "profile".into(), "email".into()],
        auto_provision: true,
    };
    let ctx = AppContext::new_with_oidc(
        db,
        AppConfig {
            db_driver: "sqlite".into(),
            ..Default::default()
        },
        Some(oidc),
    );

    // 1. Authorize: build the URL we'd send the user to at the IdP.
    let auth_url = services::oidc::oidc_authorize_url(&ctx)
        .await
        .expect("authorize url");
    let parsed = url::Url::parse(&auth_url).expect("parse authorize url");
    let state = query_value(&parsed, "state").expect("state in authorize url");
    let authorize_nonce = query_value(&parsed, "nonce").expect("nonce in authorize url");

    // 2. The IdP echoes this nonce back in the id_token.
    *nonce.lock().expect("nonce lock") = Some(authorize_nonce);

    // 3. Callback: exchange the code and verify the returned id_token.
    let resp = services::oidc::oidc_login(&ctx, "the-auth-code", &state)
        .await
        .expect("oidc login succeeds");

    assert!(!resp.data.token.is_empty(), "issued a session token");
    assert_eq!(resp.data.user.email, "sso@corp.test");
    assert!(resp
        .data
        .user
        .roles
        .iter()
        .any(|r| r.code == db::seed::ROLE_SUPER_ADMIN));
    assert!(db::seed::has_admin(&ctx.db).await.expect("has admin"));
}
