//! Import & Export DTOs (shared server + client wire contract).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Supported import/export data formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataFormat {
    Csv,
    Json,
    Yaml,
}

/// A coarse scalar kind inferred from source values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferredKind {
    String,
    Number,
    Boolean,
    Date,
    Null,
    Object,
    Array,
    Unknown,
}

/// A single inferred source field.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferredField {
    pub name: String,
    pub kind: InferredKind,
    /// Whether some values for this field were null/empty.
    #[serde(default)]
    pub nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
    /// 0..=1 confidence in the inferred kind.
    #[serde(default)]
    pub confidence: f32,
}

/// A detected target content type with a match confidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentTypeSuggestion {
    pub uid: String,
    pub display_name: String,
    /// 0..=1 match confidence.
    pub confidence: f32,
    pub matched_fields: Vec<String>,
}

/// Result of analyzing one dataset (a file may contain several datasets).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeFileResponse {
    pub filename: String,
    /// Dataset name within the file (e.g. a top-level key, or the file stem).
    pub dataset: String,
    pub format: DataFormat,
    pub record_count: usize,
    /// Preview records (capped) used for the mapping UI.
    pub preview: Vec<Value>,
    pub schema: Vec<InferredField>,
    /// Suggested field mappings against the best detected content type.
    pub suggested_mapping: Vec<MappingDto>,
    /// Best guess target content type (may be low confidence / None).
    pub detected_content_type: Option<ContentTypeSuggestion>,
    /// All candidates ranked by confidence.
    pub candidates: Vec<ContentTypeSuggestion>,
}

/// Request: analyze one or more files.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeRequest {
    pub files: Vec<FilePayload>,
}

/// Response: a flat list of dataset analyses (one per dataset across all files).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeResponse {
    pub datasets: Vec<AnalyzeFileResponse>,
}

/// A raw uploaded file (content as UTF-8 text).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePayload {
    pub filename: String,
    pub content: String,
}

/// One source field mapping to a target field.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingDto {
    pub source_field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_field: Option<String>,
    pub transform: TransformKind,
    pub status: MappingStatus,
    #[serde(default)]
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MappingStatus {
    AutoMapped,
    NeedsAttention,
    Ignored,
    Invalid,
}

/// Value transformation applied to a mapped source field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransformKind {
    None,
    Number,
    Boolean,
    Date,
    Trim,
    Lowercase,
    Uppercase,
    EmptyToNull,
    /// Replace all occurrences of `from` with `to`.
    Replace {
        from: String,
        to: String,
    },
    /// Split a string on a separator into an array.
    Split(String),
    /// Join an array with a separator into a string.
    Join(String),
    /// Apply a fixed default when the value is null/empty.
    Default(String),
    /// Parse a JSON string value into structured data.
    ParseJson,
    /// Convert a string to a URL slug.
    Slug,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportMode {
    CreateOnly,
    UpdateOnly,
    Upsert,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportState {
    Draft,
    Published,
    Preserve,
}

/// Configuration for importing one dataset into one content type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileImportConfig {
    pub filename: String,
    /// Dataset name within the file (for JSON/YAML objects with several keys).
    #[serde(default = "default_dataset")]
    pub dataset: String,
    /// Raw file content; re-parsed server-side.
    pub content: String,
    /// Target content type uid.
    pub uid: String,
    pub mapping: Vec<MappingDto>,
    pub mode: ImportMode,
    /// Field used to match existing entries for UpdateOnly / Upsert.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_field: Option<String>,
    /// Optional source field holding the publication state (when importing
    /// multiple states). If absent, `import_state` applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_field: Option<String>,
    pub import_state: ImportState,
    /// Optional source field holding the locale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale_field: Option<String>,
    pub locale: String,
}

fn default_dataset() -> String {
    "data".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub files: Vec<FileImportConfig>,
}

/// Per-content-type import summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub uid: String,
    pub display_name: String,
    pub total: usize,
    pub valid: usize,
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub warnings: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportErrorDto {
    pub file: String,
    pub row: usize,
    pub source: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_field: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResponse {
    pub completed: bool,
    pub summaries: Vec<ImportSummary>,
    pub errors: Vec<ImportErrorDto>,
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Export configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub uids: Vec<String>,
    pub format: DataFormat,
    /// Flat field projection (names present in the entry JSON). Empty = all.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Optional additional JSON filters (subset of QueryParams-style filters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<Value>,
    /// Pagination limit (default all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResponse {
    pub filename: String,
    pub format: DataFormat,
    pub content: String,
    pub counts: Vec<ExportCount>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCount {
    pub uid: String,
    pub display_name: String,
    pub count: usize,
}

/// A saved field-mapping preset.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingPreset {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub name: String,
    pub source_uid: String,
    pub target_uid: String,
    pub mapping: Vec<MappingDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingPresetUpsert {
    pub name: String,
    pub source_uid: String,
    pub target_uid: String,
    pub mapping: Vec<MappingDto>,
}
