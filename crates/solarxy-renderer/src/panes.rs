//! Split-viewport layout math shared by both shells: pane rectangles for
//! the F1-F5 layouts, the largest-pane render-target dims, the cursor to
//! pane hit test, and the split-divider rects.
//!
//! Moved verbatim from `solarxy-app/src/state/panes.rs` in the web
//! milestone's so `solarxy-web` renders, routes pointer input, and
//! positions DOM pane toolbars from the same geometry the desktop uses.
//! All functions are pure: the shells supply the viewport origin/size in
//! whatever pixel space they own (desktop: physical px anchored to the
//! egui-dock Viewport tab; web: physical canvas px at origin zero).

use solarxy_core::view_config::ViewLayout;

/// One pane's rectangle. Units are whatever the caller passed in
/// (`compute_panes` is unit-agnostic; both shells use physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PaneRect {
    /// The 3D content sub-rect — the pane minus the per-pane toolbar
    /// strip at the top. `toolbar_h` is in the pane's own pixel units.
    #[must_use]
    pub fn content(&self, toolbar_h: f32) -> PaneRect {
        PaneRect {
            x: self.x,
            y: self.y + toolbar_h,
            width: self.width,
            height: (self.height - toolbar_h).max(1.0),
        }
    }

    /// Whether `cursor` falls inside this rect (half-open on max edges).
    #[must_use]
    pub fn contains(&self, cursor: (f32, f32)) -> bool {
        let (cx, cy) = cursor;
        cx >= self.x && cx < self.x + self.width && cy >= self.y && cy < self.y + self.height
    }
}

/// Pane rectangles for `layout` inside the viewport region at `origin`
/// with `size`, split at `split_ratio` (only the two-pane layouts use it).
/// A 2px gap (1px each side of the split line) separates adjacent panes.
#[must_use]
pub fn compute_panes(
    layout: ViewLayout,
    split_ratio: f32,
    origin: (f32, f32),
    size: (f32, f32),
) -> Vec<PaneRect> {
    let (origin_x, origin_y) = origin;
    let (w, h) = size;
    let ratio = split_ratio;
    match layout {
        ViewLayout::Single => vec![PaneRect {
            x: origin_x,
            y: origin_y,
            width: w,
            height: h,
        }],
        ViewLayout::SplitVertical => {
            let split = (w * ratio).floor();
            vec![
                PaneRect {
                    x: origin_x,
                    y: origin_y,
                    width: (split - 1.0).max(1.0),
                    height: h,
                },
                PaneRect {
                    x: origin_x + split + 1.0,
                    y: origin_y,
                    width: (w - split - 1.0).max(1.0),
                    height: h,
                },
            ]
        }
        ViewLayout::SplitHorizontal => {
            let split = (h * ratio).floor();
            vec![
                PaneRect {
                    x: origin_x,
                    y: origin_y,
                    width: w,
                    height: (split - 1.0).max(1.0),
                },
                PaneRect {
                    x: origin_x,
                    y: origin_y + split + 1.0,
                    width: w,
                    height: (h - split - 1.0).max(1.0),
                },
            ]
        }
        ViewLayout::Quad => {
            let sx = (w * 0.5).floor();
            let sy = (h * 0.5).floor();
            let left_w = (sx - 1.0).max(1.0);
            let right_w = (w - sx - 1.0).max(1.0);
            let top_h = (sy - 1.0).max(1.0);
            let bot_h = (h - sy - 1.0).max(1.0);
            let rx = origin_x + sx + 1.0;
            let by = origin_y + sy + 1.0;
            vec![
                PaneRect {
                    x: origin_x,
                    y: origin_y,
                    width: left_w,
                    height: top_h,
                },
                PaneRect {
                    x: rx,
                    y: origin_y,
                    width: right_w,
                    height: top_h,
                },
                PaneRect {
                    x: origin_x,
                    y: by,
                    width: left_w,
                    height: bot_h,
                },
                PaneRect {
                    x: rx,
                    y: by,
                    width: right_w,
                    height: bot_h,
                },
            ]
        }
        ViewLayout::ThreeLeftBig => {
            let sx = (w * 0.5).floor();
            let sy = (h * 0.5).floor();
            let left_w = (sx - 1.0).max(1.0);
            let right_w = (w - sx - 1.0).max(1.0);
            let top_h = (sy - 1.0).max(1.0);
            let bot_h = (h - sy - 1.0).max(1.0);
            let rx = origin_x + sx + 1.0;
            let by = origin_y + sy + 1.0;
            vec![
                PaneRect {
                    x: origin_x,
                    y: origin_y,
                    width: left_w,
                    height: h,
                },
                PaneRect {
                    x: rx,
                    y: origin_y,
                    width: right_w,
                    height: top_h,
                },
                PaneRect {
                    x: rx,
                    y: by,
                    width: right_w,
                    height: bot_h,
                },
            ]
        }
    }
}

/// Pixel dimensions of the shared HDR render target. The target is sized
/// to the **largest** pane the layout produces and reused for every pane;
/// the composite pass scales it down to each pane's surface rect. Quad
/// uses quarter-size panes; Three-Left-Big's largest pane is the
/// full-height left pane (half width).
#[must_use]
pub fn compute_target_dimensions(layout: ViewLayout, width: u32, height: u32) -> (u32, u32) {
    let half_w = ((width as f32 * 0.5).floor() as u32).max(1);
    let half_h = ((height as f32 * 0.5).floor() as u32).max(1);
    match layout {
        ViewLayout::Single => (width, height),
        ViewLayout::SplitVertical | ViewLayout::ThreeLeftBig => (half_w, height),
        ViewLayout::SplitHorizontal => (width, half_h),
        ViewLayout::Quad => (half_w, half_h),
    }
}

/// Index of the pane under `cursor`, or `0` when the cursor misses every
/// pane (the primary pane keeps focus in the gaps and outside the window).
#[must_use]
pub fn hit_test_pane(panes: &[PaneRect], cursor: (f32, f32)) -> usize {
    for (i, pane) in panes.iter().enumerate() {
        if pane.contains(cursor) {
            return i;
        }
    }
    0
}

/// The visible split-divider line for the two-pane layouts, `thickness`
/// wide, or `None` for layouts without a draggable divider. Same unit
/// space as `compute_panes`.
#[must_use]
pub fn divider_rect(
    layout: ViewLayout,
    split_ratio: f32,
    origin: (f32, f32),
    size: (f32, f32),
    thickness: f32,
) -> Option<PaneRect> {
    let (origin_x, origin_y) = origin;
    let (w, h) = size;
    match layout {
        ViewLayout::Single | ViewLayout::Quad | ViewLayout::ThreeLeftBig => None,
        ViewLayout::SplitVertical => {
            let cx = (w * split_ratio).floor();
            Some(PaneRect {
                x: origin_x + (cx - 1.0),
                y: origin_y,
                width: thickness,
                height: h,
            })
        }
        ViewLayout::SplitHorizontal => {
            let cy = (h * split_ratio).floor();
            Some(PaneRect {
                x: origin_x,
                y: origin_y + (cy - 1.0),
                width: w,
                height: thickness,
            })
        }
    }
}

/// The divider's pointer hit rect — the visible rect expanded by `pad`
/// along the split axis so it stays grabbable at thin widths.
#[must_use]
pub fn divider_hit_rect(
    layout: ViewLayout,
    split_ratio: f32,
    origin: (f32, f32),
    size: (f32, f32),
    thickness: f32,
    pad: f32,
) -> Option<PaneRect> {
    let visible = divider_rect(layout, split_ratio, origin, size, thickness)?;
    Some(match layout {
        ViewLayout::SplitVertical => PaneRect {
            x: visible.x - pad,
            y: visible.y,
            width: visible.width + pad * 2.0,
            height: visible.height,
        },
        ViewLayout::SplitHorizontal => PaneRect {
            x: visible.x,
            y: visible.y - pad,
            width: visible.width,
            height: visible.height + pad * 2.0,
        },
        ViewLayout::Single | ViewLayout::Quad | ViewLayout::ThreeLeftBig => visible,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact literals in, exact literals out
    use super::*;

    fn pane(x: f32, y: f32, width: f32, height: f32) -> PaneRect {
        PaneRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn hit_test_single_pane() {
        let panes = [pane(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(hit_test_pane(&panes, (500.0, 500.0)), 0);
        assert_eq!(hit_test_pane(&panes, (0.0, 0.0)), 0);
        assert_eq!(hit_test_pane(&panes, (1919.0, 1079.0)), 0);
    }

    #[test]
    fn hit_test_vertical_split() {
        let half = 960.0_f32;
        let panes = [
            pane(0.0, 0.0, half - 1.0, 1080.0),
            pane(half + 1.0, 0.0, 1920.0 - half - 1.0, 1080.0),
        ];
        assert_eq!(hit_test_pane(&panes, (100.0, 500.0)), 0);
        assert_eq!(hit_test_pane(&panes, (958.0, 500.0)), 0);
        assert_eq!(hit_test_pane(&panes, (962.0, 500.0)), 1);
        assert_eq!(hit_test_pane(&panes, (1500.0, 500.0)), 1);
        assert_eq!(hit_test_pane(&panes, (960.0, 500.0)), 0);
    }

    #[test]
    fn hit_test_horizontal_split() {
        let half = 540.0_f32;
        let panes = [
            pane(0.0, 0.0, 1920.0, half - 1.0),
            pane(0.0, half + 1.0, 1920.0, 1080.0 - half - 1.0),
        ];
        assert_eq!(hit_test_pane(&panes, (500.0, 100.0)), 0);
        assert_eq!(hit_test_pane(&panes, (500.0, 600.0)), 1);
        assert_eq!(hit_test_pane(&panes, (500.0, 540.0)), 0);
    }

    #[test]
    fn hit_test_cursor_outside_window() {
        let panes = [pane(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(hit_test_pane(&panes, (-10.0, 500.0)), 0);
        assert_eq!(hit_test_pane(&panes, (2000.0, 500.0)), 0);
    }

    #[test]
    fn hit_test_exact_boundaries() {
        let panes = [pane(0.0, 0.0, 100.0, 100.0), pane(102.0, 0.0, 100.0, 100.0)];
        assert_eq!(hit_test_pane(&panes, (0.0, 0.0)), 0);
        assert_eq!(hit_test_pane(&panes, (99.9, 50.0)), 0);
        assert_eq!(hit_test_pane(&panes, (100.0, 50.0)), 0);
        assert_eq!(hit_test_pane(&panes, (102.0, 0.0)), 1);
    }

    #[test]
    fn hit_test_empty_panes() {
        let panes: [PaneRect; 0] = [];
        assert_eq!(hit_test_pane(&panes, (500.0, 500.0)), 0);
    }

    #[test]
    fn target_dims_single() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::Single, 1920, 1080),
            (1920, 1080)
        );
    }

    #[test]
    fn target_dims_vertical_split() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitVertical, 1920, 1080),
            (960, 1080)
        );
    }

    #[test]
    fn target_dims_horizontal_split() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitHorizontal, 1920, 1080),
            (1920, 540)
        );
    }

    #[test]
    fn target_dims_odd_width() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitVertical, 1921, 1080),
            (960, 1080)
        );
    }

    #[test]
    fn target_dims_minimum() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitVertical, 2, 2),
            (1, 2)
        );
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitHorizontal, 2, 2),
            (2, 1)
        );
    }

    #[test]
    fn target_dims_quad() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::Quad, 1920, 1080),
            (960, 540)
        );
    }

    #[test]
    fn target_dims_three_left_big() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::ThreeLeftBig, 1920, 1080),
            (960, 1080)
        );
    }

    #[test]
    fn target_dims_never_zero() {
        assert_eq!(compute_target_dimensions(ViewLayout::Quad, 1, 1), (1, 1));
    }

    #[test]
    fn pane_counts_per_layout() {
        let size = (1920.0, 1080.0);
        for (layout, n) in [
            (ViewLayout::Single, 1),
            (ViewLayout::SplitVertical, 2),
            (ViewLayout::SplitHorizontal, 2),
            (ViewLayout::Quad, 4),
            (ViewLayout::ThreeLeftBig, 3),
        ] {
            assert_eq!(
                compute_panes(layout, 0.5, (0.0, 0.0), size).len(),
                n,
                "{layout:?}"
            );
            assert_eq!(layout.pane_count(), n, "{layout:?} pane_count");
        }
    }

    #[test]
    fn panes_respect_origin_offset() {
        let panes = compute_panes(
            ViewLayout::SplitVertical,
            0.5,
            (100.0, 50.0),
            (800.0, 600.0),
        );
        assert_eq!(panes[0].x, 100.0);
        assert_eq!(panes[0].y, 50.0);
        assert_eq!(panes[1].x, 100.0 + 400.0 + 1.0);
    }

    #[test]
    fn split_ratio_moves_the_divide() {
        let panes = compute_panes(ViewLayout::SplitVertical, 0.25, (0.0, 0.0), (1000.0, 600.0));
        assert_eq!(panes[0].width, 249.0);
        assert_eq!(panes[1].x, 251.0);
        assert_eq!(panes[1].width, 749.0);
    }

    #[test]
    fn divider_only_on_two_pane_layouts() {
        let o = (0.0, 0.0);
        let s = (1000.0, 600.0);
        assert!(divider_rect(ViewLayout::Single, 0.5, o, s, 2.0).is_none());
        assert!(divider_rect(ViewLayout::Quad, 0.5, o, s, 2.0).is_none());
        assert!(divider_rect(ViewLayout::ThreeLeftBig, 0.5, o, s, 2.0).is_none());
        let v = divider_rect(ViewLayout::SplitVertical, 0.5, o, s, 2.0).unwrap();
        assert_eq!(v.x, 499.0);
        assert_eq!(v.width, 2.0);
        assert_eq!(v.height, 600.0);
        let h = divider_rect(ViewLayout::SplitHorizontal, 0.5, o, s, 2.0).unwrap();
        assert_eq!(h.y, 299.0);
        assert_eq!(h.height, 2.0);
    }

    #[test]
    fn divider_hit_rect_expands_along_split_axis() {
        let o = (0.0, 0.0);
        let s = (1000.0, 600.0);
        let hit = divider_hit_rect(ViewLayout::SplitVertical, 0.5, o, s, 2.0, 3.0).unwrap();
        assert_eq!(hit.x, 496.0);
        assert_eq!(hit.width, 8.0);
        assert_eq!(hit.height, 600.0);
    }

    #[test]
    fn content_rect_reserves_toolbar() {
        let p = pane(10.0, 20.0, 300.0, 200.0);
        let c = p.content(22.0);
        assert_eq!(c.x, 10.0);
        assert_eq!(c.y, 42.0);
        assert_eq!(c.width, 300.0);
        assert_eq!(c.height, 178.0);
    }
}
