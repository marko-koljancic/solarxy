//! Split-viewport layout math: pane rectangles for `F1`/`F2`/`F3`, the
//! divider hit-rect for the (currently fixed-50/50) split, and the
//! cursor→pane hit test.
//!
//! Post-egui_dock the pane rects + the wgpu HDR render-target dims are
//! both anchored to the **Viewport tab's** rect (when the dock has
//! reported one for the current surface size), so the 3D content
//! renders 1:1 inside the area the dock layout reserves for it. Falls
//! back to full-surface dims before `egui_dock` has laid out (first frame
//! after launch) or after a `WindowEvent::Resized` invalidates the
//! cached rect.

use super::view_state::ViewLayout;
use super::{Pane, State, compute_target_dimensions, hit_test_pane};

impl State {
    /// Base (width, height) in physical pixels for the 3D viewport
    /// region — Viewport tab rect when available + non-stale, else the
    /// full surface. Origin offset (top-left of the Viewport region) is
    /// returned separately via `compute_panes` since `target_dimensions`
    /// only needs the size for texture creation.
    fn viewport_base_size_px(&self) -> (u32, u32) {
        let surface_size = (self.config.width, self.config.height);
        if let Some(rect) = self.gui.viewport_rect_for_surface(surface_size) {
            let ppp = self.window.scale_factor() as f32;
            let w = ((rect.width() * ppp).round() as u32).max(1);
            let h = ((rect.height() * ppp).round() as u32).max(1);
            (w, h)
        } else {
            surface_size
        }
    }

    fn viewport_origin_px(&self) -> (f32, f32) {
        let surface_size = (self.config.width, self.config.height);
        if let Some(rect) = self.gui.viewport_rect_for_surface(surface_size) {
            let ppp = self.window.scale_factor() as f32;
            (rect.min.x * ppp, rect.min.y * ppp)
        } else {
            (0.0, 0.0)
        }
    }

    /// Per-pane toolbar strip height in physical pixels.
    pub(super) fn pane_toolbar_height_px(&self) -> f32 {
        solarxy_core::view_config::PANE_TOOLBAR_HEIGHT * self.window.scale_factor() as f32
    }

    pub(super) fn target_dimensions(&self) -> (u32, u32) {
        let (base_w, base_h) = self.viewport_base_size_px();
        let (w, h) = compute_target_dimensions(self.view.display.layout, base_w, base_h);
        // The HDR target is sized to the largest pane's 3D content — its
        // full rect minus the toolbar strip.
        let toolbar = self.pane_toolbar_height_px().round() as u32;
        (w, h.saturating_sub(toolbar).max(1))
    }

    pub(super) fn compute_panes(&self) -> Vec<Pane> {
        let (origin_x, origin_y) = self.viewport_origin_px();
        let (base_w, base_h) = self.viewport_base_size_px();
        let w = base_w as f32;
        let h = base_h as f32;
        let ratio = self.view.display.split_ratio;
        match self.view.display.layout {
            ViewLayout::Single => vec![Pane {
                x: origin_x,
                y: origin_y,
                width: w,
                height: h,
            }],
            ViewLayout::SplitVertical => {
                let split = (w * ratio).floor();
                vec![
                    Pane {
                        x: origin_x,
                        y: origin_y,
                        width: (split - 1.0).max(1.0),
                        height: h,
                    },
                    Pane {
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
                    Pane {
                        x: origin_x,
                        y: origin_y,
                        width: w,
                        height: (split - 1.0).max(1.0),
                    },
                    Pane {
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
                    Pane {
                        x: origin_x,
                        y: origin_y,
                        width: left_w,
                        height: top_h,
                    },
                    Pane {
                        x: rx,
                        y: origin_y,
                        width: right_w,
                        height: top_h,
                    },
                    Pane {
                        x: origin_x,
                        y: by,
                        width: left_w,
                        height: bot_h,
                    },
                    Pane {
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
                    Pane {
                        x: origin_x,
                        y: origin_y,
                        width: left_w,
                        height: h,
                    },
                    Pane {
                        x: rx,
                        y: origin_y,
                        width: right_w,
                        height: top_h,
                    },
                    Pane {
                        x: rx,
                        y: by,
                        width: right_w,
                        height: bot_h,
                    },
                ]
            }
        }
    }

    /// `true` when the cursor is inside some pane's 3D **content** rect —
    /// not its toolbar strip, not an inter-pane gap. The camera-input
    /// gate uses this so toolbar clicks don't orbit the scene.
    pub(crate) fn pointer_in_pane_content(&self) -> bool {
        let (cx, cy) = self.input.cursor_pos;
        let toolbar_h = self.pane_toolbar_height_px();
        self.compute_panes().iter().any(|p| {
            let c = p.content(toolbar_h);
            cx >= c.x && cx < c.x + c.width && cy >= c.y && cy < c.y + c.height
        })
    }

    pub(super) fn active_pane_index(&self) -> usize {
        if self.view.display.layout == ViewLayout::Single {
            return 0;
        }
        let panes = self.compute_panes();
        hit_test_pane(&panes, self.input.cursor_pos)
    }

    pub(super) fn compute_divider_rect(&self) -> Option<egui::Rect> {
        let surface_size = (self.config.width, self.config.height);
        let viewport = self
            .gui
            .viewport_rect_for_surface(surface_size)
            .unwrap_or_else(|| {
                let ppp = self.window.scale_factor() as f32;
                egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(
                        self.config.width as f32 / ppp,
                        self.config.height as f32 / ppp,
                    ),
                )
            });
        let ppp = self.window.scale_factor() as f32;
        let ratio = self.view.display.split_ratio;
        match self.view.display.layout {
            ViewLayout::Single | ViewLayout::Quad | ViewLayout::ThreeLeftBig => None,
            ViewLayout::SplitVertical => {
                let cx = (viewport.width() * ratio).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2(viewport.min.x + (cx - 1.0), viewport.min.y),
                    egui::vec2(2.0 / ppp, viewport.height()),
                ))
            }
            ViewLayout::SplitHorizontal => {
                let cy = (viewport.height() * ratio).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2(viewport.min.x, viewport.min.y + (cy - 1.0)),
                    egui::vec2(viewport.width(), 2.0 / ppp),
                ))
            }
        }
    }

    pub(super) fn compute_divider_hit_rect(&self) -> Option<egui::Rect> {
        let visible = self.compute_divider_rect()?;
        let ppp = self.window.scale_factor() as f32;
        let pad_logical = 3.0 / ppp;
        Some(visible.expand2(match self.view.display.layout {
            ViewLayout::SplitVertical => egui::vec2(pad_logical, 0.0),
            ViewLayout::SplitHorizontal => egui::vec2(0.0, pad_logical),
            ViewLayout::Single | ViewLayout::Quad | ViewLayout::ThreeLeftBig => {
                egui::vec2(0.0, 0.0)
            }
        }))
    }
}
