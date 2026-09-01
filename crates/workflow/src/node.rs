//! Function/task catalog (OWS editor metadata).
//!
//! The OWS document is defined by the SDK's `WorkflowDefinition`. This module
//! provides the *metadata* used by the editor's palette and the API's node
//! library: a `NodeDefinition` describes an OWS **function** (a `call` target
//! with a config schema) or a **task template** (`set`, `switch`, `wait`, ...).
//! It never runs code; the runtime implementations live in `services`, keyed by
//! the OWS function names in `crate::model::function`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Categories shown in the editor palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeCategory {
    Trigger,
    Flow,
    Data,
    Integration,
    Core,
}

impl NodeCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trigger => "Triggers",
            Self::Flow => "Flow",
            Self::Data => "Data",
            Self::Integration => "Integrations",
            Self::Core => "Core",
        }
    }
}

/// The data type of a configuration field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldType {
    String,
    Text,
    Number,
    Boolean,
    Select,
    Expression,
    Json,
    MultiSelect,
}

/// One configurable field in a task/function's inspector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeField {
    pub name: String,
    pub label: String,
    pub field_type: FieldType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<(String, String)>,
}

/// Full static metadata for one editor entry (function or task template).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinition {
    /// OWS function name (e.g. `http.request`) or task type (e.g. `set`).
    pub node_type: String,
    /// `function` | `task` | `trigger`.
    pub kind: String,
    pub display_name: String,
    pub description: String,
    pub icon: String,
    pub category: NodeCategory,
    #[serde(default)]
    pub fields: Vec<NodeField>,
    #[serde(default)]
    pub inputs: Vec<NodeInput>,
    #[serde(default)]
    pub outputs: Vec<NodeOutput>,
    #[serde(default)]
    pub credentials: Vec<NodeCredentialRequirement>,
    pub version: u32,
}

/// A named input port (informational for the editor).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInput {
    pub name: String,
    pub label: String,
}

/// A named output port (informational for the editor).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeOutput {
    pub name: String,
    pub label: String,
}

/// A credential input this function accepts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCredentialRequirement {
    pub credential_type: String,
    pub name: String,
    pub label: String,
}

/// The in-memory catalog of all editor entries.
#[derive(Clone, Debug, Default)]
pub struct NodeRegistry {
    by_type: IndexMap<String, NodeDefinition>,
}

impl NodeRegistry {
    pub fn builtin() -> Self {
        let mut reg = Self::default();
        for def in builtin_definitions() {
            reg.register(def);
        }
        reg
    }

    pub fn register(&mut self, def: NodeDefinition) {
        self.by_type.insert(def.node_type.clone(), def);
    }

    pub fn get(&self, node_type: &str) -> Option<&NodeDefinition> {
        self.by_type.get(node_type)
    }

    pub fn all(&self) -> Vec<&NodeDefinition> {
        self.by_type.values().collect()
    }

    pub fn by_category(&self, category: NodeCategory) -> Vec<&NodeDefinition> {
        self.by_type
            .values()
            .filter(|d| d.category == category)
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<&NodeDefinition> {
        let q = query.to_lowercase();
        if q.is_empty() {
            return self.all();
        }
        self.by_type
            .values()
            .filter(|d| {
                d.display_name.to_lowercase().contains(&q)
                    || d.node_type.to_lowercase().contains(&q)
                    || d.description.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Whether this is a trigger entry.
    pub fn is_trigger(&self, node_type: &str) -> bool {
        self.by_type
            .get(node_type)
            .map(|d| d.kind == "trigger")
            .unwrap_or(false)
    }
}

/// Number of built-in entries (used by tests).
pub fn builtin_count() -> usize {
    builtin_definitions().len()
}

fn def(
    node_type: &str,
    kind: &str,
    display_name: &str,
    description: &str,
    icon: &str,
    category: NodeCategory,
    fields: Vec<NodeField>,
    credentials: Vec<&'static str>,
) -> NodeDefinition {
    NodeDefinition {
        node_type: node_type.into(),
        kind: kind.into(),
        display_name: display_name.into(),
        description: description.into(),
        icon: icon.into(),
        category,
        fields,
        inputs: vec![NodeInput { name: "input".into(), label: "Input".into() }],
        outputs: vec![NodeOutput { name: "output".into(), label: "Output".into() }],
        credentials: credentials
            .into_iter()
            .map(|ct| NodeCredentialRequirement {
                credential_type: ct.into(),
                name: ct.into(),
                label: ct.into(),
            })
            .collect(),
        version: 1,
    }
}

fn str_field(name: &str, label: &str, required: bool) -> NodeField {
    NodeField {
        name: name.into(),
        label: label.into(),
        field_type: FieldType::String,
        description: None,
        placeholder: None,
        default: None,
        required,
        options: vec![],
    }
}

fn expr_field(name: &str, label: &str, required: bool) -> NodeField {
    NodeField {
        name: name.into(),
        label: label.into(),
        field_type: FieldType::Expression,
        description: None,
        placeholder: None,
        default: None,
        required,
        options: vec![],
    }
}

fn json_field(name: &str, label: &str) -> NodeField {
    NodeField {
        name: name.into(),
        label: label.into(),
        field_type: FieldType::Json,
        description: None,
        placeholder: None,
        default: Some(serde_json::json!({})),
        required: false,
        options: vec![],
    }
}

fn sel_field(name: &str, label: &str, options: Vec<(&str, &str)>, default: &str) -> NodeField {
    NodeField {
        name: name.into(),
        label: label.into(),
        field_type: FieldType::Select,
        description: None,
        placeholder: None,
        default: Some(serde_json::json!(default)),
        required: false,
        options: options
            .into_iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect(),
    }
}

/// Build the full list of editor catalog entries.
pub fn builtin_definitions() -> Vec<NodeDefinition> {
    use crate::model::function::*;
    use NodeCategory as C;
    let mut defs: Vec<NodeDefinition> = Vec::new();

    // ---- Flow task templates ----
    defs.push(def("set", "task", "Set", "Set data on the workflow context.", "edit-3", C::Core, vec![json_field("set", "Data to set")], vec![]));
    defs.push(def("switch", "task", "Switch", "Route based on conditions.", "git-branch", C::Flow, vec![expr_field("when", "Condition", false)], vec![]));
    defs.push(def("wait", "task", "Wait", "Pause for a duration.", "hourglass", C::Flow, vec![str_field("duration", "Duration", false)], vec![]));
    defs.push(def("for", "task", "For Each", "Iterate over a collection.", "list", C::Flow, vec![str_field("in", "Expression", true)], vec![]));
    defs.push(def("try", "task", "Try / Catch", "Handle errors gracefully.", "shield", C::Flow, vec![], vec![]));
    defs.push(def("do", "task", "Do (sequence)", "Run subtasks in sequence.", "layers", C::Flow, vec![], vec![]));

    // ---- CMS data functions ----
    defs.push(def(GET_CONTENT, "function", "Get Content", "Fetch a content-type entry by document id.", "file-text", C::Data, vec![str_field("contentType", "Content Type UID", true), expr_field("documentId", "Document ID", true)], vec![]));
    defs.push(def(FIND_CONTENT, "function", "Find Content", "Query content-type entries with filters.", "search", C::Data, vec![str_field("contentType", "Content Type UID", true), json_field("filters", "Filters")], vec![]));
    defs.push(def(CREATE_CONTENT, "function", "Create Content", "Create a content-type entry.", "file-plus", C::Data, vec![str_field("contentType", "Content Type UID", true), json_field("data", "Entry data")], vec![]));
    defs.push(def(UPDATE_CONTENT, "function", "Update Content", "Update a content-type entry by document id.", "edit", C::Data, vec![str_field("contentType", "Content Type UID", true), expr_field("documentId", "Document ID", true), json_field("data", "Entry data")], vec![]));
    defs.push(def(DELETE_CONTENT, "function", "Delete Content", "Delete a content-type entry by document id.", "trash", C::Data, vec![str_field("contentType", "Content Type UID", true), expr_field("documentId", "Document ID", true)], vec![]));
    defs.push(def(PUBLISH_CONTENT, "function", "Publish Content", "Publish a draft content-type entry.", "check-circle", C::Data, vec![str_field("contentType", "Content Type UID", true), expr_field("documentId", "Document ID", true)], vec![]));
    defs.push(def(UNPUBLISH_CONTENT, "function", "Unpublish Content", "Unpublish a published content-type entry.", "x-circle", C::Data, vec![str_field("contentType", "Content Type UID", true), expr_field("documentId", "Document ID", true)], vec![]));
    defs.push(def(GET_MEDIA, "function", "Get Media", "Fetch a media file by id.", "image", C::Data, vec![expr_field("id", "Media ID", true)], vec![]));
    defs.push(def(UPLOAD_MEDIA, "function", "Upload Media", "Upload a media file from binary data.", "upload", C::Data, vec![str_field("filename", "File name", true), expr_field("data", "File data (base64)", false)], vec![]));
    defs.push(def(TRANSFORM_DATA, "function", "Transform Data", "Convert between JSON and CSV.", "repeat", C::Data, vec![sel_field("direction", "Direction", vec![("jsonToCsv", "JSON → CSV"), ("csvToJson", "CSV → JSON")], "jsonToCsv")], vec![]));
    defs.push(def(JSON, "function", "JSON", "Output static or dynamic JSON.", "braces", C::Data, vec![json_field("json", "JSON")], vec![]));
    defs.push(def(CSV, "function", "CSV", "Output CSV text.", "table", C::Data, vec![str_field("csv", "CSV text", false)], vec![]));

    // ---- Core transforms ----
    defs.push(def(TRANSFORM, "function", "Transform", "Map input items via an expression.", "shuffle", C::Core, vec![expr_field("transformExpression", "Transform expression", true)], vec![]));
    defs.push(def(CODE, "function", "Code", "Run transform code.", "code", C::Core, vec![str_field("code", "Code", false)], vec![]));
    defs.push(def(EDIT_FIELDS, "function", "Edit Fields", "Set or remove fields.", "sliders", C::Core, vec![str_field("field", "Field name", true)], vec![]));

    // ---- Integrations ----
    defs.push(def(HTTP_REQUEST, "function", "HTTP Request", "Make an HTTP request.", "globe", C::Integration, vec![sel_field("method", "Method", vec![("GET", "GET"), ("POST", "POST"), ("PUT", "PUT"), ("PATCH", "PATCH"), ("DELETE", "DELETE")], "GET"), expr_field("url", "URL", true), json_field("headers", "Headers"), json_field("body", "Body")], vec!["httpApi"]));
    defs.push(def(WEBHOOK, "function", "Webhook", "Call an external webhook URL.", "webhook", C::Integration, vec![expr_field("url", "URL", true), json_field("body", "Body")], vec![]));
    defs.push(def(GRAPHQL, "function", "GraphQL Request", "Send a GraphQL query.", "git-commit", C::Integration, vec![expr_field("url", "Endpoint", true), str_field("query", "Query", true)], vec![]));
    defs.push(def(REST_API, "function", "REST API", "Call a REST API endpoint.", "link", C::Integration, vec![expr_field("url", "URL", true), str_field("method", "Method", true)], vec![]));
    defs.push(def(DB_QUERY, "function", "Database Query", "Run a SQL query.", "database", C::Integration, vec![str_field("query", "SQL query", true)], vec![]));
    defs.push(def(POSTGRES, "function", "PostgreSQL", "Run a query against PostgreSQL.", "server", C::Integration, vec![str_field("query", "SQL query", true)], vec!["postgres"]));
    defs.push(def(REDIS, "function", "Redis", "Read/write Redis keys.", "zap", C::Integration, vec![sel_field("operation", "Operation", vec![("get", "Get"), ("set", "Set"), ("del", "Delete")], "get"), expr_field("key", "Key", true)], vec!["redis"]));

    defs
}

/// Build a cached static catalog (shared, immutable).
pub static REGISTRY: LazyLock<NodeRegistry> = LazyLock::new(NodeRegistry::builtin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_invariants() {
        let reg = NodeRegistry::builtin();
        assert!(reg.get("set").unwrap().kind == "task");
        assert!(reg.get("http.request").unwrap().kind == "function");
        assert_eq!(reg.search("").len(), reg.all().len());
        assert!(reg.search("HTTP").iter().any(|d| d.node_type == "http.request"));
        assert!(reg.get("cms.getContent").is_some());
    }
}
