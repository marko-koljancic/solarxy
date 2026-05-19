//! Review-mode runtime state — the in-memory mirror of one
//! `.solarxy-review.json` plus transient UI state (selection, editing
//! draft, panel filters).
//!
//! Owns the [`ReviewAnnotation`] set for the currently-loaded model.
//! Markers are drawn as an egui overlay (see `gui::review_overlay`)
//! using this state directly — no GPU buffer involved.
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

/// Top-level review-mode state on `State`. Initialized via [`Default`]
/// (which seeds sensible filter/dock defaults); populated on model load
/// (task #7) and mutated through the popup + side panel.
#[derive(Debug)]
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
    /// `Window → Review Panel` and auto-opens on Shift+R when off.
    pub panel_open: bool,

    /// Per-category filter chips on the panel: index by
    /// `AnnotationCategory as u32` (0=Info, 1=Warning, 2=Question,
    /// 3=Change). `true` ⇒ category visible in the list. All default
    /// to true.
    pub category_filters: [bool; 4],

    /// `true` ⇒ resolved annotations are shown in their own collapsible
    /// section. `false` ⇒ resolved entries are hidden entirely.
    /// Default `true` — showing resolved keeps conversation context.
    pub show_resolved: bool,

    /// Case-insensitive substring filter applied to annotation text.
    /// Empty ⇒ no filter.
    pub text_filter: String,

    /// `Some(id)` while the cascade-delete confirmation modal is open.
    /// Cleared on Cancel or after a successful delete.
    pub delete_confirm: Option<String>,

    /// `Some(id)` while the user is in the re-anchor sub-mode for that
    /// annotation. The next valid raycast in review mode writes its
    /// anchor; Esc cancels.
    pub reanchor_target: Option<String>,

    /// One-shot flag: when `true`, the side panel scrolls the selected
    /// row into view next frame, then clears the flag. Set by marker
    /// hit-test and `begin_reanchor` to keep the panel aligned with the
    /// 3D selection.
    pub scroll_to_selected: bool,

    /// `id` of the marker currently under the cursor, if any. Updated
    /// on mouse-move by `state::input` and consumed by
    /// `gui::review_overlay` to decide which pin should expand into a
    /// card. Cleared (set to `None`) when the cursor leaves all pins.
    pub hovered: Option<String>,

    /// Monotonically-increasing counter used to key the egui popup window
    /// by *draft session* (instead of click pixel). Each new draft pulls
    /// a fresh value via [`alloc_draft_seq`]; the popup uses the seq in
    /// its `Id` so reopening a popup at a new click position resets
    /// egui's cached drag position cleanly.
    pub next_draft_seq: u64,
}

impl Default for ReviewState {
    fn default() -> Self {
        Self {
            active: false,
            annotations: Vec::new(),
            selected: None,
            editing: None,
            model_hash: None,
            mesh_hashes: Vec::new(),
            sidecar_path: None,
            dirty: false,
            author: None,
            panel_open: false,
            category_filters: [true; 4],
            show_resolved: true,
            text_filter: String::new(),
            delete_confirm: None,
            reanchor_target: None,
            scroll_to_selected: false,
            hovered: None,
            next_draft_seq: 0,
        }
    }
}

/// Best-effort first-line preview of annotation text, truncated to ~30
/// chars with a trailing ellipsis when shortened. Used by toast and
/// banner messages.
pub fn short_text_preview(text: &str) -> String {
    let first: String = text.lines().next().unwrap_or("").chars().take(30).collect();
    if text.lines().count() > 1 || text.chars().count() > first.chars().count() {
        format!("{first}\u{2026}")
    } else {
        first
    }
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
    /// For replies opened via the panel, this is the screen center.
    pub screen_pos: (f32, f32),

    /// In-progress text. Editable via `egui::TextEdit::multiline`.
    pub text: String,

    /// Selected category. Defaults to Question (the canonical "what
    /// should change here?" review interaction).
    pub category: AnnotationCategory,

    /// `Some(id)` when editing an existing annotation; `None` when
    /// creating a new one.
    pub editing_id: Option<String>,

    /// `Some(parent_id)` when the draft is a reply to an existing
    /// annotation; `None` for top-level notes. Replies share the
    /// parent's anchor and don't get their own 3D marker (see
    /// `gui::review_overlay`).
    pub reply_to: Option<String>,

    /// Unique per-draft-session seq, allocated via
    /// [`ReviewState::alloc_draft_seq`]. The popup uses this in its
    /// egui `Id` so each fresh draft gets a clean cached position.
    pub seq: u64,
}

impl EditDraft {
    /// Build a fresh draft for a new top-level annotation at the given
    /// anchor. `seq` must be allocated via
    /// [`ReviewState::alloc_draft_seq`] so the popup keys cleanly.
    pub fn new_at(seq: u64, anchor: AnchorPosition, screen_pos: (f32, f32)) -> Self {
        Self {
            anchor,
            screen_pos,
            text: String::new(),
            category: AnnotationCategory::default(),
            editing_id: None,
            reply_to: None,
            seq,
        }
    }

    /// Build a draft for a reply to `parent_id` — anchor borrowed from
    /// the parent, popup positioned at `screen_pos` (typically the
    /// viewport center when opened via the panel's Reply button).
    pub fn new_reply(
        seq: u64,
        parent_id: String,
        parent_anchor: AnchorPosition,
        screen_pos: (f32, f32),
    ) -> Self {
        Self {
            anchor: parent_anchor,
            screen_pos,
            text: String::new(),
            category: AnnotationCategory::default(),
            editing_id: None,
            reply_to: Some(parent_id),
            seq,
        }
    }
}

impl ReviewState {
    /// Allocate the next draft session id. Bumps the in-memory counter
    /// and returns the new value. Wraps on overflow (the counter is
    /// `u64` — won't realistically hit it).
    pub fn alloc_draft_seq(&mut self) -> u64 {
        self.next_draft_seq = self.next_draft_seq.wrapping_add(1);
        self.next_draft_seq
    }

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
            if let Some(ann) = self.annotations.iter_mut().find(|a| a.id == existing_id) {
                ann.text = draft.text;
                ann.category = draft.category;
                ann.updated_at = now;
            }
            self.dirty = true;
            Some(existing_id)
        } else {
            let id = Self::new_id();
            self.annotations.push(ReviewAnnotation {
                id: id.clone(),
                created_at: now.clone(),
                updated_at: now,
                author: self.author.clone(),
                anchor: draft.anchor,
                category: draft.category,
                text: draft.text,
                reply_to: draft.reply_to,
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

    /// Lookup by id (linear scan — annotation counts are small).
    pub fn find(&self, id: &str) -> Option<&ReviewAnnotation> {
        self.annotations.iter().find(|a| a.id == id)
    }

    /// Number of direct replies an annotation has. `0` for replies and
    /// for unparented leaves.
    pub fn reply_count(&self, parent_id: &str) -> usize {
        self.annotations
            .iter()
            .filter(|a| a.reply_to.as_deref() == Some(parent_id))
            .count()
    }

    /// Touch `updated_at` on an annotation. No-op if `id` isn't found.
    /// Doesn't dirty (text edits in the inline editor don't change
    /// markers); callers flip `dirty` themselves when category /
    /// resolved / anchor changes.
    pub fn touch_updated(&mut self, id: &str) {
        if let Some(ann) = self.annotations.iter_mut().find(|a| a.id == id) {
            ann.updated_at = Self::now_rfc3339();
        }
    }

    /// Delete an annotation and all its direct replies. Returns the
    /// total count removed. Clears `selected` if it pointed at any
    /// removed entry. Dirties the marker buffer so the GPU set
    /// rebuilds next frame.
    pub fn delete_cascade(&mut self, id: &str) -> usize {
        let before = self.annotations.len();
        let target = id.to_string();
        let was_selected_removed = self
            .annotations
            .iter()
            .any(|a| a.id == target || a.reply_to.as_deref() == Some(&target));
        self.annotations
            .retain(|a| a.id != target && a.reply_to.as_deref() != Some(&target));
        let removed = before - self.annotations.len();
        if removed > 0 {
            self.dirty = true;
        }
        if was_selected_removed
            && let Some(sel) = &self.selected
            && (sel == &target || !self.annotations.iter().any(|a| &a.id == sel))
        {
            self.selected = None;
        }
        if self.delete_confirm.as_deref() == Some(id) {
            self.delete_confirm = None;
        }
        removed
    }

    /// Open the popup as a reply to `parent_id`. No-op if the parent
    /// doesn't exist. `screen_pos` positions the popup; pass the
    /// viewport center when opening via the panel.
    pub fn open_reply_draft(&mut self, parent_id: &str, screen_pos: (f32, f32)) {
        let Some(parent) = self.find(parent_id) else {
            return;
        };
        let parent_anchor = parent.anchor.clone();
        let parent_id_owned = parent.id.clone();
        let seq = self.alloc_draft_seq();
        self.editing = Some(EditDraft::new_reply(
            seq,
            parent_id_owned,
            parent_anchor,
            screen_pos,
        ));
    }

    /// Project every non-reply annotation's world anchor into pane-relative
    /// screen pixels and return the id of the marker nearest to
    /// `cursor_px`, provided it falls within `threshold_px`.
    ///
    /// `cursor_px` and `viewport_size_px` are pane-relative — pass the
    /// same `(local.0, local.1)` cursor the raycaster uses, and the
    /// active pane's `(width, height)`. `view_proj` is the active
    /// camera's `clip = view_proj * world` matrix (same one written to
    /// the GPU camera uniform).
    ///
    /// Skips annotations whose projected NDC `z` is outside `[-1, 1]`
    /// (behind the camera or past the far plane). Replies are skipped
    /// (no 3D marker is drawn for them — see [`marker_instances`]).
    /// Resolved annotations are still hit-tested (they render dimmed,
    /// but remain interactable).
    pub fn marker_at_screen_pos(
        &self,
        cursor_px: (f32, f32),
        viewport_size_px: (f32, f32),
        view_proj: cgmath::Matrix4<f32>,
        threshold_px: f32,
    ) -> Option<String> {
        use cgmath::Vector4;
        let threshold_sq = threshold_px * threshold_px;
        let mut best: Option<(f32, &str)> = None;
        for ann in &self.annotations {
            if ann.reply_to.is_some() {
                continue;
            }
            let [wx, wy, wz] = ann.anchor.world_pos_fallback;
            let clip = view_proj * Vector4::new(wx, wy, wz, 1.0);
            if clip.w <= 0.0 {
                continue;
            }
            let ndc_x = clip.x / clip.w;
            let ndc_y = clip.y / clip.w;
            let ndc_z = clip.z / clip.w;
            if !(-1.0..=1.0).contains(&ndc_z) {
                continue;
            }
            let px = (ndc_x + 1.0) * 0.5 * viewport_size_px.0;
            let py = (1.0 - ndc_y) * 0.5 * viewport_size_px.1;
            let dx = px - cursor_px.0;
            let dy = py - cursor_px.1;
            let d_sq = dx * dx + dy * dy;
            if d_sq > threshold_sq {
                continue;
            }
            if best.is_none_or(|(b, _)| d_sq < b) {
                best = Some((d_sq, ann.id.as_str()));
            }
        }
        best.map(|(_, id)| id.to_string())
    }

    /// Enter the re-anchor sub-mode for an existing annotation. Sets
    /// `reanchor_target = Some(id)` and selects the row so the panel
    /// scrolls to it. No-op if the id is unknown.
    pub fn begin_reanchor(&mut self, id: String) {
        if !self.annotations.iter().any(|a| a.id == id) {
            return;
        }
        self.selected = Some(id.clone());
        self.scroll_to_selected = true;
        self.reanchor_target = Some(id);
    }

    /// Leave the re-anchor sub-mode without modifying any annotation.
    /// Leaves `selected` intact so the row stays highlighted.
    pub fn cancel_reanchor(&mut self) {
        self.reanchor_target = None;
    }

    /// Finalize the re-anchor: write `hit`'s mesh / face / barycentric /
    /// world-position into the target annotation, clear `stale`, bump
    /// `updated_at`, mark dirty, clear `reanchor_target`. Returns `true`
    /// when a target was pending; `false` (no-op) otherwise.
    pub fn complete_reanchor(&mut self, hit: &crate::state::raycast::RaycastHit) -> bool {
        let Some(id) = self.reanchor_target.take() else {
            return false;
        };
        let Some(ann) = self.annotations.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        ann.anchor.mesh_index = hit.mesh_index;
        ann.anchor.face_index = hit.face_index;
        ann.anchor.barycentric = hit.barycentric;
        ann.anchor.world_pos_fallback = [hit.world_pos.x, hit.world_pos.y, hit.world_pos.z];
        ann.stale = false;
        ann.updated_at = Self::now_rfc3339();
        self.dirty = true;
        true
    }

    /// Mark every annotation whose mesh's current hash differs from the
    /// stored one as stale. Used after loading a sidecar to flag anchors
    /// that need explicit reconciliation by the user.
    pub fn apply_stale_flags(
        &mut self,
        stored_mesh_hashes: &[String],
        current_mesh_hashes: &[String],
    ) -> usize {
        let mut stale_count = 0;
        for ann in &mut self.annotations {
            let mi = ann.anchor.mesh_index as usize;
            let stale = match (stored_mesh_hashes.get(mi), current_mesh_hashes.get(mi)) {
                (Some(old), Some(new)) => old != new,
                _ => true,
            };
            ann.stale = stale;
            if stale {
                stale_count += 1;
            }
        }
        stale_count
    }
}

use crate::state::State;
use crate::gui::ToastSeverity;

impl State {
    /// Compute hashes + resolve sidecar path + attempt load. Called from
    /// the model-load completion site in `state::update`. Replaces any
    /// previously-loaded review state.
    pub(crate) fn load_review_for_model(&mut self, model_path: &str) {
        self.review.clear_for_new_model();
        let path = std::path::Path::new(model_path);
        let model_hash = solarxy_core::review::hash_file(path).ok();
        let mesh_hashes: Vec<String> = self
            .scene
            .as_ref()
            .map(|s| {
                s.model
                    .cpu_meshes
                    .iter()
                    .map(|m| solarxy_core::review::hash_positions_indices(&m.positions, &m.indices))
                    .collect()
            })
            .unwrap_or_default();

        self.review
            .author
            .clone_from(&self.preferences.review.author);
        self.review.model_hash = model_hash;
        self.review.mesh_hashes.clone_from(&mesh_hashes);

        let project_root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let discovered = solarxy_core::project_config::discover(project_root, None)
            .ok()
            .flatten();
        if let Some((cfg_path, _)) = discovered.as_ref()
            && self.last_project_config_toast.as_ref() != Some(cfg_path)
        {
            let label = cfg_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("solarxy.toml")
                .to_string();
            self.gui.set_toast(
                &format!("Loaded project config from {label}"),
                ToastSeverity::Info,
            );
            self.last_project_config_toast = Some(cfg_path.clone());
        }
        let sidecar_dir = discovered.and_then(|(_, cfg)| cfg.review.sidecar_dir);
        let sidecar_path = solarxy_core::sidecar_path_for(path, sidecar_dir.as_deref());
        self.review.sidecar_path = Some(sidecar_path.clone());

        if !sidecar_path.exists() {
            return;
        }
        match solarxy_core::review::ReviewFile::load(&sidecar_path) {
            Ok(file) => {
                let count = file.annotations.len();
                let stored_hashes = file.mesh_hashes.clone();
                self.review.annotations = file.annotations;
                let stale = self.review.apply_stale_flags(&stored_hashes, &mesh_hashes);
                self.review.dirty = true;
                let msg = if stale == 0 {
                    format!("Loaded {count} review annotations")
                } else {
                    format!("Loaded {count} annotations ({stale} need re-anchor)")
                };
                let severity = if stale == 0 {
                    ToastSeverity::Success
                } else {
                    ToastSeverity::Warning
                };
                self.gui.set_toast(&msg, severity);
            }
            Err(e) => {
                self.gui
                    .set_toast(&format!("Review load failed: {e}"), ToastSeverity::Error);
            }
        }
    }

    /// Persist the current annotation set + hashes to the resolved sidecar
    /// path. Toasts on success or failure.
    pub(crate) fn save_review_sidecar(&mut self) {
        let Some(path) = self.review.sidecar_path.clone() else {
            self.gui.set_toast(
                "Open a model before saving review notes",
                ToastSeverity::Warning,
            );
            return;
        };
        if self.review.annotations.is_empty() && !path.exists() {
            self.gui
                .set_toast("No annotations to save", ToastSeverity::Info);
            return;
        }

        let file = solarxy_core::review::ReviewFile {
            format_version: solarxy_core::review::FORMAT_VERSION_CURRENT,
            model_hash: self.review.model_hash.clone().unwrap_or_default(),
            mesh_hashes: self.review.mesh_hashes.clone(),
            annotations: self.review.annotations.clone(),
        };
        match file.save(&path) {
            Ok(()) => {
                let count = self.review.annotations.len();
                tracing::info!(
                    target: "solarxy::toast",
                    "Saved {} annotations to {}",
                    count,
                    path.display()
                );
                self.gui.set_toast(
                    &format!("Saved {count} annotations"),
                    ToastSeverity::Success,
                );
            }
            Err(e) => {
                self.gui
                    .set_toast(&format!("Save failed: {e}"), ToastSeverity::Error);
            }
        }
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
            reply_to: None,
            seq: 0,
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
            editing: Some(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0))),
            ..Default::default()
        };
        state.commit_draft();
        assert_eq!(state.annotations[0].author.as_deref(), Some("Marko"));
    }

    #[test]
    fn commit_edit_path_mutates_existing_in_place() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        let id = state.commit_draft().unwrap();
        let created_at = state.annotations[0].created_at.clone();

        state.editing = Some(EditDraft {
            anchor: anchor_at([0.0; 3]),
            screen_pos: (0.0, 0.0),
            text: "Updated text".into(),
            category: AnnotationCategory::Change,
            editing_id: Some(id.clone()),
            reply_to: None,
            seq: 0,
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
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        state.cancel_draft();
        assert!(state.editing.is_none());
        assert_eq!(state.annotations.len(), 0);
        assert!(!state.dirty, "cancel doesn't mark dirty");
    }

    #[test]
    fn toggle_active_clears_open_draft_on_exit() {
        let mut state = ReviewState::default();
        state.toggle_active();
        assert!(state.active);
        state.editing = Some(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        state.toggle_active();
        assert!(!state.active);
        assert!(state.editing.is_none(), "draft auto-cancelled on exit");
    }

    #[test]
    fn commit_draft_with_reply_to_persists_parent_link() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        let parent_id = state.commit_draft().unwrap();

        state.open_reply_draft(&parent_id, (100.0, 100.0));
        let draft = state.editing.as_mut().expect("reply draft open");
        draft.text = "Fixed in v2".into();
        let reply_id = state.commit_draft().unwrap();

        assert_ne!(parent_id, reply_id);
        let reply = state.find(&reply_id).expect("reply persisted");
        assert_eq!(reply.reply_to.as_deref(), Some(parent_id.as_str()));
        assert_eq!(reply.text, "Fixed in v2");
    }

    #[test]
    fn open_reply_draft_inherits_parent_anchor_and_sets_reply_to() {
        let parent_anchor = anchor_at([3.5, 1.2, -0.4]);
        let mut state = state_with_draft(EditDraft {
            anchor: parent_anchor.clone(),
            screen_pos: (0.0, 0.0),
            text: "Parent".into(),
            category: AnnotationCategory::Question,
            editing_id: None,
            reply_to: None,
            seq: 0,
        });
        let parent_id = state.commit_draft().unwrap();
        state.open_reply_draft(&parent_id, (500.0, 250.0));
        let draft = state.editing.as_ref().expect("draft open");
        assert_eq!(draft.reply_to.as_deref(), Some(parent_id.as_str()));
        assert_eq!(draft.screen_pos, (500.0, 250.0));
        assert!(draft.editing_id.is_none());
        assert!(
            approx_eq_3(
                draft.anchor.world_pos_fallback,
                parent_anchor.world_pos_fallback
            ),
            "draft inherits parent anchor"
        );
    }

    #[test]
    fn open_reply_draft_is_noop_for_unknown_parent() {
        let mut state = ReviewState::default();
        state.open_reply_draft("nonexistent-id", (0.0, 0.0));
        assert!(state.editing.is_none());
    }

    #[test]
    fn delete_cascade_removes_parent_and_replies() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        let parent_id = state.commit_draft().unwrap();

        for i in 0..2 {
            state.open_reply_draft(&parent_id, (0.0, 0.0));
            state.editing.as_mut().unwrap().text = format!("reply {i}");
            state.commit_draft();
        }

        state.editing = Some(EditDraft::new_at(0, anchor_at([5.0, 0.0, 0.0]), (0.0, 0.0)));
        state.editing.as_mut().unwrap().text = "orphan".into();
        let orphan_id = state.commit_draft().unwrap();

        assert_eq!(state.annotations.len(), 4);
        let removed = state.delete_cascade(&parent_id);
        assert_eq!(removed, 3, "parent + 2 replies = 3");
        assert!(state.find(&parent_id).is_none());
        assert!(state.find(&orphan_id).is_some(), "orphan untouched");
        assert!(state.dirty, "marker buffer needs rebuild");
    }

    #[test]
    fn delete_cascade_clears_selection_when_target_removed() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        let id = state.commit_draft().unwrap();
        state.selected = Some(id.clone());
        state.delete_cascade(&id);
        assert!(state.selected.is_none());
    }

    #[test]
    fn delete_cascade_clears_selection_when_selected_reply_cascades_with_parent() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        let parent_id = state.commit_draft().unwrap();
        state.open_reply_draft(&parent_id, (0.0, 0.0));
        state.editing.as_mut().unwrap().text = "reply".into();
        let reply_id = state.commit_draft().unwrap();
        state.selected = Some(reply_id.clone());

        state.delete_cascade(&parent_id);
        assert!(state.selected.is_none(), "cascade swept the selected reply");
    }

    #[test]
    fn delete_cascade_clears_pending_confirm() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        let id = state.commit_draft().unwrap();
        state.delete_confirm = Some(id.clone());
        state.delete_cascade(&id);
        assert!(state.delete_confirm.is_none());
    }

    #[test]
    fn reply_count_counts_only_direct_children() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        let parent_id = state.commit_draft().unwrap();
        for _ in 0..3 {
            state.open_reply_draft(&parent_id, (0.0, 0.0));
            state.editing.as_mut().unwrap().text = "r".into();
            state.commit_draft();
        }
        assert_eq!(state.reply_count(&parent_id), 3);
        assert_eq!(state.reply_count("no-such-id"), 0);
    }

    #[test]
    fn apply_stale_flags_marks_only_mismatched_meshes() {
        let mut state = ReviewState::default();
        let mut anchor_a = anchor_at([0.0, 0.0, 0.0]);
        anchor_a.mesh_index = 0;
        let mut anchor_b = anchor_at([1.0, 0.0, 0.0]);
        anchor_b.mesh_index = 1;
        for (i, anchor) in [anchor_a, anchor_b].iter().enumerate() {
            state.annotations.push(ReviewAnnotation {
                id: format!("id-{i}"),
                created_at: ReviewState::now_rfc3339(),
                updated_at: ReviewState::now_rfc3339(),
                author: None,
                anchor: anchor.clone(),
                category: AnnotationCategory::Info,
                text: format!("note-{i}"),
                reply_to: None,
                resolved: false,
                stale: false,
            });
        }
        let stored = vec!["mesh0_hash".to_string(), "mesh1_hash".to_string()];
        let current = vec!["mesh0_hash".to_string(), "mesh1_DIFFERENT".to_string()];
        let stale_count = state.apply_stale_flags(&stored, &current);
        assert_eq!(stale_count, 1);
        assert!(!state.annotations[0].stale);
        assert!(state.annotations[1].stale);
    }

    #[test]
    fn apply_stale_flags_marks_out_of_range_as_stale() {
        let mut state = ReviewState::default();
        let mut anchor = anchor_at([0.0, 0.0, 0.0]);
        anchor.mesh_index = 5;
        state.annotations.push(ReviewAnnotation {
            id: "id-5".into(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor,
            category: AnnotationCategory::Info,
            text: "note".into(),
            reply_to: None,
            resolved: false,
            stale: false,
        });

        let stale_count = state.apply_stale_flags(
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &["a".to_string(), "b".to_string()],
        );
        assert_eq!(stale_count, 1);
        assert!(state.annotations[0].stale);
    }

    /// Build an identity-camera `view_proj` that maps `[-1, 1]` world coords
    /// 1:1 to NDC: positive X → right, positive Y → up, Z passes through
    /// `[-1, 1]`. With this projection, `world (0, 0, 0)` lands at the
    /// viewport center.
    fn identity_view_proj() -> cgmath::Matrix4<f32> {
        cgmath::Matrix4::from_cols(
            cgmath::Vector4::new(1.0, 0.0, 0.0, 0.0),
            cgmath::Vector4::new(0.0, 1.0, 0.0, 0.0),
            cgmath::Vector4::new(0.0, 0.0, 1.0, 0.0),
            cgmath::Vector4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    fn push_annotation(state: &mut ReviewState, id: &str, world_pos: [f32; 3]) {
        state.annotations.push(ReviewAnnotation {
            id: id.into(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor: AnchorPosition {
                mesh_index: 0,
                face_index: 0,
                barycentric: [1.0 / 3.0; 3],
                world_pos_fallback: world_pos,
            },
            category: AnnotationCategory::Question,
            text: id.into(),
            reply_to: None,
            resolved: false,
            stale: false,
        });
    }

    #[test]
    fn marker_at_screen_pos_finds_nearest_within_threshold() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "left", [-0.5, 0.0, 0.0]);
        push_annotation(&mut state, "right", [0.5, 0.0, 0.0]);

        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);

        let hit = state.marker_at_screen_pos((155.0, 100.0), viewport, view_proj, 20.0);
        assert_eq!(
            hit.as_deref(),
            Some("right"),
            "right marker wins by distance"
        );

        let hit = state.marker_at_screen_pos((45.0, 100.0), viewport, view_proj, 20.0);
        assert_eq!(hit.as_deref(), Some("left"));
    }

    #[test]
    fn marker_at_screen_pos_returns_none_when_all_outside_threshold() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);
        let hit = state.marker_at_screen_pos((150.0, 100.0), viewport, view_proj, 20.0);
        assert!(hit.is_none());
    }

    #[test]
    fn marker_at_screen_pos_skips_markers_behind_camera() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "behind", [0.0, 0.0, 2.0]);
        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);
        let hit = state.marker_at_screen_pos((100.0, 100.0), viewport, view_proj, 20.0);
        assert!(hit.is_none(), "out-of-range NDC z is skipped");
    }

    #[test]
    fn marker_at_screen_pos_skips_reply_annotations() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "parent", [0.0, 0.0, 0.0]);
        let reply = ReviewAnnotation {
            id: "reply".into(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor: AnchorPosition {
                mesh_index: 0,
                face_index: 0,
                barycentric: [1.0 / 3.0; 3],
                world_pos_fallback: [0.0, 0.0, 0.0],
            },
            category: AnnotationCategory::Info,
            text: "fixed".into(),
            reply_to: Some("parent".into()),
            resolved: false,
            stale: false,
        };
        state.annotations.push(reply);
        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);
        let hit = state.marker_at_screen_pos((100.0, 100.0), viewport, view_proj, 20.0);
        assert_eq!(hit.as_deref(), Some("parent"));
    }

    #[test]
    fn begin_reanchor_sets_target_and_selects() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        state.begin_reanchor("a".into());
        assert_eq!(state.reanchor_target.as_deref(), Some("a"));
        assert_eq!(state.selected.as_deref(), Some("a"));
        assert!(state.scroll_to_selected);
    }

    #[test]
    fn begin_reanchor_is_noop_for_unknown_id() {
        let mut state = ReviewState::default();
        state.begin_reanchor("ghost".into());
        assert!(state.reanchor_target.is_none());
        assert!(state.selected.is_none());
        assert!(!state.scroll_to_selected);
    }

    #[test]
    fn cancel_reanchor_clears_target_only() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        state.begin_reanchor("a".into());
        state.cancel_reanchor();
        assert!(state.reanchor_target.is_none());
        assert_eq!(state.selected.as_deref(), Some("a"), "selection preserved");
    }

    fn raycast_hit_at(world: [f32; 3]) -> crate::state::raycast::RaycastHit {
        crate::state::raycast::RaycastHit {
            mesh_index: 2,
            face_index: 17,
            barycentric: [0.25, 0.25, 0.5],
            world_pos: cgmath::Point3::new(world[0], world[1], world[2]),
            distance: 4.2,
        }
    }

    #[test]
    fn complete_reanchor_mutates_anchor_clears_stale_bumps_updated_at() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        state.annotations[0].stale = true;
        let pre_updated = state.annotations[0].updated_at.clone();
        state.annotations[0].updated_at = "2000-01-01T00:00:00Z".into();
        state.begin_reanchor("a".into());

        let hit = raycast_hit_at([3.0, 1.0, -2.0]);
        let ok = state.complete_reanchor(&hit);
        assert!(ok);
        let ann = &state.annotations[0];
        assert_eq!(ann.anchor.mesh_index, 2);
        assert_eq!(ann.anchor.face_index, 17);
        assert!(approx_eq_3(ann.anchor.barycentric, [0.25, 0.25, 0.5]));
        assert!(approx_eq_3(ann.anchor.world_pos_fallback, [3.0, 1.0, -2.0]));
        assert!(!ann.stale, "stale cleared after explicit re-anchor");
        assert_ne!(ann.updated_at, "2000-01-01T00:00:00Z", "updated_at bumped");
        assert!(state.dirty);
        assert!(state.reanchor_target.is_none());
        let _ = pre_updated;
    }

    #[test]
    fn complete_reanchor_returns_false_when_no_target() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        let ok = state.complete_reanchor(&raycast_hit_at([0.0, 0.0, 0.0]));
        assert!(!ok);
        assert!(!state.dirty);
    }

    #[test]
    fn short_text_preview_truncates_and_handles_multiline() {
        assert_eq!(short_text_preview("short"), "short");
        let long = "a".repeat(50);
        let preview = short_text_preview(&long);
        assert!(preview.ends_with('\u{2026}'));
        assert_eq!(preview.chars().count(), 31, "30 chars + ellipsis");
        let multi = short_text_preview("first line\nsecond line");
        assert!(multi.starts_with("first line"));
        assert!(multi.ends_with('\u{2026}'));
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
