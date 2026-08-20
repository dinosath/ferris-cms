//! Node registry and node definitions (node-definition layer).
//!
//! A node definition is *pure metadata* describing a node type's identity,
//! category, description, icon, configuration schema, ports and credential
//! requirements. It never runs code. The actual runtime implementation for each
//! node type lives in `services` (`workflow` submodule), keyed by
//! `NodeDefinition::node_type`. This separation means new node types can be
//! added by registering a definition + an executor without touching the visual
//! editor or the core engine.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Node categories shown in the node library sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeCategory {
    Trigger,
    Logic,
    Data,
    Integration,
    Core,
}

impl NodeCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trigger => "Triggers",
            Self::Logic => "Logic",
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
    /// A field whose value is an n8n-style expression template.
    Expression,
    /// A JSON editor value.
    Json,
    /// A multi-select (array of strings).
    MultiSelect,
}

/// One configurable field in a node's properties panel.
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
    /// Select options: `(value, label)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<(String, String)>,
}

/// A named input port.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInput {
    /// Port name, usually `main`. Trigger nodes have no `main` input.
    pub name: String,
    pub label: String,
}

/// A named output port.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeOutput {
    pub name: String,
    pub label: String,
}

/// A credential input this node type accepts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCredentialRequirement {
    /// Credential type key, e.g. `httpHeaderAuth`.
    pub credential_type: String,
    /// Name the node stores its credential under, e.g. `httpApi`.
    pub name: String,
    pub label: String,
}

/// Full static metadata for one node type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinition {
    pub node_type: String,
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
    /// True when this node type is a trigger (no `main` input).
    pub is_trigger: bool,
    /// Version of the node definition (bumped on breaking config changes).
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

impl NodeDefinition {
    /// Look up a field by name.
    pub fn field(&self, name: &str) -> Option<&NodeField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Default parameters for a freshly-created node of this type.
    pub fn default_parameters(&self) -> IndexMap<String, serde_json::Value> {
        let mut map = IndexMap::new();
        for f in &self.fields {
            if let Some(d) = &f.default {
                map.insert(f.name.clone(), d.clone());
            }
        }
        map
    }
}

/// The in-memory registry of all node types.
#[derive(Clone, Debug, Default)]
pub struct NodeRegistry {
    by_type: IndexMap<String, NodeDefinition>,
}

impl NodeRegistry {
    /// Build the default registry with the built-in node library.
    pub fn builtin() -> Self {
        let mut reg = Self::default();
        for def in builtin_definitions() {
            reg.register(def);
        }
        reg
    }

    /// Register (or replace) a node definition.
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
                    || d.category.label().to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn is_trigger(&self, node_type: &str) -> bool {
        self.by_type
            .get(node_type)
            .map(|d| d.is_trigger)
            .unwrap_or(false)
    }
}

/// Number of built-in node types (used by tests).
pub fn builtin_count() -> usize {
    builtin_definitions().len()
}

fn def(
    node_type: &str,
    display_name: &str,
    description: &str,
    icon: &str,
    category: NodeCategory,
    is_trigger: bool,
    fields: Vec<NodeField>,
    inputs: Vec<NodeInput>,
    outputs: Vec<NodeOutput>,
    credentials: Vec<NodeCredentialRequirement>,
) -> NodeDefinition {
    NodeDefinition {
        node_type: node_type.into(),
        display_name: display_name.into(),
        description: description.into(),
        icon: icon.into(),
        category,
        fields,
        inputs,
        outputs,
        credentials,
        is_trigger,
        version: 1,
        documentation: None,
    }
}

fn main_in() -> Vec<NodeInput> {
    vec![NodeInput {
        name: "main".into(),
        label: "Input".into(),
    }]
}

fn main_out() -> Vec<NodeOutput> {
    vec![NodeOutput {
        name: "main".into(),
        label: "Output".into(),
    }]
}

fn no_in() -> Vec<NodeInput> {
    vec![]
}

/// Trigger port: trigger nodes emit on `main`.
fn trigger_out() -> Vec<NodeOutput> {
    vec![NodeOutput {
        name: "main".into(),
        label: "Trigger".into(),
    }]
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

/// Build the full list of built-in node definitions.
pub fn builtin_definitions() -> Vec<NodeDefinition> {
    let mut defs: Vec<NodeDefinition> = Vec::new();

    // -----------------------------------------------------------------
    // Triggers
    // -----------------------------------------------------------------
    defs.push(def(
        "manualTrigger",
        "Manual Trigger",
        "Starts a workflow when a user clicks Execute.",
        "mouse-pointer",
        NodeCategory::Trigger,
        true,
        vec![],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "scheduleTrigger",
        "Schedule",
        "Triggers a workflow on a cron schedule.",
        "clock",
        NodeCategory::Trigger,
        true,
        vec![
            str_field("cronExpression", "Cron Expression", true),
            sel_field("timezone", "Timezone", vec![("UTC", "UTC")], "UTC"),
        ],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "webhookTrigger",
        "Webhook",
        "Triggers a workflow when a webhook URL is called.",
        "webhook",
        NodeCategory::Trigger,
        true,
        vec![
            str_field("path", "Webhook Path", true),
            sel_field(
                "method",
                "Method",
                vec![
                    ("GET", "GET"),
                    ("POST", "POST"),
                    ("PUT", "PUT"),
                    ("PATCH", "PATCH"),
                    ("DELETE", "DELETE"),
                ],
                "POST",
            ),
        ],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "httpTrigger",
        "HTTP Request Trigger",
        "Triggers a workflow on an incoming HTTP request.",
        "globe",
        NodeCategory::Trigger,
        true,
        vec![str_field("path", "HTTP Path", true)],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "contentCreated",
        "Content Created",
        "Triggers when a content-type entry is created.",
        "file-plus",
        NodeCategory::Trigger,
        true,
        vec![str_field("contentType", "Content Type UID", true)],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "contentUpdated",
        "Content Updated",
        "Triggers when a content-type entry is updated.",
        "edit",
        NodeCategory::Trigger,
        true,
        vec![str_field("contentType", "Content Type UID", true)],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "contentPublished",
        "Content Published",
        "Triggers when a content-type entry is published.",
        "check-circle",
        NodeCategory::Trigger,
        true,
        vec![str_field("contentType", "Content Type UID", true)],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "contentDeleted",
        "Content Deleted",
        "Triggers when a content-type entry is deleted.",
        "trash",
        NodeCategory::Trigger,
        true,
        vec![str_field("contentType", "Content Type UID", true)],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "mediaUploaded",
        "Media Uploaded",
        "Triggers when a media file is uploaded.",
        "image",
        NodeCategory::Trigger,
        true,
        vec![],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "userCreated",
        "User Created",
        "Triggers when an admin user is created.",
        "user-plus",
        NodeCategory::Trigger,
        true,
        vec![],
        no_in(),
        trigger_out(),
        vec![],
    ));

    defs.push(def(
        "workflowTrigger",
        "Workflow Trigger",
        "Triggers when another workflow calls this one.",
        "repeat",
        NodeCategory::Trigger,
        true,
        vec![],
        no_in(),
        trigger_out(),
        vec![],
    ));

    // -----------------------------------------------------------------
    // Core / Logic
    // -----------------------------------------------------------------
    defs.push(def(
        "noop",
        "No-op",
        "Does nothing; passes items through unchanged.",
        "circle",
        NodeCategory::Core,
        false,
        vec![],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "set",
        "Set",
        "Set fields on items.",
        "edit-3",
        NodeCategory::Core,
        false,
        vec![expr_field("field", "Field name", true), expr_field("value", "Value", true)],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "editFields",
        "Edit Fields",
        "Set or remove fields on items.",
        "sliders",
        NodeCategory::Core,
        false,
        vec![
            str_field("field", "Field name", true),
            expr_field("value", "Value", false),
            sel_field("operation", "Operation", vec![("set", "Set"), ("delete", "Delete")], "set"),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "transform",
        "Transform",
        "Map input items to new shapes via an expression.",
        "shuffle",
        NodeCategory::Core,
        false,
        vec![expr_field("transformExpression", "Transform expression", true)],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "code",
        "Code",
        "Run JavaScript-ish transform code over items.",
        "code",
        NodeCategory::Core,
        false,
        vec![
            str_field("language", "Language", false),
            NodeField {
                name: "code".into(),
                label: "Code".into(),
                field_type: FieldType::Text,
                description: None,
                placeholder: Some("// return the item's json to keep it\nreturn item.json;".into()),
                default: Some(serde_json::json!("return item.json;")),
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    // -----------------------------------------------------------------
    // Logic
    // -----------------------------------------------------------------
    defs.push(def(
        "if",
        "If",
        "Routes items to one of two branches based on a condition.",
        "git-branch",
        NodeCategory::Logic,
        false,
        vec![
            expr_field("condition", "Condition", false),
            expr_field("value1", "Value 1", false),
            sel_field(
                "operator",
                "Operator",
                vec![
                    ("==", "Equal"),
                    ("!=", "Not Equal"),
                    (">", "Greater than"),
                    (">=", "Greater or equal"),
                    ("<", "Less than"),
                    ("<=", "Less or equal"),
                    ("contains", "Contains"),
                    ("true", "Is True"),
                    ("false", "Is False"),
                ],
                "==",
            ),
        ],
        main_in(),
        vec![
            NodeOutput { name: "true".into(), label: "True".into() },
            NodeOutput { name: "false".into(), label: "False".into() },
        ],
        vec![],
    ));

    defs.push(def(
        "switch",
        "Switch",
        "Routes items to multiple branches by matching a value.",
        "share-2",
        NodeCategory::Logic,
        false,
        vec![
            expr_field("value", "Value", true),
            NodeField {
                name: "cases".into(),
                label: "Cases".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!(["case1", "case2"])),
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        vec![
            NodeOutput { name: "0".into(), label: "Case 1".into() },
            NodeOutput { name: "1".into(), label: "Case 2".into() },
        ],
        vec![],
    ));

    defs.push(def(
        "merge",
        "Merge",
        "Combine items from two inputs.",
        "git-merge",
        NodeCategory::Logic,
        false,
        vec![sel_field(
            "mode",
            "Mode",
            vec![("append", "Append"), ("combine", "Combine by index"), ("zip", "Zip")],
            "append",
        )],
        vec![
            NodeInput { name: "input1".into(), label: "Input 1".into() },
            NodeInput { name: "input2".into(), label: "Input 2".into() },
        ],
        main_out(),
        vec![],
    ));

    defs.push(def(
        "split",
        "Split",
        "Split an array field into individual items.",
        "scissors",
        NodeCategory::Logic,
        false,
        vec![expr_field("field", "Array field", true)],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "loop",
        "Loop",
        "Repeat a branch a set number of times.",
        "repeat",
        NodeCategory::Logic,
        false,
        vec![
            expr_field("count", "Loop count", false),
            NodeField {
                name: "loopOver".into(),
                label: "Loop over field (array)".into(),
                field_type: FieldType::Expression,
                description: None,
                placeholder: None,
                default: None,
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "forEach",
        "For Each",
        "Execute a branch for each item in the input.",
        "list",
        NodeCategory::Logic,
        false,
        vec![],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "filter",
        "Filter",
        "Keep only items that match a condition.",
        "filter",
        NodeCategory::Logic,
        false,
        vec![expr_field("condition", "Condition", true)],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "sort",
        "Sort",
        "Sort items by a field.",
        "arrow-down-narrow-wide",
        NodeCategory::Logic,
        false,
        vec![
            expr_field("field", "Sort field", true),
            sel_field("order", "Order", vec![("asc", "Ascending"), ("desc", "Descending")], "asc"),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "limit",
        "Limit",
        "Limit the number of items passed through.",
        "frame",
        NodeCategory::Logic,
        false,
        vec![NodeField {
            name: "limit".into(),
            label: "Limit".into(),
            field_type: FieldType::Number,
            description: None,
            placeholder: None,
            default: Some(serde_json::json!(10)),
            required: true,
            options: vec![],
        }],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "wait",
        "Wait / Delay",
        "Pause the workflow for a duration.",
        "hourglass",
        NodeCategory::Logic,
        false,
        vec![NodeField {
            name: "amount".into(),
            label: "Amount".into(),
            field_type: FieldType::Number,
            description: None,
            placeholder: None,
            default: Some(serde_json::json!(1)),
            required: true,
            options: vec![],
        }, sel_field(
            "unit",
            "Unit",
            vec![
                ("seconds", "Seconds"),
                ("minutes", "Minutes"),
                ("hours", "Hours"),
            ],
            "seconds",
        )],
        main_in(),
        main_out(),
        vec![],
    ));

    // -----------------------------------------------------------------
    // Data (CMS)
    // -----------------------------------------------------------------
    defs.push(def(
        "getContent",
        "Get Content",
        "Fetch a content-type entry by document id.",
        "file-text",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            expr_field("documentId", "Document ID", true),
            sel_field("status", "Status", vec![("draft", "Draft"), ("published", "Published")], "draft"),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "findContent",
        "Find Content",
        "Query content-type entries with filters.",
        "search",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            NodeField {
                name: "filters".into(),
                label: "Filters (JSON)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!({})),
                required: false,
                options: vec![],
            },
            expr_field("limit", "Limit", false),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "queryContent",
        "Query Content",
        "Run a rich query against a content-type.",
        "database",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            NodeField {
                name: "query".into(),
                label: "Query (JSON)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!({})),
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "createContent",
        "Create Content",
        "Create a content-type entry.",
        "file-plus",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            NodeField {
                name: "data".into(),
                label: "Entry data (JSON / expression)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!({})),
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "updateContent",
        "Update Content",
        "Update a content-type entry by document id.",
        "edit",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            expr_field("documentId", "Document ID", true),
            NodeField {
                name: "data".into(),
                label: "Entry data (JSON / expression)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!({})),
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "deleteContent",
        "Delete Content",
        "Delete a content-type entry by document id.",
        "trash",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            expr_field("documentId", "Document ID", true),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "publishContent",
        "Publish Content",
        "Publish a draft content-type entry.",
        "check-circle",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            expr_field("documentId", "Document ID", true),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "unpublishContent",
        "Unpublish Content",
        "Unpublish a published content-type entry.",
        "x-circle",
        NodeCategory::Data,
        false,
        vec![
            str_field("contentType", "Content Type UID", true),
            expr_field("documentId", "Document ID", true),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "getMedia",
        "Get Media",
        "Fetch a media file by id.",
        "image",
        NodeCategory::Data,
        false,
        vec![expr_field("id", "Media ID", true)],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "uploadMedia",
        "Upload Media",
        "Upload a media file from binary data or a URL.",
        "upload",
        NodeCategory::Data,
        false,
        vec![
            str_field("filename", "File name", true),
            expr_field("data", "File data (base64)", false),
            expr_field("url", "File URL", false),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "transformData",
        "Transform Data",
        "Convert between JSON and CSV.",
        "repeat",
        NodeCategory::Data,
        false,
        vec![
            sel_field("direction", "Direction", vec![("jsonToCsv", "JSON → CSV"), ("csvToJson", "CSV → JSON")], "jsonToCsv"),
            expr_field("csvData", "CSV data", false),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "jsonNode",
        "JSON",
        "Output static or dynamic JSON.",
        "braces",
        NodeCategory::Data,
        false,
        vec![NodeField {
            name: "json".into(),
            label: "JSON".into(),
            field_type: FieldType::Json,
            description: None,
            placeholder: None,
            default: Some(serde_json::json!({})),
            required: false,
            options: vec![],
        }],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "csvNode",
        "CSV",
        "Output CSV text.",
        "table",
        NodeCategory::Data,
        false,
        vec![str_field("csv", "CSV text", false)],
        main_in(),
        main_out(),
        vec![],
    ));

    // -----------------------------------------------------------------
    // Integrations
    // -----------------------------------------------------------------
    defs.push(def(
        "httpRequest",
        "HTTP Request",
        "Make an HTTP request.",
        "globe",
        NodeCategory::Integration,
        false,
        vec![
            str_field("method", "Method", true),
            expr_field("url", "URL", true),
            NodeField {
                name: "headers".into(),
                label: "Headers (JSON)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!({})),
                required: false,
                options: vec![],
            },
            NodeField {
                name: "body".into(),
                label: "Body (JSON / expression)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: None,
                required: false,
                options: vec![],
            },
            sel_field("authentication", "Authentication", vec![("none", "None"), ("predefined", "Predefined credential")], "none"),
        ],
        main_in(),
        main_out(),
        vec![NodeCredentialRequirement {
            credential_type: "httpApi".into(),
            name: "httpApi".into(),
            label: "HTTP Request API".into(),
        }],
    ));

    defs.push(def(
        "webhook",
        "Webhook",
        "Call an external webhook URL.",
        "webhook",
        NodeCategory::Integration,
        false,
        vec![expr_field("url", "URL", true), NodeField {
            name: "body".into(),
            label: "Body (JSON / expression)".into(),
            field_type: FieldType::Json,
            description: None,
            placeholder: None,
            default: Some(serde_json::json!({})),
            required: false,
            options: vec![],
        }],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "graphqlRequest",
        "GraphQL Request",
        "Send a GraphQL query or mutation.",
        "git-commit",
        NodeCategory::Integration,
        false,
        vec![
            expr_field("url", "GraphQL endpoint", true),
            str_field("query", "Query", true),
            NodeField {
                name: "variables".into(),
                label: "Variables (JSON)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!({})),
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "restApi",
        "REST API",
        "Call a REST API endpoint with full control.",
        "link",
        NodeCategory::Integration,
        false,
        vec![
            expr_field("url", "URL", true),
            str_field("method", "Method", true),
            NodeField {
                name: "headers".into(),
                label: "Headers (JSON)".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: Some(serde_json::json!({})),
                required: false,
                options: vec![],
            },
            NodeField {
                name: "body".into(),
                label: "Body".into(),
                field_type: FieldType::Json,
                description: None,
                placeholder: None,
                default: None,
                required: false,
                options: vec![],
            },
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "databaseQuery",
        "Database Query",
        "Run a SQL query against the configured database.",
        "database",
        NodeCategory::Integration,
        false,
        vec![
            str_field("query", "SQL query", true),
            sel_field("database", "Database", vec![("default", "Default (CMS)"), ("postgres", "PostgreSQL")], "default"),
        ],
        main_in(),
        main_out(),
        vec![],
    ));

    defs.push(def(
        "postgres",
        "PostgreSQL",
        "Run a query against a PostgreSQL database.",
        "server",
        NodeCategory::Integration,
        false,
        vec![
            str_field("query", "SQL query", true),
        ],
        main_in(),
        main_out(),
        vec![NodeCredentialRequirement {
            credential_type: "postgres".into(),
            name: "postgres".into(),
            label: "PostgreSQL".into(),
        }],
    ));

    defs.push(def(
        "redis",
        "Redis",
        "Read/write Redis keys.",
        "zap",
        NodeCategory::Integration,
        false,
        vec![
            sel_field("operation", "Operation", vec![("get", "Get"), ("set", "Set"), ("del", "Delete")], "get"),
            expr_field("key", "Key", true),
            expr_field("value", "Value", false),
        ],
        main_in(),
        main_out(),
        vec![NodeCredentialRequirement {
            credential_type: "redis".into(),
            name: "redis".into(),
            label: "Redis".into(),
        }],
    ));

    defs
}

/// Build a cached static registry (shared, immutable).
pub static REGISTRY: LazyLock<NodeRegistry> = LazyLock::new(NodeRegistry::builtin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_builtin_invariants() {
        let reg = NodeRegistry::builtin();
        // Trigger and non-trigger types present and consistent.
        assert!(reg.get("manualTrigger").unwrap().is_trigger);
        assert!(!reg.get("httpRequest").unwrap().is_trigger);
        assert!(reg.is_trigger("contentPublished"));
        assert!(!reg.is_trigger("if"));

        // Every node definition has at least one output.
        for d in reg.all() {
            assert!(!d.outputs.is_empty(), "{} has no outputs", d.node_type);
            // A trigger must not declare a `main` input.
            if d.is_trigger {
                assert!(
                    !d.inputs.iter().any(|i| i.name == "main"),
                    "trigger {} has a main input",
                    d.node_type
                );
            }
        }

        // Searching works.
        assert_eq!(reg.search("HTTP").len(), 2, "HTTP Request + HTTP trigger");
        assert_eq!(reg.search("").len(), reg.all().len());
        assert_eq!(reg.search("zzz-nothing").len(), 0);

        // Categories partition the registry.
        let total: usize = [
            NodeCategory::Trigger,
            NodeCategory::Logic,
            NodeCategory::Data,
            NodeCategory::Integration,
            NodeCategory::Core,
        ]
        .iter()
        .map(|c| reg.by_category(*c).len())
        .sum();
        assert_eq!(total, reg.all().len());

        // Default params respect the schema defaults.
        let limit = reg.get("limit").unwrap();
        let params = limit.default_parameters();
        assert_eq!(params["limit"], 10);
    }

    #[test]
    fn registry_serializes() {
        let reg = NodeRegistry::builtin();
        let sample = reg.get("if").unwrap();
        let v = serde_json::to_value(sample).unwrap();
        assert_eq!(v["nodeType"], "if");
        assert_eq!(v["outputs"][0]["name"], "true");
        let back: NodeDefinition = serde_json::from_value(v).unwrap();
        assert_eq!(back.node_type, "if");
    }
}
