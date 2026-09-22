//! The live readout under a transform drag: the delta so far, bottom
//! centre of the viewport, in the shared solver's own words.
//!
//! Drawn from the state's readout string rather than computed here, so the
//! two shells cannot format one drag differently: the solver produces the
//! text and each shell only places it. Nothing to click, so it takes no
//! pointer and needs no rect recorded for the routing.

use egui::{Align2, FontId, Rect, pos2, vec2};

use crate::gui::theme::Theme;

/// Above the viewport's bottom edge, logical pixels.
const INSET_Y: f32 = 12.0;
const PAD: egui::Vec2 = vec2(11.0, 5.0);

/// Paint the readout over `viewport`, if a drag is in flight.
pub(in crate::gui) fn draw_gizmo_readout(
    ui: &egui::Ui,
    viewport: Rect,
    text: Option<&str>,
    theme: Theme,
) {
    let Some(text) = text else {
        return;
    };
    let painter = ui.painter();
    let font = FontId::monospace(12.0);
    let galley = painter.layout_no_wrap(text.to_string(), font, theme.fg);
    let size = galley.size() + PAD * 2.0;
    let rect = Rect::from_center_size(
        pos2(
            viewport.center().x,
            viewport.bottom() - INSET_Y - size.y * 0.5,
        ),
        size,
    );
    painter.rect_filled(rect, 3.0, theme.bg_elevated);
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0_f32, theme.accent),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        Align2::CENTER_CENTER
            .anchor_size(rect.center(), galley.size())
            .min,
        galley,
        theme.fg,
    );
}
