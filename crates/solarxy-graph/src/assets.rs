//! The content-addressed in-memory asset table.
//!
//! Staged bytes (import files, later textures) keyed by their SHA-256 hex
//! digest: file identity is content, never name + mtime + size. The web
//! shell stages bytes once across the boundary; import cook bodies read
//! them through an [`super::cook::CookCtx`] borrow. Duplicate stages of
//! identical content are free.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::params::AssetId;

/// One staged asset.
#[derive(Debug, Clone)]
pub struct AssetEntry {
    /// The first-seen file name (for display and extension sniffing).
    pub name: String,
    /// Every OTHER name the same bytes have been staged under.
    ///
    /// Identity is the content hash, so two files with identical bytes are one
    /// entry. Keeping only the first name would lose the second: a model
    /// referencing it by name would then be told its companion is missing even
    /// though the bytes are staged. That is plausible in the wild whenever a
    /// model ships the same texture twice under different filenames.
    pub aliases: BTreeSet<String>,
    /// The MIME type reported at stage time (recorded in the `.slxy`
    /// manifest; may be empty when the source did not provide one).
    pub mime: String,
    pub bytes: Arc<Vec<u8>>,
}

impl AssetEntry {
    /// Every name these bytes are known by, first-seen first.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.aliases.iter().map(String::as_str))
    }

    /// Whether any of this entry's names matches `wanted`, compared by the
    /// trailing path component (the resolver's matching key).
    #[must_use]
    pub fn has_name(&self, wanted: &str) -> bool {
        let wanted = basename(wanted);
        self.names().any(|n| basename(n) == wanted)
    }
}

/// The trailing file-name component.
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
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

    /// Stages bytes, returning their content id. Re-staging identical content
    /// returns the same id without storing the bytes twice; the first name and
    /// mime remain primary, and a DIFFERENT name is recorded as an alias so the
    /// by-name resolver still finds these bytes under either name.
    pub fn stage(
        &mut self,
        name: impl Into<String>,
        mime: impl Into<String>,
        bytes: Vec<u8>,
    ) -> AssetId {
        let digest = Sha256::digest(&bytes);
        let id = AssetId(format!("{digest:x}"));
        let name = name.into();
        match self.entries.entry(id.clone()) {
            std::collections::btree_map::Entry::Occupied(mut e) => {
                let entry = e.get_mut();
                if entry.name != name {
                    entry.aliases.insert(name);
                }
            }
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(AssetEntry {
                    name,
                    aliases: BTreeSet::new(),
                    mime: mime.into(),
                    bytes: Arc::new(bytes),
                });
            }
        }
        id
    }

    /// Records an extra name for already-staged bytes (the `.slxy` load path
    /// replays the aliases the save captured).
    pub fn add_alias(&mut self, id: &AssetId, name: impl Into<String>) {
        if let Some(entry) = self.entries.get_mut(id) {
            let name = name.into();
            if entry.name != name {
                entry.aliases.insert(name);
            }
        }
    }

    /// The staged entry whose name (or any alias) matches `wanted`, compared by
    /// trailing path component.
    #[must_use]
    pub fn find_by_name(&self, wanted: &str) -> Option<&AssetEntry> {
        self.entries.values().find(|e| e.has_name(wanted))
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
        let a = table.stage("cube.obj", "model/obj", b"v 0 0 0".to_vec());
        let b = table.stage("copy-of-cube.obj", "text/plain", b"v 0 0 0".to_vec());
        // Same content, same id, first name and mime win, one entry.
        assert_eq!(a, b);
        assert_eq!(table.len(), 1);
        assert_eq!(table.get(&a).unwrap().name, "cube.obj");
        assert_eq!(table.get(&a).unwrap().mime, "model/obj");

        let c = table.stage("other.obj", "model/obj", b"v 1 1 1".to_vec());
        assert_ne!(a, c);
        assert_eq!(table.len(), 2);
        // The id is the SHA-256 hex digest.
        assert_eq!(a.0.len(), 64);
    }

    /// The aliasing defect: byte-identical files staged under different names
    /// collapse into one content-addressed entry. Keeping only the first name
    /// made the second look unstaged, so a model referencing it was told its
    /// companion was missing while the bytes sat right there.
    #[test]
    fn identical_bytes_under_two_names_resolve_under_both() {
        let mut table = AssetTable::new();
        let png = b"\x89PNG fake pixels".to_vec();
        let a = table.stage("albedo.png", "image/png", png.clone());
        let b = table.stage("diffuse.png", "image/png", png);

        assert_eq!(a, b, "identical content is one entry");
        assert_eq!(table.len(), 1);

        let entry = table.get(&a).unwrap();
        assert_eq!(entry.name, "albedo.png", "first name stays primary");
        assert!(entry.aliases.contains("diffuse.png"));

        // Both names resolve, which is the whole point.
        assert!(table.find_by_name("albedo.png").is_some());
        assert!(
            table.find_by_name("diffuse.png").is_some(),
            "the second name must resolve, not report missing"
        );
        // And through a relative path, as a model's material would reference it.
        assert!(table.find_by_name("textures/diffuse.png").is_some());
        assert!(table.find_by_name("nope.png").is_none());
    }

    #[test]
    fn re_staging_the_same_name_adds_no_alias() {
        let mut table = AssetTable::new();
        let bytes = b"same".to_vec();
        let id = table.stage("a.png", "image/png", bytes.clone());
        table.stage("a.png", "image/png", bytes);
        assert!(table.get(&id).unwrap().aliases.is_empty());
    }
}
