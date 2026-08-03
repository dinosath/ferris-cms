//! Connection management (design Part II §6).

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::time::Duration;

/// A shared database handle (cheap to clone).
pub type DbHandle = DatabaseConnection;

/// Connect to SQLite or Postgres based on the URL scheme.
pub async fn connect(url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(url.to_string());
    opt.max_connections(16)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        .sqlx_logging(false);
    Database::connect(opt).await
}

/// In-memory SQLite (tests + offline dev).
pub async fn connect_sqlite_memory() -> Result<DatabaseConnection, DbErr> {
    connect("sqlite::memory:").await
}
