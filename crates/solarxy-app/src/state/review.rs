//! Review-mode runtime state — the in-memory mirror of one
//! `.solarxy-review.json` plus transient UI state (selection, editing
//! draft, panel filters).
//!
//! Owns the [`ReviewAnnotation`] set for the currently-loaded model.
//! Marker GPU buffer in [`solarxy_renderer::review_markers`] is rebuilt
//! from this state whenever `dirty` flips on (annotation added /
//! edited / resolved / deleted / selected, mode toggle, model load).
//!
//! Persistence (load/save sidecar, model + mesh hashing, stale detection)
//! lives in task #7; this module is the in-memory authority.

// model_hash / mesh_hashes / sidecar_path / panel_open and the
// `clear_for_new_model` method are populated/called by the sidecar I/O
// in task #7 and the side panel in task #8 — kept as part of the type
// today so those tasks are pure additions, not breaking changes.
#![allow(dead_code)]

use std::path::PathBuf;

use solarxy_core::review::{AnchorPosition, AnnotationCategory, ReviewAnnotation};
use solarxy_renderer::review_markers::ReviewMarkerInstance;

/// Top-level review-mode state on `State`. Initialized empty; populated
/// on model load (task #7) and mutated through the popup + side panel.
#[derive(Debug, Default)]
pub struct ReviewState {
    /// True between R-press and R-press-again. Click handling in
    /// `state::input` consults this to decide whether a left-click
    /// triggers a raycast or routes to the camera controller.
    pub active: bool,

    /// All annotations for the current model. Top-level entries (no
    /// `reply_to`) get a 3D marker; replies are list-only.
    pub annotations: Vec<ReviewAnnotation>,

    /// `id` of the currently-selected annotation, if any. The marker for
    /// this annotation renders with a cyan inner ring.
    pub selected: Option<String>,

    /// Popup state when the user is creating a new annotation or editing
    /// an existing one. `None` ⇒ no popup open.
    pub editing: Option<EditDraft>,

    /// SHA-256 of the model file at load time. Used by the save path
    /// (task #7) to populate `ReviewFile.model_hash`.
    pub model_hash: Option<String>,

    /// Per-mesh SHA-256 (positions||indices), indexed identically to
    /// `Model::cpu_meshes`. Empty until the model is loaded.
    pub mesh_hashes: Vec<String>,

    /// Resolved path of the sidecar for the current model — honors
    /// `ProjectConfig.review.sidecar_dir` when present, else sibling.
    pub sidecar_path: Option<PathBuf>,

    /// `true` when in-memory annotations differ from the on-disk file
    /// (new / edited / deleted / re-anchored). Cleared by save.
    pub dirty: bool,

    /// Mirror of `Preferences::review.author`; cached here so the popup
    /// doesn't have to thread through the prefs each render. Refreshed
    /// when prefs change.
    pub author: Option<String>,

    /// Whether the side panel (task #8) is visible. Mirrors
    /// `Preferences::review.panel_open` at startup; toggleable via
    /// `Window → Review Panel`.
    pub panel_open: bool,
}

/// Popup-form-in-progress state for the new-annotation modal. Created on
/// a successful raycast in review mode; dismissed by Save (commits to
/// `annotations`) or Cancel (discards).
#[derive(Debug, Clone)]
pub struct EditDraft {
    /// Anchor produced by the raycaster. World position is also stored
    /// in `anchor.world_pos_fallback` for marker rendering.
    pub anchor: AnchorPosition,

    /// Screen-space pixel location of the click that triggered this draft
    /// — used to position the egui popup near where the user clicked.
    pub screen_pos: (f32, f32),

    /// In-progress text. Editable via `egui::TextEdit::multiline`.
    pub text: String,

    /// Selected category. Defaults to Question (the canonical "what
    /// should change here?" review interaction).
    pub category: AnnotationCategory,

    /// `Some(id)` when editing an existing annotation; `None` when
    /// creating a new one.
    pub editing_id: Option<String>,
}

impl EditDraft {
    /// Build a fresh draft for a new annotation at the given anchor.
    pub fn new_at(anchor: AnchorPosition, screen_pos: (f32, f32)) -> Self {
        Self {
            anchor,
            screen_pos,
            text: String::new(),
            category: AnnotationCategory::default(),
            editing_id: None,
        }
    }
}

impl ReviewState {
    /// Generate a fresh ULID-as-string. Wraps the workspace `ulid` dep
    /// so callers don't need to import it.
    pub fn new_id() -> String {
        ulid::Ulid::new().to_string()
    }

    /// RFC 3339 UTC timestamp ("YYYY-MM-DDTHH:MM:SS.sssZ").
    pub fn now_rfc3339() -> String {
        use time::OffsetDateTime;
        use time::format_description::well_known::Rfc3339;
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string())
    }

    /// Commit the open draft as a new annotation (or write back to an
    /// existing one when `editing_id` is set). Clears the editing slot,
    /// flips `dirty` on, and returns the new/updated annotation id.
    pub fn commit_draft(&mut self) -> Option<String> {
        let draft = self.editing.take()?;
        let now = Self::now_rfc3339();

        if let Some(existing_id) = draft.editing_id {
            // Edit path: find by id, mutate in place.
            if let Some(ann) = self.annotations.iter_mut().find(|a| a.id == existing_id) {
                ann.text = draft.text;
                ann.category = draft.category;
                ann.updated_at = now;
            }
            self.dirty = true;
            Some(existing_id)
        } else {
            // Create path.
            let id = Self::new_id();
            self.annotations.push(ReviewAnnotation {
                id: id.clone(),
                created_at: now.clone(),
                updated_at: now,
                author: self.author.clone(),
                anchor: draft.anchor,
                category: draft.category,
                text: draft.text,
                reply_to: None,
                resolved: false,
                stale: false,
            });
            self.dirty = true;
            Some(id)
        }
    }

    /// Discard the open draft (Cancel / Esc).
    pub fn cancel_draft(&mut self) {
        self.editing = None;
    }

    /// Convert all top-level (non-reply) annotations to GPU marker
    /// instances. Replies share their parent's anchor and don't get
    /// their own 3D marker.
    pub fn marker_instances(&self) -> Vec<ReviewMarkerInstance> {
        self.annotations
            .iter()
            .filter(|a| a.reply_to.is_none())
            .map(|a| {
                let cat = match a.category {
                    AnnotationCategory::Info => 0,
                    AnnotationCategory::Warning => 1,
                    AnnotationCategory::Question => 2,
                    AnnotationCategory::Change => 3,
                };
                let selected = self.selected.as_deref() == Some(a.id.as_str());
                ReviewMarkerInstance::new(a.anchor.world_pos_fallback, cat, a.resolved, selected)
            })
            .collect()
    }

    /// Toggle review mode. Sets a transient toast via the caller; this
    /// helper just flips the bit.
    pub fn toggle_active(&mut self) -> bool {
        self.active = !self.active;
        // Close any open draft when exiting review mode.
        if !self.active {
            self.editing = None;
        }
        self.active
    }

    /// Clear all per-model state (annotations, hashes, sidecar path).
    /// Called on model close / load-new-model. Active flag is preserved
    /// (so closing one review-mode-on model and opening another keeps
    /// review mode active).
    pub fn clear_for_new_model(&mut self) {
        self.annotations.clear();
        self.selected = None;
        self.editing = None;
        self.model_hash = None;
        self.mesh_hashes.clear();
        self.sidecar_path = None;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor_at(pos: [f32; 3]) -> AnchorPosition {
        AnchorPosition {
            mesh_index: 0,
            face_index: 0,
            barycentric: [1.0 / 3.0; 3],
            world_pos_fallback: pos,
        }
    }

    #[test]
    fn new_id_is_unique_per_call() {
        let a = ReviewState::new_id();
        let b = ReviewState::new_id();
        assert_ne!(a, b, "two consecutive ULIDs should differ");
        assert_eq!(a.len(), 26, "ULID string is 26 chars (Crockford base-32)");
    }

    fn state_with_draft(draft: EditDraft) -> ReviewState {
        ReviewState {
            editing: Some(draft),
            ..Default::default()
        }
    }

    fn approx_eq_3(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn commit_create_pushes_annotation_and_marks_dirty() {
        let mut state = state_with_draft(EditDraft {
            anchor: anchor_at([1.0, 2.0, 3.0]),
            screen_pos: (100.0, 200.0),
            text: "Looks off".into(),
            category: AnnotationCategory::Warning,
            editing_id: None,
        });
        let id = state.commit_draft().expect("commit returns the new id");
        assert!(state.editing.is_none(), "draft cleared on commit");
        assert!(state.dirty, "dirty flipped on after create");
        assert_eq!(state.annotations.len(), 1);
        let a = &state.annotations[0];
        assert_eq!(a.id, id);
        assert_eq!(a.text, "Looks off");
        assert_eq!(a.category, AnnotationCategory::Warning);
        assert!(!a.resolved);
        assert!(a.author.is_none(), "author None when no preference set");
    }

    #[test]
    fn commit_carries_author_from_state() {
        let mut state = ReviewState {
            author: Some("Marko".into()),
            editing: Some(EditDraft::new_at(anchor_at([0.0; 3]), (0.0, 0.0))),
            ..Default::default()
        };
        state.commit_draft();
        assert_eq!(state.annotations[0].author.as_deref(), Some("Marko"));
    }

    #[test]
    fn commit_edit_path_mutates_existing_in_place() {
        let mut state = state_with_draft(EditDraft::new_at(anchor_at([0.0; 3]), (0.0, 0.0)));
        let id = state.commit_draft().unwrap();
        let created_at = state.annotations[0].created_at.clone();

        // Edit path
        state.editing = Some(EditDraft {
            anchor: anchor_at([0.0; 3]),
            screen_pos: (0.0, 0.0),
            text: "Updated text".into(),
            category: AnnotationCategory::Change,
            editing_id: Some(id.clone()),
        });
        let returned_id = state.commit_draft().expect("edit returns the same id");
        assert_eq!(returned_id, id);
        assert_eq!(state.annotations.len(), 1, "edit does not push a new entry");
        assert_eq!(state.annotations[0].text, "Updated text");
        assert_eq!(state.annotations[0].category, AnnotationCategory::Change);
        assert_eq!(
            state.annotations[0].created_at, created_at,
            "created_at preserved"
        );
    }

    #[test]
    fn cancel_draft_discards_without_creating() {
        let mut state = state_with_draft(EditDraft::new_at(anchor_at([0.0; 3]), (0.0, 0.0)));
        state.cancel_draft();
        assert!(state.editing.is_none());
        assert_eq!(state.annotations.len(), 0);
        assert!(!state.dirty, "cancel doesn't mark dirty");
    }

    #[test]
    fn replies_are_not_emitted_as_markers() {
        let mut state = state_with_draft(EditDraft::new_at(anchor_at([1.0, 0.0, 0.0]), (0.0, 0.0)));
        let parent_id = state.commit_draft().unwrap();
        state.annotations.push(ReviewAnnotation {
            id: ReviewState::new_id(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor: anchor_at([2.0, 0.0, 0.0]),
            category: AnnotationCategory::Info,
            text: "Fixed".into(),
            reply_to: Some(parent_id),
            resolved: false,
            stale: false,
        });

        let markers = state.marker_instances();
        assert_eq!(markers.len(), 1, "reply does not emit its own marker");
        assert!(
            approx_eq_3(markers[0].world_pos, [1.0, 0.0, 0.0]),
            "marker world_pos {:?} should match parent anchor",
            markers[0].world_pos
        );
    }

    #[test]
    fn toggle_active_clears_open_draft_on_exit() {
        let mut state = ReviewState::default();
        state.toggle_active();
        assert!(state.active);
        state.editing = Some(EditDraft::new_at(anchor_at([0.0; 3]), (0.0, 0.0)));
        state.toggle_active();
        assert!(!state.active);
        assert!(state.editing.is_none(), "draft auto-cancelled on exit");
    }

    #[test]
    fn clear_for_new_model_zeroes_state() {
        let mut state = ReviewState::default();
        state.annotations.push(ReviewAnnotation {
            id: ReviewState::new_id(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor: anchor_at([0.0; 3]),
            category: AnnotationCategory::Info,
            text: "x".into(),
            reply_to: None,
            resolved: false,
            stale: false,
        });
        state.selected = Some("x".into());
        state.model_hash = Some("h".into());
        state.mesh_hashes.push("m0".into());
        state.sidecar_path = Some(PathBuf::from("/tmp/x.json"));
        state.dirty = false;

        state.clear_for_new_model();

        assert!(state.annotations.is_empty());
        assert!(state.selected.is_none());
        assert!(state.model_hash.is_none());
        assert!(state.mesh_hashes.is_empty());
        assert!(state.sidecar_path.is_none());
        assert!(
            state.dirty,
            "dirty re-set so the GPU buffer flushes to empty"
        );
    }
}
