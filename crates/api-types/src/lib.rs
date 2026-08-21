//! api-types — shared DTOs (server + client) and the Strapi query parser.
//!
//! Part V of the design: one wire contract for `api-rest` and `client-core`.

pub mod admin;
pub mod envelope;
pub mod import_export;
pub mod query;

pub use envelope::*;
pub use import_export::*;
pub use query::*;
