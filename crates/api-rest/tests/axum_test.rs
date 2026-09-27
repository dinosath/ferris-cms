//! Smoke coverage for the axum-test integration used by API-level tests.

use axum::{routing::get, Router};
use axum_test::TestServer;

#[tokio::test]
async fn axum_test_can_drive_a_router() {
    let app = Router::new().route("/health", get(|| async { "ok" }));
    let server = TestServer::new(app);

    server
        .get("/health")
        .await
        .assert_status_ok()
        .assert_text("ok");
}
