//! Split-viewport layout adapters: the pure pane math lives in
//! `solarxy_renderer::panes` (moved there so both
//! shells share it); this module anchors it to the egui-dock Viewport tab
//! rect and the window scale factor, and converts the divider rects to
//! egui's logical coordinate space.
//!
//! Post-egui_dock the pane rects + the wgpu HDR render-target dims are
//! both anchored to the **Viewport tab's** rect (when the dock has
//! reported one for the current surface size), so the 3D content
//! renders 1:1 inside the area the dock layout reserves for it. Falls
//! back to full-surface dims before `egui_dock` has laid out (first frame
//! after launch) or after a `WindowEvent::Resized` invalidates the
//! cached rect.

use solarxy_renderer::panes;

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
        // The HDR target is sized to the largest pane in full — the 3D
        // scene fills the whole pane (the toolbar labels float on top).
        compute_target_dimensions(self.view.display.layout, base_w, base_h)
    }

    pub(super) fn compute_panes(&self) -> Vec<Pane> {
        let (base_w, base_h) = self.viewport_base_size_px();
        panes::compute_panes(
            self.view.display.layout,
            self.view.display.split_ratio,
            self.viewport_origin_px(),
            (base_w as f32, base_h as f32),
        )
    }

    /// `true` when the cursor is inside some pane's 3D **content** rect —
    /// not its toolbar strip, not an inter-pane gap. The camera-input
    /// gate uses this so toolbar clicks don't orbit the scene.
    pub(crate) fn pointer_in_pane_content(&self) -> bool {
        let cursor = self.input.cursor_pos;
        let toolbar_h = self.pane_toolbar_height_px();
        self.compute_panes()
            .iter()
            .any(|p| p.content(toolbar_h).contains(cursor))
    }

    pub(super) fn active_pane_index(&self) -> usize {
        if self.view.display.layout == ViewLayout::Single {
            return 0;
        }
        let panes = self.compute_panes();
        hit_test_pane(&panes, self.input.cursor_pos)
    }

    /// The visible split divider in egui's logical coordinate space.
    fn divider_inputs(&self) -> ((f32, f32), (f32, f32), f32) {
        let surface_size = (self.config.width, self.config.height);
        let ppp = self.window.scale_factor() as f32;
        let viewport = self
            .gui
            .viewport_rect_for_surface(surface_size)
            .unwrap_or_else(|| {
                egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(
                        self.config.width as f32 / ppp,
                        self.config.height as f32 / ppp,
                    ),
                )
            });
        (
            (viewport.min.x, viewport.min.y),
            (viewport.width(), viewport.height()),
            ppp,
        )
    }

    /// The gap strips between panes, for painting. Every layout's strips,
    /// not only the draggable one: the quad and three-left-big gaps have
    /// no ratio behind them and were the ones nothing painted.
    ///
    /// Computed in the same physical-pixel space the rendered panes use,
    /// then divided into egui's logical points, so the paint covers
    /// exactly the strips the pane math reserved.
    pub(super) fn compute_gap_rects(&self) -> Vec<egui::Rect> {
        let ppp = self.window.scale_factor() as f32;
        let (base_w, base_h) = self.viewport_base_size_px();
        panes::gap_rects(
            self.view.display.layout,
            self.view.display.split_ratio,
            self.viewport_origin_px(),
            (base_w as f32, base_h as f32),
        )
        .into_iter()
        .map(|r| {
            egui::Rect::from_min_size(
                egui::pos2(r.x / ppp, r.y / ppp),
                egui::vec2(r.width / ppp, r.height / ppp),
            )
        })
        .collect()
    }

    pub(super) fn compute_divider_hit_rect(&self) -> Option<egui::Rect> {
        let (origin, size, ppp) = self.divider_inputs();
        let r = panes::divider_hit_rect(
            self.view.display.layout,
            self.view.display.split_ratio,
            origin,
            size,
            2.0 / ppp,
            3.0 / ppp,
        )?;
        Some(egui::Rect::from_min_size(
            egui::pos2(r.x, r.y),
            egui::vec2(r.width, r.height),
        ))
    }
}
