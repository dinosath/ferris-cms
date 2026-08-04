//! Lock-free schema cache (design Part II §7).
//!
//! Holds the last-applied set of `core_schema::Schema` behind `arc-swap`
//! so every request path can read the registry without contention.
//! Rebuilt atomically after every Content-Type Builder Save.

use arc_swap::ArcSwap;
use core_domain::{ContentTypeKind, Uid};
use core_schema::Schema;
use std::collections::HashMap;
use std::sync::Arc;

/// The authoritative in-memory registry of all content-type schemas.
///
/// Wrapped in `Arc<ArcSwap>` so that clones of a `SchemaCache` *share the same
/// `ArcSwap` handle*. This is essential: a per-request `AppContext` clone must
/// observe `replace()` calls made by a Content-Type Builder Save on the
/// server-wide context, and vice-versa.
pub struct SchemaCache {
    inner: Arc<ArcSwap<SchemaCacheInner>>,
}

impl Clone for SchemaCache {
    fn clone(&self) -> Self {
        // Cheap `Arc` bump on the shared handle: clones observe each other's
        // `replace()` calls, which is what we want for the schema registry.
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct SchemaCacheInner {
    /// All schemas, ordered by uid for deterministic iteration.
    schemas: Vec<Schema>,
    /// uid → index into `schemas`.
    by_uid: HashMap<Uid, usize>,
}

impl SchemaCache {
    /// Create an empty cache (used before first load).
    pub fn empty() -> Self {
        Self {
            inner: Arc::new(ArcSwap::from(Arc::new(SchemaCacheInner {
                schemas: Vec::new(),
                by_uid: HashMap::new(),
            }))),
        }
    }

    /// Replace the entire cache atomically.
    pub fn replace(&self, schemas: Vec<Schema>) {
        let mut by_uid = HashMap::with_capacity(schemas.len());
        for (i, s) in schemas.iter().enumerate() {
            by_uid.insert(s.uid.clone(), i);
        }
        self.inner
            .store(Arc::new(SchemaCacheInner { schemas, by_uid }));
    }

    /// Get the current schema snapshot (cheap `Arc::clone`).
    pub fn get_all(&self) -> Vec<Schema> {
        self.inner.load().schemas.clone()
    }

    /// Look up a schema by uid.
    pub fn get(&self, uid: &Uid) -> Option<Schema> {
        let inner = self.inner.load();
        inner
            .by_uid
            .get(uid)
            .map(|&i| inner.schemas[i].clone())
    }

    /// All collection types (for content-manager nav).
    pub fn collection_types(&self) -> Vec<Schema> {
        self.inner
            .load()
            .schemas
            .iter()
            .filter(|s| s.kind == ContentTypeKind::CollectionType)
            .cloned()
            .collect()
    }

    /// All single types.
    pub fn single_types(&self) -> Vec<Schema> {
        self.inner
            .load()
            .schemas
            .iter()
            .filter(|s| s.kind == ContentTypeKind::SingleType)
            .cloned()
            .collect()
    }

    /// All components, grouped by category.
    pub fn components(&self) -> Vec<Schema> {
        self.inner
            .load()
            .schemas
            .iter()
            .filter(|s| s.kind == ContentTypeKind::Component)
            .cloned()
            .collect()
    }
}
