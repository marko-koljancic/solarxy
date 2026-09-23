//! Central application state: [`State`], the GUI's root struct, plus
//! `PendingOpen`, `InputState`, and the pane geometry re-exported from the
//! renderer (`Pane` is `PaneRect` under its old name).
//!
//! Submodules:
//! - `init.rs`, startup wiring (surface, device, queue, renderer).
//! - `update.rs`, per-frame updates; owns the IBL chokepoint
//!   `rebuild_light_bind_group`, called on HDRI load, `IblMode` toggle, and
//!   background change.
//! - `render.rs`, `State::render`, the scene-delta drain, and the per-pane
//!   orchestration.
//! - `intents.rs`, the drain: one ordered application of everything an
//!   interface pass raised.
//! - `open.rs`, how a file becomes the open document. The only place `engine`
//!   is assigned.
//! - `camera.rs`, where the pane cameras point. `visibility.rs`, what is
//!   shown and hidden. `persist.rs`, preference write-back and the flushes on
//!   the way out.
//! - `cook.rs`, cook mode and the explicit cook. `actions.rs`, an action
//!   parameter's press. `drop.rs`, what a drop onto the window does.
//! - `dev.rs`, the debug-build developer harness on F8 and F9.
//! - `panes.rs`, split-viewport layout math. `overlap.rs`, the UV-overlap
//!   readback poll. `capture.rs`, the shell's half of a screenshot.
//! - `still/`, the tiled still render job. `review/`, the annotation state,
//!   the anchoring, and the sidecar. `input/`, keyboard, pointer, and the
//!   native pickers.
//! - `engine_scene.rs`, what the shell knows about the open document as a
//!   file. `cook_health.rs`, which nodes are failing. `hdri_info.rs`, what
//!   the Environment modal says about the loaded HDRI.
//! - `view_state.rs`, `ViewState` (re-exports the `view_config` types).
//! - `raycast`, the ray builder the viewport picks with and the hit type the
//!   review module still anchors on, now `solarxy_core::raycast` so web
//!   picking can run in Rust; re-exported here so call sites keep their
//!   paths. Picking itself asks the engine since 0.10.0, which answers with
//!   the node that produced what is under the cursor.

mod actions;
pub(crate) mod attr;
mod autosave;
mod camera;
mod capture;
mod clipboard;
mod cook;
pub(crate) mod cook_health;
#[cfg(debug_assertions)]
mod dev;
mod discard;
mod document;
mod drop;
pub(crate) mod engine_scene;
mod gizmo_drag;
pub(crate) mod hdri_info;
mod history;
mod init;
mod input;
pub(crate) use input::keymap;
pub(crate) use input::{key_scope, window_claims};
mod intents;
mod open;
mod overlap;
mod panes;
mod persist;
pub(crate) mod preview;
pub(crate) use solarxy_core::raycast;
mod render;
pub(crate) mod review;
pub(crate) mod samples;
mod still;
mod traced;
mod turntable;
mod update;
pub(crate) mod view_state;
mod visibility;

pub(super) use view_state::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout, ViewState};

pub(super) use solarxy_renderer::frame::Renderer;
pub(super) use solarxy_renderer::scene::BackgroundModeExt;

pub(super) use crate::gui::{EguiRenderer, ToastSeverity, ViewportContextMenu};
pub(super) use solarxy_core::preferences::{
    self, IblMode, MaterialOverride, PaneMode, Preferences, UvMapBackground, ViewMode,
};
pub(super) use solarxy_renderer::ibl::IblState;

use std::sync::{Arc, mpsc};
use std::time::Instant;
use winit::{keyboard::ModifiersState, window::Window};

// Pane geometry moved to `solarxy_renderer::panes`
// so both shells share the layout math; re-exported to keep call sites.
pub(super) use solarxy_renderer::panes::{PaneRect as Pane, compute_target_dimensions, hit_test_pane};

/// A model file being built into a document on a worker thread.
///
/// The whole expensive half is on the far side of the channel: natively an
/// import parses inside the cook rather than in a job, so cooking a
/// synthesized document to quiescence blocks for the entire parse. `cancel`
/// reaches that cook, so a second open, or a quit, stops the first between
/// nodes rather than waiting it out.
pub(super) struct PendingOpen {
    pub(super) receiver: mpsc::Receiver<Result<open::OpenedModel, String>>,
    pub(super) filename: String,
    pub(super) path: String,
    pub(super) cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// In-flight async HDRI load. The source path is retained so the
/// completion handler in `update.rs` can build [`hdri_info::HdriInfo`]
/// (filename + file size) once the [`IblState`] arrives.
pub(super) struct PendingHdri {
    pub(super) receiver: mpsc::Receiver<anyhow::Result<IblState>>,
    pub(super) path: std::path::PathBuf,
}

/// In-flight screenshot readback: the shared readback, plus the modal
/// context captured at arm time so the image lands with the state the user
/// triggered it under.
///
/// The readback itself is `solarxy_renderer::capture`, which is where it
/// always belonged: the padded-row arithmetic and the non-blocking poll are
/// the same on both shells, and this shell carried its own copy only because
/// it predated the shared module. What stays here is the part that is
/// genuinely the desktop's, which is the three fields below.
pub(super) struct PendingCapture {
    pub(super) readback: solarxy_renderer::capture::PendingCapture,
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
    /// Whether the primary button's press is going to release as a click,
    /// and whether that click pairs with the one before it.
    pub(super) clicks: input::click::ClickTracker,
}

pub struct State {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) is_surface_configured: bool,
    pub(super) renderer: Renderer,
    pub(super) gui: EguiRenderer,
    /// The open document, whatever file it came from.
    ///
    /// **There is one root.** A scene file already is a document and a model
    /// file becomes one, so nothing downstream branches on which was opened.
    /// The shell held a second root until 0.10.0, a directly loaded model with
    /// its own GPU buffers, and half the product was reachable at a time
    /// because the two carried different capabilities.
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
    /// The armed transform tool, its snap settings, the hovered handle and
    /// the drag in flight, over the shared solver. The drive loop that
    /// turns a drag into engine commands is `gizmo_drag.rs`, this shell's
    /// twin of the browser host's.
    pub(super) gizmo: solarxy_host::gizmo::GizmoState,
    /// Where the drag in flight writes: the context and node the target
    /// resolved to when the drag began. Taken together with the drag, so
    /// a solved value never lacks a place to go.
    pub(super) gizmo_addr: Option<gizmo_drag::GizmoAddr>,
    /// The live delta of the drag in flight, as the readout shows it.
    pub(super) gizmo_readout: Option<String>,
    /// Which tools the selection can take, for the column and the context
    /// menu to draw the rest unavailable; `None` with nothing selected,
    /// which narrows nothing.
    pub(super) tools_available: Option<Vec<solarxy_host::gizmo::ToolMode>>,
    /// The attribute strip's state: the three toggles, the picked lane and
    /// the settings behind the gear. Session-only and scene-wide, never in
    /// the file and never in undo, as the browser holds it.
    pub(super) attr_viz: solarxy_host::attr_viz::AttrVizState,
    /// Whether the label set and the arrow lines are stale against the
    /// scene or the strip. Set by a scene delta, an overlay rebuild and a
    /// strip change; cleared by the one rebuild that consumes it.
    pub(super) attr_dirty: bool,
    /// The sampling facts the strip's notice reports: the pin capacity and
    /// the total displayed points.
    pub(super) attr_pin_stats: (u32, usize),
    /// Which nodes' cooks are failing, absorbed from the engine's event
    /// stream each frame. Fresh failures toast; the standing map is what
    /// the still render consults before reporting success.
    pub(super) cook_health: cook_health::CookHealth,
    /// What the header strip says about the cook, refreshed each frame from
    /// the engine, so a scene opened in manual mode shows manual at once.
    pub(super) cook_readout: crate::gui::CookReadout,
    /// The still render in flight, if any. While it runs it owns the
    /// shared render targets, so panes are not rendered.
    pub(super) still: Option<still::StillState>,
    /// The turntable export in flight, if any. It owns the frame the same
    /// way a still does, and for the same reason: it drives the same tiled
    /// job, once per frame of the turn.
    pub(super) turntable: Option<turntable::TurntableState>,
    /// The render node whose own action opened the still dialog, so the
    /// still renders that node rather than the document's single one. Set by
    /// the action, cleared when the menu opens the dialog and when the
    /// document changes.
    pub(super) still_target: Option<(
        solarxy_graph::document::GraphContext,
        solarxy_graph::document::NodeId,
    )>,
    /// The finished floating-point picture, waiting for a save path.
    ///
    /// Beside the modal's eight-bit copy rather than inside it: the modal shows
    /// a screen image, and this one is only ever written to a file.
    pub(super) finished_float: Option<solarxy_host::still::FloatImage>,
    /// The finished still's auxiliary planes, when the render node asked for
    /// any: what the Showing combo replays and what Save All writes beside
    /// the picture. Kept with the float image, for the same reason.
    pub(super) finished_passes: Option<solarxy_host::still::StillPasses>,
    /// The traced backend, built the first time a pane or a still asks for
    /// it and kept for the session, shared by both. A pane flipped to it
    /// snapshots the scene into it, and so does every still start: the
    /// per-frame delta feed reaches it only while a pane is traced, so a
    /// scene edited with every pane raster has moved on without it.
    pub(super) tracer: Option<solarxy_renderer::pathtrace::backend::PathBackend>,
    /// Whether the tracer's environment lags the scene's. Set when an
    /// HDRI is installed or cleared, or when the tracer is first built.
    pub(super) traced_env_dirty: bool,
    /// Whether this device can build the tracer at all, asked of its limits
    /// once at startup. The pane menu offers `Path Traced` only when it can:
    /// the one entry drawn absent rather than disabled, as the browser has
    /// it, because a device that cannot trace has nothing to sequence.
    pub(super) tracing_available: bool,
    /// The camera each traced pane last accumulated under, as the shared
    /// key; a mismatch at encode drops that pane's mean. `None` after any
    /// reset, so the next encode re-anchors the pose.
    pub(super) traced_cam_keys: [Option<[f32; 13]>; 4],
    /// The environment scalars (intensity, rotation) the tracer last had;
    /// a move re-syncs it and drops every accumulation.
    pub(super) traced_env_params: (f32, f32),
    /// Each traced pane's last sample count and target, for the readout
    /// beside its labels. `None` before the first count and after a reset;
    /// a converged pane parks at its target rather than vanishing.
    pub(super) last_pane_samples: [Option<(u32, u32)>; 4],
    /// Whether the window is hidden from view. While it is, no frame asks
    /// for the next, so nothing draws, cooks or traces for nobody; the next
    /// un-occlusion or focus asks for one and the loop resumes.
    pub(super) occluded: bool,
    /// The scene camera each pane looks through, or `None` for a free view.
    ///
    /// A bound pane follows the camera node's pose each frame and composites
    /// with the camera's look. Navigating it does one of two things, decided
    /// by the lock on the shared view state: a locked pane writes its pose
    /// back to the node as one undo step, the browser's rule; an unlocked
    /// pane is released to a free view, which the browser does not do (its
    /// follow snaps an unlocked pane straight back) and which is recorded as
    /// a divergence rather than copied.
    ///
    /// Saved with the scene beside its lock, and restored from a scene file
    /// on open, because a binding is part of how the scene was authored
    /// rather than of how this window is arranged.
    pub(super) look_through: [Option<solarxy_core::scene::SceneObjectId>; 4],
    /// A locked pane is mid-navigation, so the node-to-pane follow is held
    /// off until the gesture commits; it would otherwise fight the live
    /// orbit. A fact about this shell's pointer, so it stays here rather
    /// than on the shared view state.
    pub(super) camera_editing: [bool; 4],
    /// Panes whose restored binding has not yet been checked against the
    /// cooked scene.
    ///
    /// At the moment a scene opens nothing has cooked, so the camera a
    /// binding names does not exist yet and cannot be looked up. Each frame,
    /// a flagged pane is resolved once the answer is knowable: the camera
    /// appeared, so the binding stands and the follow poses the pane; or the
    /// cook settled without it, so the binding is dropped rather than left
    /// naming a camera the document no longer has. A binding made live from
    /// the pane toolbar is never flagged, since it was picked from cameras
    /// that exist.
    pub(super) unresolved_binding: [bool; 4],
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
    /// Whether the per-mesh overlay buffers in `env.vis` describe the scene as
    /// it now stands. Set by anything that changes what is drawn; consumed by
    /// the rebuild, which only runs when a pane actually shows an overlay.
    pub(super) viz_dirty: bool,
    pub(super) pending_scene_deltas: Vec<solarxy_core::scene::SceneDelta>,
    /// Makes `SceneOp::SetEnvironment` idempotent. The engine re-emits the
    /// whole environment on every rebuild, and installing one convolves an
    /// irradiance cubemap, so this remembers what is already on the GPU.
    /// Invalidated whenever the sidebar or the HDRI dialog replaces the
    /// IBL behind the scene contract's back.
    pub(super) environment: solarxy_renderer::environment::EnvironmentTracker,
    /// Whether the `F8` developer harness has a synthetic environment
    /// installed. Debug builds only; see `state/dev.rs`.
    #[cfg(debug_assertions)]
    pub(super) dev_environment_on: bool,
    pub(super) view: ViewState,
    pub(super) input: InputState,
    pub(super) review: review::ReviewState,
    pub(super) pending_open: Option<PendingOpen>,
    /// Paths dropped onto the window since the last frame. The window
    /// delivers one event per item with nothing to say the gesture is
    /// complete, so they are collected here and handled once per frame,
    /// which is what makes a folder of models one gesture.
    pub(super) pending_drop: Vec<std::path::PathBuf>,
    /// Panes waiting to be framed on the document once it has cooked far
    /// enough to have bounds.
    ///
    /// A model file arrives already cooked, so its panes are framed at
    /// adoption. A scene file cooks over the frame loop, so at the moment it
    /// opens there is nothing to frame on and the panes would seed on the
    /// placeholder box and stay there, because the seeding is idempotent and
    /// nothing re-frames afterwards. A pane the file's own saved view supplied
    /// a camera for is never marked: an authored camera outranks framing. A
    /// marked pane that is bound to a scene camera is never framed either:
    /// the follow poses it, and framing it first would be a visible jump
    /// before the binding took over.
    pub(super) pending_frame: [bool; 4],
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
    /// The discarding action waiting on the unsaved-changes prompt.
    pub(super) pending_discard: Option<discard::DiscardAction>,
    /// The autosave ring and its timing.
    pub(super) autosave: autosave::AutosaveState,
    /// One graph context per undo step, so an undo shows where the change
    /// was made.
    pub(super) history: history::UndoContexts,
    /// The copied fragment, in memory, as the browser keeps it.
    pub(super) clipboard: Option<solarxy_graph::document::GraphFragment>,
    /// The model preview's scene, texture and parse, when one is up.
    pub(super) preview: preview::PreviewState,
    /// The engine revision the open document was last written at, or
    /// opened at. Dirty is `engine.revision() != saved_revision`, which is
    /// what keeps the answer the engine's rather than a flag's.
    pub(super) saved_revision: u64,
    /// The content hash of the HDRI staged in the engine for the save, once
    /// one is. `None` until a save stages the loaded file, or a scene opens
    /// carrying one; cleared when the HDRI is replaced or cleared.
    pub(super) hdri_hash: Option<String>,
    /// The window title as last written, so the per-frame refresh writes
    /// the window only when the text moved.
    pub(super) last_title: String,
    pub(super) last_frame_time: Instant,
    pub(super) dt: f32,
    /// The uncaptured-error queue the shared device hook fills; drained
    /// once per frame in `update` into the log and a toast.
    pub(super) gpu_faults: solarxy_renderer::faults::GpuFaults,
    pub(super) _backend_info: String,
    pub(super) preferences: Preferences,
    pub window: Arc<Window>,
}
