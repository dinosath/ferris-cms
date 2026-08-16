//! Base widget definitions (design Part VI §6).
//!
//! Each widget is defined as a struct/enum with style properties.
//! The actual rendering is done by the binary (web/desktop) using
//! the preferred UI framework. These definitions are framework-agnostic.

use serde::{Deserialize, Serialize};

/// Button variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Danger,
    Success,
    Ghost,
}

/// Button specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ButtonSpec {
    pub label: String,
    pub variant: ButtonVariant,
    pub disabled: bool,
    pub loading: bool,
    /// Width: "fill" | explicit px value.
    pub width: Option<String>,
}

impl ButtonSpec {
    pub fn primary(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Primary,
            disabled: false,
            loading: false,
            width: None,
        }
    }
    pub fn secondary(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Secondary,
            disabled: false,
            loading: false,
            width: None,
        }
    }
    pub fn danger(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Danger,
            disabled: false,
            loading: false,
            width: None,
        }
    }
    pub fn success(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Success,
            disabled: false,
            loading: false,
            width: None,
        }
    }
}

/// Text field specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextFieldSpec {
    pub label: Option<String>,
    pub placeholder: Option<String>,
    pub input_type: String,
    pub value: String,
    pub error: Option<String>,
    pub helper: Option<String>,
    pub required: bool,
    pub disabled: bool,
}

impl Default for TextFieldSpec {
    fn default() -> Self {
        Self {
            label: None,
            placeholder: None,
            input_type: "text".into(),
            value: String::new(),
            error: None,
            helper: None,
            required: false,
            disabled: false,
        }
    }
}

/// Checkbox specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckboxSpec {
    pub label: Option<String>,
    pub checked: bool,
}

/// Toggle specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToggleSpec {
    pub label: Option<String>,
    pub checked: bool,
    pub disabled: bool,
}

/// Dropdown option.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DropdownOption {
    pub value: String,
    pub label: String,
}

/// Dropdown specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DropdownSpec {
    pub label: Option<String>,
    pub options: Vec<DropdownOption>,
    pub value: String,
    pub disabled: bool,
}

/// Badge kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BadgeKind {
    New,
    Modified,
    Deleted,
    Published,
    Draft,
    Neutral,
}

impl BadgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Published => "published",
            Self::Draft => "draft",
            Self::Neutral => "neutral",
        }
    }
}

/// Badge specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BadgeSpec {
    pub text: String,
    pub kind: BadgeKind,
}

/// Table column definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableColumnSpec {
    pub key: String,
    pub label: String,
    pub sortable: bool,
}

/// Table specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableSpec {
    pub columns: Vec<TableColumnSpec>,
    pub rows: Vec<Vec<String>>,
    pub selectable: bool,
}

/// Modal specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModalSpec {
    pub title: Option<String>,
    pub width: u32,
    pub open: bool,
}

/// Card specification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardSpec {
    pub padding: u32,
}

impl Default for CardSpec {
    fn default() -> Self {
        Self { padding: 24 }
    }
}
