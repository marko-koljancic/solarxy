//! The glyphs the viewport's two columns draw: one per transform tool on
//! the left, one per attribute control on the right.
//!
//! Art is what a shell can draw, which is the one duplication the release's
//! exit criteria sanction: the browser's are React components over mixed
//! primitives (paths, rectangles, circles, filled and stroked), not a table
//! of path strings, so unlike the node glyphs they cannot be held verbatim
//! and compared character for character. They are transplanted shape by
//! shape into painter calls in the same sixteen-unit box the browser
//! authors them in, with the same round caps.

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

use solarxy_host::gizmo::ToolMode;

/// The box every glyph is authored in, matching the browser's icons.
const BOX: f32 = 16.0;

/// Paint one tool's glyph into `rect`, in `color`.
pub(in crate::gui) fn paint_tool(painter: &Painter, rect: Rect, tool: ToolMode, color: Color32) {
    let scale = rect.width().min(rect.height()) / BOX;
    let origin = rect.center() - vec2(BOX, BOX) * scale * 0.5;
    let at = |x: f32, y: f32| origin + vec2(x, y) * scale;
    let stroke = Stroke::new((1.5 * scale).max(1.0), color);
    match tool {
        ToolMode::Select => {
            // An arrow cursor, as one closed outline.
            let pts = [
                (4.55, 2.8),
                (4.55, 12.0),
                (6.75, 9.9),
                (8.25, 13.2),
                (9.95, 12.4),
                (8.45, 9.2),
                (11.45, 8.9),
            ];
            let outline: Vec<Pos2> = pts.iter().map(|(x, y)| at(*x, *y)).collect();
            painter.add(egui::Shape::closed_line(outline, stroke));
        }
        ToolMode::Move => {
            // A four-way arrow: two axes and an arrowhead at each end.
            painter.line_segment([at(8.0, 2.2), at(8.0, 13.8)], stroke);
            painter.line_segment([at(2.2, 8.0), at(13.8, 8.0)], stroke);
            for (tip, a, b) in [
                ((8.0, 2.2), (6.1, 4.3), (9.9, 4.3)),
                ((8.0, 13.8), (6.1, 11.7), (9.9, 11.7)),
                ((2.2, 8.0), (4.3, 6.1), (4.3, 9.9)),
                ((13.8, 8.0), (11.7, 6.1), (11.7, 9.9)),
            ] {
                painter.line_segment([at(tip.0, tip.1), at(a.0, a.1)], stroke);
                painter.line_segment([at(tip.0, tip.1), at(b.0, b.1)], stroke);
            }
        }
        ToolMode::Rotate => {
            // An open arc from the top to the right, with a filled head.
            let center = at(8.0, 8.0);
            let r = 5.0 * scale;
            let arc: Vec<Pos2> = (0..=28)
                .map(|i| {
                    // From the right of the arc's gap round to just short of
                    // the head, clockwise on screen.
                    let t = -0.78 + (i as f32 / 28.0) * 5.55;
                    pos2(center.x + r * t.cos(), center.y + r * t.sin())
                })
                .collect();
            painter.add(egui::Shape::line(arc, stroke));
            painter.add(egui::Shape::convex_polygon(
                vec![at(8.1, 2.9), at(11.9, 4.1), at(10.6, 6.9)],
                color,
                Stroke::NONE,
            ));
        }
        ToolMode::Scale => {
            // A filled cube low left, an outlined one high right, joined.
            painter.rect_filled(
                Rect::from_min_size(at(2.3, 10.3), vec2(3.4, 3.4) * scale),
                0.0,
                color,
            );
            painter.rect_stroke(
                Rect::from_min_size(at(10.3, 2.3), vec2(3.4, 3.4) * scale),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([at(6.4, 9.6), at(9.6, 6.4)], stroke);
        }
        ToolMode::Aim => {
            // A reticle: a ring, a dot, and four ticks.
            let center = at(8.0, 8.0);
            painter.circle_stroke(center, 4.4 * scale, stroke);
            painter.circle_filled(center, 1.0 * scale, color);
            painter.line_segment([at(8.0, 1.4), at(8.0, 3.6)], stroke);
            painter.line_segment([at(8.0, 12.4), at(8.0, 14.6)], stroke);
            painter.line_segment([at(1.4, 8.0), at(3.6, 8.0)], stroke);
            painter.line_segment([at(12.4, 8.0), at(14.6, 8.0)], stroke);
        }
    }
}

/// The attribute strip's four glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gui) enum AttrIcon {
    /// A value tag beside its anchor point: the labels mode.
    Labels,
    /// Two arrows leaving their points: the vector mode.
    Vectors,
    /// Numbered points: the point-marker mode.
    Points,
    /// A slider pair: the settings popover.
    Settings,
}

/// Paint one attribute glyph into `rect`, in `color`.
pub(in crate::gui) fn paint_attr(painter: &Painter, rect: Rect, icon: AttrIcon, color: Color32) {
    let scale = rect.width().min(rect.height()) / BOX;
    let origin = rect.center() - vec2(BOX, BOX) * scale * 0.5;
    let at = |x: f32, y: f32| origin + vec2(x, y) * scale;
    let stroke = Stroke::new((1.5 * scale).max(1.0), color);
    let dot = |x: f32, y: f32, r: f32| painter.circle_filled(at(x, y), r * scale, color);
    match icon {
        AttrIcon::Labels => {
            dot(3.6, 12.4, 1.4);
            painter.line_segment([at(6.0, 10.5), at(8.2, 8.3)], stroke);
            painter.rect_stroke(
                Rect::from_min_size(at(7.0, 3.0), vec2(7.0, 5.2) * scale),
                1.0 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([at(8.6, 5.6), at(12.4, 5.6)], stroke);
        }
        AttrIcon::Vectors => {
            dot(4.0, 12.0, 1.3);
            painter.line_segment([at(5.0, 11.0), at(10.4, 5.6)], stroke);
            painter.line_segment([at(10.4, 5.6), at(7.8, 5.6)], stroke);
            painter.line_segment([at(10.4, 5.6), at(10.4, 8.2)], stroke);
            dot(11.6, 12.6, 1.3);
            painter.line_segment([at(11.6, 11.4), at(11.6, 7.0)], stroke);
            painter.line_segment([at(11.6, 7.0), at(10.0, 8.7)], stroke);
            painter.line_segment([at(11.6, 7.0), at(13.2, 8.7)], stroke);
        }
        AttrIcon::Points => {
            dot(4.4, 4.6, 1.5);
            dot(11.4, 6.4, 1.5);
            dot(6.4, 11.6, 1.5);
            painter.line_segment([at(12.6, 11.0), at(14.6, 11.0)], stroke);
            painter.line_segment([at(13.6, 10.0), at(13.6, 12.0)], stroke);
        }
        AttrIcon::Settings => {
            painter.line_segment([at(2.5, 5.5), at(13.5, 5.5)], stroke);
            painter.line_segment([at(2.5, 10.5), at(13.5, 10.5)], stroke);
            dot(6.0, 5.5, 1.7);
            dot(10.5, 10.5, 1.7);
        }
    }
}
