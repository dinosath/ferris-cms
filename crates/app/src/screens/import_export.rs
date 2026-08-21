//! Import & Export wizards (table-first, Strapi-inspired).

use api_types::{
    AnalyzeFileResponse, AnalyzeRequest, DataFormat, ExportRequest, FileImportConfig, FilePayload,
    ImportMode, ImportRequest, ImportState, MappingDto, MappingPresetUpsert, MappingStatus,
    TransformKind,
};
use core_schema::Schema;
use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{use_global, Route};
use crate::components::{Badge, Button, Card, Dropdown, EmptyState, TextField};

const TRANSFORMS: &[(&str, &str)] = &[
    ("none", "None"),
    ("number", "Number"),
    ("boolean", "Boolean"),
    ("date", "Date"),
    ("trim", "Trim whitespace"),
    ("lowercase", "Lowercase"),
    ("uppercase", "Uppercase"),
    ("emptyToNull", "Empty → null"),
    ("slug", "Slug"),
    ("split", "Split (|)"),
    ("join", "Join (|)"),
];

fn transform_key(t: &TransformKind) -> &'static str {
    match t {
        TransformKind::None => "none",
        TransformKind::Number => "number",
        TransformKind::Boolean => "boolean",
        TransformKind::Date => "date",
        TransformKind::Trim => "trim",
        TransformKind::Lowercase => "lowercase",
        TransformKind::Uppercase => "uppercase",
        TransformKind::EmptyToNull => "emptyToNull",
        TransformKind::Slug => "slug",
        TransformKind::Split(_) => "split",
        TransformKind::Join(_) => "join",
        TransformKind::Replace { .. } => "none",
        TransformKind::Default(_) => "default",
        TransformKind::ParseJson => "none",
    }
}

fn transform_from_key(k: &str) -> TransformKind {
    match k {
        "number" => TransformKind::Number,
        "boolean" => TransformKind::Boolean,
        "date" => TransformKind::Date,
        "trim" => TransformKind::Trim,
        "lowercase" => TransformKind::Lowercase,
        "uppercase" => TransformKind::Uppercase,
        "emptyToNull" => TransformKind::EmptyToNull,
        "slug" => TransformKind::Slug,
        "split" => TransformKind::Split("|".to_string()),
        "join" => TransformKind::Join("|".to_string()),
        _ => TransformKind::None,
    }
}

/// Regenerate a suggested mapping against a (possibly different) target content
/// type: a source field maps to the matching target attribute when the names
/// agree (auto-mapped), otherwise it becomes NeedsAttention so the user can
/// assign it by hand. Used when the user changes the "Import into" target so
/// the mapping table always reflects the selected existing content.
fn remap_for_target(mapping: Vec<MappingDto>, schema: Option<&Schema>) -> Vec<MappingDto> {
    let attrs: Vec<String> = schema
        .map(|s| s.attributes.keys().cloned().collect())
        .unwrap_or_default();
    mapping
        .into_iter()
        .map(|mut m| {
            if attrs.iter().any(|k| *k == m.source_field) {
                m.target_field = Some(m.source_field.clone());
                m.status = MappingStatus::AutoMapped;
            } else {
                m.target_field = None;
                m.status = MappingStatus::NeedsAttention;
            }
            m
        })
        .collect()
}

/// Parse a filter value string into a typed JSON value (bool / number / string).
fn parse_filter_value(s: &str) -> serde_json::Value {
    let t = s.trim();
    if t.eq_ignore_ascii_case("true") {
        serde_json::json!(true)
    } else if t.eq_ignore_ascii_case("false") {
        serde_json::json!(false)
    } else if let Ok(n) = t.parse::<i64>() {
        serde_json::json!(n)
    } else if let Ok(f) = t.parse::<f64>() {
        serde_json::json!(f)
    } else {
        serde_json::json!(t)
    }
}

/// Read the content of selected files into the `on_files` callback.
#[component]
fn FileInput(on_files: EventHandler<Vec<(String, String)>>) -> Element {
    rsx! {
        div {
            style: "border:1px dashed {color::NEUTRAL_300}; border-radius:8px; padding:32px; text-align:center; background:{color::NEUTRAL_50};",
            label {
                style: "cursor:pointer; display:flex; flex-direction:column; align-items:center; gap:8px; color:{color::NEUTRAL_600};",
                span { style: "font-size:14px; font-weight:600; color:{color::PRIMARY_600};", "Choose files or drag them here" }
                span { style: "font-size:13px; color:{color::NEUTRAL_500};", "Supported: .csv, .json, .yaml, .yml" }
                input {
                    r#type: "file",
                    multiple: true,
                    accept: ".csv,.json,.yaml,.yml",
                    style: "display:none;",
                    onchange: move |e| {
                        let files: Vec<_> = e.files().into_iter().collect();
                        let cb = on_files.clone();
                        spawn(async move {
                            let mut out = Vec::new();
                            for f in files {
                                let name = f.name();
                                let content = match f.read_bytes().await {
                                    Ok(b) => String::from_utf8_lossy(b.as_ref()).to_string(),
                                    Err(_) => String::new(),
                                };
                                out.push((name, content));
                            }
                            cb.call(out);
                        });
                    },
                }
            }
        }
    }
}

/// The Import wizard: upload → analyze → map → confirm → results.
#[component]
pub fn ImportWizard(initial_uid: Option<String>) -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut schemas = use_signal(Vec::<Schema>::new);
    let mut files = use_signal(Vec::<(String, String)>::new);
    let mut datasets = use_signal(Vec::<AnalyzeFileResponse>::new);
    let mut targets = use_signal(Vec::<Option<String>>::new);
    let mut mappings = use_signal(Vec::<Vec<MappingDto>>::new);
    let mut step = use_signal(|| 1usize);
    let mut mode = use_signal(|| ImportMode::CreateOnly);
    let mut match_field = use_signal(String::new);
    let mut import_state = use_signal(|| ImportState::Draft);
    let mut locale = use_signal(|| "en".to_string());
    let mut busy = use_signal(|| false);
    let mut result = use_signal(|| None::<serde_json::Value>);
    let mut status = use_signal(|| None::<String>);
    let mut analyze_req = use_signal(|| false);
    let mut import_req = use_signal(|| false);
    let mut presets = use_signal(Vec::<serde_json::Value>::new);
    let mut preset_name = use_signal(String::new);
    let mut presets_loaded = use_signal(|| false);
    let mut presets_load_req = use_signal(|| false);
    let mut preset_save_req = use_signal(|| false);
    let mut csv_delimiter = use_signal(|| ",".to_string());
    let mut csv_has_header = use_signal(|| true);
    let mut route = global.route;

    // Load content types for the target dropdowns.
    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            let mut sc = schemas;
            spawn(async move {
                if let Ok(v) = client.ctb_list().await {
                    let list: Vec<Schema> = v
                        .get("data")
                        .and_then(|d| d.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    sc.set(list);
                }
            });
        }
    });

    let target_options: Vec<(String, String)> = schemas()
        .iter()
        .map(|s| (s.uid.as_str().to_string(), s.info.display_name.clone()))
        .collect();

    // Analyze all uploaded files (triggered by the Analyze button).
    use_effect({
        let client = client.clone();
        move || {
            if analyze_req() {
                analyze_req.set(false);
                let files = files();
                if files.is_empty() {
                    return;
                }
                let client = client.clone();
                let mut ds = datasets;
                let mut ts = targets;
                let mut ms = mappings;
                let mut step2 = step;
                let mut busy2 = busy;
                let mut status2 = status;
                busy2.set(true);
                let csv_delim = csv_delimiter();
                let csv_header = csv_has_header();
                let payloads: Vec<FilePayload> = files
                    .iter()
                    .map(|(n, c)| FilePayload {
                        filename: n.clone(),
                        content: c.clone(),
                        csv_delimiter: Some(csv_delim.clone()),
                        csv_has_header: Some(csv_header),
                    })
                    .collect();
                let prefer_uid = initial_uid.clone();
                spawn(async move {
                    match client
                        .import_export_analyze(&AnalyzeRequest {
                            files: payloads,
                            prefer_uid,
                        })
                        .await
                    {
                        Ok(v) => {
                            let list: Vec<AnalyzeFileResponse> = v
                                .get("data")
                                .and_then(|d| d.get("datasets"))
                                .and_then(|d| serde_json::from_value(d.clone()).ok())
                                .unwrap_or_default();
                            let n = list.len();
                            let mut t = vec![None; n];
                            let mut m = vec![vec![]; n];
                            for (i, d) in list.iter().enumerate() {
                                if let Some(c) = &d.detected_content_type {
                                    t[i] = Some(c.uid.clone());
                                    m[i] = d.suggested_mapping.clone();
                                }
                            }
                            ds.set(list);
                            ts.set(t);
                            ms.set(m);
                            step2.set(2);
                        }
                        Err(e) => {
                            status2.set(Some(format!("Analysis failed: {e}")));
                        }
                    }
                    busy2.set(false);
                });
            }
        }
    });

    // Run the import (triggered by the Import button).
    use_effect({
        let client = client.clone();
        move || {
            if import_req() {
                import_req.set(false);
                let files = files();
                let ds = datasets();
                let ts = targets();
                let ms = mappings();
                let mut configs = Vec::new();
                for (i, d) in ds.iter().enumerate() {
                    let Some(uid) = ts.get(i).and_then(|u| u.clone()) else {
                        continue;
                    };
                    let Some((_, content)) = files.iter().find(|(n, _)| n == &d.filename) else {
                        continue;
                    };
                    configs.push(FileImportConfig {
                        filename: d.filename.clone(),
                        dataset: d.dataset.clone(),
                        content: content.clone(),
                        uid,
                        mapping: ms.get(i).cloned().unwrap_or_default(),
                        mode: mode(),
                        match_field: if match_field().is_empty() {
                            None
                        } else {
                            Some(match_field())
                        },
                        state_field: None,
                        import_state: import_state(),
                        locale_field: None,
                        locale: locale(),
                        csv_delimiter: Some(csv_delimiter()),
                        csv_has_header: Some(csv_has_header()),
                    });
                }
                if configs.is_empty() {
                    status.set(Some("No datasets to import".to_string()));
                    return;
                }
                let client = client.clone();
                let mut busy2 = busy;
                let mut result2 = result;
                let mut step2 = step;
                let mut status2 = status;
                busy2.set(true);
                spawn(async move {
                    match client
                        .import_export_import(&ImportRequest { files: configs })
                        .await
                    {
                        Ok(v) => {
                            result2.set(v.get("data").cloned());
                            step2.set(3);
                        }
                        Err(e) => status2.set(Some(format!("Import failed: {e}"))),
                    }
                    busy2.set(false);
                });
            }
        }
    });

    // Load saved mapping presets (once on mount, and on demand).
    use_effect({
        let client = client.clone();
        move || {
            if presets_load_req() || !presets_loaded() {
                if presets_load_req() {
                    presets_load_req.set(false);
                }
                presets_loaded.set(true);
                let client = client.clone();
                let mut ps = presets;
                spawn(async move {
                    if let Ok(v) = client.import_export_mappings().await {
                        ps.set(
                            v.get("data")
                                .and_then(|d| d.as_array())
                                .cloned()
                                .unwrap_or_default(),
                        );
                    }
                });
            }
        }
    });

    // Save the first dataset's mapping as a reusable preset.
    use_effect({
        let client = client.clone();
        move || {
            if preset_save_req() {
                preset_save_req.set(false);
                let name = preset_name();
                if name.trim().is_empty() {
                    return;
                }
                let target = targets().get(0).cloned().flatten().unwrap_or_default();
                let mapping = mappings().get(0).cloned().unwrap_or_default();
                let client = client.clone();
                spawn(async move {
                    let _ = client
                        .import_export_mapping_save(&MappingPresetUpsert {
                            name,
                            source_uid: "import".to_string(),
                            target_uid: target,
                            mapping,
                        })
                        .await;
                });
            }
        }
    });

    // Precompute result stats (avoiding JSON indexing inside rsx).
    let (created, updated, skipped, failed) = result()
        .as_ref()
        .map(|res| {
            (
                res["created"].as_i64().unwrap_or(0),
                res["updated"].as_i64().unwrap_or(0),
                res["skipped"].as_i64().unwrap_or(0),
                res["failed"].as_i64().unwrap_or(0),
            )
        })
        .unwrap_or((0, 0, 0, 0));

    let error_lines: Vec<String> = result()
        .as_ref()
        .and_then(|r| r["errors"].as_array())
        .map(|arr| {
            arr.iter()
                .map(|e| {
                    format!(
                        "{} · row {} · {}",
                        e["file"].as_str().unwrap_or(""),
                        e["row"],
                        e["message"].as_str().unwrap_or("")
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let mut step_chips: Vec<(usize, String, String)> = Vec::new();
    for (n, label) in [
        (1usize, "Files".to_string()),
        (2usize, "Mapping".to_string()),
        (3usize, "Results".to_string()),
    ] {
        let style = if step() == n {
            format!("padding:6px 14px; border-radius:999px; font-size:13px; font-weight:600; background:{}; color:#fff;", color::PRIMARY_600)
        } else {
            format!("padding:6px 14px; border-radius:999px; font-size:13px; font-weight:600; background:{}; color:{};", color::NEUTRAL_100, color::NEUTRAL_700)
        };
        step_chips.push((n, label, style));
    }

    // Build per-dataset mapping cards (rendered on step 2).
    let mut mapping_cards: Vec<Element> = Vec::new();
    for (i, d) in datasets().into_iter().enumerate() {
        let target = targets().get(i).cloned().flatten().unwrap_or_default();
        let mapping = mappings().get(i).cloned().unwrap_or_default();
        let schema = schemas()
            .iter()
            .find(|s| target.as_str() == s.uid.as_str())
            .cloned();
        let field_options: Vec<(String, String)> = schema
            .as_ref()
            .map(|s| {
                std::iter::once(("".to_string(), "— Ignore —".to_string()))
                    .chain(s.attributes.keys().map(|k| (k.clone(), k.clone())))
                    .collect()
            })
            .unwrap_or_default();
        let transform_options: Vec<(String, String)> = TRANSFORMS
            .iter()
            .map(|(k, l)| (k.to_string(), l.to_string()))
            .collect();
        // Example value per source field, so the user can see what the input
        // data actually looks like while assigning target fields.
        let examples: std::collections::HashMap<String, String> = d
            .schema
            .iter()
            .filter_map(|f| f.example.as_ref().map(|v| (f.name.clone(), v.to_string())))
            .collect();
        let mut rows: Vec<Element> = Vec::new();
        for (j, m) in mapping.iter().enumerate() {
            let src = m.source_field.clone();
            let tf = m.target_field.clone().unwrap_or_default();
            let tk = transform_key(&m.transform).to_string();
            let example = examples.get(&src).map(|s| s.as_str()).unwrap_or("");
            let status_badge = match m.status {
                MappingStatus::AutoMapped => {
                    rsx! { Badge { text: "Auto".to_string(), kind: "published".to_string() } }
                }
                MappingStatus::NeedsAttention => {
                    rsx! { Badge { text: "Attention".to_string(), kind: "modified".to_string() } }
                }
                MappingStatus::Ignored => {
                    rsx! { Badge { text: "Ignored".to_string(), kind: "neutral".to_string() } }
                }
                MappingStatus::Invalid => {
                    rsx! { Badge { text: "Invalid".to_string(), kind: "danger".to_string() } }
                }
            };
            let mut mappings_i = mappings;
            let i_c = i;
            let j_c = j;
            rows.push(rsx! {
                tr { key: "{i}-{j}", style: "border-bottom:1px solid {color::NEUTRAL_150};",
                    td { style: "padding:8px 12px; font-size:13px; color:{color::NEUTRAL_800};", "{src}" }
                    td { style: "padding:8px 12px; font-size:12px; color:{color::NEUTRAL_500}; font-style:italic; white-space:nowrap; max-width:200px; overflow:hidden; text-overflow:ellipsis;", "{example}" }
                    td { style: "padding:8px 12px;",
                        select { style: "padding:6px 10px; border:1px solid {color::NEUTRAL_200}; border-radius:4px; font-size:13px;",
                            value: "{tf}",
                            onchange: move |e| {
                                let v = e.value();
                                let mut m = mappings_i();
                                if let Some(row) = m.get_mut(i_c) {
                                    if let Some(mm) = row.get_mut(j_c) {
                                        mm.target_field = if v.is_empty() { None } else { Some(v.clone()) };
                                        mm.status = if v.is_empty() { MappingStatus::Ignored } else { MappingStatus::AutoMapped };
                                    }
                                }
                                mappings_i.set(m);
                            },
                            for (ov, ol) in field_options.clone() {
                                option { value: "{ov}", "{ol}" }
                            }
                        }
                    }
                    td { style: "padding:8px 12px;",
                        select { style: "padding:6px 10px; border:1px solid {color::NEUTRAL_200}; border-radius:4px; font-size:13px;",
                            value: "{tk}",
                            onchange: move |e| {
                                let v = e.value();
                                let mut m = mappings_i();
                                if let Some(row) = m.get_mut(i_c) {
                                    if let Some(mm) = row.get_mut(j_c) {
                                        mm.transform = transform_from_key(&v);
                                    }
                                }
                                mappings_i.set(m);
                            },
                            for (ov, ol) in transform_options.clone() {
                                option { value: "{ov}", "{ol}" }
                            }
                        }
                    }
                    td { style: "padding:8px 12px;", {status_badge} }
                }
            });
        }
        let mut targets_i = targets;
        let i_c = i;
        let schema_options = target_options.clone();
        mapping_cards.push(rsx! {
            Card {
                header: format!("{} · {} records", d.dataset, d.record_count),
                div { style: "display:flex; flex-direction:column; gap:12px;",
                    div { style: "display:flex; align-items:center; gap:12px;",
                        span { style: "font-size:13px; font-weight:600; color:{color::NEUTRAL_700};", "Import into" }
                        select { style: "padding:6px 10px; border:1px solid {color::NEUTRAL_200}; border-radius:4px; font-size:13px;",
                            value: "{target}",
                            onchange: move |e| {
                                let v = e.value();
                                let mut t = targets_i();
                                if i_c < t.len() { t[i_c] = Some(v.clone()); }
                                targets_i.set(t);
                                // Regenerate the suggested mapping against the newly
                                // selected content type so the table reflects it.
                                let new_schema = schemas()
                                    .iter()
                                    .find(|s| s.uid.as_str() == v)
                                    .cloned();
                                let mut ms = mappings();
                                if let Some(row) = ms.get_mut(i_c) {
                                    *row = remap_for_target(row.clone(), new_schema.as_ref());
                                }
                                mappings.set(ms);
                            },
                            for (ov, ol) in schema_options {
                                option { value: "{ov}", "{ol}" }
                            }
                        }
                    }
                    if mapping.is_empty() {
                        div { style: "padding:16px; text-align:center; color:{color::NEUTRAL_500}; font-size:13px;", "Select a content type to map fields." }
                    } else {
                        div { style: "overflow-x:auto;",
                            table { style: "width:100%; border-collapse:collapse;",
                                thead {
                                    tr {
                                        th { style: "text-align:left; padding:8px 12px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Source field" }
                                        th { style: "text-align:left; padding:8px 12px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Example" }
                                        th { style: "text-align:left; padding:8px 12px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Target field" }
                                        th { style: "text-align:left; padding:8px 12px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Transformation" }
                                        th { style: "text-align:left; padding:8px 12px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Status" }
                                    }
                                }
                                tbody { {rows.into_iter()} }
                            }
                        }
                    }
                }
            }
        });
    }

    rsx! {
        div { style: "padding:32px; max-width:1000px;",
            div { style: "display:flex; align-items:center; gap:12px; margin-bottom:20px;",
                Button { label: "← Back".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::Home) }
                div { style: "display:flex; flex-direction:column; gap:2px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "Import" }
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Bring data into FerrisCMS from CSV, JSON or YAML." }
                }
            }

            // Step indicator
            div { style: "display:flex; gap:8px; margin-bottom:20px;",
                for (n, label, style) in step_chips {
                    div { style: "{style}", "Step {n}: {label}" }
                }
            }

            if let Some(status) = status() {
                div { style: "padding:12px; margin-bottom:16px; border-radius:4px; background:{color::WARNING_100}; color:{color::WARNING_700}; font-size:14px;", "{status}" }
            }

            if step() == 1 {
                Card { padding: 24,
                    div { style: "display:flex; flex-direction:column; gap:16px;",
                        FileInput {
                            on_files: move |f: Vec<(String, String)>| {
                                files.set(f.clone());
                            }
                        }
                        for (name, _content) in files().iter() {
                            div { style: "display:flex; align-items:center; gap:8px; padding:8px 12px; border:1px solid {color::NEUTRAL_150}; border-radius:6px;",
                                span { style: "flex:1; font-size:14px; color:{color::NEUTRAL_800};", "{name}" }
                                Badge { text: "ready".to_string(), kind: "published".to_string() }
                            }
                        }
                        div { style: "display:flex; gap:16px; align-items:flex-end; flex-wrap:wrap;",
                            TextField {
                                value: csv_delimiter(),
                                label: "CSV delimiter".to_string(),
                                placeholder: ",".to_string(),
                                oninput: move |v| csv_delimiter.set(v),
                            }
                            div { style: "display:flex; flex-direction:column; gap:4px;",
                                span { style: "font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "CSV header row" }
                                label { style: "display:flex; align-items:center; gap:6px; font-size:13px; color:{color::NEUTRAL_700}; cursor:pointer;",
                                    input { r#type: "checkbox", checked: csv_has_header(), onchange: move |e| csv_has_header.set(e.checked()) }
                                    span { "First row contains column names" }
                                }
                            }
                        }
                        div { style: "display:flex; justify-content:flex-end;",
                            Button { label: "Analyze".to_string(), loading: busy(), disabled: files().is_empty(), on_click: move |_| analyze_req.set(true) }
                        }
                    }
                }
            } else if step() == 2 {
                Card { padding: 20,
                    div { style: "font-size:{typography::EPSILON_SIZE}; font-weight:600; color:{color::NEUTRAL_900}; margin-bottom:12px;", "Saved mappings" }
                    div { style: "display:flex; gap:12px; align-items:flex-end; flex-wrap:wrap;",
                        Dropdown {
                            label: "Use saved mapping".to_string(),
                            value: String::new(),
                            options: presets().iter().map(|p| (p["id"].to_string(), p["name"].as_str().unwrap_or("").to_string())).collect(),
                            onchange: move |v: String| {
                                if let Some(p) = presets().iter().find(|p| p["id"].to_string() == v) {
                                    if let Ok(mapping) = serde_json::from_value::<Vec<MappingDto>>(p["mapping"].clone()) {
                                        let mut m = mappings();
                                        if !m.is_empty() { m[0] = mapping; }
                                        mappings.set(m);
                                    }
                                }
                            },
                        }
                        TextField {
                            value: preset_name(),
                            label: "Preset name".to_string(),
                            placeholder: "e.g. Shopify Products".to_string(),
                            oninput: move |val| preset_name.set(val),
                        }
                        Button { label: "Save mapping".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| preset_save_req.set(true) }
                        Button { label: "Refresh".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| presets_load_req.set(true) }
                    }
                }
                div { style: "display:flex; flex-direction:column; gap:16px;", {mapping_cards.into_iter()} }
                Card { padding: 24,
                    div { style: "font-size:{typography::EPSILON_SIZE}; font-weight:600; color:{color::NEUTRAL_900}; margin-bottom:12px;", "Import options" }
                    div { style: "display:grid; grid-template-columns:1fr 1fr; gap:16px;",
                        Dropdown {
                            label: "Import mode".to_string(),
                            value: match mode() { ImportMode::CreateOnly => "create".to_string(), ImportMode::UpdateOnly => "update".to_string(), ImportMode::Upsert => "upsert".to_string() },
                            options: vec![("create".into(), "Create only".into()), ("update".into(), "Update only".into()), ("upsert".into(), "Upsert (create or update)".into())],
                            onchange: move |v: String| mode.set(match v.as_str() { "update" => ImportMode::UpdateOnly, "upsert" => ImportMode::Upsert, _ => ImportMode::CreateOnly }),
                        }
                        TextField {
                            value: match_field(),
                            label: "Match field (for update/upsert)".to_string(),
                            placeholder: "e.g. sku".to_string(),
                            oninput: move |v| match_field.set(v),
                        }
                        Dropdown {
                            label: "Import as".to_string(),
                            value: match import_state() { ImportState::Draft => "draft".to_string(), ImportState::Published => "published".to_string(), ImportState::Preserve => "preserve".to_string() },
                            options: vec![("draft".into(), "Draft".into()), ("published".into(), "Published".into()), ("preserve".into(), "Preserve".into())],
                            onchange: move |v: String| import_state.set(match v.as_str() { "published" => ImportState::Published, "preserve" => ImportState::Preserve, _ => ImportState::Draft }),
                        }
                        TextField {
                            value: locale(),
                            label: "Locale".to_string(),
                            placeholder: "en".to_string(),
                            oninput: move |v| locale.set(v),
                        }
                    }
                    div { style: "display:flex; justify-content:flex-end; gap:12px; margin-top:16px;",
                        Button { label: "Back".to_string(), variant: "secondary".to_string(), on_click: move |_| step.set(1) }
                        Button { label: "Import".to_string(), loading: busy(), on_click: move |_| import_req.set(true) }
                    }
                }
            } else {
                if result().is_some() {
                    div { style: "display:grid; grid-template-columns:repeat(4,1fr); gap:16px; margin-bottom:20px;",
                        StatTile { label: "Created".to_string(), value: created, kind: "published".to_string() }
                        StatTile { label: "Updated".to_string(), value: updated, kind: "modified".to_string() }
                        StatTile { label: "Skipped".to_string(), value: skipped, kind: "neutral".to_string() }
                        StatTile { label: "Failed".to_string(), value: failed, kind: "danger".to_string() }
                    }
                    if failed > 0 {
                        Card { header: "Errors".to_string(),
                            div { style: "max-height:240px; overflow:auto; display:flex; flex-direction:column;",
                                for line in error_lines.iter() {
                                    div { style: "padding:8px 12px; border-bottom:1px solid {color::NEUTRAL_150}; font-size:13px; color:{color::NEUTRAL_700};", "{line}" }
                                }
                            }
                        }
                    }
                    div { style: "display:flex; justify-content:flex-end; margin-top:16px;",
                        Button { label: "New import".to_string(), variant: "secondary".to_string(), on_click: move |_| { step.set(1); result.set(None); datasets.set(vec![]); files.set(vec![]); } }
                    }
                } else {
                    EmptyState { title: "Import finished".to_string(), subtitle: "No result payload was returned.".to_string(), icon: "stack".to_string() }
                }
            }
        }
    }
}

#[component]
fn StatTile(label: String, value: i64, kind: String) -> Element {
    let col = match kind.as_str() {
        "published" => color::SUCCESS_700,
        "modified" => color::WARNING_700,
        "danger" => color::DANGER_700,
        _ => color::NEUTRAL_800,
    };
    rsx! {
        div { style: "background:#fff; border:1px solid {color::NEUTRAL_150}; border-radius:8px; padding:16px 20px;",
            span { style: "display:block; font-size:12px; font-weight:600; color:{color::NEUTRAL_500}; margin-bottom:4px;", "{label}" }
            span { style: "font-size:22px; font-weight:600; color:{col};", "{value}" }
        }
    }
}

/// Export wizard: select content types + format → export.
#[component]
pub fn ExportWizard(initial_uid: Option<String>) -> Element {
    let global = use_global();
    let client = global.client.clone();
    let mut schemas = use_signal(Vec::<Schema>::new);
    let mut selected = use_signal(move || initial_uid.clone().map(|u| vec![u]).unwrap_or_default());
    let mut format = use_signal(|| DataFormat::Json);
    let mut fields = use_signal(Vec::<String>::new);
    let mut filter_field = use_signal(String::new);
    let mut filter_op = use_signal(|| "$eq".to_string());
    let mut filter_value = use_signal(String::new);
    let mut filter_json = use_signal(|| None::<serde_json::Value>);
    let mut busy = use_signal(|| false);
    let mut output = use_signal(|| None::<(String, String)>);
    let mut status = use_signal(|| None::<String>);
    let mut export_req = use_signal(|| false);
    let mut route = global.route;

    use_effect({
        let client = client.clone();
        move || {
            let client = client.clone();
            let mut sc = schemas;
            spawn(async move {
                if let Ok(v) = client.ctb_list().await {
                    let list: Vec<Schema> = v
                        .get("data")
                        .and_then(|d| d.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| serde_json::from_value(x.clone()).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    sc.set(list);
                }
            });
        }
    });

    // Run the export (triggered by the Export button).
    use_effect({
        let client = client.clone();
        move || {
            if export_req() {
                export_req.set(false);
                let sel = selected();
                if sel.is_empty() {
                    status.set(Some("Select at least one content type".to_string()));
                    return;
                }
                let client = client.clone();
                let mut busy2 = busy;
                let mut output2 = output;
                let mut status2 = status;
                busy2.set(true);
                let req = ExportRequest {
                    uids: sel,
                    format: format(),
                    fields: fields(),
                    filters: filter_json(),
                    limit: None,
                    locale: None,
                    status: None,
                };
                spawn(async move {
                    match client.import_export_export(&req).await {
                        Ok(v) => {
                            if let Some(d) = v.get("data") {
                                output2.set(Some((
                                    d["filename"].as_str().unwrap_or("export").to_string(),
                                    d["content"].as_str().unwrap_or("").to_string(),
                                )));
                            }
                        }
                        Err(e) => status2.set(Some(format!("Export failed: {e}"))),
                    }
                    busy2.set(false);
                });
            }
        }
    });

    let mut type_rows: Vec<Element> = Vec::new();
    let mut sel = selected;
    for s in schemas() {
        let uid = s.uid.as_str().to_string();
        let name = s.info.display_name.clone();
        let kind = s.kind;
        let checked = selected().contains(&uid);
        let mut sel2 = sel;
        let u_row = uid.clone();
        let u_box = uid.clone();
        type_rows.push(rsx! {
            tr { style: "border-bottom:1px solid {color::NEUTRAL_150}; cursor:pointer;",
                onclick: move |_| {
                    let mut list = sel2();
                    if checked { list.retain(|x| x != &u_row); } else { list.push(u_row.clone()); }
                    sel2.set(list);
                },
                td { style: "padding:10px 16px;",
                    input { r#type: "checkbox", checked: checked, onchange: move |e| {
                        let mut list = sel2();
                        if e.checked() { if !list.contains(&u_box) { list.push(u_box.clone()); } }
                        else { list.retain(|x| x != &u_box); }
                        sel2.set(list);
                    } }
                }
                td { style: "padding:10px 16px; font-size:14px; color:{color::NEUTRAL_800};", "{name}" }
                td { style: "padding:10px 16px; font-size:13px; color:{color::NEUTRAL_600};", "{uid}" }
                td { style: "padding:10px 16px;",
                    if kind == core_domain::ContentTypeKind::CollectionType {
                        Badge { text: "Collection".to_string(), kind: "published".to_string() }
                    } else {
                        Badge { text: "Single".to_string(), kind: "neutral".to_string() }
                    }
                }
            }
        });
    }

    rsx! {
        div { style: "padding:32px; max-width:1000px;",
            div { style: "display:flex; align-items:center; gap:12px; margin-bottom:20px;",
                Button { label: "← Back".to_string(), variant: "secondary".to_string(), size: "sm".to_string(), on_click: move |_| route.set(Route::Home) }
                div { style: "display:flex; flex-direction:column; gap:2px;",
                    span { style: "font-size:{typography::DELTA_SIZE}; font-weight:600; color:{color::NEUTRAL_900};", "Export" }
                    span { style: "font-size:{typography::BODY_SIZE}; color:{color::NEUTRAL_600};", "Download content as CSV, JSON or YAML." }
                }
            }

            if let Some(status) = status() {
                div { style: "padding:12px; margin-bottom:16px; border-radius:4px; background:{color::WARNING_100}; color:{color::WARNING_700}; font-size:14px;", "{status}" }
            }

            Card { header: "Content types".to_string(),
                div { style: "overflow-x:auto;",
                    table { style: "width:100%; border-collapse:collapse;",
                        thead {
                            tr {
                                th { style: "text-align:left; padding:10px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Export" }
                                th { style: "text-align:left; padding:10px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Content type" }
                                th { style: "text-align:left; padding:10px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "UID" }
                                th { style: "text-align:left; padding:10px 16px; font-size:12px; font-weight:600; color:{color::NEUTRAL_600};", "Type" }
                            }
                        }
                        tbody { {type_rows.into_iter()} }
                    }
                }
            }

            {
                let sel_uid = selected().first().cloned().unwrap_or_default();
                let schema = schemas().iter().find(|s| s.uid.as_str() == sel_uid).cloned();
                if let Some(s) = schema {
                    let all_scalar: Vec<String> = s
                        .attributes
                        .iter()
                        .filter(|(_, a)| a.attr_type.is_scalar_column())
                        .map(|(n, _)| n.clone())
                        .collect();
                    let mut field_boxes: Vec<Element> = Vec::new();
                    for n in all_scalar.iter() {
                        let n = n.clone();
                        let checked = fields().is_empty() || fields().contains(&n);
                        let mut fs2 = fields;
                        let all = all_scalar.clone();
                        field_boxes.push(rsx! {
                            label { style: "display:inline-flex; align-items:center; gap:6px; font-size:13px; color:{color::NEUTRAL_700}; cursor:pointer;",
                                input { r#type: "checkbox", checked: checked, onchange: move |e| {
                                    let mut list = fs2();
                                    if e.checked() {
                                        if !list.contains(&n) { list.push(n.clone()); }
                                    } else if list.is_empty() {
                                        list = all.clone();
                                        list.retain(|x| x != &n);
                                    } else {
                                        list.retain(|x| x != &n);
                                    }
                                    fs2.set(list);
                                } }
                                span { "{n}" }
                            }
                        });
                    }
                    rsx! {
                        Card { padding: 20,
                            div { style: "font-size:{typography::EPSILON_SIZE}; font-weight:600; color:{color::NEUTRAL_900}; margin-bottom:4px;", "Fields" }
                            span { style: "font-size:12px; color:{color::NEUTRAL_500}; display:block; margin-bottom:10px;", "Select fields to include (all are selected by default)." }
                            div { style: "display:flex; flex-wrap:wrap; gap:12px;", {field_boxes.into_iter()} }
                        }
                    }
                } else {
                    rsx! {}
                }
            }

            {
                let sel_uid = selected().first().cloned().unwrap_or_default();
                let schema = schemas().iter().find(|s| s.uid.as_str() == sel_uid).cloned();
                let field_options: Vec<(String, String)> = schema
                    .as_ref()
                    .map(|s| {
                        s.attributes
                            .iter()
                            .filter(|(_, a)| a.attr_type.is_scalar_column())
                            .map(|(n, _)| (n.clone(), n.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                rsx! {
                    Card { padding: 20,
                        div { style: "font-size:{typography::EPSILON_SIZE}; font-weight:600; color:{color::NEUTRAL_900}; margin-bottom:4px;", "Filter" }
                        span { style: "font-size:12px; color:{color::NEUTRAL_500}; display:block; margin-bottom:10px;", "Only export entries matching one condition (optional)." }
                        div { style: "display:flex; gap:10px; align-items:flex-end; flex-wrap:wrap;",
                            Dropdown {
                                label: "Field".to_string(),
                                value: filter_field(),
                                options: field_options,
                                onchange: move |v: String| filter_field.set(v),
                            }
                            Dropdown {
                                label: "Operator".to_string(),
                                value: filter_op(),
                                options: vec![
                                    ("$eq".to_string(), "is".to_string()),
                                    ("$ne".to_string(), "is not".to_string()),
                                    ("$lt".to_string(), "less than".to_string()),
                                    ("$lte".to_string(), "≤".to_string()),
                                    ("$gt".to_string(), "greater than".to_string()),
                                    ("$gte".to_string(), "≥".to_string()),
                                    ("$contains".to_string(), "contains".to_string()),
                                ],
                                onchange: move |v: String| filter_op.set(v),
                            }
                            TextField {
                                value: filter_value(),
                                label: "Value".to_string(),
                                placeholder: "e.g. 4".to_string(),
                                oninput: move |v| filter_value.set(v),
                            }
                            Button {
                                label: "Apply filter".to_string(), variant: "secondary".to_string(), size: "sm".to_string(),
                                on_click: move |_| {
                                    if !filter_field().is_empty() {
                                        let value = parse_filter_value(&filter_value());
                                        filter_json.set(Some(serde_json::json!({
                                            "leaf": {"field": filter_field(), "op": filter_op(), "values": [value]}
                                        })));
                                    }
                                },
                            }
                            Button {
                                label: "Clear".to_string(), variant: "secondary".to_string(), size: "sm".to_string(),
                                on_click: move |_| { filter_json.set(None); filter_field.set(String::new()); filter_value.set(String::new()); },
                            }
                        }
                        if filter_json().is_some() {
                            div { style: "margin-top:10px;",
                                Badge { text: "Filter applied".to_string(), kind: "modified".to_string() }
                            }
                        }
                    }
                }
            }

            Card { padding: 24,
                div { style: "display:flex; align-items:flex-end; gap:16px;",
                    Dropdown {
                        label: "Format".to_string(),
                        value: match format() { DataFormat::Csv => "csv".to_string(), DataFormat::Json => "json".to_string(), DataFormat::Yaml => "yaml".to_string() },
                        options: vec![("json".into(), "JSON".into()), ("yaml".into(), "YAML".into()), ("csv".into(), "CSV".into())],
                        onchange: move |v: String| format.set(match v.as_str() { "csv" => DataFormat::Csv, "yaml" => DataFormat::Yaml, _ => DataFormat::Json }),
                    }
                    Button { label: "Export".to_string(), loading: busy(), on_click: move |_| export_req.set(true) }
                }
            }

            if let Some((name, content)) = output() {
                Card { header: format!("{name}"),
                    div { style: "display:flex; flex-direction:column; gap:12px;",
                        pre { style: "background:{color::NEUTRAL_100}; padding:12px; border-radius:6px; font-size:12px; max-height:360px; overflow:auto; color:{color::NEUTRAL_800}; white-space:pre-wrap;", "{content}" }
                        div { style: "display:flex; justify-content:flex-end;",
                            Button { label: "Close".to_string(), variant: "secondary".to_string(), on_click: move |_| output.set(None) }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::{ContentTypeKind, FieldType, Uid};
    use core_schema::{Attribute, SchemaInfo};

    fn schema_with(fields: &[&str]) -> Schema {
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
        for f in fields {
            s.attributes
                .insert(f.to_string(), Attribute::new(FieldType::String));
        }
        s
    }

    fn row(source: &str, target: Option<&str>, status: MappingStatus) -> MappingDto {
        MappingDto {
            source_field: source.to_string(),
            target_field: target.map(|s| s.to_string()),
            transform: TransformKind::None,
            status,
            confidence: 1.0,
        }
    }

    #[test]
    fn remap_matches_same_named_fields_as_auto() {
        let mapping = vec![
            row("name", None, MappingStatus::NeedsAttention),
            row("sku", Some("sku"), MappingStatus::AutoMapped),
            row("extra", None, MappingStatus::NeedsAttention),
        ];
        let schema = schema_with(&["name", "sku", "price"]);
        let out = remap_for_target(mapping, Some(&schema));
        assert_eq!(out.len(), 3);
        // Same-named target fields become AutoMapped.
        assert_eq!(out[0].target_field.as_deref(), Some("name"));
        assert_eq!(out[0].status, MappingStatus::AutoMapped);
        assert_eq!(out[1].target_field.as_deref(), Some("sku"));
        assert_eq!(out[1].status, MappingStatus::AutoMapped);
        // A source field with no matching target attribute stays unmapped and
        // needs attention so the user assigns it.
        assert_eq!(out[2].target_field, None);
        assert_eq!(out[2].status, MappingStatus::NeedsAttention);
    }

    #[test]
    fn remap_against_missing_target_drops_mappings() {
        let mapping = vec![row("name", Some("name"), MappingStatus::AutoMapped)];
        let out = remap_for_target(mapping, None);
        assert_eq!(out[0].target_field, None);
        assert_eq!(out[0].status, MappingStatus::NeedsAttention);
    }
}
