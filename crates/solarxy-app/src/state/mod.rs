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
pub(crate) mod cook_health;
#[cfg(debug_assertions)]
mod dev;
pub(crate) mod engine_scene;
pub(crate) mod hdri_info;
mod init;
mod input;
mod overlap;
mod panes;
pub(crate) use solarxy_core::raycast;
mod render;
pub(crate) mod review;
mod still;
mod update;
pub(crate) mod view_state;

pub(super) use view_state::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout, ViewState};

pub(super) use solarxy_renderer::composite::CompositeLook;
pub(super) use solarxy_renderer::frame::Renderer;
pub(super) use solarxy_renderer::scene::{
    BackgroundModeExt, LoadedModel, ModelScene, create_light_bind_group,
};

pub(super) use crate::gui::{EguiRenderer, ToastSeverity, ViewportContextMenu};
pub(super) use solarxy_core::preferences::{
    self, IblMode, InspectionMode, MaterialOverride, PaneMode, Preferences, UvMapBackground,
    ViewMode,
};
pub(super) use solarxy_renderer::ibl::{BrdfLut, IblState};

use std::sync::{Arc, mpsc};
use std::time::Instant;
use winit::{keyboard::ModifiersState, window::Window};

// Pane geometry moved to `solarxy_renderer::panes`
// so both shells share the layout math; re-exported to keep call sites.
pub(super) use solarxy_renderer::panes::{PaneRect as Pane, compute_target_dimensions, hit_test_pane};

pub(super) struct PendingLoad {
    pub(super) receiver: mpsc::Receiver<anyhow::Result<LoadedModel>>,
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
    /// A camera button is held in a 3D pane. What tells a pointer move that
    /// it is a navigation drag, which is the gesture that releases a
    /// look-through binding; a plain click never does.
    pub(super) nav_button_down: bool,
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
    /// The node engine, when a scene file is open.
    ///
    /// A file model and an engine scene are mutually exclusive: opening
    /// either closes the other, so `scene` and this are never both `Some`.
    /// They could coexist, since the draw list already chains both sources
    /// and framing already unions them, but one root at a time is what keeps
    /// Close unambiguous and the inspection panels presenting one tree.
    pub(super) engine: Option<Box<solarxy_graph::Engine>>,
    /// What the inspection panels read about the open scene: its file
    /// identity, per-object names, summed geometry counters, and every
    /// object's validation merged into one report.
    ///
    /// `Some` exactly when `engine` is. Rebuilt on each drained scene
    /// delta rather than per frame, because deltas are the only thing that
    /// changes it and the merged issue order has to be stable.
    pub(super) engine_scene: Option<engine_scene::EngineSceneInfo>,
    /// The scene object the viewport outlines, set by a Node Tree
    /// selection.
    ///
    /// Only a **root-context** selection lands here, because only a root
    /// geo node owns a scene object (the delta names it
    /// `SceneObjectId(geo.0)`). Selecting a node inside a container
    /// selects engine-side and leaves the viewport alone, which is what
    /// the web shell does with the same gesture.
    pub(super) selected_object: Option<solarxy_core::scene::SceneObjectId>,
    /// Which nodes' cooks are failing, absorbed from the engine's event
    /// stream each frame. Fresh failures toast; the standing map is what
    /// the still render consults before reporting success.
    pub(super) cook_health: cook_health::CookHealth,
    /// The still render in flight, if any. While it runs it owns the
    /// shared render targets, so panes are not rendered.
    pub(super) still: Option<still::StillState>,
    /// The finished floating-point picture, waiting for a save path.
    ///
    /// Beside the modal's eight-bit copy rather than inside it: the modal shows
    /// a screen image, and this one is only ever written to a file.
    pub(super) finished_float: Option<solarxy_host::still::FloatImage>,
    /// The traced backend, built on the first traced still and kept for
    /// the session. It sees no per-frame deltas (those feed the raster
    /// backend alone), so every still start snapshots the scene into it.
    pub(super) tracer: Option<solarxy_renderer::pathtrace::backend::PathBackend>,
    /// Whether the tracer's environment lags the scene's. Set when an
    /// HDRI is installed or cleared, or when the tracer is first built.
    pub(super) traced_env_dirty: bool,
    /// The scene camera each pane looks through, or `None` for a free view.
    ///
    /// Viewer-scoped look-through: a bound pane follows the camera node's
    /// pose each frame and composites with the camera's look, and any local
    /// navigation releases the pane back to a free view rather than writing
    /// the pose back to the node the way the web's locked mode does. That
    /// write-back is authoring machinery, and it waits for the desktop node
    /// canvas. Session state, deliberately not persisted, matching the web.
    pub(super) look_through: [Option<solarxy_core::scene::SceneObjectId>; 4],
    /// The rasterizer, behind the render backend contract, owning the
    /// multi-object dynamic scene drawn beside `scene`.
    ///
    /// That scene is fed by [`SceneDelta`] batches queued in
    /// `pending_scene_deltas` and applied at the top of each frame; the engine
    /// above is the producer once a scene file is open, and the developer
    /// harness is the only other one. Everything this shell asks of the
    /// document that is not rendering reads through `raster.scene()`, because
    /// that is where the answer lives.
    pub(super) raster: solarxy_host::RasterBackend,
    /// Scene-level GPU state every pane draws through: the light rig, the
    /// shadow map, the identity instance buffer bound for scene-level draws,
    /// and the grid/floor/axes buffers. Owned here rather than by `scene`, so
    /// the viewport keeps its full pass chain with no file model loaded.
    pub(super) env: solarxy_renderer::environment::SceneEnvironment,
    /// The bounds `env` was last built around (grid, floor and shadow fit).
    pub(super) env_bounds: solarxy_core::AABB,
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
    /// The uncaptured-error queue the shared device hook fills; drained
    /// once per frame in `update` into the console log and a toast.
    pub(super) gpu_faults: solarxy_renderer::faults::GpuFaults,
    pub(super) _backend_info: String,
    pub(super) preferences: Preferences,
    pub window: Arc<Window>,
}
