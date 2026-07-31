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
//! - `raycast` — CPU picking (Möller-Trumbore + AABB early-reject), now
//!   `solarxy_core::raycast` (moved so web picking can run in Rust);
//!   re-exported here so call sites keep their paths.
//! - `review.rs` — `ReviewState`: in-memory mirror of one review-file
//!   plus transient UI state (draft, selection, panel visibility).
//! - `input/` — keyboard/mouse, dialogs, menu actions.
//! - `view_state.rs` — `ViewState` (re-exports `view_config` types).

mod capture;
#[cfg(debug_assertions)]
mod dev;
pub(crate) mod hdri_info;
mod init;
mod input;
mod overlap;
mod panes;
pub(crate) use solarxy_core::raycast;
mod render;
pub(crate) mod review;
mod update;
pub(crate) mod view_state;

pub(super) use view_state::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout, ViewState};

pub(super) use solarxy_renderer::frame::{
    GradientUniform, Renderer, UvOverlapResources, WireframeParams,
};
pub(super) use solarxy_renderer::scene::{
    BackgroundModeExt, ModelScene, create_light_bind_group, create_light_bind_group_selective,
    lights_from_camera,
};

pub(super) use crate::gui::{EguiRenderer, ToastSeverity, ViewportContextMenu};
pub(super) use solarxy_core::preferences::{
    self, IblMode, InspectionMode, MaterialOverride, PaneMode, Preferences, UvMapBackground,
    ViewMode,
};
pub(super) use solarxy_renderer::camera_state::CameraState;
pub(super) use solarxy_renderer::ibl::{BrdfLut, IblState};
pub(super) use solarxy_renderer::light::LightsUniform;
pub(super) use solarxy_renderer::texture;

use std::sync::{Arc, mpsc};
use std::time::Instant;
use winit::{keyboard::ModifiersState, window::Window};

// Pane geometry moved to `solarxy_renderer::panes`
// so both shells share the layout math; re-exported to keep call sites.
pub(super) use solarxy_renderer::panes::{PaneRect as Pane, compute_target_dimensions, hit_test_pane};

pub(super) struct PendingLoad {
    pub(super) receiver: mpsc::Receiver<anyhow::Result<ModelScene>>,
    pub(super) filename: String,
    pub(super) path: String,
}

/// In-flight async HDRI load. The source path is retained so the
/// completion handler in `update.rs` can build [`hdri_info::HdriInfo`]
/// (filename + file size) once the [`IblState`] arrives.
pub(super) struct PendingHdri {
    pub(super) receiver: mpsc::Receiver<anyhow::Result<IblState>>,
    pub(super) path: std::path::PathBuf,
}

/// In-flight screenshot readback: the staging buffer with a `map_async`
/// request armed, polled non-blocking each frame (`poll_pending_capture`).
/// The modal context (filename, review flags) is captured at arm time so
/// the image lands with the state the user triggered it under.
pub(super) struct PendingCapture {
    pub(super) buffer: wgpu::Buffer,
    pub(super) padded_row_bytes: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    pub(super) filename: String,
    pub(super) review_active: bool,
    pub(super) expand_review: bool,
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
    /// Multi-object dynamic scene drawn beside `scene`. Fed by
    /// [`SceneDelta`] batches queued in `pending_scene_deltas` and applied at
    /// the top of each frame; the node engine becomes the producer in a
    /// later milestone.
    pub(super) scene_objects: solarxy_renderer::scene_objects::SceneObjects,
    pub(super) pending_scene_deltas: Vec<solarxy_core::scene::SceneDelta>,
    /// Makes `SceneOp::SetEnvironment` idempotent. The engine re-emits the
    /// whole environment on every rebuild, and installing one convolves an
    /// irradiance cubemap, so this remembers what is already on the GPU.
    /// Invalidated whenever the sidebar or the HDRI dialog replaces the
    /// IBL behind the scene contract's back.
    pub(super) environment: solarxy_renderer::environment::EnvironmentTracker,
    /// Whether the `F10` developer harness has a synthetic environment
    /// installed. Debug builds only; see `state/dev.rs`.
    #[cfg(debug_assertions)]
    pub(super) dev_environment_on: bool,
    pub(super) view: ViewState,
    pub(super) input: InputState,
    pub(super) review: review::ReviewState,
    pub(super) last_project_config_toast: Option<std::path::PathBuf>,
    pub(super) pending_load: Option<PendingLoad>,
    pub(super) pending_hdri: Option<PendingHdri>,
    pub(super) pending_capture: Option<PendingCapture>,
    /// Pending viewport right-click context menu — `Some` while the menu
    /// is open; cleared on dismiss.
    pub(super) viewport_context_menu: Option<ViewportContextMenu>,
    pub(super) capture_requested: bool,
    /// Whether the pending capture should force every review annotation
    /// card open. Set false by `C`/menu, set from the screenshot modal's
    /// checkbox on a re-capture.
    pub(super) screenshot_expand_review: bool,
    pub(super) quit_requested: bool,
    pub(super) last_frame_time: Instant,
    pub(super) dt: f32,
    pub(super) _backend_info: String,
    pub(super) preferences: Preferences,
    pub window: Arc<Window>,
}
