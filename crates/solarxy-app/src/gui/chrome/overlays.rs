use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How loud a toast is.
///
/// Three, which is the browser's set (`web/src/store/toasts.ts`). A fourth,
/// `Success`, was this shell's own: the browser has no such level and every
/// site that used it was reporting that something ordinary had happened,
/// which is what `Info` says. Severity decides the glyph, the colour, the
/// `tracing` level and, since 0.10.0, how long the toast stays up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastSeverity {
    #[default]
    Info,
    Warning,
    Error,
}

impl ToastSeverity {
    /// How long a toast of this severity stays up.
    ///
    /// The browser's two figures, and the reason for the split is the same
    /// there: an error is the one a reader may need to finish reading. This
    /// shell used to carry a `Duration` per toast and set it to five seconds
    /// everywhere, with one two-second special case for a capture.
    pub(in crate::gui) fn dwell(self) -> Duration {
        match self {
            Self::Error => Duration::from_millis(6000),
            Self::Info | Self::Warning => Duration::from_millis(3500),
        }
    }
}

#[derive(Debug)]
pub(in crate::gui) struct Toast {
    pub id: u64,
    pub message: String,
    pub severity: ToastSeverity,
    pub created: Instant,
}

/// Context for the always-on viewport overlays (toasts, loading
/// indicator, overdraw legend). The frame-time, validation and pane
/// readouts are shown nowhere: the status bar that carried them was
/// withdrawn in 0.10.0, and the browser shows none of them either.
pub(in crate::gui) struct HudCtx<'a> {
    pub toasts: &'a VecDeque<Toast>,
    pub loading_message: Option<&'a String>,
    pub overdraw_active: bool,
}

#[derive(Debug, Default)]
pub(in crate::gui) struct HudResult {
    pub dismissed_toast_id: Option<u64>,
}

pub(in crate::gui) fn overlay_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_black_alpha(160))
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin::same(4))
}

fn toast_icon(severity: ToastSeverity) -> (&'static str, egui::Color32) {
    match severity {
        ToastSeverity::Error => ("\u{2715}", egui::Color32::from_rgb(255, 100, 100)),
        ToastSeverity::Warning => ("\u{26A0}", egui::Color32::from_rgb(255, 200, 80)),
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

pub(in crate::gui) fn draw_hud_overlays(ctx: &egui::Context, hud: &HudCtx) -> HudResult {
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

/// What the viewport overlays need to say about the frame, assembled by the
/// state layer once per frame.
#[derive(Debug)]
pub(crate) struct HudInfo {
    pub has_uvs: bool,
    pub overdraw_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser's toast store, read from disk.
    fn browser_source() -> String {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/store/toasts.ts");
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "the browser's toast store is readable at {}: {e}",
                path.display()
            )
        })
    }

    /// The two dwell figures are the browser's, read from its source rather
    /// than restated here.
    ///
    /// A toast that stays up a different length of time on each shell is the
    /// kind of difference a user feels immediately when moving between them,
    /// and it is invisible to every other test in this crate.
    #[test]
    fn the_dwell_times_are_the_browsers() {
        let source = browser_source();
        // The browser writes both in one expression:
        //   severity === "error" ? 6000 : 3500
        let error_ms = ToastSeverity::Error.dwell().as_millis();
        let ordinary_ms = ToastSeverity::Info.dwell().as_millis();
        assert!(
            source.contains(&format!("? {error_ms} : {ordinary_ms}")),
            "the browser does not dwell {error_ms} on an error and {ordinary_ms} otherwise:\n{source}"
        );
        assert_eq!(
            ToastSeverity::Warning.dwell(),
            ToastSeverity::Info.dwell(),
            "only an error is given longer, on both shells"
        );
    }

    /// The severity set is the browser's three.
    ///
    /// `Success` was this shell's alone. The browser has no such level, so a
    /// fourth here would be a level whose toasts could never be produced by
    /// the same code path on both shells.
    #[test]
    fn the_severities_are_the_browsers_three() {
        let source = browser_source();
        let start = source
            .find("export type ToastSeverity")
            .expect("the reader found the browser's severity type");
        let line = &source[start..source[start..].find(';').expect("terminated") + start];
        for severity in [
            ToastSeverity::Info,
            ToastSeverity::Warning,
            ToastSeverity::Error,
        ] {
            // The browser spells the middle one `warn`; the Rust variant is
            // `Warning`. The set is what has to match, not the spelling of an
            // internal name, so the comparison is on the lowercased prefix.
            let wire = match severity {
                ToastSeverity::Info => "info",
                ToastSeverity::Warning => "warn",
                ToastSeverity::Error => "error",
            };
            assert!(
                line.contains(&format!("\"{wire}\"")),
                "the browser has no {wire} severity:\n{line}"
            );
        }
        assert_eq!(
            line.matches('"').count(),
            6,
            "the browser offers exactly three severities:\n{line}"
        );
    }
}
