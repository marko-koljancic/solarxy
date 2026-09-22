//! The menu-item vocabulary: what an entry in any of the shell's menus is.
//!
//! The browser describes a menu as data and renders the tree. This shell
//! draws in immediate mode, so the same vocabulary is **functions rather than
//! a struct**: building a list of boxed closures every frame for a menu that
//! is closed would be the wrong shape, and a menu driven by the registry is
//! the case that proves it.
//!
//! Three rules every helper keeps, so no menu has to remember them.
//!
//! - **A hint is read from the binding table**, never typed in. A rebinding
//!   cannot leave a menu describing a key that does something else, which is
//!   how a menu came to advertise a key nothing was bound to.
//! - **A disabled entry says why.** The reason is an argument rather than an
//!   afterthought, so an entry cannot be greyed out in silence.
//! - **A helper returns the response.** An intent is raised only from a widget
//!   response, and handing the response back is what keeps that true at the
//!   call site (see `gui::intent`).

use crate::state::keymap::{Action, hint};

/// A button labelled and hinted the way every entry is, for the one menu
/// that draws its rows itself.
pub(in crate::gui) fn button(label: &str, action: Option<Action>) -> egui::Button<'_> {
    let button = egui::Button::new(label);
    match action.and_then(hint) {
        Some(keys) => button.shortcut_text(keys),
        None => button,
    }
}

/// A plain entry, with the key its action is bound to.
pub(in crate::gui) fn entry(
    ui: &mut egui::Ui,
    label: &str,
    action: Option<Action>,
) -> egui::Response {
    ui.add(button(label, action))
}

/// An entry that is on or off. A button drawn selected rather than a
/// selectable label, because only a button carries a shortcut hint.
pub(in crate::gui) fn check_entry(
    ui: &mut egui::Ui,
    checked: bool,
    label: &str,
    action: Option<Action>,
) -> egui::Response {
    ui.add(button(label, action).selected(checked))
}

/// An entry that may not apply right now, and says why when it does not.
pub(in crate::gui) fn entry_if(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    action: Option<Action>,
    why_not: &str,
) -> egui::Response {
    ui.add_enabled(enabled, button(label, action))
        .on_disabled_hover_text(why_not)
}

/// An entry for something this shell does not do yet. It is listed where
/// the browser lists it, so the two menus read the same and the capability
/// reads as not yet here rather than as never having existed, and it says
/// what it is waiting for.
pub(in crate::gui) fn waiting_entry(
    ui: &mut egui::Ui,
    label: &str,
    waits_for: &str,
) -> egui::Response {
    ui.add_enabled(false, egui::Button::new(label))
        .on_disabled_hover_text(waits_for)
}
