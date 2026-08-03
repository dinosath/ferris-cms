//! db — SeaORM 2.0 system entities + connection mgmt + migrations.
//!
//! Part III of the design. System tables are fixed and migrated via
//! `sea-orm-migration`; user content-type tables are runtime DDL handled by
//! `dynamic-store`.

pub mod connection;
pub mod entities;
pub mod migration;
pub mod seed;

pub use connection::{connect, connect_sqlite_memory, DbHandle};
pub use migration::Migrator;

/// Re-export so downstream crates don't pin sea-orm separately.
pub use sea_orm;
