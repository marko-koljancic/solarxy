//! The content-addressed in-memory asset table.
//!
//! Staged bytes (import files, later textures) keyed by their SHA-256 hex
//! digest: file identity is content, never name + mtime + size. The web
//! shell stages bytes once across the boundary; import cook bodies read
//! them through an [`super::cook::CookCtx`] borrow. Duplicate stages of
//! identical content are free.

use std::collections::BTreeMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::params::AssetId;

/// One staged asset.
#[derive(Debug, Clone)]
pub struct AssetEntry {
    /// The original file name (for display and extension sniffing).
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
}

/// The table. Insertions are content-addressed; entries are immutable.
#[derive(Debug, Default, Clone)]
pub struct AssetTable {
    entries: BTreeMap<AssetId, AssetEntry>,
}

impl AssetTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stages bytes, returning their content id. Re-staging identical
    /// content returns the same id without storing twice.
    pub fn stage(&mut self, name: impl Into<String>, bytes: Vec<u8>) -> AssetId {
        let digest = Sha256::digest(&bytes);
        let id = AssetId(format!("{digest:x}"));
        self.entries
            .entry(id.clone())
            .or_insert_with(|| AssetEntry {
                name: name.into(),
                bytes: Arc::new(bytes),
            });
        id
    }

    #[must_use]
    pub fn get(&self, id: &AssetId) -> Option<&AssetEntry> {
        self.entries.get(id)
    }

    /// Iterates staged (id, entry) pairs (sidecar resolution by name).
    pub fn entries(&self) -> impl Iterator<Item = (&AssetId, &AssetEntry)> {
        self.entries.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_is_content_addressed_and_idempotent() {
        let mut table = AssetTable::new();
        let a = table.stage("cube.obj", b"v 0 0 0".to_vec());
        let b = table.stage("copy-of-cube.obj", b"v 0 0 0".to_vec());
        // Same content, same id, first name wins, one entry.
        assert_eq!(a, b);
        assert_eq!(table.len(), 1);
        assert_eq!(table.get(&a).unwrap().name, "cube.obj");

        let c = table.stage("other.obj", b"v 1 1 1".to_vec());
        assert_ne!(a, c);
        assert_eq!(table.len(), 2);
        // The id is the SHA-256 hex digest.
        assert_eq!(a.0.len(), 64);
    }
}
