//! Split-viewport layout math: pane rectangles for `F1`/`F2`/`F3`, the
//! divider hit-rect for the (currently fixed-50/50) split, and the
//! cursor→pane hit test.

use super::view_state::ViewLayout;
use super::{Pane, State, compute_target_dimensions, hit_test_pane};

impl State {
    pub(super) fn target_dimensions(&self) -> (u32, u32) {
        compute_target_dimensions(
            self.view.display.layout,
            self.config.width,
            self.config.height,
        )
    }

    pub(super) fn compute_panes(&self) -> Vec<Pane> {
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        let ratio = self.view.display.split_ratio;
        match self.view.display.layout {
            ViewLayout::Single => vec![Pane {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
            }],
            ViewLayout::SplitVertical => {
                let split = (w * ratio).floor();
                vec![
                    Pane {
                        x: 0.0,
                        y: 0.0,
                        width: (split - 1.0).max(1.0),
                        height: h,
                    },
                    Pane {
                        x: split + 1.0,
                        y: 0.0,
                        width: (w - split - 1.0).max(1.0),
                        height: h,
                    },
                ]
            }
            ViewLayout::SplitHorizontal => {
                let split = (h * ratio).floor();
                vec![
                    Pane {
                        x: 0.0,
                        y: 0.0,
                        width: w,
                        height: (split - 1.0).max(1.0),
                    },
                    Pane {
                        x: 0.0,
                        y: split + 1.0,
                        width: w,
                        height: (h - split - 1.0).max(1.0),
                    },
                ]
            }
        }
    }

    pub(super) fn active_pane_index(&self) -> usize {
        if self.view.display.layout == ViewLayout::Single {
            return 0;
        }
        let panes = self.compute_panes();
        hit_test_pane(&panes, self.input.cursor_pos)
    }

    pub(super) fn compute_divider_rect(&self) -> Option<egui::Rect> {
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        let ppp = self.window.scale_factor() as f32;
        let ratio = self.view.display.split_ratio;
        match self.view.display.layout {
            ViewLayout::Single => None,
            ViewLayout::SplitVertical => {
                let cx = (w * ratio).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2((cx - 1.0) / ppp, 0.0),
                    egui::vec2(2.0 / ppp, h / ppp),
                ))
            }
            ViewLayout::SplitHorizontal => {
                let cy = (h * ratio).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, (cy - 1.0) / ppp),
                    egui::vec2(w / ppp, 2.0 / ppp),
                ))
            }
        }
    }

    /// Wider hit-test rectangle around the divider (egui drag affordance).
    /// The visual divider is 2 px wide; this returns an 8 px-wide band
    /// centered on it so the user doesn't have to land pixel-perfect.
    pub(super) fn compute_divider_hit_rect(&self) -> Option<egui::Rect> {
        let visible = self.compute_divider_rect()?;
        let ppp = self.window.scale_factor() as f32;
        let pad_logical = 3.0 / ppp; // 3 px on each side around the visible 2 px
        Some(visible.expand2(match self.view.display.layout {
            ViewLayout::SplitVertical => egui::vec2(pad_logical, 0.0),
            ViewLayout::SplitHorizontal => egui::vec2(0.0, pad_logical),
            ViewLayout::Single => egui::vec2(0.0, 0.0),
        }))
    }
}
