//! In-document review annotations (node catalog / UX review system).
//!
//! Phase 3 shipped the data model plus annotation CRUD; Phase 7 completed
//! it: anchors carry an optional `(mesh, face, barycentric)` pin with a
//! world-space fallback and a cheap structural [`geometry_hash`] of the
//! anchored output, annotations carry author/timestamps and flat reply
//! threading, and the engine derives a runtime `needs_reanchor` flag by
//! re-hashing displayed geometry after every cook (never persisted; the
//! stored hash is the reference value).
//!
//! Timestamps and author are host-provided strings (the web shell passes
//! `new Date().toISOString()` and the preferences author), so the engine
//! stays deterministic. Ids are minted from the document counter.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::document::{GraphContext, NodeId};

/// Stable identity of one annotation, minted from the document counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnnotationId(pub u64);

/// The review category (the desktop reviewer's vocabulary; wire strings are
/// camelCase). Encoded as marker glyph and color tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewCategory {
    Info,
    Warning,
    Question,
    Change,
}

/// Where an annotation is pinned: always a node in a context; 3D-pinned
/// annotations additionally carry the picked `(mesh, face, barycentric)`
/// with a world-space fallback and the structural hash of the anchored
/// output at pin time (engine-filled, the staleness reference).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewAnchor {
    pub ctx: GraphContext,
    pub node: NodeId,
    /// Mesh index within the anchored node's displayed `GeometrySet`.
    #[serde(default)]
    pub mesh: Option<u32>,
    /// Triangle index within that mesh (triangle = 3 consecutive indices).
    #[serde(default)]
    pub face: Option<u32>,
    /// Barycentric `[u, v, w]` (sum ~ 1) on the face, for sub-face accuracy.
    #[serde(default)]
    pub barycentric: Option<[f32; 3]>,
    /// World-space position at pin time: the marker's position while the
    /// anchor is stale, and the seed for the re-anchor click.
    #[serde(default)]
    pub world_fallback: Option<[f32; 3]>,
    /// [`geometry_hash`] of the anchored node's displayed output at pin
    /// time. Engine-filled on add/re-anchor; `None` for node-only anchors.
    #[serde(default)]
    pub geometry_hash: Option<u64>,
}

/// One review annotation. May be a top-level note or a reply (chained via
/// `reply_to`, one level deep). Replies share their parent's anchor (the
/// engine enforces it) and render under the parent, without their own pin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: AnnotationId,
    pub anchor: ReviewAnchor,
    pub text: String,
    pub category: ReviewCategory,
    pub resolved: bool,
    /// `None` = anonymous. Attribution is opt-in via preferences; never
    /// derived from the OS or git.
    #[serde(default)]
    pub author: Option<String>,
    /// RFC 3339 UTC creation timestamp, host-provided (empty when unknown).
    #[serde(default)]
    pub created_at: String,
    /// RFC 3339 UTC last-edit timestamp (mirrors `created_at` on insert).
    #[serde(default)]
    pub updated_at: String,
    /// Parent annotation id, or `None` for top-level notes. Flat: a reply
    /// can never itself be replied to.
    #[serde(default)]
    pub reply_to: Option<AnnotationId>,
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

    /// The direct replies of `id`, in id order.
    pub fn replies_of(&self, id: AnnotationId) -> impl Iterator<Item = &Annotation> {
        self.annotations
            .values()
            .filter(move |a| a.reply_to == Some(id))
    }

    /// Removes `id` plus its direct replies (threading is flat, so there is
    /// no recursion). Returns how many annotations were removed.
    pub fn remove_cascade(&mut self, id: AnnotationId) -> usize {
        let reply_ids: Vec<AnnotationId> = self.replies_of(id).map(|a| a.id).collect();
        let mut removed = usize::from(self.annotations.remove(&id).is_some());
        for reply in reply_ids {
            removed += usize::from(self.annotations.remove(&reply).is_some());
        }
        removed
    }

    pub fn iter(&self) -> impl Iterator<Item = &Annotation> {
        self.annotations.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Annotation> {
        self.annotations.values_mut()
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

/// The staleness hash is truncated to 53 bits so it crosses the wasm
/// boundary as a plain JavaScript number (`Number.MAX_SAFE_INTEGER` is
/// 2^53 - 1; serde-wasm-bindgen rejects wider u64s). 53 well-distributed
/// FNV bits are far more than "did the geometry change" needs.
const HASH_MASK: u64 = (1 << 53) - 1;

/// Cheap structural hash of a displayed `GeometrySet` (the plan doc's
/// staleness reference: mesh/index structure plus a quantized bounding box,
/// deliberately NOT a byte-exact SHA-256 -- it recomputes after every cook).
/// FNV-1a over: mesh count; per mesh the position/index counts and an index
/// checksum; the total triangle count; and the union AABB with each
/// coordinate quantized to `round(c * 1e3)` so sub-millimeter float jitter
/// does not flag re-anchoring. Truncated to 53 bits ([`HASH_MASK`]).
#[must_use]
pub fn geometry_hash(set: &solarxy_kernel::GeometrySet) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    let mut feed = |v: u64| {
        for byte in v.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    };

    feed(set.meshes.len() as u64);
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for mesh in &set.meshes {
        feed(mesh.positions.len() as u64);
        feed(mesh.indices.len() as u64);
        // Order-sensitive index checksum (wrapping sum of index * position).
        let mut checksum = 0u64;
        for (i, &idx) in mesh.indices.iter().enumerate() {
            checksum = checksum.wrapping_add(u64::from(idx).wrapping_mul(i as u64 + 1));
        }
        feed(checksum);
        for p in mesh.positions.iter() {
            for c in 0..3 {
                min[c] = min[c].min(p[c]);
                max[c] = max[c].max(p[c]);
            }
        }
    }
    feed(set.triangle_count());
    let quantize = |c: f32| -> u64 {
        if c.is_finite() {
            #[allow(clippy::cast_possible_truncation)]
            let q = (f64::from(c) * 1e3).round() as i64;
            q as u64
        } else {
            // Empty set: min/max stay at their infinities; feed a marker.
            u64::MAX
        }
    };
    for c in 0..3 {
        feed(quantize(min[c]));
        feed(quantize(max[c]));
    }
    hash & HASH_MASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_kernel::{GeometrySet, KernelMesh};

    fn tri_set(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> GeometrySet {
        GeometrySet::from_mesh(KernelMesh::new("t", positions, indices))
    }

    fn base() -> GeometrySet {
        tri_set(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
    }

    fn annotation(id: u64, reply_to: Option<AnnotationId>) -> Annotation {
        Annotation {
            id: AnnotationId(id),
            anchor: ReviewAnchor {
                ctx: GraphContext::Root,
                node: NodeId(1),
                mesh: Some(0),
                face: Some(0),
                barycentric: Some([1.0 / 3.0; 3]),
                world_fallback: Some([0.0; 3]),
                geometry_hash: Some(42),
            },
            text: "note".into(),
            category: ReviewCategory::Question,
            resolved: false,
            author: None,
            created_at: "2026-07-10T12:00:00Z".into(),
            updated_at: "2026-07-10T12:00:00Z".into(),
            reply_to,
        }
    }

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(geometry_hash(&base()), geometry_hash(&base()));
    }

    #[test]
    fn hash_fits_in_a_javascript_safe_integer() {
        // The boundary contract: 53 bits, so serde-wasm-bindgen can pass it
        // as a plain number.
        assert!(geometry_hash(&base()) <= HASH_MASK);
        assert!(geometry_hash(&GeometrySet::empty()) <= HASH_MASK);
    }

    #[test]
    fn hash_differs_on_index_change() {
        let reordered = tri_set(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 2, 1],
        );
        assert_ne!(geometry_hash(&base()), geometry_hash(&reordered));
    }

    #[test]
    fn hash_ignores_sub_quantization_jitter() {
        let jittered = tri_set(
            vec![[0.0, 0.0, 0.0], [1.0001, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        assert_eq!(geometry_hash(&base()), geometry_hash(&jittered));
    }

    #[test]
    fn hash_differs_on_translation() {
        let moved = tri_set(
            vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [5.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        assert_ne!(geometry_hash(&base()), geometry_hash(&moved));
    }

    #[test]
    fn hash_differs_on_vertex_count() {
        let more = tri_set(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            vec![0, 1, 2],
        );
        assert_ne!(geometry_hash(&base()), geometry_hash(&more));
    }

    #[test]
    fn empty_set_hashes_without_panicking() {
        let empty = GeometrySet::empty();
        assert_eq!(geometry_hash(&empty), geometry_hash(&GeometrySet::empty()));
        assert_ne!(geometry_hash(&empty), geometry_hash(&base()));
    }

    #[test]
    fn replies_of_lists_direct_replies_in_id_order() {
        let mut store = ReviewStore::new();
        store.insert(annotation(1, None));
        store.insert(annotation(3, Some(AnnotationId(1))));
        store.insert(annotation(2, Some(AnnotationId(1))));
        store.insert(annotation(4, None));
        let replies: Vec<u64> = store.replies_of(AnnotationId(1)).map(|a| a.id.0).collect();
        assert_eq!(replies, vec![2, 3]);
    }

    #[test]
    fn remove_cascade_takes_parent_and_replies_only() {
        let mut store = ReviewStore::new();
        store.insert(annotation(1, None));
        store.insert(annotation(2, Some(AnnotationId(1))));
        store.insert(annotation(3, None));
        assert_eq!(store.remove_cascade(AnnotationId(1)), 2);
        assert_eq!(store.len(), 1);
        assert!(store.get(AnnotationId(3)).is_some());
    }

    #[test]
    fn phase3_shaped_annotation_deserializes_with_defaults() {
        // The pre-Phase-7 wire shape: no mesh/world/hash/author/timestamps.
        let json = r#"{
            "id": 7,
            "anchor": { "ctx": "root", "node": 3, "face": null, "barycentric": null },
            "text": "old note",
            "category": "info",
            "resolved": false
        }"#;
        let a: Annotation = serde_json::from_str(json).expect("defaults fill new fields");
        assert_eq!(a.id, AnnotationId(7));
        assert!(a.anchor.mesh.is_none());
        assert!(a.anchor.world_fallback.is_none());
        assert!(a.anchor.geometry_hash.is_none());
        assert!(a.author.is_none());
        assert_eq!(a.created_at, "");
        assert!(a.reply_to.is_none());
    }

    #[test]
    fn categories_serialize_camel_case() {
        let cases = [
            (ReviewCategory::Info, "\"info\""),
            (ReviewCategory::Warning, "\"warning\""),
            (ReviewCategory::Question, "\"question\""),
            (ReviewCategory::Change, "\"change\""),
        ];
        for (cat, wire) in cases {
            assert_eq!(serde_json::to_string(&cat).unwrap(), wire);
        }
    }

    #[test]
    fn annotation_round_trips_all_fields() {
        let mut a = annotation(9, Some(AnnotationId(1)));
        a.author = Some("Marko".into());
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"createdAt\""), "camelCase wire: {json}");
        assert!(json.contains("\"replyTo\""), "camelCase wire: {json}");
        assert!(json.contains("\"worldFallback\""), "camelCase wire: {json}");
        assert!(json.contains("\"geometryHash\""), "camelCase wire: {json}");
        let back: Annotation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }
}
