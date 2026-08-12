//! Bottom status bar — a fixed, non-dockable strip carrying the
//! load-bearing session readout that the floating FPS HUD used to show.
//!
//! Registered as an `egui::TopBottomPanel::bottom` **before** the
//! `DockArea` (egui subtracts panels from the central area in
//! registration order), so the dock fills the space between the menu bar
//! and this strip. Visibility is toggled via `Window → Status Bar`; the
//! `Preferences → Interface` tab seeds the launch default.
//!
//! Sections collapse from the right as the window narrows: the backend
//! string drops below 1200 px, the perf readout below 900 px, the pane
//! label below 600 px.

use super::theme::Theme;

/// Height of the status-bar strip, in logical pixels.
pub(super) const STATUS_BAR_HEIGHT: f32 = 22.0;

const COLLAPSE_BACKEND: f32 = 1200.0;
const COLLAPSE_PERF: f32 = 900.0;
const COLLAPSE_PANE: f32 = 600.0;

/// Per-frame data the status bar renders. Borrowed for the call only.
pub(super) struct StatusBarData<'a> {
    pub model: Option<(&'a str, &'a str)>,
    pub validation: (usize, usize),
    pub review_active: bool,
    pub pane_label: &'a str,
    pub cameras_linked: Option<bool>,
    pub avg_ms: f32,
    pub fps: u32,
    pub backend: &'a str,
    /// A running still render's `(tile, tiles, sample, samples)`.
    pub still: Option<(u32, u32, u32, u32)>,
}

/// Clicks the caller must act on after drawing.
#[derive(Debug, Default)]
pub(super) struct StatusBarResponse {
    /// The `● Review` badge was clicked — caller should exit review mode.
    pub review_badge_clicked: bool,
}

pub(super) fn draw(ctx: &egui::Context, data: &StatusBarData, theme: Theme) -> StatusBarResponse {
    let mut response = StatusBarResponse::default();

    let frame = egui::Frame::NONE
        .fill(theme.bg_elevated)
        .inner_margin(egui::Margin::symmetric(8, 0));

    egui::TopBottomPanel::bottom("solarxy_status_bar")
        .frame(frame)
        .exact_height(STATUS_BAR_HEIGHT)
        .show(ctx, |ui| {
            let width = ui.available_width();
            ui.horizontal_centered(|ui| {
                let mut drew_left = false;
                if let Some((filename, format)) = data.model {
                    ui.label(egui::RichText::new(filename).small().color(theme.fg));
                    ui.label(egui::RichText::new(format).small().color(theme.muted));
                    ui.separator();
                    draw_validation(ui, data.validation, theme);
                    drew_left = true;
                }
                if data.review_active {
                    if drew_left {
                        ui.separator();
                    }
                    if draw_review_badge(ui, theme) {
                        response.review_badge_clicked = true;
                    }
                    drew_left = true;
                }
                // A running render is the most important thing happening,
                // so its readout is always present; only its detail joins
                // the collapse order.
                if let Some((tile, tiles, sample, samples)) = data.still {
                    if drew_left {
                        ui.separator();
                    }
                    let text = if width >= COLLAPSE_PANE {
                        if samples > 1 {
                            format!(
                                "Rendering \u{00b7} tile {}/{tiles} \u{00b7} {sample}/{samples} spp",
                                (tile + 1).min(tiles.max(1))
                            )
                        } else {
                            format!(
                                "Rendering \u{00b7} tile {}/{tiles}",
                                (tile + 1).min(tiles.max(1))
                            )
                        }
                    } else {
                        "Rendering".to_owned()
                    };
                    label(ui, &text, theme.accent);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if width >= COLLAPSE_BACKEND && !data.backend.is_empty() {
                        label(ui, data.backend, theme.muted);
                        ui.separator();
                    }
                    if width >= COLLAPSE_PERF {
                        label(
                            ui,
                            &format!("{:.1} ms \u{00b7} {} fps", data.avg_ms, data.fps),
                            theme.muted,
                        );
                        ui.separator();
                    }
                    if width >= COLLAPSE_PANE && !data.pane_label.is_empty() {
                        let text = match data.cameras_linked {
                            Some(true) => format!("{} \u{00b7} linked", data.pane_label),
                            _ => data.pane_label.to_string(),
                        };
                        label(ui, &text, theme.muted);
                    }
                });
            });
        });

    response
}

fn label(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.label(egui::RichText::new(text).small().color(color));
}

fn draw_validation(ui: &mut egui::Ui, (errors, warnings): (usize, usize), theme: Theme) {
    if errors == 0 && warnings == 0 {
        label(ui, "\u{2713} clean", theme.severity_success);
        return;
    }
    if errors > 0 {
        label(ui, &format!("\u{2715} {errors}"), theme.severity_error);
    }
    if warnings > 0 {
        label(ui, &format!("\u{26a0} {warnings}"), theme.severity_warn);
    }
}

fn draw_review_badge(ui: &mut egui::Ui, theme: Theme) -> bool {
    let text = egui::RichText::new("\u{25cf} Review")
        .small()
        .color(theme.accent);
    ui.add(egui::Button::new(text).small())
        .on_hover_text("Exit review mode")
        .clicked()
}
