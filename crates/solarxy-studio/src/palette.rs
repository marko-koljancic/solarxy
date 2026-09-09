//! Where the node palette opens.
//!
//! Geometry with no toolkit in it: a pointer, the pane it was pressed in,
//! and the panel's measured size go in, and a top-left corner comes out.
//! Both shells measure differently and draw differently, and neither has
//! any business deciding this twice.
//!
//! The rule earns its own module because it replaced a CSS pin. The panel
//! used to sit at a fixed offset against a `position: fixed` backdrop,
//! which made the backdrop the containing block, so the offsets resolved
//! against the whole window and the pane the palette belonged to was
//! irrelevant. It opened in the window's corner wherever you were looking.
//! Blender and Houdini both spawn their add-node menu at the pointer and
//! drop the node there, which is what this computes instead.

/// A rectangle in the coordinate space the caller measured in.
///
/// The browser passes viewport CSS pixels; the desktop passes its own
/// screen points. The rule never asks which, because it only ever compares
/// the two against each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// The gap kept between the panel and the pane's edges when clamping.
pub const MARGIN_PX: f32 = 8.0;

/// How far the panel's top-left sits from the pointer, so the cursor lands
/// just outside the panel rather than on top of its first row.
const POINTER_OFFSET_PX: f32 = 6.0;

impl Rect {
    fn contains(self, p: Point) -> bool {
        p.x >= self.left
            && p.x <= self.left + self.width
            && p.y >= self.top
            && p.y <= self.top + self.height
    }
}

/// Clamps, and survives an inverted range.
///
/// A pane narrower than the panel makes the upper bound smaller than the
/// lower one, and the two edges cannot both be satisfied. Pinning to the
/// near edge is the answer that still shows a usable panel; letting the
/// bounds cross would put it outside the pane entirely.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi.max(lo))
}

/// The palette's top-left corner.
///
/// At the pointer while the pointer is over the pane; otherwise centred
/// horizontally and set down from the pane's top edge, which is where a
/// command palette is expected when it was opened from a menu rather than
/// from a gesture. Always clamped so the whole panel stays inside the
/// pane, because opening a menu half off-screen is worse than opening it
/// slightly away from the cursor.
#[must_use]
pub fn palette_placement(pointer: Option<Point>, pane: Rect, panel: Size, margin: f32) -> Point {
    let min_x = pane.left + margin;
    let min_y = pane.top + margin;
    let max_x = pane.left + pane.width - panel.width - margin;
    let max_y = pane.top + pane.height - panel.height - margin;

    match pointer {
        Some(p) if pane.contains(p) => Point {
            x: clamp(p.x + POINTER_OFFSET_PX, min_x, max_x),
            y: clamp(p.y + POINTER_OFFSET_PX, min_y, max_y),
        },
        _ => Point {
            x: clamp(pane.left + (pane.width - panel.width) / 2.0, min_x, max_x),
            // A sixth down reads better than dead centre: the eye is
            // already high in the pane, and it leaves room for the list to
            // grow downward.
            y: clamp(pane.top + pane.height / 6.0, min_y, max_y),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A pane offset from the origin: the defect being guarded against is
    // placement that ignores the pane and resolves against the window, so a
    // pane at the origin would hide it.
    const PANE: Rect = Rect {
        left: 400.0,
        top: 100.0,
        width: 600.0,
        height: 500.0,
    };
    const PANEL: Size = Size {
        width: 440.0,
        height: 300.0,
    };

    fn at(pointer: Option<Point>, pane: Rect, panel: Size) -> Point {
        palette_placement(pointer, pane, panel, MARGIN_PX)
    }

    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn opens_at_the_pointer_when_the_pointer_is_over_the_pane() {
        let got = at(Some(p(500.0, 200.0)), PANE, PANEL);
        assert!((got.x - 506.0).abs() < 1e-3);
        assert!((got.y - 206.0).abs() < 1e-3);
    }

    #[test]
    fn stays_inside_the_pane_when_the_pointer_is_near_the_far_edge() {
        // Bottom-right corner: naive pointer placement would put the panel
        // mostly outside the pane.
        let got = at(Some(p(995.0, 595.0)), PANE, PANEL);
        assert!((got.x - (400.0 + 600.0 - 440.0 - 8.0)).abs() < 1e-3);
        assert!((got.y - (100.0 + 500.0 - 300.0 - 8.0)).abs() < 1e-3);
        assert!(got.x + PANEL.width <= PANE.left + PANE.width);
        assert!(got.y + PANEL.height <= PANE.top + PANE.height);
    }

    #[test]
    fn never_places_the_panel_above_or_left_of_the_pane() {
        let got = at(Some(p(401.0, 101.0)), PANE, PANEL);
        assert!(got.x >= PANE.left);
        assert!(got.y >= PANE.top);
    }

    #[test]
    fn centres_over_the_pane_with_no_pointer() {
        let got = at(None, PANE, PANEL);
        assert!((got.x - (400.0 + (600.0 - 440.0) / 2.0)).abs() < 1e-3);
        assert!((got.y - (100.0 + 500.0 / 6.0)).abs() < 1e-3);
    }

    #[test]
    fn centres_when_the_pointer_is_outside_the_pane() {
        assert_eq!(at(Some(p(50.0, 50.0)), PANE, PANEL), at(None, PANE, PANEL));
    }

    #[test]
    fn lands_inside_the_pane_for_pointers_all_over_it() {
        // The regression that motivated the rule: the old placement
        // resolved against the window, so the palette appeared in the
        // window's corner regardless of the pane. Wherever it lands now, it
        // must be within the pane.
        let mut x = PANE.left;
        while x <= PANE.left + PANE.width {
            let mut y = PANE.top;
            while y <= PANE.top + PANE.height {
                let got = at(Some(p(x, y)), PANE, PANEL);
                assert!(got.x >= PANE.left && got.y >= PANE.top, "at {x},{y}");
                assert!(got.x + PANEL.width <= PANE.left + PANE.width, "at {x},{y}");
                assert!(got.y + PANEL.height <= PANE.top + PANE.height, "at {x},{y}");
                y += 41.0;
            }
            x += 37.0;
        }
    }

    #[test]
    fn degrades_to_the_panes_top_left_when_the_pane_is_smaller_than_the_panel() {
        // A pane narrower than the panel cannot satisfy both edges; it must
        // still pin to the near edge rather than producing an inverted
        // clamp that lands outside the pane.
        let tiny = Rect {
            left: 10.0,
            top: 20.0,
            width: 100.0,
            height: 80.0,
        };
        let got = at(Some(p(60.0, 60.0)), tiny, PANEL);
        assert!((got.x - 18.0).abs() < 1e-3);
        assert!((got.y - 28.0).abs() < 1e-3);
    }
}
