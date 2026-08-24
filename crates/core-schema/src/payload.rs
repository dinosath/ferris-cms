//! Payload validation against schema attributes (design Part IV §9).
//!
//! Validates user-supplied field values against the constraints declared on a
//! schema's attributes: `required`, value type, `min`/`max`, `minLength` /
//! `maxLength`, `regex` (patterns) and `enum`. It is the single source of
//! truth shared by three layers so a payload is rejected *before* it is
//! handled:
//!
//! 1. the content CRUD service (`cm_create` / `cm_update`),
//! 2. the import pipeline (before a record is written),
//! 3. the dynamic store (`insert_one` / `update_one`) for defense in depth.

use crate::model::Attribute;
use crate::Schema;
use core_domain::FieldType;
use regex::Regex;
use serde_json::{Map as JsonMap, Value as JsonValue};

/// One payload validation failure, Strapi error-details compatible.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PayloadError {
    /// Attribute (field) name the error concerns.
    pub field: String,
    /// Stable machine code: `required`, `type`, `min`, `max`, `minLength`,
    /// `maxLength`, `regex`, `enum`.
    pub code: String,
    pub message: String,
}

impl PayloadError {
    pub fn new(
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Validate a payload (a map of field name → value) against `schema`.
///
/// `enforce_required` controls whether missing required fields are reported.
/// Create paths pass `true`; partial-update paths pass `false` so that only
/// fields actually present in the payload are validated for value constraints.
pub fn validate_payload(
    schema: &Schema,
    obj: &JsonMap<String, JsonValue>,
    enforce_required: bool,
) -> Vec<PayloadError> {
    let mut errors = Vec::new();

    for (name, attr) in &schema.attributes {
        let value = obj.get(name);
        let present = value.map(|v| !v.is_null()).unwrap_or(false);

        if attr.required && enforce_required && !present {
            errors.push(PayloadError::new(
                name.clone(),
                "required",
                format!("Field '{name}' is required"),
            ));
            continue;
        }
        // A required field whose value is an empty/whitespace string is treated
        // as missing (Strapi's default string-required behavior).
        if attr.required && enforce_required {
            if let Some(JsonValue::String(s)) = value {
                if s.trim().is_empty() {
                    errors.push(PayloadError::new(
                        name.clone(),
                        "required",
                        format!("Field '{name}' is required"),
                    ));
                    continue;
                }
            }
        }

        let Some(v) = value else { continue };
        if v.is_null() {
            continue;
        }

        if !type_ok(attr.attr_type, v) {
            errors.push(PayloadError::new(
                name.clone(),
                "type",
                format!(
                    "Field '{name}' expects {}, got {}",
                    expected_type(attr.attr_type),
                    describe(v)
                ),
            ));
            continue;
        }

        validate_constraints(attr, name, v, &mut errors);
    }

    errors
}

/// Whether `v` has a shape compatible with the field's declared type.
fn type_ok(ft: FieldType, v: &JsonValue) -> bool {
    use FieldType::*;
    match ft {
        Integer | Biginteger | Decimal | Float => {
            v.is_number()
                || v.as_str()
                    .map(|s| s.trim().parse::<f64>().is_ok())
                    .unwrap_or(false)
        }
        Boolean => {
            v.is_boolean()
                || v.as_str()
                    .map(|s| {
                        matches!(
                            s.trim().to_lowercase().as_str(),
                            "true" | "false" | "1" | "0" | "yes" | "no"
                        )
                    })
                    .unwrap_or(false)
        }
        String | Text | Richtext | Email | Password | Uid | Enumeration => v.is_string(),
        Date | Datetime | Time => v.is_string(),
        Json | Blocks => v.is_object() || v.is_array(),
        Media => v.is_array() || v.is_object() || v.is_number(),
        Relation => v.is_number() || v.is_object() || v.is_array(),
        Component => v.is_object() || v.is_array(),
        Dynamiczone => v.is_array(),
    }
}

fn expected_type(ft: FieldType) -> &'static str {
    use FieldType::*;
    match ft {
        Integer | Biginteger | Decimal | Float => "a number",
        Boolean => "a boolean",
        String | Text | Richtext | Email | Password | Uid | Enumeration => "a string",
        Date | Datetime | Time => "a date/time string",
        Json | Blocks => "a JSON object or array",
        Media => "a media reference or list of media references",
        Relation => "a relation reference or list of references",
        Component => "an object or array of components",
        Dynamiczone => "an array of dynamic zone components",
    }
}

fn describe(v: &JsonValue) -> String {
    match v {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(_) => "a boolean".to_string(),
        JsonValue::Number(_) => "a number".to_string(),
        JsonValue::String(_) => "a string".to_string(),
        JsonValue::Array(_) => "an array".to_string(),
        JsonValue::Object(_) => "an object".to_string(),
    }
}

/// Apply `min`/`max`, `minLength`/`maxLength`, `regex` and `enum` constraints.
fn validate_constraints(
    attr: &Attribute,
    name: &str,
    v: &JsonValue,
    errors: &mut Vec<PayloadError>,
) {
    use FieldType::*;
    match attr.attr_type {
        Integer | Biginteger | Decimal | Float => {
            if let Some(num) = as_f64(v) {
                if let Some(min) = attr.min.as_ref().and_then(|m| m.as_f64()) {
                    if num < min {
                        errors.push(PayloadError::new(
                            name,
                            "min",
                            format!("Field '{name}' must be >= {min}"),
                        ));
                    }
                }
                if let Some(max) = attr.max.as_ref().and_then(|m| m.as_f64()) {
                    if num > max {
                        errors.push(PayloadError::new(
                            name,
                            "max",
                            format!("Field '{name}' must be <= {max}"),
                        ));
                    }
                }
            }
        }
        String | Text | Richtext | Email | Password | Uid => {
            if let Some(s) = v.as_str() {
                let len = s.chars().count() as i64;
                if let Some(minl) = attr.min_length {
                    if len < minl {
                        errors.push(PayloadError::new(
                            name,
                            "minLength",
                            format!("Field '{name}' must be at least {minl} characters"),
                        ));
                    }
                }
                if let Some(maxl) = attr.max_length {
                    if len > maxl {
                        errors.push(PayloadError::new(
                            name,
                            "maxLength",
                            format!("Field '{name}' must be at most {maxl} characters"),
                        ));
                    }
                }
                if let Some(re) = &attr.regex {
                    if let Some(r) = Regex::new(re).ok() {
                        if !r.is_match(s) {
                            errors.push(PayloadError::new(
                                name,
                                "regex",
                                format!("Field '{name}' does not match the required pattern"),
                            ));
                        }
                    }
                }
            }
        }
        Enumeration => {
            if let Some(s) = v.as_str() {
                if !attr.enum_values.is_empty() && !attr.enum_values.iter().any(|e| e == s) {
                    errors.push(PayloadError::new(
                        name,
                        "enum",
                        format!(
                            "Value '{s}' is not allowed for field '{name}' (allowed: {})",
                            attr.enum_values.join(", ")
                        ),
                    ));
                }
            }
        }
        Component => {
            if attr.repeatable == Some(true) {
                if let Some(arr) = v.as_array() {
                    count_check(attr, name, arr.len() as i64, errors);
                }
            }
        }
        Media => {
            if attr.multiple == Some(true) {
                if let Some(arr) = v.as_array() {
                    count_check(attr, name, arr.len() as i64, errors);
                }
            }
        }
        _ => {}
    }
}

/// `min`/`max` interpreted as a cardinality bound for repeatable fields.
fn count_check(attr: &Attribute, name: &str, n: i64, errors: &mut Vec<PayloadError>) {
    if let Some(min) = attr.min.as_ref().and_then(|m| m.as_i64()) {
        if n < min {
            errors.push(PayloadError::new(
                name,
                "min",
                format!("Field '{name}' requires at least {min} items"),
            ));
        }
    }
    if let Some(max) = attr.max.as_ref().and_then(|m| m.as_i64()) {
        if n > max {
            errors.push(PayloadError::new(
                name,
                "max",
                format!("Field '{name}' allows at most {max} items"),
            ));
        }
    }
}

/// Numeric value, accepting JSON numbers and numeric strings (imports may
/// arrive as strings before transformation).
fn as_f64(v: &JsonValue) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    v.as_str().and_then(|s| s.trim().parse::<f64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::{ContentTypeKind, Uid};
    use indexmap::IndexMap;

    fn string_attr(required: bool) -> Attribute {
        Attribute {
            attr_type: FieldType::String,
            required,
            ..Default::default()
        }
    }

    fn schema(attrs: &[(&str, Attribute)]) -> Schema {
        Schema {
            uid: Uid::new("api::product.product"),
            kind: ContentTypeKind::CollectionType,
            collection_name: None,
            info: crate::SchemaInfo {
                singular_name: "product".into(),
                plural_name: "products".into(),
                display_name: "Product".into(),
                description: None,
                icon: None,
            },
            options: Default::default(),
            plugin_options: None,
            attributes: attrs
                .iter()
                .map(|(n, a)| (n.to_string(), a.clone()))
                .collect::<IndexMap<_, _>>(),
        }
    }

    fn payload(json: serde_json::Value) -> JsonMap<String, JsonValue> {
        json.as_object().unwrap().clone()
    }

    #[test]
    fn required_is_enforced_only_when_requested() {
        let s = schema(&[("name", string_attr(true))]);

        let errors = validate_payload(&s, &payload(serde_json::json!({})), true);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "required");

        // Partial update: missing required field is fine.
        let errors = validate_payload(&s, &payload(serde_json::json!({})), false);
        assert!(errors.is_empty());

        // Null counts as missing.
        let errors = validate_payload(&s, &payload(serde_json::json!({"name": null})), true);
        assert_eq!(errors[0].code, "required");

        // Empty string counts as missing for a required string.
        let errors = validate_payload(&s, &payload(serde_json::json!({"name": "  "})), true);
        assert_eq!(errors[0].code, "required");
    }

    #[test]
    fn type_mismatch_is_reported() {
        let s = schema(&[("qty", Attribute::new(FieldType::Integer))]);
        let errors = validate_payload(&s, &payload(serde_json::json!({"qty": "abc"})), false);
        assert_eq!(errors[0].code, "type");

        // Numeric strings are tolerated.
        let errors = validate_payload(&s, &payload(serde_json::json!({"qty": "12"})), false);
        assert!(errors.is_empty());
    }

    #[test]
    fn min_max_number_bounds() {
        let mut a = Attribute::new(FieldType::Integer);
        a.min = Some(serde_json::json!(1));
        a.max = Some(serde_json::json!(10));
        let s = schema(&[("qty", a)]);

        assert!(validate_payload(&s, &payload(serde_json::json!({"qty": 5})), false).is_empty());
        let errors = validate_payload(&s, &payload(serde_json::json!({"qty": 0})), false);
        assert_eq!(errors[0].code, "min");
        let errors = validate_payload(&s, &payload(serde_json::json!({"qty": 11})), false);
        assert_eq!(errors[0].code, "max");
    }

    #[test]
    fn min_max_length_bounds() {
        let mut a = string_attr(false);
        a.min_length = Some(3);
        a.max_length = Some(5);
        let s = schema(&[("code", a)]);

        assert!(validate_payload(&s, &payload(serde_json::json!({"code": "abc"})), false).is_empty());
        let errors = validate_payload(&s, &payload(serde_json::json!({"code": "ab"})), false);
        assert_eq!(errors[0].code, "minLength");
        let errors = validate_payload(&s, &payload(serde_json::json!({"code": "abcdef"})), false);
        assert_eq!(errors[0].code, "maxLength");
    }

    #[test]
    fn regex_pattern_is_enforced() {
        let mut a = string_attr(false);
        a.regex = Some("^[A-Z]{2}[0-9]{3}$".into());
        let s = schema(&[("sku", a)]);

        assert!(validate_payload(&s, &payload(serde_json::json!({"sku": "AB123"})), false).is_empty());
        let errors = validate_payload(&s, &payload(serde_json::json!({"sku": "nope"})), false);
        assert_eq!(errors[0].code, "regex");
    }

    #[test]
    fn enum_values_are_checked() {
        let mut a = Attribute::new(FieldType::Enumeration);
        a.enum_values = vec!["draft".into(), "published".into()];
        let s = schema(&[("status", a)]);

        assert!(validate_payload(&s, &payload(serde_json::json!({"status": "draft"})), false).is_empty());
        let errors = validate_payload(&s, &payload(serde_json::json!({"status": "archived"})), false);
        assert_eq!(errors[0].code, "enum");
    }

    #[test]
    fn repeatable_component_min_max_count() {
        let mut a = Attribute::new(FieldType::Component);
        a.repeatable = Some(true);
        a.min = Some(serde_json::json!(1));
        a.max = Some(serde_json::json!(3));
        let s = schema(&[("items", a)]);

        assert!(
            validate_payload(&s, &payload(serde_json::json!({"items": [1, 2, 3]})), false)
                .is_empty()
        );
        let errors = validate_payload(&s, &payload(serde_json::json!({"items": []})), false);
        assert_eq!(errors[0].code, "min");
        let errors = validate_payload(&s, &payload(serde_json::json!({"items": [1, 2, 3, 4]})), false);
        assert_eq!(errors[0].code, "max");
    }

    #[test]
    fn multiple_errors_collected() {
        let mut a = string_attr(true);
        a.regex = Some("^x".into());
        let s = schema(&[("name", a)]);
        let errors = validate_payload(&s, &payload(serde_json::json!({})), true);
        // Only the missing-required is reported (value constraints are skipped).
        assert_eq!(errors.len(), 1);

        let mut b = string_attr(false);
        b.regex = Some("^x".into());
        b.min_length = Some(5);
        let s2 = schema(&[("name", b)]);
        let errors = validate_payload(&s2, &payload(serde_json::json!({"name": "no"})), false);
        assert_eq!(errors.len(), 2);
    }
}
