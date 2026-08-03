//! Content-Type Builder screen state (design Part VII §5).
//!
//! Layout: `[CTB nav 240][editor Fill]`.

use core_schema::Schema;
use serde::{Deserialize, Serialize};

/// CTB screen state — working-copy model.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CtbScreen {
    /// All schemas loaded from server (last-saved state).
    pub saved_schemas: Vec<Schema>,
    /// Working copy with unsaved changes.
    pub working_schemas: Vec<Schema>,
    /// Selected content-type uid for editing.
    pub selected_uid: Option<String>,
    /// Change stack for undo/redo.
    pub change_stack: Vec<CtbChange>,
    pub redo_stack: Vec<CtbChange>,
    /// Whether a modal is open.
    pub active_modal: Option<CtbModal>,
    /// Validation errors from last save attempt.
    pub errors: Vec<String>,
    /// Pending save.
    pub saving: bool,
}

impl CtbScreen {
    pub fn has_unsaved_changes(&self) -> bool {
        !self.change_stack.is_empty()
    }

    pub fn selected_schema(&self) -> Option<&Schema> {
        self.selected_uid.as_ref().and_then(|uid| {
            self.working_schemas.iter().find(|s| s.uid.as_str() == uid)
        })
    }

    pub fn collection_types(&self) -> Vec<&Schema> {
        self.working_schemas
            .iter()
            .filter(|s| s.kind == core_domain::ContentTypeKind::CollectionType)
            .collect()
    }

    pub fn single_types(&self) -> Vec<&Schema> {
        self.working_schemas
            .iter()
            .filter(|s| s.kind == core_domain::ContentTypeKind::SingleType)
            .collect()
    }

    pub fn components(&self) -> Vec<&Schema> {
        self.working_schemas
            .iter()
            .filter(|s| s.kind == core_domain::ContentTypeKind::Component)
            .collect()
    }
}

/// A single change in the CTB working copy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CtbChange {
    CreateCt { uid: String },
    DeleteCt { uid: String },
    AddField { ct_uid: String, field_name: String },
    EditField { ct_uid: String, field_name: String },
    DeleteField { ct_uid: String, field_name: String },
    ReorderFields { ct_uid: String },
    EditSettings { ct_uid: String },
}

/// Active modal in the CTB screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CtbModal {
    CreateCollectionType,
    CreateSingleType,
    CreateComponent,
    EditSettings { ct_uid: String },
    FieldPicker { ct_uid: String },
    FieldConfig {
        ct_uid: String,
        field_name: Option<String>,
        field_type: core_domain::FieldType,
    },
    ConfirmDelete { ct_uid: String },
    ConfirmDiscard,
}
