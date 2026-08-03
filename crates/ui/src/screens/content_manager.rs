//! Content Manager screen state (design Part VII §6).
//!
//! Layout: `[CM nav 240][view Fill]`.

use serde::{Deserialize, Serialize};

/// CM screen state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CmScreen {
    /// List of content-type uids available in CM.
    pub content_types: Vec<CmNavItem>,
    /// Currently selected content-type uid.
    pub selected_uid: Option<String>,
    /// List view state.
    pub list_view: CmListState,
    /// Edit view state.
    pub edit_view: Option<CmEditState>,
    /// Which view mode.
    pub mode: CmMode,
}

/// Content-Manager navigation item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CmNavItem {
    pub uid: String,
    pub display_name: String,
    pub kind: String,
    pub entry_count: i64,
}

/// List view state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CmListState {
    pub entries: Vec<serde_json::Value>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub search: String,
    pub selected_entry_ids: Vec<String>,
    pub loading: bool,
}

/// Edit view state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CmEditState {
    pub document_id: String,
    pub data: serde_json::Value,
    pub is_new: bool,
    pub saving: bool,
    pub errors: Vec<String>,
}

/// CM view mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmMode {
    #[default]
    List,
    Edit,
}
