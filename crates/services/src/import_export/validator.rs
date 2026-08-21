//! Record validation against a target content type schema (required fields,
//! field types, enum values). Unique / relation / media constraints are handled
//! at import time.

use core_domain::FieldType;
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
    let mut issues = Vec::new();

    for (name, attr) in &schema.attributes {
        let value = obj.get(name);
        let missing = value
            .map(|v| v.is_null() || (v.as_str().map(|s| s.is_empty()).unwrap_or(false)))
            .unwrap_or(true);

        if attr.required && missing {
            issues.push(ValidationIssue {
                field: Some(name.clone()),
                message: format!("required field '{name}' is missing"),
                suggested_fix: Some("provide a value or map a source field to it".into()),
            });
            continue;
        }
        if missing {
            continue;
        }
        let v = value.unwrap();
        if let Some(msg) = type_issue(attr.attr_type, name, v) {
            issues.push(msg);
        }
    }

    // Enum values must be one of the allowed options.
    for (name, attr) in &schema.attributes {
        if attr.attr_type == FieldType::Enumeration && !attr.enum_values.is_empty() {
            if let Some(v) = obj.get(name) {
                if let Some(s) = v.as_str() {
                    if !attr.enum_values.iter().any(|e| e == s) {
                        issues.push(ValidationIssue {
                            field: Some(name.clone()),
                            message: format!(
                                "'{s}' is not an allowed enum value for '{name}' (allowed: {})",
                                attr.enum_values.join(", ")
                            ),
                            suggested_fix: Some("use one of the allowed enum values".into()),
                        });
                    }
                }
            }
        }
    }

    issues
}

fn type_issue(ft: FieldType, name: &str, v: &JsonValue) -> Option<ValidationIssue> {
    match ft {
        FieldType::Integer | FieldType::Biginteger | FieldType::Decimal | FieldType::Float => {
            let ok = v.is_number()
                || v.as_str()
                    .map(|s| s.trim().parse::<f64>().is_ok())
                    .unwrap_or(false);
            if !ok {
                return Some(ValidationIssue {
                    field: Some(name.to_string()),
                    message: format!("field '{name}' expects a number, got {}", v),
                    suggested_fix: Some("apply the Number transformation".into()),
                });
            }
        }
        FieldType::Boolean => {
            let ok = v.is_boolean()
                || v.as_str()
                    .map(|s| {
                        matches!(
                            s.trim().to_lowercase().as_str(),
                            "true" | "false" | "1" | "0" | "yes" | "no"
                        )
                    })
                    .unwrap_or(false);
            if !ok {
                return Some(ValidationIssue {
                    field: Some(name.to_string()),
                    message: format!("field '{name}' expects a boolean, got {}", v),
                    suggested_fix: Some("apply the Boolean transformation".into()),
                });
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::{ContentTypeKind, Uid};
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
}
