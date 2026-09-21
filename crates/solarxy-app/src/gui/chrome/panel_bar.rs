//! A panel's own menu bar.
//!
//! A panel owns its commands, so its bar is drawn by the panel's module, at
//! the top of the `Ui` the dock hands it, and moves with the panel wherever
//! it is docked or floated. This is the frame every such bar shares, and the
//! one entry every one of them ends with.

use super::menu_items::entry;
use crate::gui::dock::SolarxyTab;
use crate::gui::intent::{Intent, Intents, LayoutIntent};
use crate::gui::theme::Theme;
use crate::state::keymap::Action;

/// Draw a menu bar across the top of a panel.
///
/// Filled rather than transparent, which the viewport's bar depends on: that
/// tab paints no background so the scene shows through, and a bar drawn
/// straight onto it would be text floating over the render.
pub(in crate::gui) fn panel_bar(
    ui: &mut egui::Ui,
    theme: Theme,
    contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(theme.bg_elevated)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::MenuBar::new().ui(ui, contents);
        });
}

/// The label of the entry every panel's View menu ends with. One constant,
/// so the entry and the tests that hold a menu against the browser's cannot
/// come to spell it differently.
pub(in crate::gui) const MAXIMIZE_LABEL: &str = "Maximize Panel";

/// The entry every panel's View menu ends with. It toggles, so the same
/// entry is the way back, as are the key it shows and Escape.
pub(in crate::gui) fn maximize_entry(ui: &mut egui::Ui, tab: SolarxyTab, intents: &mut Intents) {
    if entry(ui, MAXIMIZE_LABEL, Some(Action::PanelMaximize)).clicked() {
        intents.raise(Intent::Layout(LayoutIntent::ToggleMaximize(tab)));
        ui.close();
    }
}
