//! Canonical schema model (design Part IV §3).
//!
//! The JSON shape is the wire + storage contract and is Strapi-compatible:
//! `type` discriminators, camelCase keys, `attributes` ordered by insertion.

use core_domain::{
    collection_table, component_table, single_table, snake_case, ContentTypeKind, FieldType,
    RelationKind, Uid,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_none<T>(v: &Option<T>) -> bool {
    v.is_none()
}
fn is_empty<T>(v: &[T]) -> bool {
    v.is_empty()
}

/// A content-type or component definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema {
    pub uid: Uid,
    pub kind: ContentTypeKind,
    #[serde(default, skip_serializing_if = "is_none")]
    pub collection_name: Option<String>,
    pub info: SchemaInfo,
    #[serde(default)]
    pub options: SchemaOptions,
    #[serde(default, skip_serializing_if = "is_none")]
    pub plugin_options: Option<SchemaPluginOptions>,
    #[serde(default)]
    pub attributes: IndexMap<String, Attribute>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInfo {
    #[serde(default)]
    pub singular_name: String,
    #[serde(default)]
    pub plural_name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaOptions {
    #[serde(default)]
    pub draft_and_publish: bool,
    #[serde(default, skip_serializing_if = "is_none")]
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaPluginOptions {
    #[serde(default, skip_serializing_if = "is_none")]
    pub i18n: Option<I18nOptions>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct I18nOptions {
    #[serde(default)]
    pub localized: bool,
}

/// One attribute (field). All type-specific payloads are optional members so
/// the JSON stays flat exactly like Strapi's `schema.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    #[serde(rename = "type")]
    pub attr_type: FieldType,

    // ---- shared advanced settings (Part IV §2) ----
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub private: bool,
    #[serde(default, skip_serializing_if = "is_none")]
    pub configurable: Option<bool>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub plugin_options: Option<SchemaPluginOptions>,

    // ---- constraints ----
    #[serde(default, skip_serializing_if = "is_none")]
    pub min: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub max: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub min_length: Option<i64>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub max_length: Option<i64>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub regex: Option<String>,

    // ---- enumeration ----
    #[serde(rename = "enum", default, skip_serializing_if = "is_empty")]
    pub enum_values: Vec<String>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub enum_name: Option<String>,

    // ---- uid ----
    #[serde(default, skip_serializing_if = "is_none")]
    pub target_field: Option<String>,

    // ---- relation ----
    #[serde(default, skip_serializing_if = "is_none")]
    pub relation: Option<RelationKind>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub target: Option<Uid>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub inversed_by: Option<String>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub mapped_by: Option<String>,

    // ---- component ----
    #[serde(default, skip_serializing_if = "is_none")]
    pub component: Option<Uid>,
    #[serde(default, skip_serializing_if = "is_none")]
    pub repeatable: Option<bool>,

    // ---- dynamic zone ----
    #[serde(default, skip_serializing_if = "is_empty")]
    pub components: Vec<Uid>,

    // ---- media ----
    #[serde(default, skip_serializing_if = "is_none")]
    pub multiple: Option<bool>,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub allowed_types: Vec<String>,
}

impl Attribute {
    pub fn new(attr_type: FieldType) -> Self {
        Self {
            attr_type,
            ..Default::default()
        }
    }

    pub fn is_localized(&self) -> bool {
        self.plugin_options
            .as_ref()
            .and_then(|p| p.i18n.as_ref())
            .map(|i| i.localized)
            .unwrap_or(false)
    }

    /// SQL storage family for diff compatibility (design Part IV §8):
    /// changes within one family are compatible ALTERs.
    pub fn sql_family(&self) -> SqlFamily {
        use FieldType::*;
        match self.attr_type {
            String | Email | Password | Uid | Enumeration => SqlFamily::VarChar,
            Text | Richtext => SqlFamily::Text,
            Blocks | Json => SqlFamily::Json,
            Integer => SqlFamily::Integer,
            Biginteger => SqlFamily::BigInt,
            Decimal => SqlFamily::Decimal,
            Float => SqlFamily::Double,
            Date => SqlFamily::Date,
            Datetime => SqlFamily::Timestamp,
            Time => SqlFamily::Time,
            Boolean => SqlFamily::Bool,
            Media => SqlFamily::MediaLink,
            Relation => SqlFamily::RelationLink,
            Component | Dynamiczone => SqlFamily::ComponentLink,
        }
    }
}

/// Coarse SQL storage families used for compatible/incompatible ALTER rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SqlFamily {
    VarChar,
    Text,
    Json,
    Integer,
    BigInt,
    Decimal,
    Double,
    Date,
    Timestamp,
    Time,
    Bool,
    MediaLink,
    RelationLink,
    ComponentLink,
}

impl Schema {
    /// Deterministic physical table (design Part IV §5).
    pub fn table_name(&self) -> String {
        // Explicit collectionName always wins (keeps renames stable).
        if let Some(name) = &self.collection_name {
            return name.clone();
        }
        self.default_table_name()
    }

    /// Table derived from names, ignoring `collectionName` override.
    pub fn default_table_name(&self) -> String {
        match self.kind {
            ContentTypeKind::CollectionType => collection_table(&self.info.plural_name),
            ContentTypeKind::SingleType => single_table(&self.info.singular_name),
            ContentTypeKind::Component => {
                let category = self
                    .uid
                    .component_category()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "default".into());
                component_table(&category, self.uid.last_segment())
            }
        }
    }

    pub fn is_localized(&self) -> bool {
        self.plugin_options
            .as_ref()
            .and_then(|p| p.i18n.as_ref())
            .map(|i| i.localized)
            .unwrap_or(false)
    }

    pub fn draft_and_publish(&self) -> bool {
        self.options.draft_and_publish
    }

    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.get(name)
    }

    /// The default `mainField` heuristic: first required string/uid attribute,
    /// else first string-ish attribute, else `id`.
    pub fn main_field(&self) -> String {
        let stringish = |t: FieldType| {
            matches!(
                t,
                FieldType::String | FieldType::Uid | FieldType::Email | FieldType::Text
            )
        };
        self.attributes
            .iter()
            .find(|(_, a)| a.required && stringish(a.attr_type))
            .map(|(n, _)| n.clone())
            .or_else(|| {
                self.attributes
                    .iter()
                    .find(|(_, a)| stringish(a.attr_type))
                    .map(|(n, _)| n.clone())
            })
            .unwrap_or_else(|| "id".to_string())
    }
}

/// Build the conventional uid for a collection/single type.
pub fn api_uid(singular: &str) -> Uid {
    let s = snake_case(singular).replace('_', "-");
    Uid::new(format!("api::{s}.{s}"))
}

/// Build the conventional uid for a component.
pub fn component_uid(category: &str, name: &str) -> Uid {
    Uid::new(format!(
        "{}.{}",
        snake_case(category).replace('_', "-"),
        snake_case(name).replace('_', "-")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::pluralize;

    fn article_schema() -> Schema {
        let json = serde_json::json!({
            "uid": "api::article.article",
            "kind": "collectionType",
            "collectionName": "ct_articles",
            "info": { "singularName": "article", "pluralName": "articles", "displayName": "Article" },
            "options": { "draftAndPublish": true },
            "pluginOptions": { "i18n": { "localized": true } },
            "attributes": {
                "title":  { "type": "string", "required": true, "maxLength": 255 },
                "slug":   { "type": "uid", "targetField": "title", "required": true },
                "body":   { "type": "blocks" },
                "cover":  { "type": "media", "multiple": false, "allowedTypes": ["images"] },
                "author": { "type": "relation", "relation": "manyToOne", "target": "api::author.author", "inversedBy": "articles" },
                "tags":   { "type": "relation", "relation": "manyToMany", "target": "api::tag.tag", "inversedBy": "articles" },
                "seo":    { "type": "component", "component": "shared.seo", "repeatable": false },
                "blocks": { "type": "dynamiczone", "components": ["shared.hero", "shared.cta"] }
            }
        });
        serde_json::from_value(json).expect("schema parses")
    }

    #[test]
    fn canonical_json_roundtrip() {
        let schema = article_schema();
        let value = serde_json::to_value(&schema).unwrap();
        let back: Schema = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(schema, back);

        assert_eq!(schema.table_name(), "ct_articles");
        assert!(schema.draft_and_publish());
        assert!(schema.is_localized());
        assert_eq!(schema.attributes.len(), 8);
        let author = schema.attribute("author").unwrap();
        assert_eq!(author.relation, Some(core_domain::RelationKind::ManyToOne));
        assert_eq!(
            author.target.as_ref().map(|u| u.as_str()),
            Some("api::author.author")
        );
        // key order preserved
        let keys: Vec<_> = schema.attributes.keys().collect();
        assert_eq!(
            keys,
            ["title", "slug", "body", "cover", "author", "tags", "seo", "blocks"]
        );
    }

    #[test]
    fn table_names() {
        let mut s = article_schema();
        s.collection_name = None;
        assert_eq!(s.table_name(), "ct_articles");
        s.kind = ContentTypeKind::SingleType;
        s.info.singular_name = "homepage".into();
        assert_eq!(s.table_name(), "ct_homepage");
        s.kind = ContentTypeKind::Component;
        s.uid = component_uid("shared", "seo");
        assert_eq!(s.table_name(), "cmp_shared_seo");
    }

    #[test]
    fn uid_builders() {
        assert_eq!(api_uid("BlogPost").as_str(), "api::blog-post.blog-post");
        assert_eq!(component_uid("Shared", "SeoBlock").as_str(), "shared.seo-block");
        assert_eq!(pluralize("article"), "articles");
    }

    #[test]
    fn main_field_heuristic() {
        assert_eq!(article_schema().main_field(), "title");
    }
}
