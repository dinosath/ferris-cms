//! Connection management (design Part II §6).

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::time::Duration;

/// A shared database handle (cheap to clone).
pub type DbHandle = DatabaseConnection;

/// Connect to SQLite or Postgres based on the URL scheme.
///
/// The pool is configured to reclaim idle connections and recycle them before
/// a database server (e.g. Postgres) closes them, and to validate a connection
/// before handing it out. Without this, a connection the server closed while
/// idle (e.g. `peer closed connection without sending TLS close_notify`) would
/// be reused and surface as a spurious "Connection Error" on the next request.
pub async fn connect(url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(url.to_string());
    opt.max_connections(16)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        // Reclaim connections that have been idle for this long instead of
        // letting the server close them underneath us.
        .idle_timeout(Duration::from_secs(60))
        // Hard-recycle every connection periodically.
        .max_lifetime(Duration::from_secs(1800))
        // Ping a connection before it is handed out; a dead one is replaced
        // with a fresh connection instead of failing the caller.
        .test_before_acquire(true)
        .sqlx_logging(false);
    Database::connect(opt).await
}

/// In-memory SQLite (tests + offline dev).
pub async fn connect_sqlite_memory() -> Result<DatabaseConnection, DbErr> {
    connect("sqlite::memory:").await
}
