//! Side panel for browsing the open `.solarxy-review.json` annotation set.
//!
//! Docked-or-floating like the Console (`crate::gui::console_view`):
//! [`draw_review_panel_docked`] and [`draw_review_panel_floating`] share
//! [`draw_review_panel_content`]; a Dock/Detach toggle in the header
//! lets the user swap modes at runtime.
//!
//! Scaffold version (commit 8a): filter chips + text search + grouped
//! list with click-to-select. Inline editor (commit 8b) and marker
//! hit-test / re-place flow (commit 8c) follow.

use solarxy_core::review::AnnotationCategory;

use super::review_visuals::{category_color, category_letter as category_label_short};
use super::theme::Theme;
use crate::state::review::ReviewState;

fn category_index(c: AnnotationCategory) -> usize {
    match c {
        AnnotationCategory::Info => 0,
        AnnotationCategory::Warning => 1,
        AnnotationCategory::Question => 2,
        AnnotationCategory::Change => 3,
    }
}

/// Category filter chip. Active = saturated category fill + white text;
/// inactive = transparent fill + 1px category-color stroke + colored text.
/// Both states pass WCAG-AA against the dark panel background — replaces
/// the earlier `Button::selected(on)` which inherited egui's default
/// teal "selected" fill and made the pastel chip text unreadable.
fn draw_category_chip(
    ui: &mut egui::Ui,
    cat: AnnotationCategory,
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
            egui::Stroke::new(1.0, color),
        )
    };
    let btn = egui::Button::new(egui::RichText::new(text).color(text_color).strong())
        .fill(fill)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(4))
        .small();
    ui.add(btn)
}

/// Review-panel content for hosting inside an `egui_dock` tab. Header
/// `×` closes the panel (writes `*visible = false`); dock placement is
/// owned by `gui::dock`.
pub(super) fn draw_review_panel_content(
    ui: &mut egui::Ui,
    review: &mut ReviewState,
    visible: &mut bool,
    theme: Theme,
) {
    ui.horizontal(|ui| {
        let total = review.annotations.len();
        ui.heading(format!("Review ({total})"));
        if review.dirty {
            ui.label(egui::RichText::new("\u{25CF}").color(theme.review.selection_accent))
                .on_hover_text("Unsaved changes");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("\u{00D7}")
                .on_hover_text("Close panel")
                .clicked()
            {
                *visible = false;
            }
            if ui
                .add_enabled(review.dirty, egui::Button::new("Save").small())
                .on_hover_text("Write review notes to the sidecar file (Cmd/Ctrl+S)")
                .clicked()
            {
                review.save_requested = true;
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

    // Sidecar location + save state — so it is never a mystery where the
    // notes live or whether they are persisted.
    if let Some(path) = &review.sidecar_path {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("review.json");
        let prefix = if review.dirty {
            "Unsaved \u{2014} "
        } else {
            "Saved \u{2014} "
        };
        ui.label(
            egui::RichText::new(format!("{prefix}{name}"))
                .small()
                .weak(),
        )
        .on_hover_text(path.display().to_string());
    }
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Show:").small().weak());
        for &cat in AnnotationCategory::ALL {
            let idx = category_index(cat);
            let on = review.category_filters[idx];
            let chip_text = format!("{} {}", category_label_short(cat), cat);
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

    if review.annotations.is_empty() {
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
    let filter_text = !needle.is_empty();
    let show_resolved = review.show_resolved;
    let cat_filters = review.category_filters;

    let mut open_idx: Vec<usize> = Vec::new();
    let mut resolved_idx: Vec<usize> = Vec::new();
    let mut stale_idx: Vec<usize> = Vec::new();

    for (i, ann) in review.annotations.iter().enumerate() {
        if ann.reply_to.is_some() {
            continue;
        }
        if !cat_filters[category_index(ann.category)] {
            continue;
        }
        if filter_text && !ann.text.to_lowercase().contains(&needle) {
            continue;
        }
        if ann.stale {
            stale_idx.push(i);
        } else if ann.resolved {
            if show_resolved {
                resolved_idx.push(i);
            }
        } else {
            open_idx.push(i);
        }
    }

    let selected_id = review.selected.clone();
    let reanchor_id = review.reanchor_target.clone();
    let scroll_to = if review.scroll_to_selected {
        selected_id.clone()
    } else {
        None
    };
    let mut click_target: Option<String> = None;
    let mut reanchor_click: Option<String> = None;
    let mut cancel_reanchor_click = false;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            draw_section(
                ui,
                "Open",
                &open_idx,
                &review.annotations,
                selected_id.as_deref(),
                reanchor_id.as_deref(),
                scroll_to.as_deref(),
                false,
                &mut click_target,
                &mut reanchor_click,
                &mut cancel_reanchor_click,
                theme,
            );
            if !stale_idx.is_empty() {
                draw_section(
                    ui,
                    "Needs re-anchor",
                    &stale_idx,
                    &review.annotations,
                    selected_id.as_deref(),
                    reanchor_id.as_deref(),
                    scroll_to.as_deref(),
                    true,
                    &mut click_target,
                    &mut reanchor_click,
                    &mut cancel_reanchor_click,
                    theme,
                );
            }
            if !resolved_idx.is_empty() {
                draw_section(
                    ui,
                    "Complete",
                    &resolved_idx,
                    &review.annotations,
                    selected_id.as_deref(),
                    reanchor_id.as_deref(),
                    scroll_to.as_deref(),
                    false,
                    &mut click_target,
                    &mut reanchor_click,
                    &mut cancel_reanchor_click,
                    theme,
                );
            }
        });

    if review.scroll_to_selected {
        review.scroll_to_selected = false;
    }

    if let Some(id) = click_target {
        if review.selected.as_deref() == Some(id.as_str()) {
            review.selected = None;
        } else {
            review.selected = Some(id.clone());
            // Fly the active camera to the annotation (drained by the
            // state layer after the egui pass).
            review.focus_request = Some(id);
        }
    }

    if let Some(id) = reanchor_click {
        review.begin_reanchor(id);
    }
    if cancel_reanchor_click {
        review.cancel_reanchor();
    }
    if review.selected.is_some() {
        draw_selected_editor(ui, review, theme);
    }
}

fn draw_selected_editor(ui: &mut egui::Ui, review: &mut ReviewState, theme: Theme) {
    let Some(selected_id) = review.selected.clone() else {
        return;
    };
    let Some(idx) = review.annotations.iter().position(|a| a.id == selected_id) else {
        return;
    };
    let is_stale = review.annotations[idx].stale;
    let reanchor_active = review.reanchor_target.as_deref() == Some(selected_id.as_str());

    ui.separator();
    ui.label(egui::RichText::new("Selected note").small().weak());

    let mut any_change = false;
    let mut reply_clicked = false;
    let mut delete_clicked = false;
    let mut reanchor_click: Option<String> = None;
    let mut cancel_reanchor_click = false;
    {
        let ann = &mut review.annotations[idx];

        ui.horizontal(|ui| {
            ui.label("Category:");
            let prev_cat = ann.category;
            egui::ComboBox::from_id_salt("review_selected_category")
                .selected_text(
                    egui::RichText::new(ann.category.to_string())
                        .color(category_color(theme, ann.category)),
                )
                .show_ui(ui, |ui| {
                    for &cat in AnnotationCategory::ALL {
                        ui.selectable_value(
                            &mut ann.category,
                            cat,
                            egui::RichText::new(cat.to_string()).color(category_color(theme, cat)),
                        );
                    }
                });
            if ann.category != prev_cat {
                any_change = true;
            }
        });

        let text_resp = ui.add(
            egui::TextEdit::multiline(&mut ann.text)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if text_resp.changed() {
            any_change = true;
        }

        let prev_resolved = ann.resolved;
        ui.checkbox(&mut ann.resolved, "Complete");
        if ann.resolved != prev_resolved {
            any_change = true;
        }

        if any_change {
            ann.updated_at = ReviewState::now_rfc3339();
        }

        ui.horizontal(|ui| {
            if ui
                .button("\u{21B3} Reply")
                .on_hover_text("Add a threaded reply to this note")
                .clicked()
            {
                reply_clicked = true;
            }
            if is_stale {
                if reanchor_active {
                    let amber = theme.review.selection_accent;
                    if ui
                        .button(egui::RichText::new("Cancel re-anchor").color(amber))
                        .on_hover_text("Exit re-anchor sub-mode without changes (Esc)")
                        .clicked()
                    {
                        cancel_reanchor_click = true;
                    }
                } else if ui
                    .button("Re-place here")
                    .on_hover_text("Then click on the model to set a new anchor")
                    .clicked()
                {
                    reanchor_click = Some(selected_id.clone());
                }
            }
            if ui
                .button(
                    egui::RichText::new("Delete").color(egui::Color32::from_rgb(0xE0, 0x6C, 0x6C)),
                )
                .on_hover_text("Remove this annotation (cascade-deletes replies)")
                .clicked()
            {
                delete_clicked = true;
            }
        });
    }

    if any_change {
        review.dirty = true;
    }

    if reply_clicked {
        let center = ui.ctx().content_rect().center();
        review.open_reply_draft(&selected_id, (center.x, center.y));
    }

    if delete_clicked {
        if review.reply_count(&selected_id) > 0 {
            review.delete_confirm = Some(selected_id.clone());
        } else {
            review.delete_cascade(&selected_id);
        }
    }

    if let Some(id) = reanchor_click {
        review.begin_reanchor(id);
    }
    if cancel_reanchor_click {
        review.cancel_reanchor();
    }
}

/// Confirmation modal for cascade-delete of an annotation that has
/// replies. Drawn in `gui::renderer::render_ui` after the panel itself
/// so it overlays correctly. No-op when `review.delete_confirm` is
/// `None`.
pub(super) fn draw_delete_confirm_modal(ctx: &egui::Context, review: &mut ReviewState) {
    let Some(target_id) = review.delete_confirm.clone() else {
        return;
    };
    let Some(target) = review.find(&target_id) else {
        review.delete_confirm = None;
        return;
    };
    let preview: String = target
        .text
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(60)
        .collect();
    let preview_label = if target.text.is_empty() {
        "(no text)".to_string()
    } else if target.text.chars().count() > 60 || target.text.lines().count() > 1 {
        format!("{preview}…")
    } else {
        preview
    };
    let reply_count = review.reply_count(&target_id);

    let mut do_delete = false;
    let mut do_cancel = false;

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        do_cancel = true;
    }

    egui::Window::new("Delete annotation?")
        .id(egui::Id::new("solarxy_review_delete_confirm"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(280.0);
            ui.label(format!("\u{201C}{preview_label}\u{201D}"));
            ui.add_space(4.0);
            ui.label(if reply_count == 1 {
                "This will also delete 1 reply.".to_string()
            } else {
                format!("This will also delete {reply_count} replies.")
            });
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
        review.delete_cascade(&target_id);
    } else if do_cancel {
        review.delete_confirm = None;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_section(
    ui: &mut egui::Ui,
    title: &str,
    indices: &[usize],
    annotations: &[solarxy_core::review::ReviewAnnotation],
    selected: Option<&str>,
    reanchor_target: Option<&str>,
    scroll_to: Option<&str>,
    is_stale_section: bool,
    click_target: &mut Option<String>,
    reanchor_click: &mut Option<String>,
    cancel_reanchor_click: &mut bool,
    theme: Theme,
) {
    egui::CollapsingHeader::new(format!("{title} ({})", indices.len()))
        .default_open(true)
        .show(ui, |ui| {
            for &i in indices {
                let ann = &annotations[i];
                let row_resp =
                    draw_annotation_row(ui, ann, selected, reanchor_target, click_target, theme);
                if scroll_to == Some(ann.id.as_str()) {
                    row_resp.scroll_to_me(Some(egui::Align::Center));
                }
                if is_stale_section {
                    draw_replace_button(
                        ui,
                        &ann.id,
                        reanchor_target,
                        reanchor_click,
                        cancel_reanchor_click,
                        theme,
                    );
                }
                let reply_indices: Vec<usize> = annotations
                    .iter()
                    .enumerate()
                    .filter_map(|(j, a)| {
                        if a.reply_to.as_deref() == Some(ann.id.as_str()) {
                            Some(j)
                        } else {
                            None
                        }
                    })
                    .collect();
                if !reply_indices.is_empty() {
                    ui.indent(format!("replies_{}", ann.id), |ui| {
                        for j in reply_indices {
                            draw_reply_row(ui, &annotations[j]);
                        }
                    });
                }
            }
        });
}

/// One annotation row — a full-width, single click target painted
/// manually (no child widgets, which would each steal the click and shrink
/// the hit region to the text). Mirrors `material_inspector::draw_material_row`.
/// Shows a category-letter column, a 2-line wrapped text preview, and an
/// author · time line; the row sizes to that content.
fn draw_annotation_row(
    ui: &mut egui::Ui,
    ann: &solarxy_core::review::ReviewAnnotation,
    selected: Option<&str>,
    reanchor_target: Option<&str>,
    click_target: &mut Option<String>,
    theme: Theme,
) -> egui::Response {
    const PAD_X: f32 = 6.0;
    const PAD_Y: f32 = 4.0;
    const LETTER_W: f32 = 16.0;
    const LINE_GAP: f32 = 2.0;
    const STALE_ORANGE: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x9C, 0x57);

    let is_selected = selected == Some(ann.id.as_str());
    let is_reanchor = reanchor_target == Some(ann.id.as_str());
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
    } else if ann.stale {
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
                egui::Stroke::new(1.0, theme.muted)
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

    let author = ann.author.as_deref().unwrap_or("anonymous");
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
        *click_target = Some(ann.id.clone());
    }
    resp
}

fn draw_replace_button(
    ui: &mut egui::Ui,
    annotation_id: &str,
    reanchor_target: Option<&str>,
    reanchor_click: &mut Option<String>,
    cancel_reanchor_click: &mut bool,
    theme: Theme,
) {
    let is_active = reanchor_target == Some(annotation_id);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        if is_active {
            let amber = theme.review.selection_accent;
            let btn =
                egui::Button::new(egui::RichText::new("Cancel re-anchor").color(amber)).small();
            if ui
                .add(btn)
                .on_hover_text("Exit re-anchor sub-mode without changes (Esc)")
                .clicked()
            {
                *cancel_reanchor_click = true;
            }
        } else if ui
            .small_button("Re-place here")
            .on_hover_text("Then click on the model to set a new anchor")
            .clicked()
        {
            *reanchor_click = Some(annotation_id.to_string());
        }
    });
}

fn draw_reply_row(ui: &mut egui::Ui, ann: &solarxy_core::review::ReviewAnnotation) {
    ui.horizontal_top(|ui| {
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
            ui.label(t);
            let author = ann.author.as_deref().unwrap_or("anonymous");
            ui.label(
                egui::RichText::new(format!("{author} · {}", short_time(&ann.created_at)))
                    .small()
                    .weak(),
            );
        });
    });
}

fn short_time(rfc3339: &str) -> String {
    rfc3339.chars().take(16).collect()
}
