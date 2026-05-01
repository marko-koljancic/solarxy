//! Central application state — [`State`] (the GUI's root struct), plus
//! `Pane`, `PendingLoad`, `InputState`, and per-pane geometry helpers.
//!
//! Submodules:
//! - `init.rs` — startup wiring (surface, device, queue, renderer).
//! - `update.rs` — per-frame updates; owns the IBL chokepoint
//!   `rebuild_light_bind_group` called on HDRI load, `IblMode` toggle, and
//!   background change.
//! - `render.rs` — `State::render`, per-pane orchestration.
//! - `panes.rs` — split-viewport layout math.
//! - `overlap.rs` — UV-overlap GPU readback polling.
//! - `capture.rs` — screenshot capture.
//! - `input/` — keyboard/mouse, dialogs, menu actions.
//! - `view_state.rs` — `ViewState` (re-exports `view_config` types).

mod capture;
mod init;
mod input;
mod overlap;
mod panes;
mod render;
mod update;
pub(crate) mod view_state;

pub(super) use view_state::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout, ViewState};

pub(super) use solarxy_renderer::frame::{
    GradientUniform, IblResources, PostProcessing, RenderTargets, Renderer, UvOverlapResources,
    ValidationColorResources, WireframeParams, WireframeResources,
};
pub(super) use solarxy_renderer::scene::{
    BackgroundModeExt, ModelScene, create_light_bind_group, create_light_bind_group_selective,
    lights_from_camera,
};

pub(super) use crate::gui::{EguiRenderer, ToastSeverity};
pub(super) use solarxy_core::preferences::{
    self, IblMode, InspectionMode, MaterialOverride, PaneMode, Preferences, UvMapBackground,
    ViewMode,
};
pub(super) use solarxy_renderer::ibl::{BrdfLut, IblState};
pub(super) use solarxy_renderer::light::LightsUniform;
pub(super) use solarxy_renderer::texture;

use std::sync::{Arc, mpsc};
use std::time::Instant;
use winit::{keyboard::ModifiersState, window::Window};

pub(super) struct Pane {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub(super) struct PendingLoad {
    pub(super) receiver: mpsc::Receiver<anyhow::Result<ModelScene>>,
    pub(super) filename: String,
    pub(super) path: String,
}

pub(super) struct InputState {
    pub(super) cursor_pos: (f32, f32),
    pub(super) modifiers: ModifiersState,
    pub(super) uv_last_mouse_pos: Option<(f32, f32)>,
    pub(super) uv_left_pressed: bool,
    pub(super) uv_middle_pressed: bool,
}

pub struct State {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) is_surface_configured: bool,
    pub(super) renderer: Renderer,
    pub(super) gui: EguiRenderer,
    pub(super) scene: Option<ModelScene>,
    pub(super) view: ViewState,
    pub(super) input: InputState,
    pub(super) pending_load: Option<PendingLoad>,
    pub(super) pending_hdri: Option<mpsc::Receiver<anyhow::Result<IblState>>>,
    pub(super) capture_requested: bool,
    pub(super) quit_requested: bool,
    pub(super) last_frame_time: Instant,
    pub(super) dt: f32,
    pub(super) _backend_info: String,
    pub(super) preferences: Preferences,
    pub window: Arc<Window>,
}

pub(super) fn compute_target_dimensions(layout: ViewLayout, width: u32, height: u32) -> (u32, u32) {
    match layout {
        ViewLayout::Single => (width, height),
        ViewLayout::SplitVertical => {
            let half = (width as f32 * 0.5).floor() as u32;
            (half.max(1), height)
        }
        ViewLayout::SplitHorizontal => {
            let half = (height as f32 * 0.5).floor() as u32;
            (width, half.max(1))
        }
    }
}

pub(super) fn hit_test_pane(panes: &[Pane], cursor: (f32, f32)) -> usize {
    let (cx, cy) = cursor;
    for (i, pane) in panes.iter().enumerate() {
        if cx >= pane.x && cx < pane.x + pane.width && cy >= pane.y && cy < pane.y + pane.height {
            return i;
        }
    }
    0
}

pub(super) fn cam_routing(active_pane: usize, cameras_linked: bool) -> (bool, bool) {
    (
        active_pane == 0 || cameras_linked,
        active_pane == 1 || cameras_linked,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(x: f32, y: f32, width: f32, height: f32) -> Pane {
        Pane {
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
        let panes: [Pane; 0] = [];
        assert_eq!(hit_test_pane(&panes, (500.0, 500.0)), 0);
    }

    #[test]
    fn cam_routing_single_pane() {
        assert_eq!(cam_routing(0, false), (true, false));
    }

    #[test]
    fn cam_routing_split_unlinked() {
        assert_eq!(cam_routing(0, false), (true, false));
        assert_eq!(cam_routing(1, false), (false, true));
    }

    #[test]
    fn cam_routing_split_linked() {
        assert_eq!(cam_routing(0, true), (true, true));
        assert_eq!(cam_routing(1, true), (true, true));
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
}
