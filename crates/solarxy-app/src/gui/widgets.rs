//! The widget helpers more than one surface uses.
//!
//! Both of these lived inside a panel until the interface split made the
//! borrowing visible: a modal reached into the sidebar for a labelled combo,
//! and another reached into the pane toolbar for the background picker. A
//! shared widget belongs with the shared chrome, not inside whichever panel
//! happened to need it first.
//!
//! Both return what the user picked rather than writing through a borrow,
//! which is the rule everywhere a panel draws: what it wants travels as an
//! intent, applied after the pass.

use solarxy_core::preferences::{BackgroundMode, BuiltinBg, CustomBackground};

/// A labelled combo over `all`, returning the variant the user picked.
pub(in crate::gui) fn combo_with_tooltip<T>(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    current: T,
    all: &[T],
) -> Option<T>
where
    T: Copy + PartialEq + std::fmt::Display,
{
    let mut picked = None;
    ui.horizontal(|ui| {
        let mut value = current;
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.to_string())
            .width(140.0)
            .show_ui(ui, |ui| {
                for &variant in all {
                    if ui
                        .selectable_value(&mut value, variant, variant.to_string())
                        .changed()
                    {
                        picked = Some(variant);
                    }
                }
            });
        ui.label(label).on_hover_text(shortcut);
    });
    picked
}

/// The background picker as a combo: the builtins, with `HDRI Sky` gated on
/// one being loaded, then every user custom under a separator.
///
/// Retained as a combo for the preferences modal's custom-background editor;
/// the pane toolbars draw the same choice as a menu instead.
pub(in crate::gui) fn background_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    current: &mut BackgroundMode,
    customs: &[CustomBackground],
    hdri_available: bool,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.label(customs))
        .show_ui(ui, |ui| {
            for &builtin in BuiltinBg::ALL {
                if builtin == BuiltinBg::HdriSky && !hdri_available {
                    continue;
                }
                ui.selectable_value(
                    current,
                    BackgroundMode::Builtin(builtin),
                    builtin.to_string(),
                );
            }
            if !customs.is_empty() {
                ui.separator();
                for custom in customs {
                    ui.selectable_value(current, BackgroundMode::Custom(custom.id), &custom.name);
                }
            }
        })
        .response
        .on_hover_text("Background");
}
