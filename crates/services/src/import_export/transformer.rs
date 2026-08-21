//! Value transformations applied to mapped source fields before validation.

use api_types::TransformKind;
use serde_json::Value as JsonValue;

/// Apply a transformation to a source value. Best-effort: on failure the
/// original value is returned and the validator flags the type.
pub fn apply_transform(transform: &TransformKind, value: &JsonValue) -> JsonValue {
    match transform {
        TransformKind::None => value.clone(),
        TransformKind::Number => to_number(value),
        TransformKind::Boolean => to_bool(value),
        TransformKind::Date => value.clone(), // stored as string; validator checks format
        TransformKind::Trim => {
            if let Some(s) = value.as_str() {
                JsonValue::String(s.trim().to_string())
            } else {
                value.clone()
            }
        }
        TransformKind::Lowercase => {
            if let Some(s) = value.as_str() {
                JsonValue::String(s.to_lowercase())
            } else {
                value.clone()
            }
        }
        TransformKind::Uppercase => {
            if let Some(s) = value.as_str() {
                JsonValue::String(s.to_uppercase())
            } else {
                value.clone()
            }
        }
        TransformKind::EmptyToNull => {
            if value.is_null() {
                JsonValue::Null
            } else if let Some(s) = value.as_str() {
                if s.trim().is_empty() {
                    JsonValue::Null
                } else {
                    value.clone()
                }
            } else {
                value.clone()
            }
        }
        TransformKind::Replace { from, to } => {
            if let Some(s) = value.as_str() {
                JsonValue::String(s.replace(from.as_str(), to.as_str()))
            } else {
                value.clone()
            }
        }
        TransformKind::Split(sep) => {
            if let Some(s) = value.as_str() {
                JsonValue::Array(
                    s.split(sep.as_str())
                        .map(|p| JsonValue::String(p.to_string()))
                        .collect(),
                )
            } else {
                value.clone()
            }
        }
        TransformKind::Join(sep) => {
            if let Some(arr) = value.as_array() {
                let joined = arr
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(sep.as_str());
                JsonValue::String(joined)
            } else {
                value.clone()
            }
        }
        TransformKind::Default(d) => {
            let empty =
                value.is_null() || value.as_str().map(|s| s.trim().is_empty()).unwrap_or(false);
            if empty {
                JsonValue::String(d.clone())
            } else {
                value.clone()
            }
        }
        TransformKind::ParseJson => {
            if let Some(s) = value.as_str() {
                serde_json::from_str(s).unwrap_or_else(|_| value.clone())
            } else {
                value.clone()
            }
        }
        TransformKind::Slug => {
            if let Some(s) = value.as_str() {
                JsonValue::String(slugify(s))
            } else {
                value.clone()
            }
        }
    }
}

fn to_number(value: &JsonValue) -> JsonValue {
    if value.is_number() {
        return value.clone();
    }
    if let Some(s) = value.as_str() {
        if let Ok(f) = s.trim().parse::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return JsonValue::Number(n);
            }
        }
        if let Ok(i) = s.trim().parse::<i64>() {
            return JsonValue::Number(i.into());
        }
    }
    value.clone()
}

fn to_bool(value: &JsonValue) -> JsonValue {
    if let Some(b) = value.as_bool() {
        return JsonValue::Bool(b);
    }
    if let Some(s) = value.as_str() {
        let t = s.trim().to_lowercase();
        match t.as_str() {
            "true" | "1" | "yes" | "y" | "on" => return JsonValue::Bool(true),
            "false" | "0" | "no" | "n" | "off" => return JsonValue::Bool(false),
            _ => {}
        }
    }
    value.clone()
}

/// Simple URL slug: lowercase, non-alnum → `-`.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !out.is_empty() && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_number_bool_date() {
        assert_eq!(
            apply_transform(&TransformKind::Number, &JsonValue::from("3.50")),
            serde_json::json!(3.5)
        );
        assert_eq!(
            apply_transform(&TransformKind::Boolean, &JsonValue::from("yes")),
            serde_json::json!(true)
        );
    }

    #[test]
    fn trims_and_case() {
        assert_eq!(
            apply_transform(&TransformKind::Trim, &JsonValue::from("  hi  ")),
            JsonValue::from("hi")
        );
        assert_eq!(
            apply_transform(&TransformKind::Uppercase, &JsonValue::from("hi")),
            JsonValue::from("HI")
        );
    }

    #[test]
    fn split_join_and_default() {
        assert_eq!(
            apply_transform(
                &TransformKind::Split("|".into()),
                &JsonValue::from("Beer|Lager")
            ),
            serde_json::json!(["Beer", "Lager"])
        );
        assert_eq!(
            apply_transform(&TransformKind::Default("n/a".into()), &JsonValue::Null),
            JsonValue::from("n/a")
        );
    }

    #[test]
    fn slugify_works() {
        assert_eq!(slugify("Ferris Lager"), "ferris-lager");
        assert_eq!(slugify("  Hi There  "), "hi-there");
    }

    #[test]
    fn number_parse_fails_gracefully() {
        // A non-numeric string is left untouched (validator flags it later).
        assert_eq!(
            apply_transform(&TransformKind::Number, &JsonValue::from("€12,50")),
            JsonValue::from("€12,50")
        );
    }

    #[test]
    fn boolean_accepts_variants() {
        assert_eq!(
            apply_transform(&TransformKind::Boolean, &JsonValue::from("on")),
            serde_json::json!(true)
        );
        assert_eq!(
            apply_transform(&TransformKind::Boolean, &JsonValue::from("0")),
            serde_json::json!(false)
        );
    }

    #[test]
    fn replace_and_parse_json() {
        assert_eq!(
            apply_transform(
                &TransformKind::Replace {
                    from: " ".into(),
                    to: "_".into()
                },
                &JsonValue::from("a b")
            ),
            JsonValue::from("a_b")
        );
        assert_eq!(
            apply_transform(&TransformKind::ParseJson, &JsonValue::from("{\"a\":1}")),
            serde_json::json!({"a":1})
        );
    }
}
