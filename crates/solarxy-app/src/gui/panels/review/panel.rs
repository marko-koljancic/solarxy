//! Side panel for browsing the document's annotations.
//!
//! A dock tab like every other panel, so where it sits is the dock's
//! business. `draw_review_panel_content` draws it and
//! `draw_delete_confirm_modal` the delete confirmation. It carries the four
//! category filter chips, the text filter, the three sections in the
//! browser's order (`Needs re-anchor`, `Open`, `Complete`), a
//! scroll-to-selected jump, and the selected note's actions: Complete,
//! Reply, Edit, Re-place and Delete, which are the browser's five.
//!
//! The notes are the engine's snapshot for the frame. A change to one is an
//! intent the drain turns into a command, so nothing here writes a note;
//! what the panel writes directly is its own interaction state.

use solarxy_graph::engine::AnnotationSnapshot;
use solarxy_graph::review::{AnnotationId, ReviewCategory};

use crate::gui::dock::SolarxyTab;
use crate::gui::intent::{Intent, Intents, LayoutIntent, ReviewIntent};
use crate::gui::panels::review::visuals::{
    CATEGORIES, category_color, category_index, category_letter as category_label_short,
};
use crate::gui::theme::Theme;
use crate::state::review::ReviewState;

/// Which section a top-level note lists under. Resolved wins over stale,
/// which is the browser's rule: a complete note is complete wherever its
/// marker is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    NeedsReanchor,
    Open,
    Complete,
}

impl Section {
    const ORDER: [Self; 3] = [Self::NeedsReanchor, Self::Open, Self::Complete];

    fn title(self) -> &'static str {
        match self {
            Self::NeedsReanchor => "Needs re-anchor",
            Self::Open => "Open",
            Self::Complete => "Complete",
        }
    }
}

fn section_of(note: &AnnotationSnapshot) -> Section {
    if note.annotation.resolved {
        Section::Complete
    } else if note.needs_reanchor {
        Section::NeedsReanchor
    } else {
        Section::Open
    }
}

/// Whether a top-level note passes the panel's filters. The text filter
/// matches the note's text, its author, and the text of any reply, as the
/// browser's does.
fn passes_filters(
    note: &AnnotationSnapshot,
    notes: &[AnnotationSnapshot],
    needle: &str,
    category_filters: [bool; 4],
    show_resolved: bool,
) -> bool {
    let ann = &note.annotation;
    if !category_filters[category_index(ann.category)] {
        return false;
    }
    if ann.resolved && !show_resolved {
        return false;
    }
    if needle.is_empty() {
        return true;
    }
    let lower = |s: &str| s.to_lowercase();
    lower(&ann.text).contains(needle)
        || ann
            .author
            .as_deref()
            .is_some_and(|a| lower(a).contains(needle))
        || notes
            .iter()
            .filter(|n| n.annotation.reply_to == Some(ann.id))
            .any(|n| lower(&n.annotation.text).contains(needle))
}

/// The browser's delete confirmation message: the reply count named when
/// there is one, with its plural.
fn delete_message(reply_count: usize) -> String {
    match reply_count {
        0 => "Delete this note?".to_string(),
        1 => "Delete this note and its 1 reply?".to_string(),
        n => format!("Delete this note and its {n} replies?"),
    }
}

fn find(notes: &[AnnotationSnapshot], id: AnnotationId) -> Option<&AnnotationSnapshot> {
    notes.iter().find(|n| n.annotation.id == id)
}

fn reply_count(notes: &[AnnotationSnapshot], parent: AnnotationId) -> usize {
    notes
        .iter()
        .filter(|n| n.annotation.reply_to == Some(parent))
        .count()
}

/// Category filter chip. Active = saturated category fill + white text;
/// inactive = transparent fill + 1px category-color stroke + colored text.
/// Both states pass WCAG-AA against the dark panel background — replaces
/// the earlier `Button::selected(on)` which inherited egui's default
/// teal "selected" fill and made the pastel chip text unreadable.
fn draw_category_chip(
    ui: &mut egui::Ui,
    cat: ReviewCategory,
    on: bool,
    text: &str,
    theme: Theme,
) -> egui::Response {
    let color = category_color(theme, cat);
    let (fill, text_color, stroke) = if on {
        (color, egui::Color32::WHITE, egui::Stroke::NONE)
    } else {
        (
            egui::Color32::TRANSPARENT,
            color,
            egui::Stroke::new(1.0_f32, color),
        )
    };
    let btn = egui::Button::new(egui::RichText::new(text).color(text_color).strong())
        .fill(fill)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(4))
        .small();
    ui.add(btn)
}

/// Review-panel content for hosting inside an `egui_dock` tab. The header
/// `×` closes the panel; dock placement is owned by `gui::dock`.
pub(in crate::gui) fn draw_review_panel_content(
    ui: &mut egui::Ui,
    notes: &[AnnotationSnapshot],
    review: &mut ReviewState,
    intents: &mut Intents,
    theme: Theme,
) {
    ui.horizontal(|ui| {
        let total = notes.len();
        ui.heading(format!("Review ({total})"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("\u{00D7}")
                .on_hover_text("Close panel")
                .clicked()
            {
                // The same toggle the Review menu's panel entry raises. Before the intent
                // queue this wrote an out-parameter the dock declared and
                // never read back, so the button did nothing.
                intents.raise(Intent::Layout(LayoutIntent::ToggleTab(
                    SolarxyTab::ReviewPanel,
                )));
            }
            if ui
                .add_enabled(!notes.is_empty(), egui::Button::new("Export").small())
                .on_hover_text("Write the review notes to a sidecar file beside the scene")
                .clicked()
            {
                intents.raise(Intent::Review(ReviewIntent::ExportNotes));
            }
            // Markers toggle — suppresses the 3D viewport overlay while
            // the panel keeps listing every annotation.
            let markers_shown = !review.markers_hidden;
            if ui
                .selectable_label(markers_shown, "Markers")
                .on_hover_text(if markers_shown {
                    "Hide review markers in the viewport"
                } else {
                    "Show review markers in the viewport"
                })
                .clicked()
            {
                review.markers_hidden = markers_shown;
            }
        });
    });
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Show:").small().weak());
        for cat in CATEGORIES {
            let idx = category_index(cat);
            let on = review.category_filters[idx];
            let chip_text = format!(
                "{} {}",
                category_label_short(cat),
                crate::gui::panels::review::visuals::category_label(cat)
            );
            if draw_category_chip(ui, cat, on, &chip_text, theme).clicked() {
                review.category_filters[idx] = !on;
            }
        }
        ui.checkbox(&mut review.show_resolved, "Complete");
    });

    ui.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut review.text_filter)
                .hint_text("filter notes")
                .desired_width(f32::INFINITY),
        );
        if !review.text_filter.is_empty() && ui.small_button("\u{00D7}").clicked() {
            review.text_filter.clear();
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            review.text_filter.clear();
        }
    });
    ui.separator();

    if notes.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No annotations yet").weak());
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Press Shift+R, then click on the model")
                    .small()
                    .weak(),
            );
        });
        return;
    }

    let needle = review.text_filter.to_lowercase();
    let show_resolved = review.show_resolved;
    let cat_filters = review.category_filters;

    let mut sections: [Vec<&AnnotationSnapshot>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for note in notes {
        if note.annotation.reply_to.is_some() {
            continue;
        }
        if !passes_filters(note, notes, &needle, cat_filters, show_resolved) {
            continue;
        }
        let slot = match section_of(note) {
            Section::NeedsReanchor => 0,
            Section::Open => 1,
            Section::Complete => 2,
        };
        sections[slot].push(note);
    }

    let selected_id = review.selected;
    let reanchor_id = review.reanchor_target;
    let scroll_to = if review.scroll_to_selected {
        selected_id
    } else {
        None
    };
    let mut click_target: Option<AnnotationId> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (section, members) in Section::ORDER.iter().zip(sections.iter()) {
                draw_section(
                    ui,
                    section.title(),
                    members,
                    notes,
                    selected_id,
                    reanchor_id,
                    scroll_to,
                    &mut click_target,
                    theme,
                );
            }
        });

    if review.scroll_to_selected {
        review.scroll_to_selected = false;
    }

    if let Some(id) = click_target {
        if review.selected == Some(id) {
            review.selected = None;
        } else {
            review.selected = Some(id);
            // Fly the active camera to the annotation (drained by the
            // state layer after the egui pass).
            review.focus_request = Some(id);
        }
    }

    if review.selected.is_some() {
        draw_selected_actions(ui, notes, review, intents, theme);
    }
}

/// The selected note's actions, the browser's five: Complete, Reply, Edit,
/// Re-place, Delete. Each is one intent or one draft, so each is one undo
/// step once it lands.
fn draw_selected_actions(
    ui: &mut egui::Ui,
    notes: &[AnnotationSnapshot],
    review: &mut ReviewState,
    intents: &mut Intents,
    theme: Theme,
) {
    let Some(selected_id) = review.selected else {
        return;
    };
    let Some(note) = find(notes, selected_id) else {
        return;
    };
    let ann = note.annotation.clone();
    let reanchor_active = review.reanchor_target == Some(selected_id);

    ui.separator();
    ui.label(egui::RichText::new("Selected note").small().weak());
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(crate::gui::panels::review::visuals::category_label(
                ann.category,
            ))
            .color(category_color(theme, ann.category)),
        );
        let author = ann.author.as_deref().unwrap_or("Anonymous");
        ui.label(
            egui::RichText::new(format!("{author} \u{00b7} {}", short_time(&ann.created_at)))
                .small()
                .weak(),
        );
    });

    let mut reply_clicked = false;
    let mut edit_clicked = false;
    let mut replace_clicked = false;
    let mut delete_clicked = false;

    ui.horizontal_wrapped(|ui| {
        let mut resolved = ann.resolved;
        if ui.checkbox(&mut resolved, "Complete").changed() {
            intents.raise(Intent::Review(ReviewIntent::Resolve {
                id: selected_id,
                resolved,
            }));
        }
        if ui
            .button("\u{21B3} Reply")
            .on_hover_text("Add a threaded reply to this note")
            .clicked()
        {
            reply_clicked = true;
        }
        if ann.reply_to.is_none()
            && ui
                .button("Edit")
                .on_hover_text("Change the text or the category")
                .clicked()
        {
            edit_clicked = true;
        }
        if ann.reply_to.is_none()
            && ui
                .add_enabled(!reanchor_active, egui::Button::new("Re-place"))
                .on_hover_text("Then click on the geometry to move the marker there")
                .on_disabled_hover_text("Click on the geometry, or press Esc to cancel")
                .clicked()
        {
            replace_clicked = true;
        }
        if ui
            .button(egui::RichText::new("Delete").color(egui::Color32::from_rgb(0xE0, 0x6C, 0x6C)))
            .on_hover_text("Remove this note and its replies")
            .clicked()
        {
            delete_clicked = true;
        }
    });

    let center = ui.ctx().content_rect().center();
    if reply_clicked {
        // A reply to a reply is refused by the engine; offer it on the
        // parent instead, which is where the thread lives.
        let parent = ann
            .reply_to
            .and_then(|p| find(notes, p))
            .map_or(&ann, |n| &n.annotation);
        review.open_reply_draft(parent, (center.x, center.y));
    }
    if edit_clicked {
        review.open_edit_draft(&ann, (center.x, center.y));
    }
    if replace_clicked {
        review.begin_reanchor(selected_id);
    }
    if delete_clicked {
        review.delete_confirm = Some(selected_id);
    }
}

/// The delete confirmation, drawn in `gui::renderer::render_ui` after the
/// panel itself so it overlays correctly. No-op when `review.delete_confirm`
/// is `None`. Delete raises the intent and leaves the confirmation open;
/// the drain closes it once the delete has landed, so the dialog can never
/// close on a delete that was refused.
pub(in crate::gui) fn draw_delete_confirm_modal(
    ctx: &egui::Context,
    notes: &[AnnotationSnapshot],
    review: &mut ReviewState,
    intents: &mut Intents,
) {
    let Some(target_id) = review.delete_confirm else {
        return;
    };
    let Some(target) = find(notes, target_id) else {
        review.delete_confirm = None;
        return;
    };
    let text = &target.annotation.text;
    let preview: String = text.lines().next().unwrap_or("").chars().take(60).collect();
    let preview_label = if text.is_empty() {
        "(no text)".to_string()
    } else if text.chars().count() > 60 || text.lines().count() > 1 {
        format!("{preview}\u{2026}")
    } else {
        preview
    };
    let message = delete_message(reply_count(notes, target_id));

    let mut do_delete = false;
    let mut do_cancel = false;

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        do_cancel = true;
    }

    egui::Window::new("Delete note")
        .id(egui::Id::new("solarxy_review_delete_confirm"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(280.0);
            ui.label(format!("\u{201C}{preview_label}\u{201D}"));
            ui.add_space(4.0);
            ui.label(message);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    do_cancel = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(
                            egui::RichText::new("Delete")
                                .color(egui::Color32::from_rgb(0xE0, 0x6C, 0x6C)),
                        )
                        .clicked()
                    {
                        do_delete = true;
                    }
                });
            });
        });

    if do_delete {
        intents.raise(Intent::Review(ReviewIntent::Delete { id: target_id }));
    } else if do_cancel {
        review.delete_confirm = None;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_section(
    ui: &mut egui::Ui,
    title: &str,
    members: &[&AnnotationSnapshot],
    notes: &[AnnotationSnapshot],
    selected: Option<AnnotationId>,
    reanchor_target: Option<AnnotationId>,
    scroll_to: Option<AnnotationId>,
    click_target: &mut Option<AnnotationId>,
    theme: Theme,
) {
    egui::CollapsingHeader::new(format!("{title} ({})", members.len()))
        .default_open(true)
        .show(ui, |ui| {
            for note in members {
                let row_resp =
                    draw_annotation_row(ui, note, selected, reanchor_target, click_target, theme);
                if scroll_to == Some(note.annotation.id) {
                    row_resp.scroll_to_me(Some(egui::Align::Center));
                }
                let replies: Vec<&AnnotationSnapshot> = notes
                    .iter()
                    .filter(|n| n.annotation.reply_to == Some(note.annotation.id))
                    .collect();
                if !replies.is_empty() {
                    ui.indent(("replies", note.annotation.id.0), |ui| {
                        for reply in replies {
                            draw_reply_row(ui, reply, selected, click_target);
                        }
                    });
                }
            }
        });
}

/// One annotation row — a full-width, single click target painted
/// manually (no child widgets, which would each steal the click and shrink
/// the hit region to the text).
/// Shows a category-letter column, a 2-line wrapped text preview, and an
/// author · time line; the row sizes to that content.
fn draw_annotation_row(
    ui: &mut egui::Ui,
    note: &AnnotationSnapshot,
    selected: Option<AnnotationId>,
    reanchor_target: Option<AnnotationId>,
    click_target: &mut Option<AnnotationId>,
    theme: Theme,
) -> egui::Response {
    const PAD_X: f32 = 6.0;
    const PAD_Y: f32 = 4.0;
    const LETTER_W: f32 = 16.0;
    const LINE_GAP: f32 = 2.0;
    const STALE_ORANGE: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x9C, 0x57);

    let ann = &note.annotation;
    let is_selected = selected == Some(ann.id);
    let is_reanchor = reanchor_target == Some(ann.id);
    let category_color = category_color(theme, ann.category);

    let full_w = ui.available_width();
    let text_x_off = PAD_X + LETTER_W + 4.0;
    let text_w = (full_w - text_x_off - PAD_X).max(24.0);

    // Preview: up to two wrapped rows of the note text, ellipsised.
    let trimmed = ann.text.trim();
    let (preview_src, empty) = if trimmed.is_empty() {
        ("(no text)", true)
    } else {
        (trimmed, false)
    };
    let preview_color = if empty || ann.resolved {
        theme.muted
    } else if note.needs_reanchor {
        STALE_ORANGE
    } else {
        theme.fg
    };
    let mut preview_job = egui::text::LayoutJob::single_section(
        preview_src.chars().take(220).collect(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(13.0),
            color: preview_color,
            strikethrough: if ann.resolved {
                egui::Stroke::new(1.0_f32, theme.muted)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        },
    );
    preview_job.wrap = egui::text::TextWrapping {
        max_width: text_w,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    let preview_galley = ui.painter().layout_job(preview_job);

    let author = ann.author.as_deref().unwrap_or("Anonymous");
    let meta_galley = ui.painter().layout(
        format!("{author} \u{00b7} {}", short_time(&ann.created_at)),
        egui::FontId::proportional(11.0),
        theme.muted,
        text_w,
    );

    let row_h = PAD_Y + preview_galley.size().y + LINE_GAP + meta_galley.size().y + PAD_Y;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(full_w, row_h), egui::Sense::click());

    let painter = ui.painter();
    if is_selected {
        painter.rect_filled(rect, 0.0, theme.selection);
    } else if resp.hovered() {
        painter.rect_filled(rect, 0.0, theme.widget_hover);
    }
    if is_reanchor {
        let t = ui.ctx().input(|i| i.time);
        let phase = ((t * std::f64::consts::TAU / 0.6).sin().mul_add(0.5, 0.5)) as f32;
        let alpha = (30.0 + 40.0 * phase).round() as u8;
        let sa = theme.review.selection_accent;
        let amber = egui::Color32::from_rgba_unmultiplied(sa.r(), sa.g(), sa.b(), alpha);
        painter.rect_filled(rect, 0.0, amber);
        ui.ctx().request_repaint();
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // Category letter, left column, aligned with the first preview row.
    painter.text(
        egui::pos2(rect.left() + PAD_X, rect.top() + PAD_Y),
        egui::Align2::LEFT_TOP,
        category_label_short(ann.category),
        egui::FontId::proportional(13.0),
        category_color,
    );
    let text_x = rect.left() + text_x_off;
    painter.galley(
        egui::pos2(text_x, rect.top() + PAD_Y),
        preview_galley.clone(),
        preview_color,
    );
    painter.galley(
        egui::pos2(
            text_x,
            rect.top() + PAD_Y + preview_galley.size().y + LINE_GAP,
        ),
        meta_galley,
        theme.muted,
    );

    if resp.clicked() {
        *click_target = Some(ann.id);
    }
    resp
}

fn draw_reply_row(
    ui: &mut egui::Ui,
    note: &AnnotationSnapshot,
    selected: Option<AnnotationId>,
    click_target: &mut Option<AnnotationId>,
) {
    let ann = &note.annotation;
    let resp = ui
        .horizontal_top(|ui| {
            ui.label(egui::RichText::new("\u{21B3}").small().weak());
            ui.vertical(|ui| {
                let preview: String = ann
                    .text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect();
                let mut t = egui::RichText::new(preview).small();
                if ann.resolved {
                    t = t.strikethrough().weak();
                }
                if selected == Some(ann.id) {
                    t = t.strong();
                }
                ui.label(t);
                let author = ann.author.as_deref().unwrap_or("Anonymous");
                ui.label(
                    egui::RichText::new(format!(
                        "{author} \u{00b7} {}",
                        short_time(&ann.created_at)
                    ))
                    .small()
                    .weak(),
                );
            });
        })
        .response;
    if resp.interact(egui::Sense::click()).clicked() {
        *click_target = Some(ann.id);
    }
}

fn short_time(rfc3339: &str) -> String {
    rfc3339.chars().take(16).collect()
}

#[cfg(test)]
mod tests {
    use solarxy_graph::document::{GraphContext, NodeId};
    use solarxy_graph::review::{Annotation, ReviewAnchor};

    use super::*;

    fn note(id: u64, text: &str, resolved: bool, stale: bool) -> AnnotationSnapshot {
        AnnotationSnapshot {
            annotation: Annotation {
                id: AnnotationId(id),
                anchor: ReviewAnchor {
                    ctx: GraphContext::Root,
                    node: NodeId(1),
                    mesh: Some(0),
                    face: Some(0),
                    barycentric: Some([1.0, 0.0, 0.0]),
                    world_fallback: Some([0.0; 3]),
                    geometry_hash: None,
                },
                text: text.into(),
                category: ReviewCategory::Question,
                resolved,
                author: Some("Mara".into()),
                created_at: String::new(),
                updated_at: String::new(),
                reply_to: None,
            },
            needs_reanchor: stale,
        }
    }

    fn reply(id: u64, parent: u64, text: &str) -> AnnotationSnapshot {
        let mut n = note(id, text, false, false);
        n.annotation.reply_to = Some(AnnotationId(parent));
        n
    }

    #[test]
    fn a_resolved_note_is_complete_even_when_stale() {
        // The browser's rule: `complete` is filtered first, so a resolved
        // note never lists under `Needs re-anchor`.
        assert_eq!(section_of(&note(1, "x", true, true)), Section::Complete);
        assert_eq!(
            section_of(&note(1, "x", false, true)),
            Section::NeedsReanchor
        );
        assert_eq!(section_of(&note(1, "x", false, false)), Section::Open);
        assert_eq!(section_of(&note(1, "x", true, false)), Section::Complete);
    }

    #[test]
    fn the_sections_draw_in_the_browsers_order() {
        let titles: Vec<&str> = Section::ORDER.iter().map(|s| s.title()).collect();
        assert_eq!(titles, ["Needs re-anchor", "Open", "Complete"]);
    }

    #[test]
    fn the_text_filter_matches_text_author_and_replies() {
        let notes = vec![
            note(1, "Seam on the lid", false, false),
            reply(2, 1, "Fixed in the second pass"),
            note(3, "Other", false, false),
        ];
        let all = [true; 4];
        assert!(passes_filters(&notes[0], &notes, "lid", all, true));
        assert!(
            passes_filters(&notes[0], &notes, "mara", all, true),
            "author"
        );
        assert!(
            passes_filters(&notes[0], &notes, "second pass", all, true),
            "reply text"
        );
        assert!(!passes_filters(&notes[2], &notes, "lid", all, true));
        assert!(
            passes_filters(&notes[2], &notes, "", all, true),
            "no needle passes"
        );
    }

    #[test]
    fn a_hidden_category_and_hidden_resolved_notes_are_filtered() {
        let notes = vec![note(1, "x", true, false)];
        let mut filters = [true; 4];
        assert!(
            !passes_filters(&notes[0], &notes, "", filters, false),
            "resolved hidden"
        );
        assert!(passes_filters(&notes[0], &notes, "", filters, true));
        filters[category_index(ReviewCategory::Question)] = false;
        assert!(
            !passes_filters(&notes[0], &notes, "", filters, true),
            "category off"
        );
    }

    #[test]
    fn the_delete_message_is_the_browsers_with_its_plural() {
        assert_eq!(delete_message(0), "Delete this note?");
        assert_eq!(delete_message(1), "Delete this note and its 1 reply?");
        assert_eq!(delete_message(3), "Delete this note and its 3 replies?");
    }
}
