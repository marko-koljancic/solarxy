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
        match self.view.display.layout {
            ViewLayout::Single => vec![Pane {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
            }],
            ViewLayout::SplitVertical => {
                let half = (w * 0.5).floor();
                vec![
                    Pane {
                        x: 0.0,
                        y: 0.0,
                        width: half - 1.0,
                        height: h,
                    },
                    Pane {
                        x: half + 1.0,
                        y: 0.0,
                        width: w - half - 1.0,
                        height: h,
                    },
                ]
            }
            ViewLayout::SplitHorizontal => {
                let half = (h * 0.5).floor();
                vec![
                    Pane {
                        x: 0.0,
                        y: 0.0,
                        width: w,
                        height: half - 1.0,
                    },
                    Pane {
                        x: 0.0,
                        y: half + 1.0,
                        width: w,
                        height: h - half - 1.0,
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
        match self.view.display.layout {
            ViewLayout::Single => None,
            ViewLayout::SplitVertical => {
                let cx = (w * 0.5).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2((cx - 1.0) / ppp, 0.0),
                    egui::vec2(2.0 / ppp, h / ppp),
                ))
            }
            ViewLayout::SplitHorizontal => {
                let cy = (h * 0.5).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, (cy - 1.0) / ppp),
                    egui::vec2(w / ppp, 2.0 / ppp),
                ))
            }
        }
    }
}
