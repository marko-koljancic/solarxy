//! Sockets, and where they sit.
//!
//! **Colour says what a wire carries and shape separates the members of a
//! family**, so a reader who cannot tell two hues apart can still tell one
//! port from another. Both channels come from `solarxy_studio::types`, so
//! the two shells cannot disagree about what a wire type looks like;
//! neither is authored here.
//!
//! ## Why the sockets are placed by hand
//!
//! The substrate puts pins on a node's left and right edges, in all three
//! layouts it offers, and its source carries a note saying vertical ones
//! are still to come. The browser's canvas flows downward: inputs on the
//! top edge, outputs on the bottom, spread evenly along it. Matching that
//! is a matter of returning a different rectangle, because the library
//! uses whatever [`SnarlPin::pin_rect`] answers for three separate things:
//! where the socket draws, where it is clicked, and where a wire attaches.
//! One override moves all three.

use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2, pos2};
use egui_snarl::ui::{PinWireInfo, SnarlPin, SnarlStyle, WireStyle};
use solarxy_studio::types::{HandleShape, PortSide};

use super::art::NODE_BOX;

/// A socket on the box's leading or trailing edge.
pub(super) struct EdgePin {
    /// The node's layout box in the substrate's own coordinates. `None`
    /// only on a frame where the node has not been drawn yet, which is
    /// the one frame a node first appears.
    pub box_rect: Option<Rect>,
    pub side: PortSide,
    pub index: usize,
    pub count: usize,
    pub shape: HandleShape,
    pub fill: Color32,
    pub border: Color32,
    pub wire_style: WireStyle,
}

impl SnarlPin for EdgePin {
    fn pin_rect(&self, x: f32, y0: f32, y1: f32, size: f32) -> Rect {
        let rect = self
            .box_rect
            .unwrap_or_else(|| derive_box(x, y0, y1, self.side));
        Rect::from_center_size(
            centre(rect, self.side, self.index, self.count),
            Vec2::splat(size),
        )
    }

    fn draw(
        self,
        _snarl_style: &SnarlStyle,
        _style: &egui::Style,
        rect: Rect,
        painter: &Painter,
    ) -> PinWireInfo {
        paint_handle(painter, self.shape, rect, self.fill, self.border);
        PinWireInfo {
            color: self.fill,
            style: self.wire_style,
        }
    }
}

/// Where one socket sits along its edge.
///
/// Evenly spread with a half gap at each end, so a single socket lands on
/// the centre line and a pair straddles it. Pure, and the same arithmetic
/// the marker pass uses, which is what keeps a drawn socket and the wire
/// leaving it in the same place.
#[must_use]
pub(super) fn centre(box_rect: Rect, side: PortSide, index: usize, count: usize) -> Pos2 {
    #[allow(clippy::cast_precision_loss)]
    let fraction = (index + 1) as f32 / (count + 1) as f32;
    let y = match side {
        PortSide::Input => box_rect.top(),
        PortSide::Output => box_rect.bottom(),
    };
    pos2(box_rect.left() + box_rect.width() * fraction, y)
}

/// The node's box, recovered from what the substrate hands a pin.
///
/// Only reached on a node's first frame, before its own draw has recorded
/// the real rectangle. With edge pin placement the horizontal edge is
/// exact, and the box is a fixed size, so the only thing being guessed is
/// where the collapsed pin rows sit relative to the header.
fn derive_box(x: f32, y0: f32, y1: f32, side: PortSide) -> Rect {
    let left = match side {
        PortSide::Input => x,
        PortSide::Output => x - NODE_BOX.x,
    };
    let bottom = y0.max(y1);
    Rect::from_min_size(pos2(left, bottom - NODE_BOX.y), NODE_BOX)
}

/// Draw one socket.
///
/// Three of the seven are filled outlines and four are cut shapes, which
/// is the browser's split too: a bar, a triangle and a hexagon read as
/// silhouettes and gain nothing from a border, while a ring is a border
/// with nothing inside it.
pub(super) fn paint_handle(
    painter: &Painter,
    shape: HandleShape,
    rect: Rect,
    fill: Color32,
    border: Color32,
) {
    let centre = rect.center();
    let size = rect.width().min(rect.height());
    let half = size / 2.0;
    let outline = Stroke::new(2.0_f32, border);

    match shape {
        HandleShape::Round => {
            painter.circle(centre, half, fill, outline);
        }
        HandleShape::Square => {
            painter.rect(
                Rect::from_center_size(centre, Vec2::splat(size)),
                egui::CornerRadius::same(2),
                fill,
                outline,
                egui::StrokeKind::Inside,
            );
        }
        HandleShape::Diamond => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(centre.x, centre.y - half),
                    pos2(centre.x + half, centre.y),
                    pos2(centre.x, centre.y + half),
                    pos2(centre.x - half, centre.y),
                ],
                fill,
                outline,
            ));
        }
        // A ring is a border with nothing inside it, which is what makes
        // a material handle read as a slot rather than as a value.
        HandleShape::Ring => {
            painter.circle_stroke(centre, half * 0.7, Stroke::new(3.0_f32, fill));
        }
        // The three cut shapes carry no border: an outline on a silhouette
        // this small closes the shape up into a blob.
        HandleShape::Bar => {
            painter.rect_filled(
                Rect::from_center_size(centre, Vec2::new(size, size * 0.46)),
                egui::CornerRadius::ZERO,
                fill,
            );
        }
        HandleShape::Triangle => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(centre.x, centre.y - half),
                    pos2(centre.x + half, centre.y + half),
                    pos2(centre.x - half, centre.y + half),
                ],
                fill,
                Stroke::NONE,
            ));
        }
        HandleShape::Hexagon => {
            let quarter = half * 0.5;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(centre.x, centre.y - half),
                    pos2(centre.x + half, centre.y - quarter),
                    pos2(centre.x + half, centre.y + quarter),
                    pos2(centre.x, centre.y + half),
                    pos2(centre.x - half, centre.y + quarter),
                    pos2(centre.x - half, centre.y - quarter),
                ],
                fill,
                Stroke::NONE,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_rect() -> Rect {
        Rect::from_min_size(pos2(100.0, 200.0), NODE_BOX)
    }

    /// Sockets sit on the edges the flow runs between, not on the sides
    /// the substrate would put them on.
    #[test]
    fn inputs_sit_on_the_top_edge_and_outputs_on_the_bottom() {
        let rect = box_rect();
        assert!((centre(rect, PortSide::Input, 0, 1).y - rect.top()).abs() < 0.01);
        assert!((centre(rect, PortSide::Output, 0, 1).y - rect.bottom()).abs() < 0.01);
    }

    /// One socket lands on the centre line, two straddle it, and three
    /// are evenly spread with a half gap at each end.
    #[test]
    fn sockets_spread_evenly_with_a_half_gap_at_each_end() {
        let rect = box_rect();
        let at = |index, count| centre(rect, PortSide::Input, index, count).x - rect.left();

        assert!((at(0, 1) - NODE_BOX.x / 2.0).abs() < 0.01);

        let (first, second) = (at(0, 2), at(1, 2));
        assert!((first - NODE_BOX.x / 3.0).abs() < 0.01);
        assert!(
            (f32::midpoint(first, second) - NODE_BOX.x / 2.0).abs() < 0.01,
            "a pair must straddle the centre"
        );

        let three: Vec<f32> = (0..3).map(|i| at(i, 3)).collect();
        for window in three.windows(2) {
            assert!((window[1] - window[0] - NODE_BOX.x / 4.0).abs() < 0.01);
        }
    }

    /// A socket never leaves the box it belongs to, however many there
    /// are, or a wire would attach beside the node rather than to it.
    #[test]
    fn a_socket_stays_within_its_box() {
        let rect = box_rect();
        for count in 1..12 {
            for index in 0..count {
                let p = centre(rect, PortSide::Input, index, count);
                assert!(
                    p.x > rect.left() && p.x < rect.right(),
                    "socket {index} of {count} left the box at {p:?}"
                );
            }
        }
    }

    /// The fallback recovers the box from what a pin is handed, which is
    /// the horizontal edge and the row it sits in. Exact horizontally in
    /// both directions, because the box is a fixed width.
    #[test]
    fn the_fallback_recovers_the_box_from_the_edge_it_is_given() {
        let rect = box_rect();
        let from_input = derive_box(rect.left(), rect.bottom(), rect.bottom(), PortSide::Input);
        let from_output = derive_box(rect.right(), rect.bottom(), rect.bottom(), PortSide::Output);

        assert!((from_input.left() - rect.left()).abs() < 0.01);
        assert!((from_output.left() - rect.left()).abs() < 0.01);
        assert!((from_input.width() - rect.width()).abs() < 0.01);
    }
}
