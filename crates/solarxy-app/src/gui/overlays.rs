use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastSeverity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug)]
pub(super) struct Toast {
    pub id: u64,
    pub message: String,
    pub severity: ToastSeverity,
    pub created: Instant,
    pub duration: Duration,
}

pub(super) struct HudCtx<'a> {
    pub avg_ms: f32,
    pub fps: u32,
    pub toasts: &'a VecDeque<Toast>,
    pub loading_message: Option<&'a String>,
    pub has_model: bool,
    pub fps_hud_visible: bool,
    pub backend_info: &'a str,
    pub pane_label: &'a str,
    pub cameras_linked: Option<bool>,
    pub validation_counts: (usize, usize),
}

#[derive(Debug, Default)]
pub(super) struct HudResult {
    pub dismissed_toast_id: Option<u64>,
}

pub(super) fn overlay_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_black_alpha(160))
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin::same(4))
}

#[allow(clippy::too_many_arguments)]
fn draw_fps_hud(
    ctx: &egui::Context,
    avg_ms: f32,
    fps: u32,
    backend_info: &str,
    pane_label: &str,
    cameras_linked: Option<bool>,
    has_model: bool,
    validation_counts: (usize, usize),
) {
    let screen = ctx.content_rect();
    let default_pos = egui::pos2(screen.right() - 8.0, screen.top() + 8.0);
    egui::Window::new("fps_hud")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .movable(true)
        .default_pos(default_pos)
        .pivot(egui::Align2::RIGHT_TOP)
        .order(egui::Order::Foreground)
        .frame(overlay_frame())
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("{avg_ms:.1} ms  {fps} fps"))
                    .small()
                    .color(egui::Color32::from_white_alpha(200)),
            );
            if !backend_info.is_empty() {
                ui.label(
                    egui::RichText::new(backend_info)
                        .small()
                        .color(egui::Color32::from_white_alpha(160)),
                );
            }
            if has_model && !pane_label.is_empty() {
                ui.label(
                    egui::RichText::new(pane_label)
                        .small()
                        .color(egui::Color32::from_white_alpha(180)),
                );
            }
            if let Some(linked) = cameras_linked {
                let text = if linked {
                    "Cameras: Linked"
                } else {
                    "Cameras: Independent"
                };
                ui.label(
                    egui::RichText::new(text)
                        .small()
                        .color(egui::Color32::from_white_alpha(140)),
                );
            }
            let (errors, warnings) = validation_counts;
            if errors > 0 || warnings > 0 {
                let mut parts = Vec::new();
                if errors > 0 {
                    parts.push(format!(
                        "\u{2715} {} error{}",
                        errors,
                        if errors == 1 { "" } else { "s" }
                    ));
                }
                if warnings > 0 {
                    parts.push(format!(
                        "\u{26a0} {} warning{}",
                        warnings,
                        if warnings == 1 { "" } else { "s" }
                    ));
                }
                let color = if errors > 0 {
                    egui::Color32::from_rgb(255, 100, 100)
                } else {
                    egui::Color32::from_rgb(255, 200, 80)
                };
                ui.label(egui::RichText::new(parts.join("  ")).small().color(color));
            }
        });
}

fn toast_icon(severity: ToastSeverity) -> (&'static str, egui::Color32) {
    match severity {
        ToastSeverity::Error => ("\u{2715}", egui::Color32::from_rgb(255, 100, 100)),
        ToastSeverity::Warning => ("\u{26A0}", egui::Color32::from_rgb(255, 200, 80)),
        ToastSeverity::Success => ("\u{2713}", egui::Color32::from_rgb(100, 220, 120)),
        ToastSeverity::Info => ("\u{2139}", egui::Color32::from_rgb(120, 180, 255)),
    }
}

fn draw_toast_card(ui: &mut egui::Ui, toast: &Toast) -> egui::Response {
    let (icon, icon_color) = toast_icon(toast.severity);
    let frame_resp = egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(30, 30, 40, 230))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(icon)
                        .color(icon_color)
                        .strong()
                        .size(14.0),
                );
                ui.label(
                    egui::RichText::new(&toast.message)
                        .color(egui::Color32::from_white_alpha(230))
                        .size(13.0),
                );
            });
        })
        .response
        .interact(egui::Sense::click());
    if frame_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    frame_resp
}

fn draw_toast_queue(ctx: &egui::Context, toasts: &VecDeque<Toast>) -> Option<u64> {
    if toasts.is_empty() {
        return None;
    }
    let content = ctx.content_rect();
    let mut y = content.bottom() - 16.0;
    let mut dismissed = None;
    for toast in toasts.iter().rev() {
        let area_id = egui::Id::new(("toast_queue", toast.id));
        let inner = egui::Area::new(area_id)
            .fixed_pos(egui::pos2(content.center().x, y))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| draw_toast_card(ui, toast));
        let card = inner.inner;
        if card.clicked() {
            dismissed = Some(toast.id);
        }
        y -= card.rect.height() + 6.0;
    }
    dismissed
}

pub(super) fn draw_hud_overlays(ctx: &egui::Context, hud: &HudCtx) -> HudResult {
    let mut result = HudResult::default();

    if hud.fps_hud_visible {
        draw_fps_hud(
            ctx,
            hud.avg_ms,
            hud.fps,
            hud.backend_info,
            hud.pane_label,
            hud.cameras_linked,
            hud.has_model,
            hud.validation_counts,
        );
    }

    result.dismissed_toast_id = draw_toast_queue(ctx, hud.toasts);

    if let Some(msg) = hud.loading_message {
        egui::Area::new(egui::Id::new("loading_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                overlay_frame().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(msg)
                            .size(16.0)
                            .color(egui::Color32::from_rgb(128, 179, 255)),
                    );
                });
            });
    }

    result
}
