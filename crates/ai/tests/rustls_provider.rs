//! Verifies the runtime TLS setup for rig's HTTP client: reqwest 0.13 is built
//! with `rustls-no-provider`, so `ai::ensure_crypto_provider()` must install
//! the ring provider before the first TLS connection.
//!
//! Network-dependent, so ignored by default:
//!   cargo test -p ai --test rustls_provider -- --ignored

#[tokio::test]
#[ignore = "requires network"]
async fn ring_provider_supports_https() {
    ai::ensure_crypto_provider();
    // reqwest 0.13's `get` is itself async (no separate `.send()`).
    let resp = reqwest13::get("https://example.com")
        .await
        .expect("HTTPS request with the ring provider");
    assert!(resp.status().is_success());
}
