//! The guard in front of every action that would discard the document.
//!
//! Quitting, starting a new scene and opening another file all replace or
//! drop what is open. Each asks the same question when the document is
//! dirty, through one prompt, and runs at once when it is not. The action
//! waits here while the prompt is up; the answer decides whether it runs.
//!
//! **A save that does not happen keeps the document.** Choosing Save and
//! then backing out of the file dialog, or a save that fails, leaves the
//! application exactly where it was: the action is dropped rather than run
//! over an unsaved document, because the user asked for the document to be
//! kept and it was not.

use std::path::PathBuf;

use super::State;
use crate::gui::{DiscardWhat, UnsavedChoice};

/// An action that replaces or drops the open document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiscardAction {
    Quit,
    NewScene,
    /// Open through the file router, which is where a scene or a model
    /// replaces the document.
    OpenFile(PathBuf),
}

impl DiscardAction {
    fn what(&self) -> DiscardWhat {
        match self {
            Self::Quit => DiscardWhat::Quit,
            Self::NewScene => DiscardWhat::NewScene,
            Self::OpenFile(_) => DiscardWhat::OpenFile,
        }
    }
}

/// Whether the action goes ahead, given the answer and whether a save the
/// answer asked for actually happened.
///
/// Pure, and the whole rule: Cancel never proceeds, Discard always does, and
/// Save proceeds only when the file was written.
pub(super) fn proceeds(choice: UnsavedChoice, saved: bool) -> bool {
    match choice {
        UnsavedChoice::Cancel => false,
        UnsavedChoice::Discard => true,
        UnsavedChoice::Save => saved,
    }
}

impl State {
    /// Run `action` now, or ask first when the document has unsaved changes.
    pub(super) fn guard_discard(&mut self, action: DiscardAction) {
        if !self.is_dirty() {
            self.run_discard(action);
            return;
        }
        let filename = self
            .engine_scene
            .as_ref()
            .map_or("Untitled", |s| s.filename.as_str())
            .to_string();
        self.gui.open_unsaved_prompt(&filename, action.what());
        self.pending_discard = Some(action);
    }

    /// The prompt's answer, applied to the action waiting on it.
    pub(super) fn resolve_discard(&mut self, choice: UnsavedChoice) {
        let Some(action) = self.pending_discard.take() else {
            return;
        };
        let saved = match choice {
            UnsavedChoice::Save => self.save_document(),
            UnsavedChoice::Discard => {
                // Chosen, so a launch must not offer back what was let go.
                self.clear_autosaves();
                false
            }
            UnsavedChoice::Cancel => false,
        };
        if proceeds(choice, saved) {
            self.run_discard(action);
        }
    }

    /// The action itself, past the guard.
    fn run_discard(&mut self, action: DiscardAction) {
        match action {
            DiscardAction::Quit => self.quit_requested = true,
            DiscardAction::NewScene => self.new_scene_now(),
            DiscardAction::OpenFile(path) => self.open_file_now(path),
        }
    }

    /// Ask to leave, from the window's close button or the File menu.
    pub fn request_quit(&mut self) {
        self.guard_discard(DiscardAction::Quit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::Command;
    use solarxy_graph::document::GraphContext;
    use solarxy_graph::engine::Engine;

    /// Cancel never proceeds, Discard always does, and Save proceeds only
    /// when the file was actually written: a save backed out of or failed
    /// leaves the document where it was.
    #[test]
    fn the_action_proceeds_only_when_the_document_is_kept_or_knowingly_dropped() {
        assert!(!proceeds(UnsavedChoice::Cancel, false));
        assert!(!proceeds(UnsavedChoice::Cancel, true));
        assert!(proceeds(UnsavedChoice::Discard, false));
        assert!(proceeds(UnsavedChoice::Save, true));
        assert!(
            !proceeds(UnsavedChoice::Save, false),
            "a save that did not happen keeps the document"
        );
    }

    /// The dirty state is the engine's revision, so this pins what moves it:
    /// a command does, an undo does, and a read or an idle tick does not.
    /// The two consequences this shares with the browser are pinned too,
    /// because they are decisions rather than accidents: a selection is a
    /// command and dirties, and undoing back to the saved point stays dirty.
    #[test]
    fn the_revision_is_the_dirty_authority_and_moves_only_on_commands() {
        let mut engine = Engine::new().expect("engine");
        let saved = engine.revision();

        let _ = engine.document();
        let _ = engine.node_report(GraphContext::Root, solarxy_graph::document::NodeId(1));
        let _ = engine.tick();
        assert_eq!(
            engine.revision(),
            saved,
            "reads and an idle tick do not dirty"
        );

        engine
            .apply(Command::AddNode {
                ctx: GraphContext::Root,
                node_type: "sopnet".to_string(),
                position: [0.0, 0.0],
            })
            .expect("adds");
        let after_add = engine.revision();
        assert_ne!(after_add, saved, "a command dirties");

        engine
            .apply(Command::SetSelection {
                ctx: GraphContext::Root,
                ids: Vec::new(),
            })
            .expect("selects");
        assert_ne!(
            engine.revision(),
            after_add,
            "a selection change is a command and dirties"
        );

        engine.apply(Command::Undo).expect("undoes");
        engine.apply(Command::Undo).expect("undoes");
        assert_ne!(
            engine.revision(),
            saved,
            "undoing back to the saved point stays dirty, as it does in the browser"
        );
    }
}
