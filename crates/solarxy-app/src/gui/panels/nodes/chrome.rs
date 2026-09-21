//! What sits on top of the canvas: the overview inset and the zoom
//! controls, and the request type the panel's menu and its keys both fill.
//!
//! All of it is a reading aid rather than a view of the document, which
//! is why every toggle persists in preferences and none of it raises a
//! command. The one exception is auto-layout, which is in the panel's View
//! menu because that is where a user looks for it, and which moves nodes
//! and therefore is one command like every other gesture.
//!
//! There was a strip of eleven buttons here until 0.10.0. The panel's menu
//! bar carries what they did, as the browser's does, and the zoom cluster
//! moved onto the canvas, where the preference that was always meant to
//! show and hide it finally does.

use egui::{Color32, Rect, Stroke, Ui, pos2, vec2};
use solarxy_graph::document::Graph;

use super::art::NODE_BOX;
use crate::gui::theme::Theme;

/// The grid a snapped node lands on, and what the background draws.
/// Eighteen pixels, which is the browser's spacing, so a graph tidied or
/// snapped on either shell sits on the same lattice.
pub(super) const GRID: f32 = 18.0;

/// The overview inset's size, and how far it sits from the corner.
const MINIMAP: egui::Vec2 = vec2(160.0, 110.0);
const MARGIN: f32 = 8.0;

/// The room the zoom controls are laid out in.
const ZOOM_STRIP: egui::Vec2 = vec2(120.0, 24.0);

/// What the panel's menu or one of its keys asked for. One type for both,
/// so an entry and the key it shows are one path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ChromeRequest {
    pub layout: bool,
    /// Swap between the graph and the rows.
    pub view: bool,
    pub fit: bool,
    /// A preference that was toggled, applied by the drain so it is
    /// written and saved in one place.
    pub toggled: Option<Toggle>,
    /// Open the info card on the selected node.
    pub info: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ZoomStep {
    In,
    Out,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Toggle {
    Grid,
    Snap,
    Minimap,
    Controls,
}

/// Round a position onto the grid.
///
/// Applied when a drag commits rather than while it runs, which is the
/// difference between the two shells and not a visible one: the browser
/// snaps the node under the pointer, this snaps what the document
/// records, and the document ends up holding the same numbers.
#[must_use]
pub(super) fn snap(position: [f32; 2]) -> [f32; 2] {
    [
        (position[0] / GRID).round() * GRID,
        (position[1] / GRID).round() * GRID,
    ]
}

/// The zoom controls, in the canvas's bottom-left corner, which is where
/// the browser's sit and the corner the overview inset leaves free.
///
/// Laid out in the panel's own layer after the canvas has drawn, so a press
/// here is the button's rather than the canvas's underneath it.
pub(super) fn zoom_controls(ui: &mut Ui, area: Rect, scale: f32, theme: Theme) -> Option<ZoomStep> {
    let strip = Rect::from_min_size(
        pos2(area.left() + MARGIN, area.bottom() - ZOOM_STRIP.y - MARGIN),
        ZOOM_STRIP,
    );
    let mut step = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| {
        egui::Frame::new()
            .fill(theme.bg.gamma_multiply(0.9))
            .stroke(Stroke::new(1.0_f32, theme.border))
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(4, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("-").on_hover_text("Zoom out").clicked() {
                        step = Some(ZoomStep::Out);
                    }
                    if ui
                        .small_button(format!("{:.0}%", scale * 100.0))
                        .on_hover_text("Reset the zoom")
                        .clicked()
                    {
                        step = Some(ZoomStep::Reset);
                    }
                    if ui.small_button("+").on_hover_text("Zoom in").clicked() {
                        step = Some(ZoomStep::In);
                    }
                });
            });
    });
    step
}

/// The overview inset: every node as a dot in its category's fill, and a
/// frame showing what is on screen.
///
/// Drawn from the document rather than from the canvas, so it is right
/// even on the frame a node first appears, and cheap enough to redraw
/// every frame because a context holds a handful of nodes.
pub(super) fn minimap(
    ui: &Ui,
    area: Rect,
    graph: &Graph,
    registry: &solarxy_graph::registry::Registry,
    viewport: Rect,
    to_screen: egui::emath::TSTransform,
    theme: Theme,
) {
    let Some(bounds) = graph_bounds(graph) else {
        return;
    };
    let inset = Rect::from_min_size(
        pos2(
            area.right() - MINIMAP.x - MARGIN,
            area.bottom() - MINIMAP.y - MARGIN,
        ),
        MINIMAP,
    );
    let painter = ui.painter_at(inset);
    painter.rect_filled(
        inset,
        egui::CornerRadius::same(4),
        theme.bg.gamma_multiply(0.9),
    );
    painter.rect_stroke(
        inset,
        egui::CornerRadius::same(4),
        Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );

    let map = fit_into(bounds, inset.shrink(6.0));
    let palette = solarxy_core::theme::Palette::for_dark(theme.dark);
    for node in graph.nodes() {
        let fill = registry.get(&node.type_id).map_or(theme.muted, |desc| {
            let rgb = solarxy_studio::types::node_fill(desc.category, desc.opens, &palette);
            Color32::from_rgb(rgb.r, rgb.g, rgb.b)
        });
        let at = map(pos2(node.position[0], node.position[1]));
        painter.circle_filled(at, 2.0, fill);
    }

    // What the viewport is looking at, mapped through the same fit.
    let seen = to_screen.inverse() * viewport;
    let frame = Rect::from_min_max(map(seen.min), map(seen.max)).intersect(inset);
    painter.rect_stroke(
        frame,
        egui::CornerRadius::ZERO,
        Stroke::new(1.0_f32, theme.accent),
        egui::StrokeKind::Inside,
    );
}

/// The box every node in a graph occupies, in graph space.
fn graph_bounds(graph: &Graph) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for node in graph.nodes() {
        let rect = Rect::from_min_size(pos2(node.position[0], node.position[1]), NODE_BOX);
        bounds = Some(bounds.map_or(rect, |b: Rect| b.union(rect)));
    }
    bounds
}

/// Map a source box onto a target, keeping the aspect ratio.
fn fit_into(source: Rect, target: Rect) -> impl Fn(egui::Pos2) -> egui::Pos2 {
    let scale =
        (target.width() / source.width().max(1.0)).min(target.height() / source.height().max(1.0));
    let offset = target.center() - (source.center().to_vec2() * scale);
    move |p| pos2(offset.x + p.x * scale, offset.y + p.y * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 0.001 && (a[1] - b[1]).abs() < 0.001
    }

    /// The grid is the browser's eighteen pixels, so a graph snapped on
    /// either shell sits on the same lattice.
    #[test]
    fn snapping_lands_on_the_browsers_lattice() {
        assert!((GRID - 18.0).abs() < f32::EPSILON);
        assert!(close(snap([0.0, 0.0]), [0.0, 0.0]));
        assert!(close(snap([8.0, 10.0]), [0.0, 18.0]));
        assert!(close(snap([-8.0, -10.0]), [0.0, -18.0]));
        assert!(close(snap([36.0, 54.0]), [36.0, 54.0]));
    }

    /// Snapping twice is snapping once, or a node would creep every time
    /// it was touched.
    #[test]
    fn snapping_is_idempotent() {
        for position in [[13.0, 47.0], [-101.0, 9.0], [0.5, -0.5]] {
            assert!(close(snap(snap(position)), snap(position)));
        }
    }

    /// A fit keeps the aspect ratio, so a wide graph does not become a
    /// square one in the inset.
    #[test]
    fn the_inset_fit_keeps_the_aspect_ratio() {
        let source = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 100.0));
        let target = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 200.0));
        let map = fit_into(source, target);
        let width = map(source.right_top()).x - map(source.left_top()).x;
        let height = map(source.left_bottom()).y - map(source.left_top()).y;
        assert!(
            (width / height - 4.0).abs() < 0.01,
            "a four-to-one graph must stay four to one"
        );
    }
}
