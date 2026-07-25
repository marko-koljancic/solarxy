//! Sticky-note annotations anchored to mesh faces — the spatial-review
//! sidecar format consumed by the GUI's review mode.
//!
//! A `.solarxy-review.json` file lives next to its model by default (or at a
//! configurable directory per `solarxy.toml`'s `[review]` section).
//! Annotations anchor to a `(mesh_index, face_index, barycentric)` tuple;
//! per-mesh SHA-256 hashes ([`hash_mesh`]) detect topology changes between
//! exports so the GUI can flag stale anchors for explicit user reconciliation
//! rather than silently re-placing them at approximate positions.
//!
//! ULID generation and RFC 3339 timestamping live caller-side (in
//! `solarxy-app`) where annotations are constructed — this module keeps the
//! data shape pure and accepts pre-built strings.
//!
//! Available with the `serialization` feature. JSON Schema derives are
//! emitted when `schemars-gen` is also enabled.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::geometry::RawMeshData;

/// `format_version` value written by this build. Bump when the sidecar shape
/// changes incompatibly; additive optional fields don't require a bump.
pub const FORMAT_VERSION_CURRENT: u32 = 1;

/// Suffix appended to the model's file stem to derive the sidecar filename
/// (`hero.glb` ⇒ `hero.solarxy-review.json`).
pub const SIDECAR_SUFFIX: &str = ".solarxy-review.json";

/// Root container persisted to disk. One `.solarxy-review.json` per model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ReviewFile {
    /// Schema version of this file; clients SHOULD warn on a future major.
    #[serde(default = "default_format_version")]
    pub format_version: u32,

    /// SHA-256 (hex, lowercase) of the model file's full bytes at the time
    /// the file was first written. Used as a coarse "same-model?" guard.
    pub model_hash: String,

    /// SHA-256 (hex, lowercase) of each mesh's positions + indices, indexed
    /// by mesh. A per-annotation re-anchor check uses
    /// `mesh_hashes[anchor.mesh_index]` against the current model's
    /// [`hash_mesh`] output to flag staleness.
    #[serde(default)]
    pub mesh_hashes: Vec<String>,

    /// All annotations on this model. Order is preserved on save; new
    /// annotations are appended.
    #[serde(default)]
    pub annotations: Vec<ReviewAnnotation>,
}

impl ReviewFile {
    /// Construct an empty `ReviewFile` for a model with the given hashes.
    pub fn empty(model_hash: String, mesh_hashes: Vec<String>) -> Self {
        Self {
            format_version: FORMAT_VERSION_CURRENT,
            model_hash,
            mesh_hashes,
            annotations: Vec::new(),
        }
    }

    /// Read and parse a `.solarxy-review.json` from disk.
    pub fn load(path: &Path) -> Result<Self, ReviewError> {
        let raw = std::fs::read_to_string(path).map_err(|source| ReviewError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&raw).map_err(|source| ReviewError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Parse from an in-memory string (testable without disk).
    pub fn parse(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<Self>(raw)
    }

    /// Serialize to pretty-printed JSON (the on-disk representation).
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Atomic write: serialize, write to `<path>.tmp`, then rename. Creates
    /// parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), ReviewError> {
        let json = self.to_pretty_json().map_err(|source| ReviewError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| ReviewError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json.as_bytes()).map_err(|source| ReviewError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, path).map_err(|source| ReviewError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(())
    }
}

fn default_format_version() -> u32 {
    FORMAT_VERSION_CURRENT
}

/// One spatially-anchored note. May be a top-level annotation or a reply
/// (chained via `reply_to`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ReviewAnnotation {
    /// Sortable identifier (ULID recommended; any string with deterministic
    /// ordering is acceptable). Persisted as-is.
    pub id: String,

    /// RFC 3339 UTC timestamp of creation.
    pub created_at: String,

    /// RFC 3339 UTC timestamp of last edit (mirrors `created_at` on insert).
    pub updated_at: String,

    /// `None` ⇒ anonymous. Set only when the user has explicitly configured
    /// `Preferences::review.author`. Solarxy never reads `git config` or OS
    /// username; attribution is opt-in.
    #[serde(default)]
    pub author: Option<String>,

    /// Spatial position. Replies share their parent's anchor in the 3D view
    /// but each carries its own `AnchorPosition` for forward-compat
    /// (per-reply marker styling, etc).
    pub anchor: AnchorPosition,

    /// Category. Affects marker icon, color tint, and panel grouping.
    pub category: AnnotationCategory,

    /// Free-form note text.
    pub text: String,

    /// Parent annotation `id`, or `None` for top-level notes. Replies are
    /// rendered indented under their parent in the panel; replies don't get
    /// their own 3D markers.
    #[serde(default)]
    pub reply_to: Option<String>,

    /// `true` once the discussion is closed. Markers dim and the row moves
    /// to the "Resolved" section of the panel.
    #[serde(default)]
    pub resolved: bool,

    /// Runtime flag (not persisted): set during load when the annotation's
    /// `mesh_hash` doesn't match the current model's mesh, indicating the
    /// anchor needs explicit reconciliation by the user. Cleared once the
    /// user re-places the annotation.
    #[serde(default, skip)]
    pub stale: bool,
}

/// Annotation classification. Encoded as marker shape (in the 3D view) and
/// color tint (in both 3D and the side panel).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub enum AnnotationCategory {
    /// General informational note.
    Info,
    /// A concern or potential issue.
    Warning,
    /// An open question for the author. Default category for new annotations
    /// — code-review interactions are most often questions.
    #[default]
    Question,
    /// A requested change.
    Change,
}

impl AnnotationCategory {
    pub const ALL: &[Self] = &[Self::Info, Self::Warning, Self::Question, Self::Change];
}

impl std::fmt::Display for AnnotationCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Question => "Question",
            Self::Change => "Change",
        };
        f.write_str(s)
    }
}

/// Spatial anchor for an annotation: a mesh face plus barycentric
/// coordinates, with a world-space fallback used for stale-anchor
/// reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct AnchorPosition {
    /// Index into `RawModelData::meshes` at the time of creation.
    pub mesh_index: u32,

    /// Triangle index within that mesh (triangle = 3 consecutive indices).
    pub face_index: u32,

    /// Barycentric `[u, v, w]` (sum ≈ 1) — for sub-face accuracy.
    pub barycentric: [f32; 3],

    /// World-space position at creation. Used to render the marker when the
    /// anchor is stale (mesh topology changed) and as the seed point for
    /// the user's re-anchor click.
    pub world_pos_fallback: [f32; 3],
}

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error("{path}: I/O error: {source}", path = path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: parse error: {source}", path = path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Hex-encoded SHA-256 of a byte slice. Lowercase, no separators.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Hex-encoded SHA-256 of the bytes of a file on disk. Reads the whole file
/// into memory; intended for typical model sizes (≤500 MB), not streams.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(hash_bytes(&bytes))
}

/// Hex-encoded SHA-256 of a mesh's positions + indices.
///
/// Lower-level than [`hash_mesh`]: works directly off borrowed slices, so
/// callers that already hold their own geometry buffers (e.g. the renderer's
/// `Model::cpu_meshes`) don't need a `RawMeshData` round-trip.
///
/// Same topology-sensitivity contract as [`hash_mesh`].
pub fn hash_positions_indices(positions: &[[f32; 3]], indices: &[u32]) -> String {
    let mut hasher = Sha256::new();
    // Positions: feed as little-endian f32 bytes.
    for pos in positions {
        for component in pos {
            hasher.update(component.to_le_bytes());
        }
    }
    // Separator to disambiguate position-end / index-start (defensive vs.
    // a degenerate "all positions are zero" mesh that accidentally collides
    // with an empty-position mesh of nonzero indices).
    hasher.update(b"|idx|");
    for idx in indices {
        hasher.update(idx.to_le_bytes());
    }
    hex_encode(&hasher.finalize())
}

/// Hex-encoded SHA-256 of a mesh's positions + indices. Topology-sensitive:
/// reordering vertices or remeshing changes the hash; rigid transforms also
/// change the hash (positions are baked in).
///
/// The intent is "did the mesh's *contents* change between exports" — if
/// you want a hash that ignores rigid transforms, hash a normalized copy
/// (subtract centroid, divide by AABB diagonal) at a higher layer.
pub fn hash_mesh(mesh: &RawMeshData) -> String {
    hash_positions_indices(&mesh.positions, &mesh.indices)
}

/// Compute per-mesh hashes for a model. Convenience wrapper around
/// [`hash_mesh`] applied across `meshes`.
pub fn hash_meshes(meshes: &[RawMeshData]) -> Vec<String> {
    meshes.iter().map(hash_mesh).collect()
}

/// Compute the sidecar file path for a model.
///
/// - `model_path` — the model being annotated (e.g. `assets/hero.glb`).
/// - `sidecar_dir` — optional override from `ProjectConfig.review.sidecar_dir`.
///   When provided as a relative path, it's resolved relative to the model's
///   parent directory; absolute paths are used as-is.
///
/// Returned path always ends with the model's file stem plus
/// [`SIDECAR_SUFFIX`].
pub fn sidecar_path_for(model_path: &Path, sidecar_dir: Option<&Path>) -> PathBuf {
    let stem = model_path.file_stem().map_or_else(
        || "review".to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    let filename = format!("{stem}{SIDECAR_SUFFIX}");

    let parent = model_path.parent().unwrap_or_else(|| Path::new("."));
    let dir = match sidecar_dir {
        Some(d) if d.is_absolute() => d.to_path_buf(),
        Some(d) => parent.join(d),
        None => parent.to_path_buf(),
    };
    dir.join(filename)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::RawMeshData;

    fn sample_mesh(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> RawMeshData {
        RawMeshData {
            name: "t".to_owned(),
            positions,
            indices,
            normals: None,
            tex_coords: None,
            material_index: None,
            topology: crate::geometry::MeshTopology::Triangles,
            colors: None,
        }
    }

    fn sample_annotation(id: &str, category: AnnotationCategory) -> ReviewAnnotation {
        ReviewAnnotation {
            id: id.to_owned(),
            created_at: "2026-05-18T12:00:00Z".to_owned(),
            updated_at: "2026-05-18T12:00:00Z".to_owned(),
            author: None,
            anchor: AnchorPosition {
                mesh_index: 0,
                face_index: 0,
                barycentric: [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
                world_pos_fallback: [0.0, 0.0, 0.0],
            },
            category,
            text: "test note".to_owned(),
            reply_to: None,
            resolved: false,
            stale: false,
        }
    }

    #[test]
    fn format_version_constant_is_one() {
        assert_eq!(FORMAT_VERSION_CURRENT, 1);
    }

    #[test]
    fn sidecar_suffix_matches_documented_value() {
        assert_eq!(SIDECAR_SUFFIX, ".solarxy-review.json");
    }

    #[test]
    fn empty_constructor_seeds_format_version() {
        let f = ReviewFile::empty("abc".into(), vec!["m0".into()]);
        assert_eq!(f.format_version, 1);
        assert_eq!(f.model_hash, "abc");
        assert_eq!(f.mesh_hashes, vec!["m0".to_owned()]);
        assert!(f.annotations.is_empty());
    }

    #[test]
    fn hash_bytes_is_deterministic_and_lowercase_hex() {
        let a = hash_bytes(b"hello");
        let b = hash_bytes(b"hello");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn hash_bytes_differs_for_different_inputs() {
        assert_ne!(hash_bytes(b"foo"), hash_bytes(b"bar"));
    }

    #[test]
    fn hash_mesh_changes_when_position_changes() {
        let m1 = sample_mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let m2 = sample_mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.1, 0.0]],
            vec![0, 1, 2],
        );
        assert_ne!(hash_mesh(&m1), hash_mesh(&m2));
    }

    #[test]
    fn hash_mesh_changes_when_indices_change() {
        let m1 = sample_mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let m2 = sample_mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 2, 1],
        );
        assert_ne!(hash_mesh(&m1), hash_mesh(&m2));
    }

    #[test]
    fn hash_mesh_separator_prevents_collision() {
        let m_empty_pos = sample_mesh(vec![], vec![1, 2, 3]);
        let m_empty_idx = sample_mesh(vec![[0.0, 0.0, 0.0]], vec![]);
        assert_ne!(hash_mesh(&m_empty_pos), hash_mesh(&m_empty_idx));
    }

    #[test]
    fn hash_positions_indices_matches_hash_mesh() {
        let m = sample_mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let via_mesh = hash_mesh(&m);
        let via_slices = hash_positions_indices(&m.positions, &m.indices);
        assert_eq!(via_mesh, via_slices);
    }

    #[test]
    fn hash_meshes_matches_per_mesh_hashes() {
        let m1_a = sample_mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let m2_a = sample_mesh(
            vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let m1_b = sample_mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let m2_b = sample_mesh(
            vec![[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let hashes = hash_meshes(&[m1_a, m2_a]);
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes[0], hash_mesh(&m1_b));
        assert_eq!(hashes[1], hash_mesh(&m2_b));
    }

    #[test]
    fn roundtrip_empty_file() {
        let f = ReviewFile::empty("abc".into(), vec!["m0".into()]);
        let json = f.to_pretty_json().unwrap();
        let parsed = ReviewFile::parse(&json).unwrap();
        assert_eq!(parsed.format_version, 1);
        assert_eq!(parsed.model_hash, "abc");
        assert_eq!(parsed.annotations.len(), 0);
    }

    #[test]
    fn roundtrip_with_all_categories() {
        let mut f = ReviewFile::empty("h".into(), vec!["m".into()]);
        for c in AnnotationCategory::ALL {
            f.annotations
                .push(sample_annotation(&format!("id-{c}"), *c));
        }
        let json = f.to_pretty_json().unwrap();
        let parsed = ReviewFile::parse(&json).unwrap();
        assert_eq!(parsed.annotations.len(), 4);
        assert_eq!(parsed.annotations[0].category, AnnotationCategory::Info);
        assert_eq!(parsed.annotations[1].category, AnnotationCategory::Warning);
        assert_eq!(parsed.annotations[2].category, AnnotationCategory::Question);
        assert_eq!(parsed.annotations[3].category, AnnotationCategory::Change);
    }

    #[test]
    fn category_serializes_snake_case() {
        let a = sample_annotation("id-1", AnnotationCategory::Question);
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"category\":\"question\""));
    }

    #[test]
    fn roundtrip_threaded_reply_preserves_reply_to() {
        let mut parent = sample_annotation("01H7A", AnnotationCategory::Question);
        parent.text = "Looks weird".into();
        let mut reply = sample_annotation("01H7B", AnnotationCategory::Info);
        reply.text = "Fixed in v2".into();
        reply.reply_to = Some(parent.id.clone());

        let mut f = ReviewFile::empty("h".into(), vec!["m".into()]);
        f.annotations.push(parent);
        f.annotations.push(reply);

        let json = f.to_pretty_json().unwrap();
        let parsed = ReviewFile::parse(&json).unwrap();
        assert_eq!(parsed.annotations[1].reply_to.as_deref(), Some("01H7A"));
    }

    #[test]
    fn resolved_defaults_false_and_roundtrips() {
        let mut a = sample_annotation("id-1", AnnotationCategory::Change);
        a.resolved = true;
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"resolved\":true"));
        let parsed: ReviewAnnotation = serde_json::from_str(&json).unwrap();
        assert!(parsed.resolved);
        let b = sample_annotation("id-2", AnnotationCategory::Change);
        assert!(!b.resolved);
    }

    #[test]
    fn stale_is_runtime_only_not_persisted() {
        let mut a = sample_annotation("id-1", AnnotationCategory::Change);
        a.stale = true;
        let json = serde_json::to_string(&a).unwrap();
        assert!(!json.contains("\"stale\""));
        let parsed: ReviewAnnotation = serde_json::from_str(&json).unwrap();
        assert!(!parsed.stale);
    }

    #[test]
    fn author_defaults_anonymous_and_serializes_null() {
        let a = sample_annotation("id-1", AnnotationCategory::Question);
        assert!(a.author.is_none());
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"author\":null"));
    }

    #[test]
    fn author_set_serializes_string() {
        let mut a = sample_annotation("id-1", AnnotationCategory::Question);
        a.author = Some("Marko".into());
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"author\":\"Marko\""));
    }

    #[test]
    fn sidecar_path_default_sibling() {
        let p = Path::new("/tmp/models/hero.glb");
        let s = sidecar_path_for(p, None);
        assert_eq!(s, PathBuf::from("/tmp/models/hero.solarxy-review.json"));
    }

    #[test]
    fn sidecar_path_relative_override() {
        let p = Path::new("/tmp/models/hero.glb");
        let s = sidecar_path_for(p, Some(Path::new(".solarxy")));
        assert_eq!(
            s,
            PathBuf::from("/tmp/models/.solarxy/hero.solarxy-review.json")
        );
    }

    #[test]
    fn sidecar_path_absolute_override() {
        let p = Path::new("/tmp/models/hero.glb");
        let s = sidecar_path_for(p, Some(Path::new("/var/reviews")));
        assert_eq!(s, PathBuf::from("/var/reviews/hero.solarxy-review.json"));
    }

    #[test]
    fn sidecar_path_handles_bare_filename() {
        let p = Path::new("hero.glb");
        let s = sidecar_path_for(p, None);
        assert_eq!(s, PathBuf::from("hero.solarxy-review.json"));
    }

    #[test]
    fn unknown_field_rejected_by_deny_unknown_fields() {
        let json = r#"{"format_version":1,"model_hash":"h","mesh_hashes":[],"annotations":[],"some_future_field":42}"#;
        let result = ReviewFile::parse(json);
        assert!(result.is_err());
    }

    #[test]
    fn missing_optional_fields_default_correctly() {
        let json = r#"{"format_version":1,"model_hash":"h"}"#;
        let parsed = ReviewFile::parse(json).expect("optional fields should default");
        assert!(parsed.mesh_hashes.is_empty());
        assert!(parsed.annotations.is_empty());
    }

    #[test]
    fn save_and_load_round_trip_disk() {
        let tmp =
            std::env::temp_dir().join(format!("solarxy-review-test-{}.json", std::process::id()));
        let mut f = ReviewFile::empty("h".into(), vec!["m".into()]);
        f.annotations
            .push(sample_annotation("01H7A", AnnotationCategory::Warning));
        f.save(&tmp).unwrap();
        let loaded = ReviewFile::load(&tmp).unwrap();
        assert_eq!(loaded.annotations.len(), 1);
        assert_eq!(loaded.annotations[0].id, "01H7A");
        std::fs::remove_file(&tmp).ok();
    }
}
