//! The keyboard reference, generated entirely from the binding table.
//!
//! Nothing is written here. Before this, the reference was a hand-written
//! list beside a hand-written dispatcher, which is two sources for one fact,
//! and it had already drifted: it omitted bindings the map had and named one
//! that no longer worked. Now a binding that is not in the table cannot
//! appear, and one that is always does.
//!
//! The shape is the browser's: groups in the table's own order, a row per
//! binding carrying its description and the scope it belongs to, then the
//! notes for the keys that mean something else somewhere else.

use crate::state::keymap::{BINDINGS, Binding, KeyGroup, KeyScope, format_keys};

#[derive(Default)]
pub(in crate::gui) struct KeyboardShortcutsModalState {
    pub open: bool,
}

/// The bindings a group lists, in table order, skipping the ones the
/// reference does not show.
fn rows(group: KeyGroup) -> impl Iterator<Item = &'static Binding> {
    BINDINGS
        .iter()
        .filter(move |b| b.listed && b.group == group)
}

/// The scope a row names beside its description, or `None` for a binding
/// that works anywhere. The browser marks its rows the same way, and for the
/// same reason: a letter that means two things needs to say where.
fn scope_label(scope: KeyScope) -> Option<&'static str> {
    match scope {
        KeyScope::Global => None,
        KeyScope::Canvas => Some("canvas"),
        KeyScope::Viewport => Some("viewport"),
    }
}

pub(in crate::gui) fn draw_keyboard_shortcuts_modal(
    ctx: &egui::Context,
    state: &mut KeyboardShortcutsModalState,
) {
    if !state.open {
        return;
    }

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.open = false;
        return;
    }

    let mut open = state.open;
    let default_pos = ctx.content_rect().center() - egui::vec2(240.0, 280.0);

    egui::Window::new("Keyboard Shortcuts")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_width(480.0)
        .default_height(560.0)
        .default_pos(default_pos)
        .movable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut drawn = 0usize;
                for group in KeyGroup::ALL {
                    let mut group_rows = rows(*group).peekable();
                    if group_rows.peek().is_none() {
                        continue;
                    }
                    if drawn > 0 {
                        ui.add_space(2.0);
                        ui.separator();
                    }
                    drawn += 1;
                    ui.add_space(4.0);
                    ui.heading(group.label());
                    ui.add_space(4.0);
                    egui::Grid::new(format!("shortcuts_grid_{}", group.label()))
                        .num_columns(2)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            for binding in group_rows {
                                ui.label(
                                    egui::RichText::new(format_keys(binding.keys))
                                        .monospace()
                                        .color(ui.visuals().hyperlink_color),
                                );
                                ui.horizontal(|ui| {
                                    ui.label(binding.description);
                                    if let Some(scope) = scope_label(binding.scope) {
                                        ui.label(
                                            egui::RichText::new(scope)
                                                .small()
                                                .color(ui.visuals().weak_text_color()),
                                        );
                                    }
                                });
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                }

                let notes: Vec<&Binding> = BINDINGS
                    .iter()
                    .filter(|b| b.listed && b.note.is_some())
                    .collect();
                if !notes.is_empty() {
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(4.0);
                    for binding in notes {
                        let Some(note) = binding.note else { continue };
                        ui.label(
                            egui::RichText::new(format!("{}: {note}", format_keys(binding.keys)))
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                }

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("User-remappable shortcuts land in a future release.")
                        .small()
                        .italics()
                        .color(ui.visuals().weak_text_color()),
                );
            });
        });

    state.open = open && state.open;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The reference shows every listed binding and nothing else.
    ///
    /// Both halves matter and they fail differently: a binding missing from
    /// every group is a key a user cannot discover, and a row with no
    /// binding behind it is the reference lying, which is what the
    /// hand-written one did.
    #[test]
    fn the_reference_is_exactly_the_listed_bindings() {
        let shown: Vec<_> = KeyGroup::ALL
            .iter()
            .flat_map(|group| rows(*group))
            .map(|b| b.action)
            .collect();
        let listed: HashSet<_> = BINDINGS
            .iter()
            .filter(|b| b.listed)
            .map(|b| b.action)
            .collect();
        assert_eq!(
            shown.len(),
            listed.len(),
            "the reference shows {} rows for {} listed bindings",
            shown.len(),
            listed.len()
        );
        let shown: HashSet<_> = shown.into_iter().collect();
        assert_eq!(shown, listed);
    }

    /// The debug harness is dispatched and must not be advertised.
    #[test]
    fn the_debug_harness_is_not_in_the_reference() {
        let shown: HashSet<_> = KeyGroup::ALL
            .iter()
            .flat_map(|group| rows(*group))
            .map(|b| b.keys)
            .collect();
        assert!(!shown.contains("f8"));
        assert!(!shown.contains("f9"));
    }

    /// A row says which surface it belongs to unless it works anywhere,
    /// which is what makes a letter with two meanings readable.
    #[test]
    fn a_scoped_row_names_its_surface() {
        assert_eq!(scope_label(KeyScope::Global), None);
        assert_eq!(scope_label(KeyScope::Canvas), Some("canvas"));
        assert_eq!(scope_label(KeyScope::Viewport), Some("viewport"));
    }
}
