//! Exporting entries as JSON / YAML / CSV, or the versioned FerrisCMS portable
//! format (lossless round-trip).

use crate::AppContext;
use api_types::QueryParams;
use api_types::{DataFormat, ExportCount};
use serde_json::Value as JsonValue;

/// Load + project entries for one content type.
pub async fn load_entries(
    ctx: &AppContext,
    uid: &str,
    fields: &[String],
    limit: Option<i64>,
    locale: Option<String>,
    status: Option<String>,
) -> Result<(String, Vec<JsonValue>), crate::ServiceError> {
    let schema = crate::content::load_schema(ctx, uid)?;
    let mut params = QueryParams {
        pagination: Some(api_types::PaginationParams::Page {
            page: 1,
            page_size: limit.unwrap_or(1_000_000),
            with_count: Some(true),
        }),
        locale,
        status: status.map(|s| {
            if s == "published" {
                core_domain::PublicationState::Published
            } else {
                core_domain::PublicationState::Draft
            }
        }),
        ..Default::default()
    };
    // Apply simple field projection via `fields`.
    if !fields.is_empty() {
        params.fields = Some(fields.iter().cloned().collect());
    }
    let list = crate::content::cm_list(ctx, uid, &params).await?;
    Ok((schema.info.display_name.clone(), list.data))
}

/// Build the portable FerrisCMS export document.
pub fn build_portable(datasets: &[(String, Vec<JsonValue>)], generated_at: &str) -> JsonValue {
    let content_types = datasets
        .iter()
        .map(|(uid, rows)| {
            (
                uid.clone(),
                JsonValue::Array(rows.iter().map(|r| r.clone()).collect()),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "format": "ferriscms-export",
        "version": 1,
        "generated_at": generated_at,
        "content_types": content_types,
    })
}

/// Serialize datasets to the requested format.
pub fn serialize(
    format: DataFormat,
    datasets: &[(String, Vec<JsonValue>)],
    generated_at: &str,
) -> Result<String, String> {
    match format {
        DataFormat::Json => {
            if datasets.len() == 1 {
                serde_json::to_string_pretty(&datasets[0].1).map_err(|e| e.to_string())
            } else {
                serde_json::to_string_pretty(&build_portable(datasets, generated_at))
                    .map_err(|e| e.to_string())
            }
        }
        DataFormat::Yaml => {
            let value = if datasets.len() == 1 {
                JsonValue::Array(datasets[0].1.clone())
            } else {
                build_portable(datasets, generated_at)
            };
            let y = serde_yaml::to_value(&value).map_err(|e| e.to_string())?;
            serde_yaml::to_string(&y).map_err(|e| e.to_string())
        }
        DataFormat::Csv => {
            // CSV only supports a single dataset; flatten scalar values.
            let (_, rows) = &datasets[0];
            let mut headers: Vec<String> = Vec::new();
            for r in rows {
                if let Some(obj) = r.as_object() {
                    for k in obj.keys() {
                        if !headers.contains(k) {
                            headers.push(k.clone());
                        }
                    }
                }
            }
            let mut out = String::new();
            out.push_str(&headers.join(","));
            out.push('\n');
            for r in rows {
                let row: Vec<String> = headers
                    .iter()
                    .map(|h| r.get(h).map(cell_text).unwrap_or_default())
                    .collect();
                out.push_str(&row.join(","));
                out.push('\n');
            }
            Ok(out)
        }
    }
}

/// Render a value as a CSV cell (quoted when it contains a comma/newline).
fn cell_text(v: &JsonValue) -> String {
    let s = match v {
        JsonValue::Null => String::new(),
        JsonValue::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.contains(',') || s.contains('\n') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// Convenience: run an export and return filename + content + counts.
pub async fn run_export(
    ctx: &AppContext,
    req: &api_types::ExportRequest,
) -> Result<api_types::ExportResponse, crate::ServiceError> {
    let mut datasets: Vec<(String, Vec<JsonValue>)> = Vec::new();
    let mut counts = Vec::new();
    for uid in &req.uids {
        let (display_name, rows) = load_entries(
            ctx,
            uid,
            &req.fields,
            req.limit,
            req.locale.clone(),
            req.status.clone(),
        )
        .await?;
        counts.push(ExportCount {
            uid: uid.clone(),
            display_name,
            count: rows.len(),
        });
        datasets.push((uid.clone(), rows));
    }
    let generated_at = chrono::Utc::now().to_rfc3339();
    let ext = match req.format {
        DataFormat::Csv => "csv",
        DataFormat::Json => "json",
        DataFormat::Yaml => "yaml",
    };
    let filename = format!("ferriscms-export.{}", ext);
    let content =
        serialize(req.format, &datasets, &generated_at).map_err(crate::ServiceError::Internal)?;
    Ok(api_types::ExportResponse {
        filename,
        format: req.format,
        content,
        counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_cells() {
        assert_eq!(cell_text(&JsonValue::from("a,b")), "\"a,b\"");
        assert_eq!(cell_text(&JsonValue::from("plain")), "plain");
        assert_eq!(cell_text(&JsonValue::Null), "");
    }

    #[test]
    fn builds_portable_document() {
        let doc = build_portable(
            &[(
                "api::product.product".to_string(),
                vec![serde_json::json!({"name": "A"})],
            )],
            "now",
        );
        assert_eq!(doc["format"], "ferriscms-export");
        assert_eq!(doc["version"], 1);
        assert!(doc["content_types"]["api::product.product"].is_array());
    }
}
