//! The recovery offer: restore the newest autosave, or discard it.
//!
//! Asked once, on launch, and only when the ring holds something, which is
//! only after a session ended without the user choosing to end it. It
//! cannot be dismissed without answering, as the browser's cannot: a stale
//! autosave left pending would nag on every launch, and a dismissed one
//! would be lost without a word.

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryChoice {
    Restore,
    Discard,
}

#[derive(Default)]
pub(in crate::gui) struct RecoveryModalState {
    pub open: bool,
    name: String,
    when: String,
    choice: Option<RecoveryChoice>,
}

impl RecoveryModalState {
    pub(in crate::gui) fn open(&mut self, name: &str, when: &str) {
        self.open = true;
        self.name = name.to_string();
        self.when = when.to_string();
        self.choice = None;
    }

    pub(in crate::gui) fn take_choice(&mut self) -> Option<RecoveryChoice> {
        let choice = self.choice.take()?;
        self.open = false;
        Some(choice)
    }
}

/// What the prompt says about what it found.
pub(crate) fn description(name: &str, when: &str) -> String {
    format!("Unsaved work on {name} was autosaved at {when}.")
}

pub(in crate::gui) fn draw_recovery_modal(ctx: &egui::Context, modal: &mut RecoveryModalState) {
    if !modal.open {
        return;
    }
    let mut choice = None;
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        choice = Some(RecoveryChoice::Restore);
    }

    egui::Window::new("Recover unsaved work?")
        .id(egui::Id::new("solarxy_recovery"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label(description(&modal.name, &modal.when));
            ui.add_space(4.0);
            ui.label("Restore it to keep working, or discard it.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Discard").clicked() {
                    choice = Some(RecoveryChoice::Discard);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Restore").clicked() {
                        choice = Some(RecoveryChoice::Restore);
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

    /// The offer names what was found and when, and an answer closes it.
    #[test]
    fn the_offer_names_the_work_and_an_answer_closes_it() {
        assert_eq!(
            description("shot.slxy", "2026-09-11 14:02"),
            "Unsaved work on shot.slxy was autosaved at 2026-09-11 14:02."
        );
        let mut modal = RecoveryModalState::default();
        modal.open("shot.slxy", "2026-09-11 14:02");
        assert!(modal.open);
        assert_eq!(modal.take_choice(), None);
        assert!(modal.open, "unanswered, it stays");
        modal.choice = Some(RecoveryChoice::Restore);
        assert_eq!(modal.take_choice(), Some(RecoveryChoice::Restore));
        assert!(!modal.open);
    }
}
