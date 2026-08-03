//! api-types — shared DTOs (server + client) and the Strapi query parser.
//!
//! Part V of the design: one wire contract for `api-rest` and `client-core`.

pub mod admin;
pub mod envelope;
pub mod query;

pub use envelope::*;
pub use query::*;
