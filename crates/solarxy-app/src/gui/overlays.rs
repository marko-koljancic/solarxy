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

/// Context for the always-on viewport overlays (toasts, loading
/// indicator, overdraw legend). The frame-time / validation / pane
/// readout moved to the bottom status bar (`gui::status_bar`).
pub(super) struct HudCtx<'a> {
    pub toasts: &'a VecDeque<Toast>,
    pub loading_message: Option<&'a String>,
    pub overdraw_active: bool,
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
    let result = HudResult {
        dismissed_toast_id: draw_toast_queue(ctx, hud.toasts),
    };

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

    if hud.overdraw_active {
        draw_overdraw_legend(ctx);
    }

    result
}

/// Color-ramp legend matching the 6 stops in `overdraw_show.wgsl`. Bottom
/// -left of the viewport, transparent dark frame, small font — meant to
/// communicate the mapping at a glance, not dominate the view.
fn draw_overdraw_legend(ctx: &egui::Context) {
    let content = ctx.content_rect();
    let stops: &[(&str, [u8; 3])] = &[
        ("0", [0, 0, 0]),
        ("1", [30, 58, 138]),
        ("2-3", [14, 165, 233]),
        ("4-6", [252, 211, 77]),
        ("7-10", [249, 115, 22]),
        ("11+", [220, 38, 38]),
    ];
    egui::Area::new(egui::Id::new("overdraw_legend"))
        .fixed_pos(egui::pos2(content.left() + 12.0, content.bottom() - 12.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            overlay_frame().show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Overdraw (draws / pixel)")
                        .small()
                        .color(egui::Color32::from_white_alpha(220)),
                );
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    for (label, [r, g, b]) in stops {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(18.0, 12.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 2.0, egui::Color32::from_rgb(*r, *g, *b));
                        ui.label(
                            egui::RichText::new(*label)
                                .small()
                                .color(egui::Color32::from_white_alpha(200)),
                        );
                    }
                });
            });
        });
}
