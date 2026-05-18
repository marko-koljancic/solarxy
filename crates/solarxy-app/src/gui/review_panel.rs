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

use crate::state::review::ReviewState;

/// Color tints per category. Mirrors `shaders/review_marker.wgsl` so
/// chips in the panel match the 3D marker color exactly.
const COLOR_INFO: egui::Color32 = egui::Color32::from_rgb(0x5C, 0x9E, 0xFF);
const COLOR_WARNING: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xB2, 0x3D);
const COLOR_QUESTION: egui::Color32 = egui::Color32::from_rgb(0xA0, 0x6D, 0xFF);
const COLOR_CHANGE: egui::Color32 = egui::Color32::from_rgb(0x3D, 0xC9, 0x7A);

fn category_color(c: AnnotationCategory) -> egui::Color32 {
    match c {
        AnnotationCategory::Info => COLOR_INFO,
        AnnotationCategory::Warning => COLOR_WARNING,
        AnnotationCategory::Question => COLOR_QUESTION,
        AnnotationCategory::Change => COLOR_CHANGE,
    }
}

fn category_label_short(c: AnnotationCategory) -> &'static str {
    match c {
        AnnotationCategory::Info => "i",
        AnnotationCategory::Warning => "!",
        AnnotationCategory::Question => "?",
        AnnotationCategory::Change => "✎",
    }
}

fn category_index(c: AnnotationCategory) -> usize {
    match c {
        AnnotationCategory::Info => 0,
        AnnotationCategory::Warning => 1,
        AnnotationCategory::Question => 2,
        AnnotationCategory::Change => 3,
    }
}

/// Right-side docked variant. Lays out as a vertical column matching the
/// existing left sidebar visually. `visible` mirrors `review.panel_open`
/// via the Window-menu canonical flag — see `gui::renderer::render_ui`.
pub(super) fn draw_review_panel_docked(
    ctx: &egui::Context,
    review: &mut ReviewState,
    visible: &mut bool,
) {
    egui::SidePanel::right("review_panel")
        .resizable(true)
        .default_width(300.0)
        .min_width(220.0)
        .max_width(520.0)
        .show_animated(ctx, *visible, |ui| {
            draw_review_panel_content(ui, review, visible);
        });
}

/// Floating-window variant — toggled via the Dock button in the panel
/// header. Default position is near the right edge so it doesn't
/// immediately overlap the viewport center.
pub(super) fn draw_review_panel_floating(
    ctx: &egui::Context,
    review: &mut ReviewState,
    visible: &mut bool,
) {
    let mut open = *visible;
    egui::Window::new("Review")
        .open(&mut open)
        .resizable(true)
        .collapsible(true)
        .default_size([320.0, 480.0])
        .default_pos([800.0, 80.0])
        .show(ctx, |ui| {
            draw_review_panel_content(ui, review, visible);
        });
    *visible = open;
}

fn draw_review_panel_content(ui: &mut egui::Ui, review: &mut ReviewState, visible: &mut bool) {
    ui.horizontal(|ui| {
        let total = review.annotations.len();
        ui.heading(format!("Review ({total})"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("\u{00D7}")
                .on_hover_text("Close panel")
                .clicked()
            {
                *visible = false;
            }
            let dock_label = if review.panel_docked {
                "\u{2197} Detach"
            } else {
                "\u{2199} Dock"
            };
            if ui
                .small_button(dock_label)
                .on_hover_text("Toggle dock / floating")
                .clicked()
            {
                review.panel_docked = !review.panel_docked;
            }
        });
    });
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Show:").small().weak());
        for &cat in AnnotationCategory::ALL {
            let idx = category_index(cat);
            let on = review.category_filters[idx];
            let chip_text = format!("{} {}", category_label_short(cat), cat);
            let resp = ui.add(
                egui::Button::new(egui::RichText::new(chip_text).color(category_color(cat)))
                    .selected(on)
                    .small(),
            );
            if resp.clicked() {
                review.category_filters[idx] = !on;
            }
        }
        ui.checkbox(&mut review.show_resolved, "Resolved");
    });

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("\u{1F50D}").small());
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
    let mut click_target: Option<String> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            draw_section(
                ui,
                "Open",
                &open_idx,
                &review.annotations,
                selected_id.as_deref(),
                &mut click_target,
            );
            if !stale_idx.is_empty() {
                draw_section(
                    ui,
                    "Needs re-anchor",
                    &stale_idx,
                    &review.annotations,
                    selected_id.as_deref(),
                    &mut click_target,
                );
            }
            if !resolved_idx.is_empty() {
                draw_section(
                    ui,
                    "Resolved",
                    &resolved_idx,
                    &review.annotations,
                    selected_id.as_deref(),
                    &mut click_target,
                );
            }
        });

    if let Some(id) = click_target {
        if review.selected.as_deref() == Some(id.as_str()) {
            review.selected = None;
        } else {
            review.selected = Some(id);
        }
        review.dirty = true;
    }
}

fn draw_section(
    ui: &mut egui::Ui,
    title: &str,
    indices: &[usize],
    annotations: &[solarxy_core::review::ReviewAnnotation],
    selected: Option<&str>,
    click_target: &mut Option<String>,
) {
    egui::CollapsingHeader::new(format!("{title} ({})", indices.len()))
        .default_open(true)
        .show(ui, |ui| {
            for &i in indices {
                let ann = &annotations[i];
                draw_annotation_row(ui, ann, selected, click_target);
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

fn draw_annotation_row(
    ui: &mut egui::Ui,
    ann: &solarxy_core::review::ReviewAnnotation,
    selected: Option<&str>,
    click_target: &mut Option<String>,
) {
    let is_selected = selected == Some(ann.id.as_str());
    let row_resp = ui
        .scope(|ui| {
            ui.horizontal_top(|ui| {
                let color = category_color(ann.category);
                let text = egui::RichText::new(category_label_short(ann.category))
                    .strong()
                    .color(color);
                ui.label(text);

                ui.vertical(|ui| {
                    let preview: String = ann
                        .text
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(64)
                        .collect();
                    let preview_label =
                        if ann.text.lines().count() > 1 || ann.text.chars().count() > 64 {
                            format!("{preview}…")
                        } else {
                            preview
                        };
                    let mut row_text = egui::RichText::new(preview_label);
                    if ann.resolved {
                        row_text = row_text.strikethrough().weak();
                    }
                    if ann.stale {
                        row_text = row_text.color(egui::Color32::from_rgb(0xFF, 0x9C, 0x57));
                    }
                    ui.label(row_text);
                    let author = ann.author.as_deref().unwrap_or("anonymous");
                    ui.label(
                        egui::RichText::new(format!("{author} · {}", short_time(&ann.created_at)))
                            .small()
                            .weak(),
                    );
                });
            });
        })
        .response;

    if is_selected {
        ui.painter().rect_stroke(
            row_resp.rect.expand(2.0),
            2.0,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(0x33, 0xE0, 0xFF)),
            egui::StrokeKind::Outside,
        );
    }

    let clickable = row_resp.interact(egui::Sense::click());
    if clickable.clicked() {
        *click_target = Some(ann.id.clone());
    }
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

/// Best-effort RFC 3339 → "HH:MM" or first-10-chars fallback. We don't
/// parse the timestamp here — just trim the seconds + timezone for
/// display purposes.
fn short_time(rfc3339: &str) -> String {
    rfc3339.chars().take(16).collect()
}
