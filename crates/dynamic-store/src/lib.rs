//! dynamic-store — runtime DDL + CRUD for user-defined content-types.
//!
//! Design Part II §2 / Part III §3 / Part IV §4-§8. Tables are dynamic
//! (runtime-defined), so everything here goes through SeaQuery builders and
//! rows move as `serde_json::Value`.

pub mod ddl;
pub mod dml;
pub mod error;
pub mod value;

pub use error::StoreError;

/// Base columns present on every content table (design Part III §3).
pub mod base_columns {
    pub const ID: &str = "id";
    pub const DOCUMENT_ID: &str = "document_id";
    pub const LOCALE: &str = "locale";
    pub const PUBLICATION_STATE: &str = "publication_state";
    pub const CREATED_AT: &str = "created_at";
    pub const UPDATED_AT: &str = "updated_at";
    pub const PUBLISHED_AT: &str = "published_at";
    pub const CREATED_BY: &str = "created_by_id";
    pub const UPDATED_BY: &str = "updated_by_id";
    pub const SYNC_VERSION: &str = "sync_version";
    pub const ORIGIN_NODE: &str = "origin_node_id";
    pub const DELETED_AT: &str = "deleted_at";

    /// Well-known fields users may filter/sort on, mapped to physical columns.
    pub const FIELD_ALIASES: [(&str, &str); 9] = [
        ("id", "id"),
        ("documentId", "document_id"),
        ("document_id", "document_id"),
        ("locale", "locale"),
        ("createdAt", "created_at"),
        ("updatedAt", "updated_at"),
        ("publishedAt", "published_at"),
        ("state", "publication_state"),
        ("publicationState", "publication_state"),
    ];

    pub fn resolve_field(field: &str) -> Option<&'static str> {
        FIELD_ALIASES
            .iter()
            .find(|(alias, _)| *alias == field)
            .map(|(_, col)| *col)
    }
}
