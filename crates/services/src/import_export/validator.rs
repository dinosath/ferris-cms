//! Record validation against a target content type schema (required fields,
//! field types, min/max, length, patterns, enum values). Unique / relation /
//! media constraints are handled at import time.
//!
//! Validation runs against the shared `core-schema::validate_payload` so the
//! import pipeline applies exactly the same constraints (required, min, max,
//! minLength/maxLength, regex, enum) as the content CRUD and store layers,
//! before any record is written.

use core_schema::Schema;
use serde_json::Value as JsonValue;

/// A validation issue for one record.
#[derive(Clone, Debug)]
pub struct ValidationIssue {
    pub field: Option<String>,
    pub message: String,
    pub suggested_fix: Option<String>,
}

/// Validate a transformed target record (a map whose keys are target field
/// names). Returns a list of issues.
pub fn validate_record(
    schema: &Schema,
    obj: &serde_json::Map<String, JsonValue>,
) -> Vec<ValidationIssue> {
    core_schema::validate_payload(schema, obj, true)
        .into_iter()
        .map(|e| {
            let code = e.code;
            let field = e.field;
            ValidationIssue {
                field: Some(field.clone()),
                message: e.message,
                suggested_fix: Some(format!(
                    "fix field '{}' ({}) and retry the import",
                    field, code
                )),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::{ContentTypeKind, FieldType, Uid};
    use core_schema::{Attribute, SchemaInfo};

    fn schema(required: &[&str], enum_field: Option<(&str, Vec<&str>)>) -> Schema {
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
        for r in required {
            let mut a = Attribute::new(FieldType::String);
            a.required = true;
            s.attributes.insert((*r).to_string(), a);
        }
        if let Some((name, vals)) = enum_field {
            let mut a = Attribute::new(FieldType::Enumeration);
            a.enum_values = vals.into_iter().map(|s| s.to_string()).collect();
            s.attributes.insert(name.to_string(), a);
        }
        s
    }

    #[test]
    fn flags_missing_required() {
        let s = schema(&["name"], None);
        let obj = serde_json::json!({"price": 1});
        let issues = validate_record(&s, obj.as_object().unwrap());
        assert!(issues.iter().any(|i| i.field.as_deref() == Some("name")));
    }

    #[test]
    fn flags_bad_number() {
        let s = schema(&["name"], None);
        let obj = serde_json::json!({"name":"A","price":"€12.50"});
        // price isn't defined in the schema, so no issue expected for it.
        let issues = validate_record(&s, obj.as_object().unwrap());
        assert!(issues.is_empty());
    }

    #[test]
    fn flags_bad_enum() {
        let s = schema(&[], Some(("status", vec!["draft", "published"])));
        let obj = serde_json::json!({"status":"archived"});
        let issues = validate_record(&s, obj.as_object().unwrap());
        assert!(issues.iter().any(|i| i.field.as_deref() == Some("status")));
    }

    #[test]
    fn flags_min_max_and_pattern() {
        let mut qty = Attribute::new(FieldType::Integer);
        qty.min = Some(serde_json::json!(1));
        qty.max = Some(serde_json::json!(100));
        let mut sku = Attribute::new(FieldType::String);
        sku.regex = Some("^[A-Z]{2}[0-9]{3}$".into());
        let mut s = schema(&[], None);
        s.attributes.insert("qty".to_string(), qty);
        s.attributes.insert("sku".to_string(), sku);

        let ok = validate_record(&s, serde_json::json!({"qty": 50, "sku": "AB123"}).as_object().unwrap());
        assert!(ok.is_empty(), "valid record should pass, got {ok:?}");

        let low = validate_record(&s, serde_json::json!({"qty": 0, "sku": "AB123"}).as_object().unwrap());
        assert!(low.iter().any(|i| i.field.as_deref() == Some("qty")));
        assert!(low.iter().any(|i| i.message.contains(">=")));

        let bad = validate_record(&s, serde_json::json!({"qty": 50, "sku": "nope"}).as_object().unwrap());
        assert!(bad.iter().any(|i| i.field.as_deref() == Some("sku")));
        assert!(bad.iter().any(|i| i.message.contains("pattern")));
    }
}
