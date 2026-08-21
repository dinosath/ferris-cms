//! Schema inference and content-type detection from parsed records.

use api_types::{ContentTypeSuggestion, InferredField, InferredKind};
use core_schema::Schema;
use serde_json::Value as JsonValue;

/// Normalize a field name for fuzzy matching (lowercase, strip non-alnum).
pub fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Classify a single JSON value into a coarse kind.
pub fn classify(value: &JsonValue) -> InferredKind {
    match value {
        JsonValue::Null => InferredKind::Null,
        JsonValue::Bool(_) => InferredKind::Boolean,
        JsonValue::Number(_) => InferredKind::Number,
        JsonValue::Array(_) => InferredKind::Array,
        JsonValue::Object(_) => InferredKind::Object,
        JsonValue::String(s) => classify_string(s),
    }
}

/// Classify a string: numeric-like → Number, date-like → Date, else String.
pub fn classify_string(s: &str) -> InferredKind {
    let t = s.trim();
    if t.is_empty() {
        return InferredKind::String;
    }
    if looks_like_number(t) {
        return InferredKind::Number;
    }
    if looks_like_date(t) {
        return InferredKind::Date;
    }
    InferredKind::String
}

fn looks_like_number(s: &str) -> bool {
    s.parse::<f64>().is_ok()
}

fn looks_like_date(s: &str) -> bool {
    // ISO 8601-ish or common date formats.
    if chrono::DateTime::parse_from_rfc3339(s).is_ok() {
        return true;
    }
    let compact = s.replace(['-', '/', ' '], "");
    if compact.len() == 8 && compact.chars().all(|c| c.is_ascii_digit()) {
        return true; // YYYYMMDD
    }
    // e.g. 2024-01-15 or 01/15/2024
    let parts: Vec<&str> = s.split(['-', '/']).collect();
    if parts.len() == 3 {
        return parts
            .iter()
            .all(|p| p.len() >= 1 && p.len() <= 4 && p.chars().all(|c| c.is_ascii_digit()));
    }
    false
}

/// Infer a schema (field list) from records.
pub fn infer_schema(records: &[JsonValue]) -> Vec<InferredField> {
    // Ordered union of keys.
    let mut fields: Vec<String> = Vec::new();
    for rec in records {
        if let Some(obj) = rec.as_object() {
            for k in obj.keys() {
                if !fields.contains(k) {
                    fields.push(k.clone());
                }
            }
        }
    }

    let mut out = Vec::new();
    for name in fields {
        let mut kind_counts: Vec<(InferredKind, usize)> = Vec::new();
        let mut nullable = false;
        let mut example: Option<JsonValue> = None;
        for rec in records {
            let val = rec.get(&name);
            match val {
                None | Some(JsonValue::Null) => nullable = true,
                Some(v) => {
                    if example.is_none() {
                        example = Some(v.clone());
                    }
                    let kind = classify(v);
                    if let Some(entry) = kind_counts.iter_mut().find(|(k, _)| *k == kind) {
                        entry.1 += 1;
                    } else {
                        kind_counts.push((kind, 1));
                    }
                }
            }
        }
        let total = kind_counts.iter().map(|(_, c)| c).sum::<usize>().max(1);
        let (kind, count) = kind_counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .unwrap_or((InferredKind::Unknown, 0));
        out.push(InferredField {
            name: name.clone(),
            kind,
            nullable,
            example,
            confidence: count as f32 / total as f32,
        });
    }
    out
}

/// Rank existing content types by how well they match the inferred fields.
/// Confidence is the fraction of inferred field names that match a schema
/// attribute (fuzzy), biased by coverage of the schema's fields.
pub fn detect_content_types(
    schemas: &[Schema],
    inferred: &[InferredField],
) -> Vec<ContentTypeSuggestion> {
    let mut candidates: Vec<ContentTypeSuggestion> = schemas
        .iter()
        .map(|s| {
            let schema_names: Vec<String> = s.attributes.keys().cloned().collect();
            let matched: Vec<String> = inferred
                .iter()
                .filter(|f| {
                    schema_names
                        .iter()
                        .any(|a| normalize(a) == normalize(&f.name))
                })
                .map(|f| f.name.clone())
                .collect();
            let inferred_names: Vec<String> = inferred.iter().map(|f| normalize(&f.name)).collect();
            let schema_matches = schema_names
                .iter()
                .filter(|a| inferred_names.contains(&normalize(a)))
                .count();
            // Precision = fraction of source fields matched; recall = fraction
            // of schema fields matched. Combine for a balanced score.
            let precision = if inferred_names.is_empty() {
                0.0
            } else {
                matched.len() as f32 / inferred_names.len() as f32
            };
            let recall = if schema_names.is_empty() {
                0.0
            } else {
                schema_matches as f32 / schema_names.len() as f32
            };
            let confidence = if precision == 0.0 && recall == 0.0 {
                0.0
            } else {
                0.7 * precision + 0.3 * recall
            };
            ContentTypeSuggestion {
                uid: s.uid.as_str().to_string(),
                display_name: s.info.display_name.clone(),
                confidence,
                matched_fields: matched,
            }
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

/// Choose the best content type suggestion if it meets a confidence threshold.
pub fn best_suggestion(
    candidates: &[ContentTypeSuggestion],
    threshold: f32,
) -> Option<ContentTypeSuggestion> {
    candidates
        .first()
        .filter(|c| c.confidence >= threshold)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(uid: &str, fields: &[&str]) -> Schema {
        let mut s = core_schema::Schema {
            uid: core_domain::Uid::new(uid.to_string()),
            kind: core_domain::ContentTypeKind::CollectionType,
            collection_name: None,
            info: core_schema::SchemaInfo {
                singular_name: "x".into(),
                plural_name: "xs".into(),
                display_name: uid.to_string(),
                description: None,
                icon: None,
            },
            options: Default::default(),
            plugin_options: None,
            attributes: Default::default(),
        };
        for f in fields {
            s.attributes.insert(
                (*f).to_string(),
                core_schema::Attribute::new(core_domain::FieldType::String),
            );
        }
        s
    }

    #[test]
    fn classifies_values() {
        assert_eq!(classify_string("3.5"), InferredKind::Number);
        assert_eq!(classify_string("2024-01-15"), InferredKind::Date);
        assert_eq!(classify_string("hello"), InferredKind::String);
    }

    #[test]
    fn infers_schema_from_records() {
        let records = vec![
            serde_json::json!({"name": "Ferris", "price": 3.5, "available": true}),
            serde_json::json!({"name": "IPA", "price": 4.0, "available": false}),
        ];
        let schema = infer_schema(&records);
        assert_eq!(schema.len(), 3);
        let price = schema.iter().find(|f| f.name == "price").unwrap();
        assert_eq!(price.kind, InferredKind::Number);
        let avail = schema.iter().find(|f| f.name == "available").unwrap();
        assert_eq!(avail.kind, InferredKind::Boolean);
    }

    #[test]
    fn detects_content_type_by_overlap() {
        let schemas = vec![
            schema(
                "api::product.product",
                &["name", "sku", "price", "category"],
            ),
            schema("api::category.category", &["title"]),
        ];
        let inferred = infer_schema(&[
            serde_json::json!({"name": "A", "sku": "S1", "price": 1, "category": "Beer"}),
        ]);
        let cands = detect_content_types(&schemas, &inferred);
        assert_eq!(cands[0].uid, "api::product.product");
    }

    #[test]
    fn never_silently_selects_low_confidence() {
        let schemas = vec![schema("api::product.product", &["name", "sku"])];
        let inferred = infer_schema(&[serde_json::json!({"zzz": 1, "qqq": 2})]);
        let cands = detect_content_types(&schemas, &inferred);
        assert!(cands[0].confidence < 0.4, "confidence should be low");
        assert!(
            best_suggestion(&cands, 0.4).is_none(),
            "must not auto-select a low-confidence type"
        );
    }

    #[test]
    fn infers_nullable_and_empty() {
        let records = vec![
            serde_json::json!({"name": "A", "note": null}),
            serde_json::json!({"name": "B"}),
        ];
        let schema = infer_schema(&records);
        let note = schema.iter().find(|f| f.name == "note").unwrap();
        assert!(note.nullable, "missing/null values should mark nullable");
    }

    #[test]
    fn empty_records_yield_no_fields() {
        assert!(infer_schema(&[]).is_empty());
    }
}
