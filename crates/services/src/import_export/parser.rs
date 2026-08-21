//! File parsing: format detection and CSV / JSON / YAML → datasets.
//!
//! A *dataset* is an ordered list of records (JSON objects) plus a name. A file
//! may yield several datasets (e.g. a JSON object whose values are arrays, like
//! `{categories: [...], products: [...]}`).

use api_types::DataFormat;
use serde_json::Value as JsonValue;

/// One named dataset parsed from a file.
#[derive(Clone, Debug)]
pub struct Dataset {
    pub name: String,
    pub records: Vec<JsonValue>,
}

/// Detect the data format from the filename extension first, then the content.
pub fn detect_format(filename: &str, content: &str) -> DataFormat {
    let lower = filename.to_lowercase();
    if lower.ends_with(".csv") {
        return DataFormat::Csv;
    }
    if lower.ends_with(".json") {
        return DataFormat::Json;
    }
    if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        return DataFormat::Yaml;
    }
    // Sniff by content.
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Try JSON; if it fails it is likely YAML (which also allows `{`? no).
        if serde_json::from_str::<JsonValue>(trimmed).is_ok() {
            return DataFormat::Json;
        }
        return DataFormat::Yaml;
    }
    if trimmed.contains('\t') || (trimmed.lines().count() > 1 && trimmed.contains(',')) {
        return DataFormat::Csv;
    }
    DataFormat::Json
}

/// Parse a file's content into datasets.
pub fn parse_content(filename: &str, content: &str) -> Result<Vec<Dataset>, String> {
    let format = detect_format(filename, content);
    match format {
        DataFormat::Csv => parse_csv(content),
        DataFormat::Json => parse_json(filename, content),
        DataFormat::Yaml => parse_yaml(filename, content),
    }
}

fn parse_csv(content: &str) -> Result<Vec<Dataset>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(content.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| format!("invalid CSV header: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect();
    let mut records = Vec::new();
    for (i, row) in rdr.records().enumerate() {
        let row = row.map_err(|e| format!("CSV row {}: {e}", i + 1))?;
        let mut obj = serde_json::Map::new();
        for (h, field) in headers.iter().zip(row.iter()) {
            obj.insert(h.clone(), JsonValue::String(field.to_string()));
        }
        records.push(JsonValue::Object(obj));
    }
    Ok(vec![Dataset {
        name: "data".to_string(),
        records,
    }])
}

fn split_datasets(name: String, value: JsonValue) -> Vec<Dataset> {
    match value {
        JsonValue::Array(items) => vec![Dataset {
            name,
            records: items,
        }],
        JsonValue::Object(map) => {
            // If every value is an array, treat each key as a dataset.
            let all_arrays = !map.is_empty() && map.values().all(|v| v.is_array());
            if all_arrays {
                map.into_iter()
                    .map(|(k, v)| Dataset {
                        name: k,
                        records: v.as_array().cloned().unwrap_or_default(),
                    })
                    .collect()
            } else {
                vec![Dataset {
                    name,
                    records: vec![JsonValue::Object(map)],
                }]
            }
        }
        other => vec![Dataset {
            name,
            records: vec![other],
        }],
    }
}

fn parse_json(filename: &str, content: &str) -> Result<Vec<Dataset>, String> {
    let value: JsonValue =
        serde_json::from_str(content).map_err(|e| format!("invalid JSON: {e}"))?;
    Ok(split_datasets(stem(filename), value))
}

fn parse_yaml(filename: &str, content: &str) -> Result<Vec<Dataset>, String> {
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| format!("invalid YAML: {e}"))?;
    let value: JsonValue = serde_json::to_value(&yaml).map_err(|e| format!("YAML to JSON: {e}"))?;
    Ok(split_datasets(stem(filename), value))
}

fn stem(filename: &str) -> String {
    std::path::Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_format_by_extension() {
        assert_eq!(detect_format("a.csv", "x"), DataFormat::Csv);
        assert_eq!(detect_format("a.json", "x"), DataFormat::Json);
        assert_eq!(detect_format("a.yaml", "x"), DataFormat::Yaml);
        assert_eq!(detect_format("a.yml", "x"), DataFormat::Yaml);
    }

    #[test]
    fn parses_csv_rows() {
        let content = "name,price\nFerris Lager,3.50\nFerris IPA,4.00\n";
        let ds = parse_content("products.csv", content).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].records.len(), 2);
        assert_eq!(ds[0].records[0]["name"], "Ferris Lager");
        assert_eq!(ds[0].records[1]["price"], "4.00");
    }

    #[test]
    fn parses_json_array() {
        let content = r#"[{"name":"A","price":1},{"name":"B","price":2}]"#;
        let ds = parse_content("products.json", content).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].records.len(), 2);
    }

    #[test]
    fn parses_json_multiple_datasets() {
        let content = r#"{"categories":[{"name":"Beer"}],"products":[{"name":"A"}]}"#;
        let ds = parse_content("data.json", content).unwrap();
        assert_eq!(ds.len(), 2);
        assert!(ds.iter().any(|d| d.name == "categories"));
        assert!(ds.iter().any(|d| d.name == "products"));
    }

    #[test]
    fn parses_yaml() {
        let content = "- name: A\n  price: 1\n- name: B\n  price: 2\n";
        let ds = parse_content("products.yaml", content).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].records.len(), 2);
    }

    #[test]
    fn invalid_json_errors() {
        assert!(parse_content("a.json", "{not json").is_err());
    }
}
