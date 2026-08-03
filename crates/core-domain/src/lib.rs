//! core-domain — pure domain types, no IO.
//!
//! Part I/II of the design: `ContentTypeKind`, `FieldType`, `RelationKind`,
//! `Uid`, `ApiId` plus deterministic physical naming helpers.

pub mod naming;
mod types;

pub use naming::*;
pub use types::*;
