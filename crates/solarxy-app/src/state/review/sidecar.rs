//! The review sidecar: the `.solarxy-review.json` an earlier release kept
//! beside a model, written from the document's annotations.
//!
//! The only part of review that touches the disk, and the only part that is
//! an `impl State` rather than an `impl ReviewState`: where the file goes is
//! a question about the open document rather than about the annotations,
//! and the annotations themselves are the engine's.
//!
//! The format is a shipped one with a version and users who have files, so
//! it stays. What changed is its direction: the document is the store, and
//! this is the way out. The way in, the import, arrives with the export's
//! menu entry.

use std::collections::BTreeMap;

use solarxy_core::review::{
    AnchorPosition, AnnotationCategory, FORMAT_VERSION_CURRENT, ReviewAnnotation, ReviewFile,
};
use solarxy_graph::Engine;
use solarxy_graph::document::NodeId;
use solarxy_graph::review::{AnnotationId, ReviewCategory};

use crate::gui::ToastSeverity;
use crate::state::State;

/// The sidecar's category for the engine's. The two vocabularies match by
/// name; only the serialized spelling differs.
pub(crate) fn file_category(category: ReviewCategory) -> AnnotationCategory {
    match category {
        ReviewCategory::Info => AnnotationCategory::Info,
        ReviewCategory::Warning => AnnotationCategory::Warning,
        ReviewCategory::Question => AnnotationCategory::Question,
        ReviewCategory::Change => AnnotationCategory::Change,
    }
}

/// The document's annotations as a sidecar file an earlier release reads.
///
/// The file anchors a note to a mesh index into one model and a face within
/// it; the document anchors to a node and a mesh within that node's
/// displayed geometry. The export flattens every displayed mesh in display
/// order, which is the model's own mesh order for a document built from one
/// file, and writes the per-mesh hashes that release compares on load, so a
/// note lands on the mesh it was placed on and is flagged stale rather than
/// lost where the geometry differs. A node-only note, which the file cannot
/// express, writes mesh and face zero and its fallback point, which is what
/// that release positions a marker from anyway.
///
/// Ids are minted fresh per export: the file's are strings with a
/// deterministic order and the document's are counters, and a reply follows
/// its parent through the same map.
pub(crate) fn export_review_file(engine: &Engine) -> ReviewFile {
    let mut mesh_hashes = Vec::new();
    let mut flat_index: BTreeMap<(NodeId, u32), u32> = BTreeMap::new();
    for (node, set, _) in engine.display_geometries() {
        for (mesh_index, mesh) in set.meshes.iter().enumerate() {
            let flat = u32::try_from(mesh_hashes.len()).unwrap_or(u32::MAX);
            let index = u32::try_from(mesh_index).unwrap_or(u32::MAX);
            flat_index.insert((node, index), flat);
            mesh_hashes.push(solarxy_core::review::hash_positions_indices(
                &mesh.positions,
                &mesh.indices,
            ));
        }
    }

    let review = engine.document().review();
    let ids: BTreeMap<AnnotationId, String> = review
        .iter()
        .map(|a| (a.id, ulid::Ulid::new().to_string()))
        .collect();
    let annotations = review
        .iter()
        .map(|a| ReviewAnnotation {
            id: ids[&a.id].clone(),
            created_at: a.created_at.clone(),
            updated_at: a.updated_at.clone(),
            author: a.author.clone(),
            anchor: AnchorPosition {
                mesh_index: a
                    .anchor
                    .mesh
                    .and_then(|m| flat_index.get(&(a.anchor.node, m)).copied())
                    .unwrap_or(0),
                face_index: a.anchor.face.unwrap_or(0),
                barycentric: a.anchor.barycentric.unwrap_or([1.0, 0.0, 0.0]),
                world_pos_fallback: a.anchor.world_fallback.unwrap_or([0.0; 3]),
            },
            category: file_category(a.category),
            text: a.text.clone(),
            reply_to: a.reply_to.and_then(|p| ids.get(&p).cloned()),
            resolved: a.resolved,
            stale: false,
        })
        .collect();

    ReviewFile {
        format_version: FORMAT_VERSION_CURRENT,
        model_hash: String::new(),
        mesh_hashes,
        annotations,
    }
}

impl State {
    /// Write the document's annotations to the sidecar beside the scene
    /// file. Toasts on success or failure; the toast is the log, so nothing
    /// here logs the same event twice.
    pub(crate) fn save_review_sidecar(&mut self) {
        let Some(engine) = self.engine.as_deref() else {
            return;
        };
        if engine.document().review().is_empty() {
            self.gui
                .set_toast("No annotations to save", ToastSeverity::Info);
            return;
        }
        let scene_path = self
            .engine_scene
            .as_ref()
            .map(|s| s.path.as_str())
            .filter(|p| !p.is_empty());
        let Some(scene_path) = scene_path else {
            self.gui.set_toast(
                "Save the scene first, so its review notes have a place beside it",
                ToastSeverity::Warning,
            );
            return;
        };
        let path = solarxy_core::review::sidecar_path_for(std::path::Path::new(scene_path), None);
        let file = export_review_file(engine);
        let count = file.annotations.len();
        match file.save(&path) {
            Ok(()) => {
                self.gui
                    .set_toast(&format!("Saved {count} annotations"), ToastSeverity::Info);
            }
            Err(e) => {
                self.gui
                    .set_toast(&format!("Save failed: {e}"), ToastSeverity::Error);
            }
        }
    }
}
