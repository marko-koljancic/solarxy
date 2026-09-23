//! The review sidecar: the `.solarxy-review.json` an earlier release kept
//! beside a model, as a way in and a way out of the document's annotations.
//!
//! The only part of review that touches the disk, and the part that is an
//! `impl State` rather than an `impl ReviewState`: where the file goes is a
//! question about the open document rather than about the annotations, and
//! the annotations themselves are the engine's.
//!
//! The format is a shipped one with a version and users who have files, so
//! it stays. What changed is its direction: the document is the store, the
//! export writes it back in the shape that release reads, and the import
//! reads one in. The two models are different shapes (string ids against
//! counters, a required anchor against an optional one, a mesh index into a
//! model against a node and a mesh within it), so both directions are a
//! mapping rather than a copy, and the mapping is written here once.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use solarxy_core::review::{
    AnchorPosition, AnnotationCategory, FORMAT_VERSION_CURRENT, ReviewAnnotation, ReviewFile,
    SIDECAR_SUFFIX,
};
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::EngineError;
use solarxy_graph::review::{AnnotationId, ReviewAnchor, ReviewCategory};
use solarxy_graph::{Command, Engine};

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

/// The engine's category for the sidecar's.
pub(crate) fn engine_category(category: AnnotationCategory) -> ReviewCategory {
    match category {
        AnnotationCategory::Info => ReviewCategory::Info,
        AnnotationCategory::Warning => ReviewCategory::Warning,
        AnnotationCategory::Question => ReviewCategory::Question,
        AnnotationCategory::Change => ReviewCategory::Change,
    }
}

/// One displayed mesh as the sidecar addresses it: the node it belongs to,
/// its index within that node's geometry, and its triangle count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlatMesh {
    node: NodeId,
    mesh: u32,
    triangles: u32,
}

/// Every displayed mesh in display order, flattened into the single list
/// the sidecar's `mesh_index` indexes. For a document built from one model
/// file this is that model's own mesh order, which is what the file was
/// written against.
fn flattened_meshes(engine: &Engine) -> Vec<FlatMesh> {
    let mut out = Vec::new();
    for (node, set, _) in engine.display_geometries() {
        for (mesh, data) in set.meshes.iter().enumerate() {
            out.push(FlatMesh {
                node,
                mesh: u32::try_from(mesh).unwrap_or(u32::MAX),
                triangles: u32::try_from(data.indices.len() / 3).unwrap_or(u32::MAX),
            });
        }
    }
    out
}

/// What identifies a note across an export and an import, so a file
/// imported twice adds nothing the second time. The category rides as its
/// ordinal, which orders where the enum itself does not.
type NoteKey = (String, String, Option<String>, u8);

fn key_of(created_at: &str, text: &str, author: Option<&str>, category: ReviewCategory) -> NoteKey {
    let ordinal = match category {
        ReviewCategory::Info => 0,
        ReviewCategory::Warning => 1,
        ReviewCategory::Question => 2,
        ReviewCategory::Change => 3,
    };
    (
        created_at.to_string(),
        text.to_string(),
        author.map(str::to_string),
        ordinal,
    )
}

/// What an import did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ImportReport {
    /// Notes added to the document.
    pub imported: usize,
    /// Of those, notes the file anchored to a mesh or face the document does
    /// not have, placed node-only at their fallback point.
    pub unanchored: usize,
    /// Notes the document already held, left alone.
    pub skipped: usize,
}

/// Read a sidecar's notes into the document as one undo step.
///
/// The file anchors a note to a mesh index into one model and a face within
/// it; the document anchors to a node and a mesh within that node's
/// displayed geometry. The mesh index is resolved against the displayed
/// meshes flattened in display order, and the face against that mesh's
/// triangle count. A note that resolves is anchored as if placed here; one
/// that does not is placed node-only at its fallback point, on the mesh's
/// node when the mesh exists and on the first displayed node when it does
/// not, and counted as unanchored, so the rest of the file still arrives.
/// A note the document already holds, by its creation time, text, author and
/// category, is skipped, which is what makes a second import add none.
/// Parents land before replies, and a reply follows its parent through the
/// file's ids, including a parent that was skipped as already present.
pub(crate) fn import_review_file(
    engine: &mut Engine,
    file: &ReviewFile,
) -> Result<ImportReport, EngineError> {
    let meshes = flattened_meshes(engine);
    let fallback_node = meshes.first().map(|m| m.node).or_else(|| {
        engine
            .document()
            .graph(GraphContext::Root)
            .ok()
            .and_then(|graph| graph.nodes().map(|n| n.id).next())
    });
    let Some(fallback_node) = fallback_node else {
        return Ok(ImportReport::default());
    };

    let mut existing: BTreeMap<NoteKey, AnnotationId> = engine
        .document()
        .review()
        .iter()
        .map(|a| {
            (
                key_of(&a.created_at, &a.text, a.author.as_deref(), a.category),
                a.id,
            )
        })
        .collect();

    // Parents first, so every reply finds its parent's document id.
    let mut ordered: Vec<&ReviewAnnotation> = file.annotations.iter().collect();
    ordered.sort_by_key(|a| a.reply_to.is_some());

    let mut ids: BTreeMap<&str, AnnotationId> = BTreeMap::new();
    let mut report = ImportReport::default();

    engine.apply(Command::BeginTransaction {
        label: "Import Review Notes".to_string(),
    })?;
    let outcome = (|| -> Result<(), EngineError> {
        for note in ordered {
            let category = engine_category(note.category);
            let key = key_of(
                &note.created_at,
                &note.text,
                note.author.as_deref(),
                category,
            );
            if let Some(&id) = existing.get(&key) {
                ids.insert(note.id.as_str(), id);
                report.skipped += 1;
                continue;
            }
            let reply_to = note.reply_to.as_deref().and_then(|p| ids.get(p).copied());
            let (anchor, anchored) = resolve_anchor(&meshes, fallback_node, &note.anchor);
            if !anchored {
                report.unanchored += 1;
            }
            let before: BTreeSet<AnnotationId> =
                engine.document().review().iter().map(|a| a.id).collect();
            engine.apply(Command::AddAnnotation {
                anchor,
                text: note.text.clone(),
                category,
                author: note.author.clone(),
                created_at: note.created_at.clone(),
                reply_to,
            })?;
            let Some(id) = engine
                .document()
                .review()
                .iter()
                .map(|a| a.id)
                .find(|id| !before.contains(id))
            else {
                continue;
            };
            if note.resolved {
                engine.apply(Command::ResolveAnnotation {
                    id,
                    resolved: true,
                    updated_at: note.updated_at.clone(),
                })?;
            }
            ids.insert(note.id.as_str(), id);
            existing.insert(key, id);
            report.imported += 1;
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => {
            engine.apply(Command::EndTransaction)?;
            Ok(report)
        }
        Err(e) => {
            let _ = engine.apply(Command::CancelTransaction);
            Err(e)
        }
    }
}

/// The document anchor for a sidecar anchor, and whether it resolved to a
/// face rather than falling back to the node.
fn resolve_anchor(
    meshes: &[FlatMesh],
    fallback_node: NodeId,
    anchor: &AnchorPosition,
) -> (ReviewAnchor, bool) {
    let node_only = |node: NodeId| ReviewAnchor {
        ctx: GraphContext::Root,
        node,
        mesh: None,
        face: None,
        barycentric: None,
        world_fallback: Some(anchor.world_pos_fallback),
        geometry_hash: None,
    };
    let Some(mesh) = meshes.get(anchor.mesh_index as usize) else {
        return (node_only(fallback_node), false);
    };
    if anchor.face_index >= mesh.triangles {
        return (node_only(mesh.node), false);
    }
    (
        ReviewAnchor {
            ctx: GraphContext::Root,
            node: mesh.node,
            mesh: Some(mesh.mesh),
            face: Some(anchor.face_index),
            barycentric: Some(anchor.barycentric),
            world_fallback: Some(anchor.world_pos_fallback),
            geometry_hash: None,
        },
        true,
    )
}

/// The document's annotations as a sidecar file an earlier release reads.
///
/// The export flattens every displayed mesh in display order and writes the
/// per-mesh hashes that release compares on load, so a note lands on the
/// mesh it was placed on and is flagged stale rather than lost where the
/// geometry differs. A node-only note, which the file cannot express, writes
/// mesh and face zero and its fallback point, which is what that release
/// positions a marker from anyway. Ids are minted fresh per export, and a
/// reply follows its parent through the same map.
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

/// The toast an import earns: how many arrived, and how many of those could
/// not be anchored.
fn import_message(report: ImportReport) -> String {
    if report.imported == 0 {
        return "No new notes to import".to_string();
    }
    let noun = if report.imported == 1 {
        "note"
    } else {
        "notes"
    };
    match report.unanchored {
        0 => format!("Imported {} {noun}", report.imported),
        n => format!(
            "Imported {} {noun}, {n} could not be anchored",
            report.imported
        ),
    }
}

impl State {
    /// Where the export writes without asking: beside the scene file, in the
    /// project configuration's sidecar directory when one names it. `None`
    /// for a document with no file yet, which is what routes the export to a
    /// dialog.
    fn review_export_path(&self) -> Option<PathBuf> {
        let scene_path = self
            .engine_scene
            .as_ref()
            .map(|s| s.path.as_str())
            .filter(|p| !p.is_empty())?;
        let scene_path = Path::new(scene_path);
        let sidecar_dir = scene_path
            .parent()
            .and_then(|dir| {
                solarxy_core::discover_project_config(dir, None)
                    .ok()
                    .flatten()
            })
            .and_then(|(_, cfg)| cfg.review.sidecar_dir);
        Some(solarxy_core::review::sidecar_path_for(
            scene_path,
            sidecar_dir.as_deref(),
        ))
    }

    /// Write the document's annotations to the sidecar an earlier release
    /// reads: beside the scene file, or where a dialog says for a document
    /// that has no file yet. Toasts on success or failure; the toast is the
    /// log, so nothing here logs the same event twice.
    pub(crate) fn export_review_notes(&mut self) {
        let Some(engine) = self.engine.as_deref() else {
            return;
        };
        if engine.document().review().is_empty() {
            self.gui
                .set_toast("No review notes to export", ToastSeverity::Info);
            return;
        }
        // A document with no file yet has nowhere to write beside, so the
        // dialog asks, suggesting the name the sibling rule would have used.
        let Some(path) = self.review_export_path().or_else(|| {
            rfd::FileDialog::new()
                .add_filter("Review Notes", &["json"])
                .set_file_name(format!("Untitled{SIDECAR_SUFFIX}"))
                .save_file()
        }) else {
            return;
        };
        let Some(engine) = self.engine.as_deref() else {
            return;
        };
        let file = export_review_file(engine);
        let count = file.annotations.len();
        let noun = if count == 1 { "note" } else { "notes" };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map_or_else(|| path.display().to_string(), str::to_string);
        match file.save(&path) {
            Ok(()) => self.gui.set_toast(
                &format!("Exported {count} {noun} to {name}"),
                ToastSeverity::Info,
            ),
            Err(e) => self
                .gui
                .set_toast(&format!("Export failed: {e}"), ToastSeverity::Error),
        }
    }

    /// Pick a sidecar and read its notes into the document.
    pub(crate) fn import_review_notes(&mut self) {
        if self.engine.is_none() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Review Notes", &["json"])
            .add_filter("All Files", &["*"])
            .pick_file()
        else {
            return;
        };
        let file = match ReviewFile::load(&path) {
            Ok(file) => file,
            Err(e) => {
                self.gui
                    .set_toast(&format!("Import failed: {e}"), ToastSeverity::Error);
                return;
            }
        };
        let Some(engine) = self.engine.as_deref_mut() else {
            return;
        };
        match import_review_file(engine, &file) {
            Ok(report) => {
                let severity = if report.unanchored > 0 {
                    ToastSeverity::Warning
                } else {
                    ToastSeverity::Info
                };
                self.gui.set_toast(&import_message(report), severity);
            }
            Err(e) => self
                .gui
                .set_toast(&format!("Import failed: {e}"), ToastSeverity::Error),
        }
    }
}

#[cfg(test)]
mod tests {
    use solarxy_graph::engine::EngineEvent;

    use super::*;

    fn add(engine: &mut Engine, ctx: GraphContext, ty: &str) -> NodeId {
        let batch = engine
            .apply(Command::AddNode {
                ctx,
                node_type: ty.to_string(),
                position: [0.0, 0.0],
            })
            .expect("the node adds");
        batch
            .events
            .iter()
            .find_map(|ev| match ev {
                EngineEvent::NodeAdded { node, .. } => Some(node.id),
                _ => None,
            })
            .expect("a node was added")
    }

    /// A cooked document displaying one default box.
    fn displayed_box() -> (Engine, NodeId) {
        let mut engine = Engine::new().expect("engine");
        let geo = add(&mut engine, GraphContext::Root, "sopnet");
        add(&mut engine, GraphContext::Subflow(geo), "box");
        engine.cook(&mut || true);
        (engine, geo)
    }

    fn file_note(id: &str, text: &str, mesh: u32, face: u32) -> ReviewAnnotation {
        ReviewAnnotation {
            id: id.to_string(),
            created_at: format!("2026-07-10T09:00:0{id}Z"),
            updated_at: "2026-07-11T09:00:00Z".to_string(),
            author: Some("Mara".to_string()),
            anchor: AnchorPosition {
                mesh_index: mesh,
                face_index: face,
                barycentric: [0.2, 0.3, 0.5],
                world_pos_fallback: [0.1, 0.2, 0.5],
            },
            category: AnnotationCategory::Warning,
            text: text.to_string(),
            reply_to: None,
            resolved: false,
            stale: false,
        }
    }

    /// Three notes: one on the box's first face, one on a face the box does
    /// not have, and a reply to the first, which is also resolved.
    fn fixture() -> ReviewFile {
        let mut reply = file_note("3", "Fixed", 0, 0);
        reply.reply_to = Some("1".to_string());
        reply.resolved = true;
        ReviewFile {
            format_version: FORMAT_VERSION_CURRENT,
            model_hash: String::new(),
            mesh_hashes: Vec::new(),
            annotations: vec![
                reply,
                file_note("1", "Seam on the lid", 0, 0),
                file_note("2", "Nowhere", 0, 9_999),
            ],
        }
    }

    #[test]
    fn import_places_notes_on_the_displayed_mesh_and_reports_the_unanchored() {
        let (mut engine, geo) = displayed_box();
        let report = import_review_file(&mut engine, &fixture()).expect("imports");
        assert_eq!(
            report,
            ImportReport {
                imported: 3,
                unanchored: 1,
                skipped: 0
            }
        );
        let notes: Vec<_> = engine.document().review().iter().collect();
        assert_eq!(notes.len(), 3);
        let seam = notes
            .iter()
            .find(|a| a.text == "Seam on the lid")
            .expect("the anchored note");
        assert_eq!(seam.anchor.node, geo);
        assert_eq!(seam.anchor.mesh, Some(0));
        assert_eq!(seam.anchor.face, Some(0));
        assert_eq!(seam.anchor.barycentric, Some([0.2, 0.3, 0.5]));
        assert_eq!(seam.author.as_deref(), Some("Mara"));
        assert_eq!(seam.category, ReviewCategory::Warning);
        assert!(!seam.resolved);
        let nowhere = notes
            .iter()
            .find(|a| a.text == "Nowhere")
            .expect("the unanchored note");
        assert_eq!(nowhere.anchor.node, geo, "on the mesh's node");
        assert!(nowhere.anchor.face.is_none(), "node-only");
        assert_eq!(nowhere.anchor.world_fallback, Some([0.1, 0.2, 0.5]));
        let reply = notes.iter().find(|a| a.text == "Fixed").expect("the reply");
        assert_eq!(
            reply.reply_to,
            Some(seam.id),
            "the reply follows its parent"
        );
        assert!(reply.resolved, "the resolved state arrives");
    }

    #[test]
    fn a_second_import_of_the_same_file_adds_none() {
        let (mut engine, _) = displayed_box();
        import_review_file(&mut engine, &fixture()).expect("imports");
        let again = import_review_file(&mut engine, &fixture()).expect("imports");
        assert_eq!(
            again,
            ImportReport {
                imported: 0,
                unanchored: 0,
                skipped: 3
            }
        );
        assert_eq!(engine.document().review().len(), 3);
    }

    #[test]
    fn an_import_is_one_undo_step() {
        let (mut engine, _) = displayed_box();
        import_review_file(&mut engine, &fixture()).expect("imports");
        assert_eq!(engine.document().review().len(), 3);
        engine.apply(Command::Undo).expect("undo");
        assert!(
            engine.document().review().is_empty(),
            "one step undoes the whole import"
        );
    }

    #[test]
    fn export_round_trips_through_parse_and_import() {
        let (mut engine, _) = displayed_box();
        import_review_file(&mut engine, &fixture()).expect("imports");
        let exported = export_review_file(&engine);
        assert_eq!(exported.format_version, FORMAT_VERSION_CURRENT);
        assert_eq!(exported.mesh_hashes.len(), 1, "one displayed mesh");
        assert_eq!(exported.annotations.len(), 3);
        let json = exported.to_pretty_json().expect("serializes");
        let parsed = ReviewFile::parse(&json).expect("an earlier release parses this");

        let (mut fresh, _) = displayed_box();
        let report = import_review_file(&mut fresh, &parsed).expect("imports");
        assert_eq!(report.imported, 3);
        let notes: Vec<_> = fresh.document().review().iter().collect();
        let seam = notes.iter().find(|a| a.text == "Seam on the lid").unwrap();
        let reply = notes.iter().find(|a| a.text == "Fixed").unwrap();
        assert_eq!(reply.reply_to, Some(seam.id));
        assert!(reply.resolved);
        assert!(notes.iter().all(|a| a.author.as_deref() == Some("Mara")));
        assert_eq!(seam.anchor.face, Some(0), "the anchored note anchors again");
    }

    #[test]
    fn a_node_only_note_exports_mesh_and_face_zero() {
        let (mut engine, geo) = displayed_box();
        engine
            .apply(Command::AddAnnotation {
                anchor: ReviewAnchor {
                    ctx: GraphContext::Root,
                    node: geo,
                    mesh: None,
                    face: None,
                    barycentric: None,
                    world_fallback: Some([1.0, 2.0, 3.0]),
                    geometry_hash: None,
                },
                text: "node-only".to_string(),
                category: ReviewCategory::Info,
                author: None,
                created_at: "2026-07-10T09:00:00Z".to_string(),
                reply_to: None,
            })
            .expect("adds");
        let exported = export_review_file(&engine);
        let note = &exported.annotations[0];
        assert_eq!(note.anchor.mesh_index, 0);
        assert_eq!(note.anchor.face_index, 0);
        assert_eq!(
            note.anchor.world_pos_fallback.map(f32::to_bits),
            [1.0f32, 2.0, 3.0].map(f32::to_bits),
            "the fallback point is copied exactly"
        );
        assert_eq!(note.category, AnnotationCategory::Info);
        assert!(note.author.is_none());
    }

    #[test]
    fn notes_import_node_only_when_nothing_is_displayed() {
        // A root container with no display output: every note lands
        // node-only on the first root node, and the toast will say so.
        let mut engine = Engine::new().expect("engine");
        let geo = add(&mut engine, GraphContext::Root, "sopnet");
        engine.cook(&mut || true);
        let report = import_review_file(&mut engine, &fixture()).expect("imports");
        assert_eq!(report.imported, 3);
        assert_eq!(report.unanchored, 3);
        assert!(
            engine
                .document()
                .review()
                .iter()
                .all(|a| a.anchor.node == geo && a.anchor.face.is_none())
        );
    }

    #[test]
    fn the_import_message_counts_and_names_the_unanchored() {
        assert_eq!(
            import_message(ImportReport::default()),
            "No new notes to import"
        );
        assert_eq!(
            import_message(ImportReport {
                imported: 1,
                unanchored: 0,
                skipped: 2
            }),
            "Imported 1 note"
        );
        assert_eq!(
            import_message(ImportReport {
                imported: 4,
                unanchored: 2,
                skipped: 0
            }),
            "Imported 4 notes, 2 could not be anchored"
        );
    }
}
