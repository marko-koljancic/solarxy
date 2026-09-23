//! Review-mode interaction state: what the interface is doing right now
//! with the document's annotations, which the engine owns.
//!
//! The annotations live in the document's review store and reach the panels
//! as a per-frame snapshot; every change to one is an engine command and one
//! undo step, so nothing here is a store and nothing here is dirty. What
//! lives here is the rest: whether the mode is on, which note is selected or
//! hovered, the draft in the popup, the panel's filters, and a pending
//! re-anchor. Markers are drawn as an egui overlay (see
//! `gui::panels::review::overlay`) from the snapshot and this state.
//!
//! The sidecar half lives in `sidecar.rs`, the only part that touches disk.

// Three fields describe the sidecar a file-loaded model kept beside it, which
// nothing writes now that the document carries the notes. They go with the
// import and export work, and the allowance goes with them.
#![allow(dead_code)]

use std::path::PathBuf;

use solarxy_graph::Command;
use solarxy_graph::review::{Annotation, AnnotationId, ReviewAnchor, ReviewCategory};

/// Top-level review-mode state on `State`. Initialized via [`Default`]
/// (which seeds the filter defaults) and written by the popup, the panel,
/// the overlay's hover pass and the pointer ladder.
#[derive(Debug)]
pub struct ReviewState {
    /// True between R-press and R-press-again. The click ladder in
    /// `state::input` consults this to decide whether a click in a pane
    /// places a note.
    pub active: bool,

    /// The selected annotation, if any. Its marker renders with the accent
    /// ring and the panel shows its editor.
    pub selected: Option<AnnotationId>,

    /// Popup state while the user is writing a new note, a reply, or an
    /// edit of an existing note. `None` means no popup is open.
    pub editing: Option<EditDraft>,

    /// SHA-256 of the model file at load time, from before the document
    /// carried the notes. Nothing writes it.
    pub model_hash: Option<String>,

    /// Per-mesh SHA-256 of a file-loaded model, from before the document
    /// carried the notes. Nothing writes it.
    pub mesh_hashes: Vec<String>,

    /// The sidecar a file-loaded model kept beside it. Nothing writes it.
    pub sidecar_path: Option<PathBuf>,

    /// Mirror of `Preferences::review.author`; cached here so the draft
    /// commit does not thread through the preferences each frame. Refreshed
    /// when the preferences change.
    pub author: Option<String>,

    /// Whether the side panel is visible. Mirrors dock membership after the
    /// drain; entering review mode sets it so the panel opens.
    pub panel_open: bool,

    /// Per-category filter chips on the panel, indexed by the category's
    /// ordinal (Info, Warning, Question, Change). `true` means visible.
    pub category_filters: [bool; 4],

    /// `true` shows resolved annotations in their own section; `false`
    /// hides them entirely. Default `true`, since a resolved note keeps the
    /// conversation's context.
    pub show_resolved: bool,

    /// `true` suppresses the viewport marker overlay; the panel still lists
    /// every annotation. Session-only, never persisted.
    pub markers_hidden: bool,

    /// Case-insensitive substring filter over a note's text, its author and
    /// its replies. Empty means no filter.
    pub text_filter: String,

    /// `Some(id)` while the delete confirmation is open. Cleared on Cancel,
    /// and by the drain once the delete has landed.
    pub delete_confirm: Option<AnnotationId>,

    /// `Some(id)` while the user is in the re-anchor sub-mode for that
    /// annotation. The next click on geometry re-places it; Esc cancels.
    pub reanchor_target: Option<AnnotationId>,

    /// One-shot flag: when `true`, the panel scrolls the selected row into
    /// view next frame, then clears it. Set by a pin click and by
    /// `begin_reanchor` to keep the panel aligned with the viewport.
    pub scroll_to_selected: bool,

    /// One-shot request, `Some(id)` after a row is clicked in the panel. The
    /// state layer flies the active pane's camera to that note's marker,
    /// then clears it.
    pub focus_request: Option<AnnotationId>,

    /// One-shot: set by the panel's Save button; the state layer writes the
    /// sidecar and clears it.
    pub save_requested: bool,

    /// The marker under the cursor, if any. Written by the overlay's hover
    /// pass from what it drew, read by the click ladder so a click on a pin
    /// selects it rather than picking through it. `None` when the cursor is
    /// over no pin.
    pub hovered: Option<AnnotationId>,

    /// Monotonically increasing counter keying the popup window by draft
    /// session rather than by click pixel. Each new draft takes a fresh
    /// value via [`Self::alloc_draft_seq`] so egui's cached window position
    /// resets cleanly for a draft opened somewhere else.
    pub next_draft_seq: u64,
}

impl Default for ReviewState {
    fn default() -> Self {
        Self {
            active: false,
            selected: None,
            editing: None,
            model_hash: None,
            mesh_hashes: Vec::new(),
            sidecar_path: None,
            author: None,
            panel_open: false,
            category_filters: [true; 4],
            show_resolved: true,
            markers_hidden: false,
            text_filter: String::new(),
            delete_confirm: None,
            reanchor_target: None,
            scroll_to_selected: false,
            focus_request: None,
            save_requested: false,
            hovered: None,
            next_draft_seq: 0,
        }
    }
}

/// Best-effort first-line preview of annotation text, truncated to about
/// 30 characters with a trailing ellipsis when shortened. Used by toast and
/// banner messages.
pub fn short_text_preview(text: &str) -> String {
    let first: String = text.lines().next().unwrap_or("").chars().take(30).collect();
    if text.lines().count() > 1 || text.chars().count() > first.chars().count() {
        format!("{first}\u{2026}")
    } else {
        first
    }
}

/// The popup's form in progress: a new note, a reply, or an edit. Created
/// by the click ladder or the panel; committed as one engine command through
/// [`ReviewState::take_draft_command`], or discarded.
#[derive(Debug, Clone)]
pub struct EditDraft {
    /// Where the note is pinned. From the pick for a new note; a reply and
    /// an edit carry their note's anchor so the popup has one to show, and
    /// the engine ignores a reply's in favour of the parent's.
    pub anchor: ReviewAnchor,

    /// Logical screen position of the click that opened the draft, used to
    /// place the popup near it. The viewport centre for drafts opened from
    /// the panel.
    pub screen_pos: (f32, f32),

    /// The text so far.
    pub text: String,

    /// The chosen category. A new note starts as a question, the canonical
    /// "what should change here?"; a reply starts as its parent's.
    pub category: ReviewCategory,

    /// `Some(id)` when editing an existing note; `None` when creating one.
    pub editing_id: Option<AnnotationId>,

    /// `Some(parent)` when the draft is a reply; `None` for a top-level
    /// note. Replies share the parent's anchor and draw no marker of their
    /// own.
    pub reply_to: Option<AnnotationId>,

    /// Unique per draft session, from [`ReviewState::alloc_draft_seq`]; the
    /// popup keys its window on it.
    pub seq: u64,
}

impl EditDraft {
    /// A fresh draft for a new top-level note at `anchor`.
    pub fn new_at(seq: u64, anchor: ReviewAnchor, screen_pos: (f32, f32)) -> Self {
        Self {
            anchor,
            screen_pos,
            text: String::new(),
            category: ReviewCategory::Question,
            editing_id: None,
            reply_to: None,
            seq,
        }
    }

    /// A draft replying to `parent`, in the parent's category and at the
    /// parent's anchor, as the browser opens one.
    pub fn new_reply(seq: u64, parent: &Annotation, screen_pos: (f32, f32)) -> Self {
        Self {
            anchor: parent.anchor.clone(),
            screen_pos,
            text: String::new(),
            category: parent.category,
            editing_id: None,
            reply_to: Some(parent.id),
            seq,
        }
    }

    /// A draft editing `note`, pre-filled with its text and category.
    pub fn for_edit(seq: u64, note: &Annotation, screen_pos: (f32, f32)) -> Self {
        Self {
            anchor: note.anchor.clone(),
            screen_pos,
            text: note.text.clone(),
            category: note.category,
            editing_id: Some(note.id),
            reply_to: note.reply_to,
            seq,
        }
    }
}

impl ReviewState {
    /// Allocate the next draft session id.
    pub fn alloc_draft_seq(&mut self) -> u64 {
        self.next_draft_seq = self.next_draft_seq.wrapping_add(1);
        self.next_draft_seq
    }

    /// RFC 3339 UTC timestamp ("YYYY-MM-DDTHH:MM:SS.sssZ").
    pub fn now_rfc3339() -> String {
        use time::OffsetDateTime;
        use time::format_description::well_known::Rfc3339;
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string())
    }

    /// Take the open draft as the engine command it stands for: an add for a
    /// new note or a reply, an edit when `editing_id` is set. Clears the
    /// draft. `now` stamps the note; the author is this state's mirror of
    /// the preference. `None` when no draft is open.
    pub fn take_draft_command(&mut self, now: String) -> Option<Command> {
        let draft = self.editing.take()?;
        Some(match draft.editing_id {
            Some(id) => Command::EditAnnotation {
                id,
                text: draft.text,
                category: draft.category,
                updated_at: now,
            },
            None => Command::AddAnnotation {
                anchor: draft.anchor,
                text: draft.text,
                category: draft.category,
                author: self.author.clone(),
                created_at: now,
                reply_to: draft.reply_to,
            },
        })
    }

    /// Discard the open draft (Cancel / Esc).
    pub fn cancel_draft(&mut self) {
        self.editing = None;
    }

    /// Toggle review mode. The caller gives the toast; this flips the bit.
    pub fn toggle_active(&mut self) -> bool {
        self.active = !self.active;
        // Leaving review mode closes any open draft and collapses any
        // expanded marker card: selection and hover are review-mode
        // interface state with no meaning once the mode is off.
        if !self.active {
            self.editing = None;
            self.selected = None;
            self.hovered = None;
        }
        self.active
    }

    /// Drop every piece of interaction state that named a note of the
    /// document being replaced. The mode and the filters are the user's and
    /// survive, so closing one scene in review mode and opening another
    /// keeps review mode on.
    pub fn clear_for_new_document(&mut self) {
        self.selected = None;
        self.editing = None;
        self.hovered = None;
        self.focus_request = None;
        self.reanchor_target = None;
        self.delete_confirm = None;
        self.scroll_to_selected = false;
    }

    /// Open the popup as a reply to `parent`. `screen_pos` places the popup;
    /// pass the viewport centre when opening from the panel.
    pub fn open_reply_draft(&mut self, parent: &Annotation, screen_pos: (f32, f32)) {
        let seq = self.alloc_draft_seq();
        self.editing = Some(EditDraft::new_reply(seq, parent, screen_pos));
    }

    /// Open the popup editing `note`, pre-filled.
    pub fn open_edit_draft(&mut self, note: &Annotation, screen_pos: (f32, f32)) {
        let seq = self.alloc_draft_seq();
        self.editing = Some(EditDraft::for_edit(seq, note, screen_pos));
    }
}

mod anchor;
mod sidecar;

pub(crate) use anchor::anchor_from_pick;

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::document::{GraphContext, NodeId};

    fn anchor_at(pos: [f32; 3]) -> ReviewAnchor {
        ReviewAnchor {
            ctx: GraphContext::Root,
            node: NodeId(7),
            mesh: Some(0),
            face: Some(0),
            barycentric: Some([1.0 / 3.0; 3]),
            world_fallback: Some(pos),
            geometry_hash: None,
        }
    }

    fn note(id: u64, text: &str, category: ReviewCategory) -> Annotation {
        Annotation {
            id: AnnotationId(id),
            anchor: anchor_at([3.5, 1.2, -0.4]),
            text: text.into(),
            category,
            resolved: false,
            author: Some("Tester".into()),
            created_at: "2026-07-10T09:00:00Z".into(),
            updated_at: "2026-07-10T09:00:00Z".into(),
            reply_to: None,
        }
    }

    fn state_with_draft(draft: EditDraft) -> ReviewState {
        ReviewState {
            editing: Some(draft),
            ..Default::default()
        }
    }

    #[test]
    fn take_draft_command_for_a_new_note_is_an_add_with_the_drafts_fields() {
        let mut state = ReviewState {
            author: Some("Marko".into()),
            ..Default::default()
        };
        state.editing = Some(EditDraft {
            anchor: anchor_at([1.0, 2.0, 3.0]),
            screen_pos: (100.0, 200.0),
            text: "Looks off".into(),
            category: ReviewCategory::Warning,
            editing_id: None,
            reply_to: None,
            seq: 0,
        });
        let cmd = state
            .take_draft_command("2026-09-23T10:00:00Z".into())
            .expect("a draft was open");
        assert!(state.editing.is_none(), "the draft is taken");
        match cmd {
            Command::AddAnnotation {
                anchor,
                text,
                category,
                author,
                created_at,
                reply_to,
            } => {
                assert_eq!(anchor, anchor_at([1.0, 2.0, 3.0]));
                assert_eq!(text, "Looks off");
                assert_eq!(category, ReviewCategory::Warning);
                assert_eq!(author.as_deref(), Some("Marko"));
                assert_eq!(created_at, "2026-09-23T10:00:00Z");
                assert!(reply_to.is_none());
            }
            other => panic!("a new note is an add, got {other:?}"),
        }
    }

    #[test]
    fn take_draft_command_for_a_reply_carries_the_parent_id_and_category() {
        let parent = note(4, "Parent", ReviewCategory::Change);
        let mut state = ReviewState::default();
        state.open_reply_draft(&parent, (500.0, 250.0));
        let draft = state.editing.as_ref().expect("reply draft open");
        assert_eq!(draft.reply_to, Some(AnnotationId(4)));
        assert_eq!(draft.screen_pos, (500.0, 250.0));
        assert!(draft.editing_id.is_none());
        assert_eq!(
            draft.anchor, parent.anchor,
            "the draft carries the parent's anchor"
        );
        assert_eq!(draft.category, ReviewCategory::Change);
        state.editing.as_mut().unwrap().text = "Fixed in v2".into();
        match state.take_draft_command("now".into()).unwrap() {
            Command::AddAnnotation { reply_to, text, .. } => {
                assert_eq!(reply_to, Some(AnnotationId(4)));
                assert_eq!(text, "Fixed in v2");
            }
            other => panic!("a reply is an add, got {other:?}"),
        }
    }

    #[test]
    fn take_draft_command_for_an_edit_is_an_edit_of_that_id() {
        let existing = note(9, "Old text", ReviewCategory::Info);
        let mut state = ReviewState::default();
        state.open_edit_draft(&existing, (0.0, 0.0));
        {
            let draft = state.editing.as_mut().expect("edit draft open");
            assert_eq!(draft.text, "Old text", "pre-filled with the note's text");
            assert_eq!(draft.category, ReviewCategory::Info);
            draft.text = "Updated text".into();
            draft.category = ReviewCategory::Change;
        }
        match state.take_draft_command("later".into()).unwrap() {
            Command::EditAnnotation {
                id,
                text,
                category,
                updated_at,
            } => {
                assert_eq!(id, AnnotationId(9));
                assert_eq!(text, "Updated text");
                assert_eq!(category, ReviewCategory::Change);
                assert_eq!(updated_at, "later");
            }
            other => panic!("an edit is an edit, got {other:?}"),
        }
    }

    #[test]
    fn take_draft_command_answers_none_without_a_draft() {
        let mut state = ReviewState::default();
        assert!(state.take_draft_command("now".into()).is_none());
    }

    #[test]
    fn cancel_draft_discards_without_a_command() {
        let mut state = state_with_draft(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        state.cancel_draft();
        assert!(state.editing.is_none());
        assert!(state.take_draft_command("now".into()).is_none());
    }

    #[test]
    fn a_new_note_starts_as_a_question() {
        let draft = EditDraft::new_at(1, anchor_at([0.0; 3]), (0.0, 0.0));
        assert_eq!(draft.category, ReviewCategory::Question);
        assert!(draft.reply_to.is_none());
        assert!(draft.editing_id.is_none());
    }

    #[test]
    fn toggle_active_clears_open_draft_on_exit() {
        let mut state = ReviewState::default();
        state.toggle_active();
        assert!(state.active);
        state.editing = Some(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));
        state.selected = Some(AnnotationId(1));
        state.toggle_active();
        assert!(!state.active);
        assert!(state.editing.is_none(), "draft auto-cancelled on exit");
        assert!(state.selected.is_none());
    }

    #[test]
    fn clear_for_new_document_keeps_the_mode_and_drops_the_interaction() {
        let mut state = ReviewState {
            active: true,
            selected: Some(AnnotationId(1)),
            hovered: Some(AnnotationId(1)),
            focus_request: Some(AnnotationId(2)),
            reanchor_target: Some(AnnotationId(1)),
            delete_confirm: Some(AnnotationId(3)),
            scroll_to_selected: true,
            category_filters: [true, false, true, true],
            ..Default::default()
        };
        state.editing = Some(EditDraft::new_at(0, anchor_at([0.0; 3]), (0.0, 0.0)));

        state.clear_for_new_document();

        assert!(state.active, "the mode is the user's");
        assert_eq!(state.category_filters, [true, false, true, true]);
        assert!(state.selected.is_none());
        assert!(state.hovered.is_none());
        assert!(state.editing.is_none());
        assert!(state.focus_request.is_none());
        assert!(state.reanchor_target.is_none());
        assert!(state.delete_confirm.is_none());
        assert!(!state.scroll_to_selected);
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
}
