//! Schema diffing (design Part IV §8).
//!
//! `diff(current, desired)` describes the change per content-type:
//! added / modified / removed attributes plus table create/drop.
//! `dynamic-store::apply_schema` turns this into DDL.

use crate::model::Schema;
use core_domain::Uid;

/// What happened to one content-type between current and desired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Table does not exist yet -> CREATE TABLE.
    Created,
    /// Table exists; attribute-level changes attached.
    Updated,
    /// Schema removed -> unmap (default, no hard drop).
    Removed,
    /// No changes.
    Unchanged,
}

/// One attribute whose definition changed.
#[derive(Clone, Debug, PartialEq)]
pub struct AttrChange {
    pub name: String,
    pub from: crate::Attribute,
    pub to: crate::Attribute,
    /// Same SQL family -> plain ALTER. Otherwise drop+add semantics
    /// (data retained but detached, logged).
    pub compatible: bool,
}

/// Per-schema diff.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaDiff {
    pub uid: Uid,
    /// Physical table of the desired (or removed) schema.
    pub table: String,
    pub kind: DiffKind,
    /// Added attributes (name + definition), in desired order.
    pub added_attrs: Vec<(String, crate::Attribute)>,
    /// Removed attribute names, in current order.
    pub removed_attrs: Vec<String>,
    /// Changed attributes, in desired order.
    pub changed_attrs: Vec<AttrChange>,
    /// The desired schema (None when Removed).
    pub desired: Option<Schema>,
}

impl SchemaDiff {
    pub fn is_noop(&self) -> bool {
        matches!(self.kind, DiffKind::Unchanged)
            || (matches!(self.kind, DiffKind::Updated)
                && self.added_attrs.is_empty()
                && self.removed_attrs.is_empty()
                && self.changed_attrs.is_empty())
    }
}

/// Diff one schema: `current` = registry state (None = new), `desired` = target.
pub fn diff(current: Option<&Schema>, desired: &Schema) -> SchemaDiff {
    let table = desired.table_name();
    let Some(current) = current else {
        return SchemaDiff {
            uid: desired.uid.clone(),
            table,
            kind: DiffKind::Created,
            added_attrs: desired
                .attributes
                .iter()
                .map(|(n, a)| (n.clone(), a.clone()))
                .collect(),
            removed_attrs: vec![],
            changed_attrs: vec![],
            desired: Some(desired.clone()),
        };
    };

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for (name, attr) in &desired.attributes {
        match current.attributes.get(name) {
            None => added.push((name.clone(), attr.clone())),
            Some(prev) if prev != attr => changed.push(AttrChange {
                name: name.clone(),
                from: prev.clone(),
                to: attr.clone(),
                compatible: prev.sql_family() == attr.sql_family(),
            }),
            _ => {}
        }
    }
    for name in current.attributes.keys() {
        if !desired.attributes.contains_key(name) {
            removed.push(name.clone());
        }
    }

    let structural_flags_changed = current.kind != desired.kind
        || current.draft_and_publish() != desired.draft_and_publish()
        || current.is_localized() != desired.is_localized();

    let kind = if added.is_empty() && removed.is_empty() && changed.is_empty() {
        if structural_flags_changed {
            DiffKind::Updated
        } else {
            DiffKind::Unchanged
        }
    } else {
        DiffKind::Updated
    };

    SchemaDiff {
        uid: desired.uid.clone(),
        table,
        kind,
        added_attrs: added,
        removed_attrs: removed,
        changed_attrs: changed,
        desired: Some(desired.clone()),
    }
}

/// Diff a schema removal (Part IV §8: default unmap, don't hard-drop).
pub fn diff_removed(current: &Schema) -> SchemaDiff {
    SchemaDiff {
        uid: current.uid.clone(),
        table: current.table_name(),
        kind: DiffKind::Removed,
        added_attrs: vec![],
        removed_attrs: current.attributes.keys().cloned().collect(),
        changed_attrs: vec![],
        desired: None,
    }
}

/// Compute diffs for a whole desired set against the current registry.
/// Schemas present in current but missing from desired are `Removed`.
pub fn diff_registry(current: &[Schema], desired: &[Schema]) -> Vec<SchemaDiff> {
    let mut out = Vec::new();
    for d in desired {
        let cur = current.iter().find(|c| c.uid == d.uid);
        out.push(diff(cur, d));
    }
    for c in current {
        if !desired.iter().any(|d| d.uid == c.uid) {
            out.push(diff_removed(c));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attribute, Schema, SchemaInfo};
    use core_domain::{ContentTypeKind, FieldType, Uid};
    use indexmap::IndexMap;

    fn article(attrs: &[(&str, Attribute)]) -> Schema {
        Schema {
            uid: Uid::new("api::article.article"),
            kind: ContentTypeKind::CollectionType,
            collection_name: None,
            info: SchemaInfo {
                singular_name: "article".into(),
                plural_name: "articles".into(),
                display_name: "Article".into(),
                description: None,
                icon: None,
            },
            options: Default::default(),
            plugin_options: None,
            attributes: attrs
                .iter()
                .map(|(n, a)| (n.to_string(), a.clone()))
                .collect::<IndexMap<_, _>>(),
        }
    }

    #[test]
    fn create_diff() {
        let desired = article(&[("title", Attribute::new(FieldType::String))]);
        let d = diff(None, &desired);
        assert_eq!(d.kind, DiffKind::Created);
        assert_eq!(d.table, "ct_articles");
        assert_eq!(d.added_attrs.len(), 1);
    }

    #[test]
    fn update_diffs() {
        let current = article(&[
            ("title", Attribute::new(FieldType::String)),
            ("views", Attribute::new(FieldType::Integer)),
            ("old", Attribute::new(FieldType::Text)),
        ]);
        let mut title2 = Attribute::new(FieldType::String);
        title2.required = true;
        let mut views2 = Attribute::new(FieldType::Biginteger);
        views2.default = Some(serde_json::json!(0));
        let desired = article(&[
            ("title", title2),
            ("views", views2),
            ("body", Attribute::new(FieldType::Blocks)),
        ]);
        let d = diff(Some(&current), &desired);
        assert_eq!(d.kind, DiffKind::Updated);
        assert_eq!(d.added_attrs[0].0, "body");
        assert_eq!(d.removed_attrs, vec!["old".to_string()]);
        assert_eq!(d.changed_attrs.len(), 2);
        // title: string -> string (required flag) compatible
        assert!(
            d.changed_attrs
                .iter()
                .find(|c| c.name == "title")
                .unwrap()
                .compatible
        );
        // views: integer -> biginteger is a family change
        assert!(
            !d.changed_attrs
                .iter()
                .find(|c| c.name == "views")
                .unwrap()
                .compatible
        );
    }

    #[test]
    fn unchanged_and_removed() {
        let s = article(&[("title", Attribute::new(FieldType::String))]);
        assert!(diff(Some(&s), &s).is_noop());
        let d = diff_registry(std::slice::from_ref(&s), &[]);
        assert_eq!(d[0].kind, DiffKind::Removed);
    }
}
