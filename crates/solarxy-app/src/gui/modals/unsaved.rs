//! The unsaved-changes prompt: Save, Don't Save, or Cancel.
//!
//! One prompt for every path that would discard the document (quitting,
//! starting a new scene, opening another file), so the wording and the
//! buttons cannot drift between them. The modal owns only the question and
//! the answer; what the answer does is the state layer's business, drained
//! through `take_choice` per the modals contract.

use crate::gui::theme::Theme;

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsavedChoice {
    /// Write the document, then go on with the action.
    Save,
    /// Go on with the action and lose the changes.
    Discard,
    /// Stay, with the document intact.
    Cancel,
}

/// The action the prompt stands in front of, for its wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscardWhat {
    Quit,
    NewScene,
    OpenFile,
}

impl DiscardWhat {
    fn phrase(self) -> &'static str {
        match self {
            Self::Quit => "before quitting",
            Self::NewScene => "before starting a new scene",
            Self::OpenFile => "before opening another file",
        }
    }
}

#[derive(Default)]
pub(in crate::gui) struct UnsavedModalState {
    pub open: bool,
    filename: String,
    what: Option<DiscardWhat>,
    choice: Option<UnsavedChoice>,
}

impl UnsavedModalState {
    /// Ask the question. A prompt already open is replaced, which is what a
    /// second discarding gesture during the first should do.
    pub(in crate::gui) fn open(&mut self, filename: &str, what: DiscardWhat) {
        self.open = true;
        self.filename = filename.to_string();
        self.what = Some(what);
        self.choice = None;
    }

    /// The answer, once, closing the prompt.
    pub(in crate::gui) fn take_choice(&mut self) -> Option<UnsavedChoice> {
        let choice = self.choice.take()?;
        self.open = false;
        Some(choice)
    }
}

/// The question the prompt asks, given the file and the action.
pub(crate) fn question(filename: &str, what: DiscardWhat) -> String {
    format!("Save changes to {filename} {}?", what.phrase())
}

pub(in crate::gui) fn draw_unsaved_modal(
    ctx: &egui::Context,
    modal: &mut UnsavedModalState,
    theme: &Theme,
) {
    if !modal.open {
        return;
    }
    let Some(what) = modal.what else {
        modal.open = false;
        return;
    };

    let mut choice = None;
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        choice = Some(UnsavedChoice::Cancel);
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        choice = Some(UnsavedChoice::Save);
    }

    egui::Window::new("Unsaved changes")
        .id(egui::Id::new("solarxy_unsaved_changes"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(320.0);
            ui.label(question(&modal.filename, what));
            ui.add_space(4.0);
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    choice = Some(UnsavedChoice::Cancel);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(UnsavedChoice::Save);
                    }
                    if ui
                        .button(egui::RichText::new("Don't Save").color(theme.severity_error))
                        .clicked()
                    {
                        choice = Some(UnsavedChoice::Discard);
                    }
                });
            });
        });

    if choice.is_some() {
        modal.choice = choice;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wording names the file and the action, so a user who pressed
    /// quit and a user who pressed new scene read different questions.
    #[test]
    fn the_question_names_the_file_and_the_action() {
        assert_eq!(
            question("shot.slxy", DiscardWhat::Quit),
            "Save changes to shot.slxy before quitting?"
        );
        assert_eq!(
            question("Untitled", DiscardWhat::NewScene),
            "Save changes to Untitled before starting a new scene?"
        );
        assert_eq!(
            question("shot.slxy", DiscardWhat::OpenFile),
            "Save changes to shot.slxy before opening another file?"
        );
    }

    /// An answer is handed out once and closes the prompt; a prompt with no
    /// answer hands out nothing and stays open.
    #[test]
    fn an_answer_is_taken_once_and_closes_the_prompt() {
        let mut modal = UnsavedModalState::default();
        modal.open("shot.slxy", DiscardWhat::Quit);
        assert!(modal.open);
        assert_eq!(modal.take_choice(), None);
        assert!(modal.open, "no answer yet, so the prompt stays");

        modal.choice = Some(UnsavedChoice::Discard);
        assert_eq!(modal.take_choice(), Some(UnsavedChoice::Discard));
        assert!(!modal.open);
        assert_eq!(modal.take_choice(), None, "the answer was already taken");
    }

    /// Reopening replaces the question and drops a stale answer, so a second
    /// gesture cannot be answered by the first prompt's button.
    #[test]
    fn reopening_replaces_the_question_and_drops_a_stale_answer() {
        let mut modal = UnsavedModalState::default();
        modal.open("a.slxy", DiscardWhat::Quit);
        modal.choice = Some(UnsavedChoice::Save);
        modal.open("b.slxy", DiscardWhat::NewScene);
        assert_eq!(modal.filename, "b.slxy");
        assert_eq!(modal.what, Some(DiscardWhat::NewScene));
        assert_eq!(modal.take_choice(), None);
    }
}
