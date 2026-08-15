//! Core value types (design Part I §2, Part XII glossary).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Stable identifier for a content-type or component.
/// `api::<singular>.<singular>` for content-types, `<category>.<name>` for components.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Uid(pub String);

impl Uid {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `api::article.article` -> scope `api`, rest `article.article`
    pub fn scope(&self) -> Option<&str> {
        self.0.split_once("::").map(|(s, _)| s)
    }

    /// True for component uids (`<category>.<name>` without `::` scope).
    pub fn is_component(&self) -> bool {
        !self.0.contains("::") && self.0.contains('.')
    }

    /// Category of a component uid (`shared.seo` -> `shared`).
    pub fn component_category(&self) -> Option<&str> {
        if self.is_component() {
            self.0.split('.').next()
        } else {
            None
        }
    }

    /// Name part after the last `.`.
    pub fn last_segment(&self) -> &str {
        self.0.rsplit('.').next().unwrap_or(&self.0)
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Uid {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

/// Public API id (`articles`, `homepage`). Lowercase kebab/identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiId(pub String);

impl ApiId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApiId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Kind of a schema (design Part IV §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContentTypeKind {
    /// Many entries, table `ct_<plural>`.
    CollectionType,
    /// At most one entry per locale/state, table `ct_<singular>`.
    SingleType,
    /// Reusable field group, table `cmp_<category>_<name>`.
    Component,
}

impl ContentTypeKind {
    /// Storage string used in the `content_type_schemas.kind` column.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::CollectionType => "collectionType",
            Self::SingleType => "singleType",
            Self::Component => "component",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "collectionType" => Some(Self::CollectionType),
            "singleType" => Some(Self::SingleType),
            "component" => Some(Self::Component),
            _ => None,
        }
    }
}

/// The six Strapi relation kinds (design Part IV §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationKind {
    /// FK on A, B unaware.
    #[serde(rename = "oneWay")]
    OneWay,
    /// FK on A unique, inversedBy on B.
    #[serde(rename = "oneToOne")]
    OneToOne,
    /// FK on B (mappedBy).
    #[serde(rename = "oneToMany")]
    OneToMany,
    /// FK on A.
    #[serde(rename = "manyToOne")]
    ManyToOne,
    /// Join table + order columns.
    #[serde(rename = "manyToMany")]
    ManyToMany,
    /// Join table, one-directional.
    #[serde(rename = "manyWay")]
    ManyWay,
}

impl RelationKind {
    /// True when the owning side carries a `<field>_id` FK column.
    pub fn owner_has_fk(&self) -> bool {
        matches!(
            self,
            Self::OneWay | Self::OneToOne | Self::ManyToOne
        )
    }

    /// True when a join table is required.
    pub fn uses_join_table(&self) -> bool {
        matches!(self, Self::ManyToMany | Self::ManyWay)
    }

    /// Parse a relation kind from its camelCase name (e.g. "oneToMany").
    pub fn parse(s: &str) -> Self {
        use RelationKind::*;
        match s {
            "oneWay" => OneWay,
            "oneToOne" => OneToOne,
            "oneToMany" => OneToMany,
            "manyToOne" => ManyToOne,
            "manyToMany" => ManyToMany,
            "manyWay" => ManyWay,
            _ => OneToOne,
        }
    }
}

/// Draft & Publish variant discriminator (design Part III §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PublicationState {
    Draft,
    Published,
}

impl PublicationState {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            _ => None,
        }
    }
}

/// Attribute type discriminators, 1:1 with Strapi (design Part IV §2/§3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Text,
    /// Rich text (Markdown source).
    Richtext,
    /// Rich text (Blocks JSON).
    Blocks,
    Integer,
    Biginteger,
    Decimal,
    Float,
    Date,
    Datetime,
    Time,
    Boolean,
    Email,
    Password,
    Enumeration,
    Json,
    Uid,
    Media,
    Relation,
    Component,
    Dynamiczone,
}

impl Default for FieldType {
    fn default() -> Self {
        Self::String
    }
}

impl FieldType {
    pub const ALL: [FieldType; 21] = [
        Self::String,
        Self::Text,
        Self::Richtext,
        Self::Blocks,
        Self::Integer,
        Self::Biginteger,
        Self::Decimal,
        Self::Float,
        Self::Date,
        Self::Datetime,
        Self::Time,
        Self::Boolean,
        Self::Email,
        Self::Password,
        Self::Enumeration,
        Self::Json,
        Self::Uid,
        Self::Media,
        Self::Relation,
        Self::Component,
        Self::Dynamiczone,
    ];

    /// Wire name used in schema JSON (`type` discriminator).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Text => "text",
            Self::Richtext => "richtext",
            Self::Blocks => "blocks",
            Self::Integer => "integer",
            Self::Biginteger => "biginteger",
            Self::Decimal => "decimal",
            Self::Float => "float",
            Self::Date => "date",
            Self::Datetime => "datetime",
            Self::Time => "time",
            Self::Boolean => "boolean",
            Self::Email => "email",
            Self::Password => "password",
            Self::Enumeration => "enumeration",
            Self::Json => "json",
            Self::Uid => "uid",
            Self::Media => "media",
            Self::Relation => "relation",
            Self::Component => "component",
            Self::Dynamiczone => "dynamiczone",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }

    /// True when the attribute maps to a scalar SQL column on the host table.
    pub fn is_scalar_column(&self) -> bool {
        !matches!(
            self,
            Self::Media | Self::Relation | Self::Component | Self::Dynamiczone
        )
    }
}

/// API token type (design Part III §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiTokenType {
    ReadOnly,
    FullAccess,
    Custom,
}

impl ApiTokenType {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::FullAccess => "full-access",
            Self::Custom => "custom",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "read-only" => Some(Self::ReadOnly),
            "full-access" => Some(Self::FullAccess),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Well-known system uids and reserved names (design Part IV §9).
pub mod reserved {
    /// Attribute names that collide with base table columns or engine keys.
    pub const RESERVED_ATTRIBUTES: [&str; 18] = [
        "id",
        "document_id",
        "documentId",
        "locale",
        "publication_state",
        "created_at",
        "updated_at",
        "published_at",
        "created_by_id",
        "updated_by_id",
        "createdAt",
        "updatedAt",
        "publishedAt",
        "createdBy",
        "updatedBy",
        "sync_version",
        "origin_node_id",
        "deleted_at",
    ];

    /// API ids that collide with built-in route groups.
    pub const RESERVED_API_IDS: [&str; 12] = [
        "admin",
        "api",
        "content-type-builder",
        "content-manager",
        "upload",
        "uploads",
        "users",
        "user",
        "auth",
        "i18n",
        "sync",
        "webhooks",
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_helpers() {
        let uid = Uid::new("api::article.article");
        assert_eq!(uid.scope(), Some("api"));
        assert!(!uid.is_component());
        assert_eq!(uid.last_segment(), "article");

        let cmp = Uid::new("shared.seo");
        assert!(cmp.is_component());
        assert_eq!(cmp.component_category(), Some("shared"));
        assert_eq!(cmp.last_segment(), "seo");
    }

    #[test]
    fn field_type_roundtrip() {
        for t in FieldType::ALL {
            assert_eq!(FieldType::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn relation_storage_rules() {
        assert!(RelationKind::ManyToOne.owner_has_fk());
        assert!(RelationKind::OneWay.owner_has_fk());
        assert!(!RelationKind::OneToMany.owner_has_fk());
        assert!(RelationKind::ManyToMany.uses_join_table());
        assert!(RelationKind::ManyWay.uses_join_table());
        assert!(!RelationKind::OneToOne.uses_join_table());
    }

    #[test]
    fn relation_kind_parse() {
        use RelationKind::*;
        assert_eq!(RelationKind::parse("oneWay"), OneWay);
        assert_eq!(RelationKind::parse("oneToOne"), OneToOne);
        assert_eq!(RelationKind::parse("oneToMany"), OneToMany);
        assert_eq!(RelationKind::parse("manyToOne"), ManyToOne);
        assert_eq!(RelationKind::parse("manyToMany"), ManyToMany);
        assert_eq!(RelationKind::parse("manyWay"), ManyWay);
        // Unknown falls back to oneToOne (safe default).
        assert_eq!(RelationKind::parse("nonsense"), OneToOne);
    }

    #[test]
    fn content_type_kind_roundtrip() {
        for (k, s) in [
            (ContentTypeKind::CollectionType, "collectionType"),
            (ContentTypeKind::SingleType, "singleType"),
            (ContentTypeKind::Component, "component"),
        ] {
            assert_eq!(k.as_db_str(), s);
            assert_eq!(ContentTypeKind::from_db_str(s), Some(k));
        }
        assert_eq!(ContentTypeKind::from_db_str("nope"), None);
    }

    #[test]
    fn api_token_type_roundtrip() {
        for (t, s) in [
            (ApiTokenType::ReadOnly, "read-only"),
            (ApiTokenType::FullAccess, "full-access"),
            (ApiTokenType::Custom, "custom"),
        ] {
            assert_eq!(t.as_db_str(), s);
            assert_eq!(ApiTokenType::from_db_str(s), Some(t));
        }
        assert_eq!(ApiTokenType::from_db_str("nope"), None);
    }

    #[test]
    fn field_type_scalar_rules() {
        for t in [
            FieldType::String,
            FieldType::Integer,
            FieldType::Boolean,
            FieldType::Uid,
            FieldType::Json,
        ] {
            assert!(t.is_scalar_column(), "{t:?} should be scalar");
        }
        for t in [
            FieldType::Media,
            FieldType::Relation,
            FieldType::Component,
            FieldType::Dynamiczone,
        ] {
            assert!(!t.is_scalar_column(), "{t:?} should not be scalar");
        }
        assert_eq!(FieldType::default(), FieldType::String);
    }
}
