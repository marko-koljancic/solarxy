//! Save the current arrangement under a name.
//!
//! The one place a name is typed, so it is a dialog rather than an intent:
//! an intent is `Copy` and carries no text. The state layer drains the
//! committed name and does the saving, because what is saved, the layout,
//! three canvas preferences and the pane split, is not the dialog's to read.

use crate::gui::arrangement::{BUILT_IN, saved_name};

#[derive(Default)]
pub(in crate::gui) struct ArrangementSaveModal {
    pub open: bool,
    typed: String,
    /// Set for the one frame after opening, so the field takes the focus
    /// once rather than holding it against a click elsewhere.
    focus_field: bool,
    committed: Option<String>,
}

impl ArrangementSaveModal {
    pub(in crate::gui) fn open(&mut self) {
        self.open = true;
        self.typed.clear();
        self.focus_field = true;
        self.committed = None;
    }

    /// The name to save under, once, and the dialog closes with it.
    pub(in crate::gui) fn take_committed(&mut self) -> Option<String> {
        let name = self.committed.take()?;
        self.open = false;
        Some(name)
    }

    /// Commit what is typed, if it names anything.
    fn commit(&mut self) {
        if let Some(name) = saved_name(&self.typed) {
            self.committed = Some(name);
        }
    }
}

/// What saving under `name` would do to an arrangement that already has it,
/// said before the user commits rather than discovered after.
pub(in crate::gui) fn replaces(name: &str, users: &[String]) -> Option<&'static str> {
    if users.iter().any(|existing| existing == name) {
        Some("Replaces your arrangement of that name.")
    } else if BUILT_IN.iter().any(|built_in| built_in.name == name) {
        Some("Takes the place of the built-in of that name.")
    } else {
        None
    }
}

pub(in crate::gui) fn draw_arrangement_save_modal(
    ctx: &egui::Context,
    modal: &mut ArrangementSaveModal,
    users: &[String],
) {
    if !modal.open {
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        modal.open = false;
        return;
    }
    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

    let mut save = enter;
    let mut cancel = false;
    egui::Window::new("Save Current As")
        .id(egui::Id::new("solarxy_arrangement_save"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(300.0);
            ui.label("Name");
            let field = ui.add(
                egui::TextEdit::singleline(&mut modal.typed)
                    .hint_text("An arrangement of your own")
                    .desired_width(f32::INFINITY),
            );
            if modal.focus_field {
                field.request_focus();
                modal.focus_field = false;
            }
            let name = saved_name(&modal.typed);
            let note = name.as_deref().and_then(|name| replaces(name, users));
            ui.add_space(2.0);
            ui.label(egui::RichText::new(note.unwrap_or(" ")).small().weak());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(name.is_some(), egui::Button::new("Save"))
                        .on_disabled_hover_text("Type a name first")
                        .clicked()
                    {
                        save = true;
                    }
                });
            });
        });

    if cancel {
        modal.open = false;
    } else if save {
        modal.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name commits once and closes the dialog; nothing typed commits
    /// nothing and leaves it open.
    #[test]
    fn a_name_commits_once_and_an_empty_one_does_not() {
        let mut modal = ArrangementSaveModal::default();
        modal.open();
        assert!(modal.open);

        modal.typed = "   ".to_string();
        modal.commit();
        assert_eq!(modal.take_committed(), None);
        assert!(modal.open, "nothing named, so it stays");

        modal.typed = "  Sculpt ".to_string();
        modal.commit();
        assert_eq!(modal.take_committed().as_deref(), Some("Sculpt"));
        assert!(!modal.open);
        assert_eq!(modal.take_committed(), None, "taken once");
    }

    /// Reopening starts from an empty field rather than the last name.
    #[test]
    fn reopening_forgets_the_last_name() {
        let mut modal = ArrangementSaveModal::default();
        modal.open();
        modal.typed = "Sculpt".to_string();
        modal.open();
        assert!(modal.typed.is_empty());
    }

    #[test]
    fn the_dialog_says_what_a_used_name_would_replace() {
        let users = ["Sculpt".to_string()];
        assert_eq!(
            replaces("Sculpt", &users),
            Some("Replaces your arrangement of that name.")
        );
        assert_eq!(
            replaces("Review", &users),
            Some("Takes the place of the built-in of that name.")
        );
        assert_eq!(replaces("Fresh", &users), None);
    }
}
