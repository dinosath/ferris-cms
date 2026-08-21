//! Field mapping: match incoming source fields to a target content type's
//! attributes using progressively weaker strategies (exact → alias → fuzzy →
//! type compatibility).

use api_types::{InferredField, InferredKind, MappingDto, MappingStatus, TransformKind};
use core_domain::FieldType;
use core_schema::Schema;

use super::analyzer::normalize;

/// A small alias dictionary: normalized alias → canonical field name.
const ALIASES: &[(&str, &str)] = &[
    ("name", "name"),
    ("title", "name"),
    ("productname", "name"),
    ("displayname", "name"),
    ("sku", "sku"),
    ("skucode", "sku"),
    ("unitprice", "price"),
    ("price", "price"),
    ("amount", "price"),
    ("description", "description"),
    ("desc", "description"),
    ("body", "description"),
    ("summary", "description"),
    ("descriptiontext", "description"),
    ("slug", "slug"),
    ("url", "slug"),
    ("permalink", "slug"),
    ("email", "email"),
    ("image", "image"),
    ("imageurl", "image"),
    ("photo", "image"),
    ("category", "category"),
    ("categoryname", "category"),
    ("status", "status"),
    ("published", "status"),
    ("locale", "locale"),
];

/// Check whether an inferred kind is compatible with a target field type.
pub fn type_compatible(kind: &InferredKind, ft: FieldType) -> bool {
    match kind {
        InferredKind::Number => matches!(
            ft,
            FieldType::Integer | FieldType::Biginteger | FieldType::Decimal | FieldType::Float
        ),
        InferredKind::Boolean => matches!(ft, FieldType::Boolean),
        InferredKind::Date => matches!(ft, FieldType::Date | FieldType::Datetime | FieldType::Time),
        InferredKind::String | InferredKind::Unknown => matches!(
            ft,
            FieldType::String
                | FieldType::Text
                | FieldType::Richtext
                | FieldType::Email
                | FieldType::Password
                | FieldType::Uid
                | FieldType::Enumeration
        ),
        InferredKind::Array => matches!(ft, FieldType::Json | FieldType::Dynamiczone),
        InferredKind::Object => matches!(
            ft,
            FieldType::Json | FieldType::Component | FieldType::Dynamiczone
        ),
        _ => false,
    }
}

/// Score a source field against a candidate target attribute name.
fn score(source: &str, target: &str) -> f32 {
    let s = normalize(source);
    let t = normalize(target);
    if s == t {
        return 1.0;
    }
    // Alias rules.
    for (alias, canonical) in ALIASES {
        if (s == *alias && t == *canonical) || (s == *canonical && t == *alias) {
            return 0.8;
        }
    }
    // Contains / suffix.
    if s.contains(&t) || t.contains(&s) {
        return 0.6;
    }
    if let Some(canon) = alias_canonical(&s) {
        if canon == t {
            return 0.75;
        }
    }
    0.0
}

/// Resolve a normalized source field to a canonical name via aliases.
fn alias_canonical(normalized: &str) -> Option<&'static str> {
    ALIASES
        .iter()
        .find(|(a, _)| *a == normalized)
        .map(|(_, c)| *c)
}

/// Build an automatic mapping from inferred source fields to the target schema.
pub fn build_mappings(source_fields: &[InferredField], schema: &Schema) -> Vec<MappingDto> {
    let mut used: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for f in source_fields {
        // Best matching target attribute not already taken.
        let mut best: Option<(String, f32)> = None;
        for (name, attr) in &schema.attributes {
            if used.contains(name) {
                continue;
            }
            let sc = score(&f.name, name);
            if sc > 0.0 && best.as_ref().map(|(_, b)| sc > *b).unwrap_or(true) {
                best = Some((name.clone(), sc));
            }
        }
        if let Some((target, confidence)) = best {
            used.push(target.clone());
            let ft = schema.attributes.get(&target).map(|a| a.attr_type);
            let compatible = ft.map(|t| type_compatible(&f.kind, t)).unwrap_or(true);
            let status = if confidence >= 0.7 && compatible {
                MappingStatus::AutoMapped
            } else if confidence >= 0.4 {
                MappingStatus::NeedsAttention
            } else {
                MappingStatus::NeedsAttention
            };
            out.push(MappingDto {
                source_field: f.name.clone(),
                target_field: Some(target),
                transform: TransformKind::None,
                status,
                confidence,
            });
        } else {
            out.push(MappingDto {
                source_field: f.name.clone(),
                target_field: None,
                transform: TransformKind::None,
                status: MappingStatus::NeedsAttention,
                confidence: 0.0,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::{ContentTypeKind, Uid};
    use core_schema::{Attribute, SchemaInfo};

    fn schema_with(fields: &[(&str, FieldType)]) -> Schema {
        let mut s = Schema {
            uid: Uid::new("api::product.product".to_string()),
            kind: ContentTypeKind::CollectionType,
            collection_name: None,
            info: SchemaInfo {
                singular_name: "product".into(),
                plural_name: "products".into(),
                display_name: "Product".into(),
                description: None,
                icon: None,
            },
            options: Default::default(),
            plugin_options: None,
            attributes: Default::default(),
        };
        for (n, t) in fields {
            s.attributes.insert((*n).to_string(), Attribute::new(*t));
        }
        s
    }

    #[test]
    fn maps_exact_and_alias_fields() {
        let schema = schema_with(&[
            ("name", FieldType::String),
            ("sku", FieldType::String),
            ("price", FieldType::Decimal),
            ("description", FieldType::Text),
        ]);
        let inferred = vec![
            InferredField {
                name: "product_name".into(),
                kind: InferredKind::String,
                nullable: false,
                example: None,
                confidence: 1.0,
            },
            InferredField {
                name: "sku_code".into(),
                kind: InferredKind::String,
                nullable: false,
                example: None,
                confidence: 1.0,
            },
            InferredField {
                name: "price".into(),
                kind: InferredKind::Number,
                nullable: false,
                example: None,
                confidence: 1.0,
            },
            InferredField {
                name: "description_text".into(),
                kind: InferredKind::String,
                nullable: false,
                example: None,
                confidence: 1.0,
            },
        ];
        let mappings = build_mappings(&inferred, &schema);
        let name = mappings
            .iter()
            .find(|m| m.source_field == "product_name")
            .unwrap();
        assert_eq!(name.target_field.as_deref(), Some("name"));
        let sku = mappings
            .iter()
            .find(|m| m.source_field == "sku_code")
            .unwrap();
        assert_eq!(sku.target_field.as_deref(), Some("sku"));
        let price = mappings.iter().find(|m| m.source_field == "price").unwrap();
        assert_eq!(price.target_field.as_deref(), Some("price"));
    }

    #[test]
    fn unmapped_field_gets_needs_attention() {
        let schema = schema_with(&[("name", FieldType::String)]);
        let inferred = vec![InferredField {
            name: "totally_unknown".into(),
            kind: InferredKind::String,
            nullable: false,
            example: None,
            confidence: 1.0,
        }];
        let mappings = build_mappings(&inferred, &schema);
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].target_field, None);
        assert_eq!(mappings[0].status, MappingStatus::NeedsAttention);
    }
}
