//! New-annotation / edit-annotation popup. Floating egui window
//! anchored near the click that triggered it.
//!
//! Driven by [`crate::state::review::EditDraft`]. Two actions exit:
//! Save (commits via [`ReviewState::commit_draft`]) and Cancel
//! (discards via [`ReviewState::cancel_draft`]). Cmd/Ctrl+Enter is the
//! save accelerator, Esc the cancel one.

use solarxy_core::review::AnnotationCategory;

use crate::state::review::ReviewState;

/// Draw the new-annotation popup if a draft is open.
///
/// Returns `true` when the user committed a new/updated annotation this
/// frame — callers use that signal to mark the marker buffer dirty.
pub(super) fn draw_review_popup(ctx: &egui::Context, review: &mut ReviewState) -> bool {
    let Some(draft) = review.editing.as_mut() else {
        return false;
    };

    let title = if draft.editing_id.is_some() {
        "Edit Review Note"
    } else {
        "New Review Note"
    };

    let screen = ctx.content_rect();
    let popup_size = egui::vec2(320.0, 200.0);
    let mut x = draft.screen_pos.0 + 12.0;
    let mut y = draft.screen_pos.1 + 12.0;
    if x + popup_size.x > screen.max.x {
        x = (screen.max.x - popup_size.x - 8.0).max(8.0);
    }
    if y + popup_size.y > screen.max.y {
        y = (screen.max.y - popup_size.y - 8.0).max(8.0);
    }

    let cmd_enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
    let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));

    let mut want_save = cmd_enter;
    let mut want_cancel = esc;
    let mut close_requested = false;

    egui::Window::new(title)
        .id(egui::Id::new("solarxy_review_popup"))
        .collapsible(false)
        .resizable(false)
        .default_size(popup_size)
        .fixed_pos(egui::pos2(x, y))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Category:");
                egui::ComboBox::from_id_salt("solarxy_review_category")
                    .selected_text(draft.category.to_string())
                    .show_ui(ui, |ui| {
                        for &c in AnnotationCategory::ALL {
                            ui.selectable_value(&mut draft.category, c, c.to_string());
                        }
                    });
            });

            ui.add_space(4.0);
            ui.label("Note:");
            ui.add(
                egui::TextEdit::multiline(&mut draft.text)
                    .hint_text("What needs attention here?")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2(0.0, 80.0)),
            );

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    want_cancel = true;
                }
                let save_text = if draft.editing_id.is_some() {
                    "Update"
                } else {
                    "Save"
                };
                let save_btn =
                    ui.add_enabled(!draft.text.trim().is_empty(), egui::Button::new(save_text));
                if save_btn.clicked() {
                    want_save = true;
                }
                ui.label(egui::RichText::new("Cmd/Ctrl+Enter").weak().small());
            });
        })
        .map(|r| r.response);

    if want_save && draft.text.trim().is_empty() {
        want_save = false;
    }

    if want_save {
        let _ = review.commit_draft();
        close_requested = true;
    } else if want_cancel {
        review.cancel_draft();
        close_requested = true;
    }

    close_requested
}
