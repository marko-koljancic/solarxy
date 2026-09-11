//! The draft-and-commit contract every text-like row follows.
//!
//! **One edit is one command, and therefore one undo step.** A field that
//! wrote per keystroke would recook the graph on every character, and on a
//! `name` parameter it would also rewrite every expression in the document
//! that referenced the old name, so typing a seven-character rename would
//! cost seven commands and seven undo steps to get back.
//!
//! Three rules, and each of them closes a failure that was found rather
//! than imagined. The browser learned all three; this is the same contract
//! as a state machine rather than as a hook, and the one part of it that
//! is a *rule* rather than a shell's plumbing is shared
//! ([`solarxy_studio::expression::should_commit`]).
//!
//! **The stored value wins whenever there is no draft.** An undo, a redo,
//! a name the engine uniquified, or an edit from another surface all move
//! the stored value underneath the field. A field holding its own text
//! permanently would show `box` while the document held `box1`, forever,
//! with no way to notice.
//!
//! **A commit compares against what was last sent, not against the stored
//! value.** The stored value has not travelled back through the cook by
//! the time a commit key also drops focus, so a field comparing against
//! storage sends the same edit twice. The cost is not a wasted write, it
//! is a wrong undo stack: one undo pops the duplicate and the document
//! does not move.
//!
//! **A draft belongs to one row.** Selecting another node, or switching
//! tabs, abandons it rather than carrying half-typed text onto a different
//! parameter.

use solarxy_graph::document::NodeId;

/// The row being typed into.
#[derive(Debug, Clone)]
pub(in crate::gui) struct Draft {
    node: NodeId,
    key: String,
    /// The in-flight text.
    text: String,
    /// What was last written from this draft, so a commit that changes
    /// nothing writes nothing.
    sent: String,
}

impl Draft {
    /// Start a draft on a row, seeded from what is stored there.
    pub(in crate::gui) fn begin(node: NodeId, key: &str, stored: &str) -> Self {
        Self {
            node,
            key: key.to_string(),
            text: stored.to_string(),
            sent: stored.to_string(),
        }
    }

    /// Whether this draft is the given row's.
    pub(in crate::gui) fn owns(&self, node: NodeId, key: &str) -> bool {
        self.node == node && self.key == key
    }

    pub(in crate::gui) fn text(&self) -> &str {
        &self.text
    }

    pub(in crate::gui) fn set(&mut self, text: String) {
        self.text = text;
    }

    /// The text to write, or nothing when the draft changed nothing since
    /// it was last written.
    ///
    /// Marks the text as sent, so a commit key that also drops focus
    /// commits once rather than twice.
    pub(in crate::gui) fn take_commit(&mut self) -> Option<String> {
        if !solarxy_studio::expression::should_commit(&self.text, &self.sent) {
            return None;
        }
        self.sent.clone_from(&self.text);
        Some(self.text.clone())
    }
}

/// What a text row shows: its own draft where it has one, the stored value
/// otherwise.
pub(in crate::gui) fn shown_text<'a>(
    draft: Option<&'a Draft>,
    node: NodeId,
    key: &str,
    stored: &'a str,
) -> &'a str {
    match draft {
        Some(draft) if draft.owns(node, key) => draft.text(),
        _ => stored,
    }
}

/// One frame of the draft-and-commit contract for a text row: the text
/// the widget now shows, and whether it committed.
///
/// A change updates or begins the draft. Escape abandons it, checked before
/// the commit because egui surrenders focus on Escape and the commit would
/// otherwise write the very text the user asked to discard. Losing focus
/// commits, and so does the platform's command modifier with Enter on a
/// prose row. The draft ends whether or not it wrote, which is what lets a
/// value the engine changed underneath come back into the row.
#[allow(clippy::too_many_arguments)]
pub(in crate::gui) fn step(
    ui: &egui::Ui,
    draft: &mut Option<Draft>,
    node: NodeId,
    key: &str,
    stored: &str,
    prose: bool,
    changed: bool,
    response: &egui::Response,
    text: String,
) -> Option<String> {
    if changed {
        if let Some(existing) = draft.as_mut().filter(|d| d.owns(node, key)) {
            existing.set(text);
        } else {
            let mut fresh = Draft::begin(node, key, stored);
            fresh.set(text);
            *draft = Some(fresh);
        }
        return None;
    }
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escaped && (response.has_focus() || response.lost_focus()) {
        *draft = None;
        return None;
    }
    let commit_key = prose
        && response.has_focus()
        && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    if !(response.lost_focus() || commit_key) {
        return None;
    }
    let out = draft
        .as_mut()
        .filter(|d| d.owns(node, key))
        .and_then(Draft::take_commit);
    *draft = None;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: NodeId = NodeId(1);
    const OTHER: NodeId = NodeId(2);

    #[test]
    fn a_row_with_no_draft_shows_what_is_stored() {
        assert_eq!(shown_text(None, NODE, "name", "box"), "box");
    }

    #[test]
    fn a_draft_belongs_to_one_row() {
        let draft = Draft::begin(NODE, "name", "box");
        assert_eq!(shown_text(Some(&draft), NODE, "name", "box"), "box");
        // Another node, and another parameter on the same node, both show
        // storage rather than this draft's text.
        assert_eq!(shown_text(Some(&draft), OTHER, "name", "sphere"), "sphere");
        assert_eq!(shown_text(Some(&draft), NODE, "label", "hello"), "hello");
    }

    #[test]
    fn a_draft_shows_its_own_text_over_the_stored_one() {
        let mut draft = Draft::begin(NODE, "name", "box");
        draft.set("boxe".to_string());
        assert_eq!(
            shown_text(Some(&draft), NODE, "name", "box"),
            "boxe",
            "typing is not discarded by the next frame's read of the document"
        );
    }

    /// A field focused and left alone writes nothing.
    #[test]
    fn an_unchanged_draft_commits_nothing() {
        let mut draft = Draft::begin(NODE, "name", "box");
        assert_eq!(draft.take_commit(), None);
        draft.set("box".to_string());
        assert_eq!(draft.take_commit(), None, "the same text is not an edit");
    }

    /// The commit key drops focus too, so the commit runs twice. It must
    /// write once.
    #[test]
    fn a_commit_that_runs_twice_writes_once() {
        let mut draft = Draft::begin(NODE, "name", "box");
        draft.set("crate".to_string());
        assert_eq!(draft.take_commit().as_deref(), Some("crate"));
        assert_eq!(
            draft.take_commit(),
            None,
            "the second call is the blur that the commit key itself caused"
        );
        // And a further real edit still writes, so the guard is not just a
        // one-shot latch.
        draft.set("crate_2".to_string());
        assert_eq!(draft.take_commit().as_deref(), Some("crate_2"));
    }

    /// The uniquify case, which is the reason a commit ends the draft
    /// rather than keeping it: two nodes cannot both be `box`, so the
    /// engine stores `box1` and the row has to show that.
    #[test]
    fn dropping_the_draft_is_what_lets_a_uniquified_name_come_back() {
        let mut draft = Draft::begin(NODE, "name", "sphere");
        draft.set("box".to_string());
        assert_eq!(draft.take_commit().as_deref(), Some("box"));
        // The engine uniquified it. With the draft dropped, the row reads
        // storage again.
        assert_eq!(shown_text(None, NODE, "name", "box1"), "box1");
        // Had the draft been kept, it would have gone on claiming the name
        // the document does not hold.
        assert_eq!(shown_text(Some(&draft), NODE, "name", "box1"), "box");
    }
}
