//! New-note, reply and edit popup. A floating egui window anchored near
//! the click that opened it.
//!
//! Driven by [`crate::state::review::EditDraft`]. Two actions exit: Save
//! raises [`ReviewIntent::CommitDraft`], which the drain turns into the add
//! or edit command; Cancel discards the draft in place, since a discarded
//! draft is interface state and nothing else. Cmd/Ctrl+Enter is the save
//! accelerator, Esc the cancel one.

use crate::gui::intent::{Intent, Intents, ReviewIntent};
use crate::gui::panels::review::visuals::{CATEGORIES, category_label};
use crate::state::review::ReviewState;

/// Draw the note popup if a draft is open. Returns `true` when the user
/// saved or cancelled this frame.
pub(in crate::gui) fn draw_review_popup(
    ctx: &egui::Context,
    review: &mut ReviewState,
    intents: &mut Intents,
) -> bool {
    let Some(draft) = review.editing.as_mut() else {
        return false;
    };

    let title = if draft.editing_id.is_some() {
        "Edit Review Note"
    } else if draft.reply_to.is_some() {
        "Reply"
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

    let popup_id = match draft.editing_id {
        Some(id) => egui::Id::new(("solarxy_review_popup_edit", id.0)),
        None => egui::Id::new(("solarxy_review_popup_new", draft.seq)),
    };

    egui::Window::new(title)
        .id(popup_id)
        .collapsible(false)
        .resizable(false)
        .movable(true)
        .default_size(popup_size)
        .default_pos(egui::pos2(x, y))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Category:");
                egui::ComboBox::from_id_salt("solarxy_review_category")
                    .selected_text(category_label(draft.category))
                    .show_ui(ui, |ui| {
                        for c in CATEGORIES {
                            ui.selectable_value(&mut draft.category, c, category_label(c));
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
        // The draft stays open until the drain takes it as a command, which
        // happens after this pass; the popup does not draw again before then.
        intents.raise(Intent::Review(ReviewIntent::CommitDraft));
        close_requested = true;
    } else if want_cancel {
        review.cancel_draft();
        close_requested = true;
    }

    close_requested
}
