//! In-document review annotations (node catalog / UX review system).
//!
//! Phase 3 scope is the data model plus annotation CRUD: an annotation
//! anchors to a node (and, later, a face/barycentric point) and carries a
//! category and resolved flag. The geometry-hash re-anchoring that keeps a
//! marker attached across edits, and the DOM overlay, are Phase 7; the
//! `Reanchor` command is present as a shell so the surface is frozen now.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::document::{GraphContext, NodeId};

/// Stable identity of one annotation, minted from the document counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnnotationId(pub u64);

/// The review category (mirrors the desktop review sidecar's kinds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewCategory {
    Info,
    Issue,
    Question,
    Suggestion,
}

/// Where an annotation is pinned. Phase 3 anchors to a node in a context;
/// the optional face/barycentric fields are reserved for Phase 7
/// re-anchoring (kept in the serde shape so files round-trip).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewAnchor {
    pub ctx: GraphContext,
    pub node: NodeId,
    /// Reserved: the picked face index (Phase 7).
    #[serde(default)]
    pub face: Option<u32>,
    /// Reserved: the barycentric coordinate on the face (Phase 7).
    #[serde(default)]
    pub barycentric: Option<[f32; 3]>,
}

/// One review annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub anchor: ReviewAnchor,
    pub text: String,
    pub category: ReviewCategory,
    pub resolved: bool,
}

/// The document's review store: annotations keyed by id (deterministic
/// order). Small, so undo snapshots the whole store.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewStore {
    annotations: BTreeMap<AnnotationId, Annotation>,
}

impl ReviewStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, annotation: Annotation) {
        self.annotations.insert(annotation.id, annotation);
    }

    #[must_use]
    pub fn get(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.get(&id)
    }

    #[must_use]
    pub fn get_mut(&mut self, id: AnnotationId) -> Option<&mut Annotation> {
        self.annotations.get_mut(&id)
    }

    pub fn remove(&mut self, id: AnnotationId) -> Option<Annotation> {
        self.annotations.remove(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Annotation> {
        self.annotations.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.annotations.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.annotations.is_empty()
    }
}
