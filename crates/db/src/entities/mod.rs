//! System table entities (design Part III), SeaORM 2.0 dense format.

use sea_orm::entity::prelude::*;

/// `content_type_schemas` — the content-type registry (design Part III §2).
/// `schema_json` is authoritative; other columns are denormalized copies.
pub mod content_type_schema {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "content_type_schemas")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub uid: String,
        /// collectionType | singleType | component
        pub kind: String,
        pub category: Option<String>,
        pub display_name: String,
        pub singular_api_id: Option<String>,
        pub plural_api_id: Option<String>,
        /// Canonical schema JSON (core-schema).
        pub schema_json: Json,
        pub draft_and_publish: bool,
        pub i18n_localized: bool,
        pub is_system: bool,
        pub version: i64,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        // ---- sync columns (Part III note; sync engine ships later) ----
        pub sync_version: i64,
        pub origin_node_id: Option<String>,
        pub deleted_at: Option<DateTimeUtc>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// Logical ↔ physical mapping for safe renames (design Part III §2).
pub mod content_type_table_map {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "content_type_table_map")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub schema_uid: String,
        pub physical_table: String,
        pub logical_attr: Option<String>,
        pub physical_column: Option<String>,
        pub join_table: Option<String>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `admin_user` (design Part III §4).
pub mod admin_user {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "admin_user")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub email: String,
        pub first_name: Option<String>,
        pub last_name: Option<String>,
        pub username: Option<String>,
        /// argon2id hash.
        pub password_hash: String,
        pub is_active: bool,
        pub blocked: bool,
        pub prefered_locale: Option<String>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `admin_role` (design Part III §4). Seeded: Super Admin, Editor, Author.
pub mod admin_role {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "admin_role")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub name: String,
        #[sea_orm(unique)]
        pub code: String,
        pub description: Option<String>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `admin_permission` (design Part III §4).
pub mod admin_permission {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "admin_permission")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub role_id: i64,
        pub action: String,
        pub subject: Option<String>,
        pub properties_json: Json,
        pub conditions_json: Json,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `admin_user_role` join (design Part III §4).
pub mod admin_user_role {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "admin_user_role")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub user_id: i64,
        pub role_id: i64,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `api_token` (design Part III §4). Key stored as sha256 hash, shown once.
pub mod api_token {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "api_token")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        /// read-only | full-access | custom
        #[sea_orm(column_name = "type")]
        pub token_type: String,
        pub access_key_hash: String,
        pub last_used_at: Option<DateTimeUtc>,
        pub expires_at: Option<DateTimeUtc>,
        pub lifespan: Option<i64>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `api_token_permission` (design Part III §4).
pub mod api_token_permission {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "api_token_permission")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub token_id: i64,
        pub action: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `upload_file` (design Part III §5).
pub mod upload_file {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "upload_file")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub document_id: String,
        pub name: String,
        pub alternative_text: Option<String>,
        pub caption: Option<String>,
        pub width: Option<i64>,
        pub height: Option<i64>,
        /// thumbnail/small/medium/large variants.
        pub formats_json: Option<Json>,
        pub hash: String,
        pub ext: Option<String>,
        pub mime: String,
        /// KB with 3 decimal places kept as f64 like Strapi.
        pub size: f64,
        pub url: String,
        pub preview_url: Option<String>,
        pub provider: String,
        pub folder_id: Option<i64>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        // ---- sync columns ----
        pub sync_version: i64,
        pub origin_node_id: Option<String>,
        pub deleted_at: Option<DateTimeUtc>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `upload_folder` — nested via materialized path (design Part III §5).
pub mod upload_folder {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "upload_folder")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub path_id: i64,
        /// Materialized path, e.g. `/1/4/`.
        pub path: String,
        pub parent_id: Option<i64>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        // ---- sync columns ----
        pub sync_version: i64,
        pub origin_node_id: Option<String>,
        pub deleted_at: Option<DateTimeUtc>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `i18n_locale` (design Part III §6). Seeds `en`.
pub mod i18n_locale {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "i18n_locale")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub code: String,
        pub name: String,
        pub is_default: bool,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `core_store` — view configs + plugin settings (design Part III §7).
pub mod core_store {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "core_store")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub key: String,
        pub value_json: Option<Json>,
        #[sea_orm(column_name = "type")]
        pub store_type: Option<String>,
        pub environment: Option<String>,
        pub tag: Option<String>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `schema_change_log` — drives sync/audit (design Part III §7).
pub mod schema_change_log {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "schema_change_log")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub schema_uid: String,
        pub from_version: i64,
        pub to_version: i64,
        pub diff_json: Json,
        pub applied_at: DateTimeUtc,
        pub applied_by: Option<String>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `sync_state` (design Part VIII §3). `node_id` is the primary key.
pub mod sync_state {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sync_state")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub node_id: String,
        pub remote_url: Option<String>,
        pub last_pulled_version: i64,
        pub last_pushed_version: i64,
        pub last_synced_at: Option<DateTimeUtc>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `sync_oplog` — append-only, written in the mutation transaction
/// (design Part VIII §3).
pub mod sync_oplog {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sync_oplog")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub entity: String,
        pub document_id: String,
        pub op: String,
        pub sync_version: i64,
        pub payload_json: Option<Json>,
        pub created_at: DateTimeUtc,
        pub pushed: bool,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

// ---------------------------------------------------------------------------
// Workflow automation (n8n-style). Stored as stable, structured JSON so the
// workflow definition (nodes + connections + settings) is versionable,
// importable/exportable and API-accessible — the frontend canvas is never the
// source of truth.
// ---------------------------------------------------------------------------

/// `workflow` — one persisted workflow definition.
pub mod workflow {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "workflow")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub version: i64,
        pub active: bool,
        /// The full `workflow::model::Workflow` JSON (nodes, connections,
        /// settings, variables, tags, timestamps).
        pub definition_json: Json,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        pub created_by: Option<i64>,
        pub updated_by: Option<i64>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `workflow_credential` — a saved credential for integration nodes.
/// The sensitive data is stored encrypted (see the credential service); this
/// table only holds the encrypted blob + metadata, never plaintext.
pub mod workflow_credential {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "workflow_credential")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub credential_type: String,
        /// Encrypted credential payload.
        pub data_encrypted: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `workflow_execution` — a run of a workflow, with its overall status.
pub mod workflow_execution {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "workflow_execution")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub workflow_id: i64,
        pub status: String,
        /// manual | trigger | schedule | webhook
        pub mode: String,
        pub trigger: String,
        pub started_at: DateTimeUtc,
        pub finished_at: Option<DateTimeUtc>,
        pub duration_ms: Option<i64>,
        pub error: Option<String>,
        /// Optional trigger data snapshot for inspection.
        pub data_json: Option<Json>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `workflow_node_run` — one node's execution record within an execution.
pub mod workflow_node_run {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "workflow_node_run")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub execution_id: i64,
        pub node_id: String,
        pub node_name: String,
        pub node_type: String,
        /// notExecuted | running | success | failed | skipped | waiting
        pub status: String,
        pub started_at: Option<DateTimeUtc>,
        pub finished_at: Option<DateTimeUtc>,
        pub duration_ms: Option<i64>,
        pub input_json: Option<Json>,
        pub output_json: Option<Json>,
        pub error: Option<String>,
        pub attempts: i64,
        /// Deterministic topological execution order index.
        pub order: i64,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `import_export_mapping_preset` — saved Import/Export field mappings.
pub mod import_export_mapping_preset {
    use super::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "import_export_mapping_preset")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub source_uid: String,
        pub target_uid: String,
        /// The field mapping (Vec<MappingDto>) as JSON.
        pub mapping_json: Json,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    impl ActiveModelBehavior for ActiveModel {}
}
