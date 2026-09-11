//! The `SolarxyApp` wasm-bindgen class: the browser host over the engine
//! and the full `solarxy-renderer` pipeline (the stopgap forward renderer
//! it started on is retired).
//!
//! The React frontend holds one instance: it dispatches `Command`s (in) and
//! receives `EventBatch`es (out), calls `frame` each rAF tick to cook under
//! a budget and render every pane, routes pointer gestures to the per-pane
//! cameras, and mirrors the host-owned view state (`ViewStateDto` returns +
//! `take_host_events`). Cooked geometry never crosses into JavaScript.
//!
//! Architecture: this file is the web port of the desktop shell's
//! `state/render.rs` + `state/update.rs` orchestration, minus egui. The
//! scene environment (lights, shadow, grid/floor) is a
//! [`SceneEnvironment`] rebuilt when the scene bounds move; geometry
//! arrives through `SceneObjects` deltas; pane geometry comes from the
//! shared `solarxy_renderer::panes` math, so the F1-F5 layouts are the
//! same rectangles the desktop produces.

use std::collections::BTreeMap;

use cgmath::{InnerSpace, Point3, Vector3};
use serde::{Deserialize, Serialize};
use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, PaneMode, ProjectionMode, ResolvedBackground, ToneMode,
};
use solarxy_core::raycast::{Ray, screen_to_world_ray};
use solarxy_core::scene::{SceneDelta, SceneObjectId, SceneOp};
use solarxy_core::validation::{
    ValidationConfig, ValidationResult, ValidationThresholds, validate_raw_model_with_config,
};
use solarxy_core::view_config::{
    DisplaySettings, PaneDisplaySettings, PaneEngine, PaneLook, ViewLayout,
};
use solarxy_core::AABB;
use solarxy_graph::assets::AssetTable;
use solarxy_graph::cook::{ImportOptions, JobId, JobRequest, JobResult, ParsedModel};
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::{EngineEvent, GizmoTarget, SceneSidecar};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::{Command, Engine, EventBatch};
use solarxy_kernel::transfer;
use solarxy_renderer::manipulator::{self, ManipulatorState};

use solarxy_host::{HostViewState, RasterBackend};
use solarxy_host::still::StillPasses;
use solarxy_host::attr_viz::{AttrColorMode, AttrVizState, ramp_color};
use solarxy_host::display_defaults::{self, DisplayDefaults};
use solarxy_core::gizmo::TransformParams;
use solarxy_host::gizmo::{self, GizmoPose, GizmoState, ToolMode};
use solarxy_renderer::camera::{turntable_up, Camera};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::backend::{FrameCtx, FrameOutcome, PaneContent, RenderBackend, UvSource};
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};
use solarxy_renderer::pathtrace::denoise::DenoiseSettings;
use solarxy_renderer::capture::CaptureTarget;
use solarxy_renderer::frame::{Renderer, RendererInit};
use solarxy_renderer::model::GizmoVertex;
use solarxy_renderer::input::PointerButton;
use solarxy_renderer::light::LightsUniform;
use solarxy_renderer::panes::{self, PaneRect};
use solarxy_renderer::visualization::grid_plane_for;
use solarxy_renderer::scene::{create_light_bind_group, lights_from_camera, BackgroundModeExt};
use solarxy_renderer::scene_objects::SceneObjects;
use solarxy_renderer::visualization::VisualizationState;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The synchronous cook budget per frame, in milliseconds (about half a
/// 60fps frame, leaving headroom for render + the browser).
const COOK_BUDGET_MS: f64 = 6.0;
const MSAA_SAMPLES: u32 = 4;
const SHADOW_MAP_SIZE: u32 = 2048;

/// The current host time in milliseconds (`performance.now`).
///
/// Monotonic and high-resolution, counting from page load. Right for
/// measuring how long a cook took; useless as a date, which is why node
/// timestamps use `web_epoch_ms` instead.
fn web_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

/// The current wall time in Unix milliseconds (`Date.now`).
///
/// The counterpart to `web_now`: coarse and not monotonic (it moves when
/// the system clock is set), but it is an actual date, which is what a
/// node's created / modified stamp has to be.
fn web_epoch_ms() -> f64 {
    js_sys::Date::now()
}

/// The console policy, in one place: a clean boot of a shipped build is
/// SILENT, so the first message a user ever sees in the console means
/// something. Informational output therefore lives behind the off-by-default
/// `diagnostics` feature, gated at the call site as well, so a shipped
/// artifact carries neither the call nor its strings. Failures are the
/// opposite case: they go through [`error`], which is always present and
/// writes to `console.error`, where the crash reporter's interception sees
/// them. A panic stays legible through `console_error_panic_hook`, which is
/// untouched by any of this.
#[cfg(feature = "diagnostics")]
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// A failure, reported. Always compiled in: the `diagnostics` gate covers
/// chatter, never problems, so nothing can accidentally silence one. Writes
/// through `console.error` so the crash reporter's last-error capture
/// (`web/src/telemetry.ts`) can attach it to a report.
fn error(msg: &str) {
    web_sys::console::error_1(&JsValue::from_str(msg));
}

/// Placeholder scene bounds before anything cooks (frames the grid).
fn default_bounds() -> AABB {
    solarxy_renderer::environment::placeholder_bounds()
}

/// The pane display defaults every pane starts from (desktop parity:
/// shaded, gradient background, grid on).
fn default_pane_settings() -> PaneDisplaySettings {
    use solarxy_core::preferences::{
        LineWeight, MaterialOverride, NormalsMode, UvMapBackground, UvMode, ViewMode,
    };
    use solarxy_core::view_config::BoundsMode;
    PaneDisplaySettings {
        view_mode: ViewMode::Shaded,
        prev_non_ghosted_mode: ViewMode::Shaded,
        ghosted_wireframe: false,
        normals_mode: NormalsMode::Off,
        background_mode: BackgroundMode::GRADIENT,
        uv_mode: UvMode::Off,
        bounds_mode: BoundsMode::Off,
        // The enum's own default (Light), matching the desktop; the user's
        // persisted preference overwrites this at boot via
        // `set_display_defaults`.
        line_weight: LineWeight::default(),
        show_grid: true,
        show_axis_gizmo: false,
        show_local_axes: false,
        inspection_mode: InspectionMode::Shaded,
        material_override: MaterialOverride::None,
        texel_density_target: 1.0,
        pane_mode: PaneMode::Scene3D,
        uv_bg: UvMapBackground::Dark,
        uv_offset: [0.0, 0.0],
        uv_zoom: 1.0,
        show_uv_overlap: false,
        show_validation: false,
        // On: a light with no marker is invisible, and this is the shell that
        // can grab one.
        show_light_markers: true,
        turntable_active: false,
        pane_engine: PaneEngine::Raster,
    }
}

fn default_display_settings() -> DisplaySettings {
    DisplaySettings {
        turntable_active: false,
        turntable_rpm: 6.0,
        lights_locked: false,
        layout: ViewLayout::Single,
        split_ratio: DisplaySettings::DEFAULT_SPLIT_RATIO,
        roughness_scale: 1.0,
        metallic_scale: 1.0,
        hdri_rotation: 0.0,
        hdri_intensity: solarxy_core::view_config::DEFAULT_HDRI_INTENSITY,
        point_size: solarxy_core::view_config::DEFAULT_POINT_SIZE,
    }
}

/// The key a pane's look rides under inside `PaneJson::display`.
///
/// That field is declared opaque and round-tripped uninterpreted by the
/// scene file, which makes it the right place for pane state the schema
/// does not name: persisting here costs no `schema_version` bump and no
/// `min_reader` gate, and a scene written before this existed simply has
/// no such key.
const PANE_LOOK_KEY: &str = "look";

// This shell's view state is `solarxy_host::HostViewState` plus four fields
// held directly on `SolarxyApp`: `pane_looks`, `look_through`, `camera_locked`
// and `camera_editing`. Those four describe panes looking through `camera`
// nodes, and the desktop shell has no camera nodes until it gains an engine,
// so they stay here rather than sitting on the shared type as fields one
// consumer sets and the other never reads. Same judgement the shared crate
// makes about a renderer trait: one consumer is not yet something to share.

mod assets;
mod capture;
mod gizmo_drag;
mod lifecycle;
mod pointer;
mod presentation;
mod preview;
mod queries;
mod render;
mod scenefile;
mod still;
mod view_state;

/// Async happenings the frontend drains once per frame.
///
/// `rename_all_fields` is load-bearing and was missing. `rename_all` on an
/// enum renames the VARIANTS and nothing else, so a multi-word field went out
/// in snake case while `web/src/engine/types.ts` declared it camel: the
/// frontend read `undefined` and said so in the interface. Every single-word
/// field hid it, and `renderProgress` was the first variant to carry one that
/// was not, which is why the still dialog's elapsed and remaining readouts
/// were blank. The engine's `Command` enum has carried both attributes from
/// the start; this one now matches it.
#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum HostEvent {
    /// Pane rectangles changed (layout, split, or resize), in CSS pixels.
    PaneRects { rects: Vec<RectDto> },
    /// The hovered (active) pane changed via pointer routing.
    ActivePane { pane: usize },
    /// The UV overlap readback advanced: a fresh percentage, or a pending
    /// run (`pct` holds the stale value or `None` while computing).
    UvOverlap { pct: Option<f32>, pending: bool },
    /// Host-side pointer input mutated view state (UV pan/zoom); the
    /// frontend refreshes its view-state mirror.
    ViewChanged,
    /// The attribute-label sampling facts changed (cook, lane, toggle, cap
    /// edit): `capacity` labels drawn of `total` displayed points; the
    /// strip's sampling notice reads `capacity < total`. Total rides f64
    /// for the 53-bit JS number boundary.
    AttrPinStats { capacity: u32, total: f64 },
    /// A still render advanced: which tile of how many, and how many samples
    /// of how many within it. `done` is set on the frame the last tile lands,
    /// which is what closes a modal's progress out.
    RenderProgress {
        tile: u32,
        tiles: u32,
        sample: u32,
        samples: u32,
        done: bool,
        /// How long the render has taken, and how much longer it will take.
        ///
        /// `remaining_ms` is absent while there is not enough to say, which is
        /// the first chunks of a render and the moment after the last one. A
        /// confident wrong number is worse than an honest blank, so the shape
        /// says so rather than sending a zero the dialog would have to guess
        /// about.
        elapsed_ms: f64,
        remaining_ms: Option<f64>,
    },
    /// The renderer left something out of the scene it just ingested.
    ///
    /// Pushed once when a traced render starts rather than when it finishes,
    /// because the useful moment to learn that your curves will not be in the
    /// picture is before you wait for the picture.
    RenderNotice { message: String },
    /// A traced pane's accumulation advanced: how many samples its mean
    /// averages, of the preview's target. The counter that marks a
    /// converging image as converging. Pushed on change, not per frame.
    PaneSamples {
        pane: usize,
        samples: u32,
        target: u32,
    },
    /// What the current selection can be manipulated with: the tools that
    /// apply to it, and the parameters its transform is made of.
    ///
    /// One answer to two questions, deliberately. The tool column greys out
    /// what is missing and the context menu resets exactly these params, and
    /// deriving the second from the first somewhere in the frontend would put
    /// a second opinion about what a light is where there should be none.
    ///
    /// Pushed on change rather than polled: a selection changes far less often
    /// than a frame ticks.
    SelectionCapability {
        tools: Vec<&'static str>,
        transform_params: Vec<&'static str>,
    },
    /// The device reported an uncaptured graphics fault. The frontend
    /// writes the full message to `console.error`, which the crash
    /// reporter attaches to its next report as context, and toasts a
    /// short pointer at it. `count` collapses identical consecutive
    /// errors, since a fault in a per-frame path fires at frame rate.
    GpuFault {
        kind: &'static str,
        message: String,
        count: u32,
    },
}

#[derive(Serialize, Clone, Copy, PartialEq)]
struct RectDto {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// A detailed pick result (the review anchor source); canvas coordinates in,
/// mesh/face/barycentric/world out.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickDetailDto {
    node: f64,
    mesh: u32,
    face: u32,
    barycentric: [f32; 3],
    world_pos: [f32; 3],
    distance: f32,
    pane: usize,
}

/// One marker pin's screen position (canvas CSS px) in one pane. Deliberately
/// minimal: category/resolved/stale ride the structure channel
/// (`review_annotations`), which refreshes on `reviewChanged`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkerScreenDto {
    id: f64,
    pane: usize,
    x: f32,
    y: f32,
}

/// What a `render` node says, as the frontend displays it.
///
/// Produced by the engine rather than assembled by the frontend, which is the
/// point: the browser and a headless command resolve the same document through
/// one rule instead of two that agree until they do not. It travels out for the
/// dialog to show and never travels back in, because a render is asked for by
/// naming the node.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
// A mirror of the node's own parameters, and the node has four independent
// switches. Grouping them into a sub-struct to satisfy the count would put a
// shape on the boundary that the document does not have.
#[allow(clippy::struct_excessive_bools)]
struct RenderSettingsDto {
    width: u32,
    height: u32,
    samples: u32,
    engine: String,
    bounces: u32,
    transmissive_bounces: u32,
    denoise: bool,
    /// The `camera` node to shoot through, or `null` to shoot the active
    /// pane's current view.
    ///
    /// The still never moves a pane. A shot is a property of the scene and the
    /// viewport is where someone happens to be looking, so the job builds its
    /// own camera from this and leaves every pane where it was.
    camera: Option<f64>,
    /// The auxiliary passes the run writes beside the image.
    ///
    /// The production half of the pass model: what a render *makes*. Which of
    /// them a window *shows* is a separate control that lives in the window,
    /// because wanting to watch the normal pass to understand what the denoiser
    /// is steering by is a different intention from wanting the albedo written
    /// because a compositor asked for it.
    aov_albedo: bool,
    aov_normal: bool,
    aov_depth: bool,
    /// Whether the render carries a matte. The window reads it for two
    /// things a picture cannot say about itself: showing the checker only
    /// behind a render that actually has transparency, and routing the
    /// eight-bit save through the engine's own encoder, whose straight alpha
    /// a canvas round trip would corrupt.
    transparent_background: bool,
}

impl From<solarxy_graph::nodes::RenderSettings> for RenderSettingsDto {
    fn from(s: solarxy_graph::nodes::RenderSettings) -> Self {
        Self {
            width: s.width,
            height: s.height,
            samples: s.samples,
            engine: match s.engine {
                solarxy_graph::nodes::RenderEngine::PathTraced => "pathTraced",
                solarxy_graph::nodes::RenderEngine::Raster => "raster",
            }
            .to_string(),
            bounces: s.bounces,
            transmissive_bounces: s.transmissive_bounces,
            denoise: s.denoise,
            #[allow(clippy::cast_precision_loss)]
            camera: s.camera.map(|n| n.0 as f64),
            aov_albedo: s.aov_albedo,
            aov_normal: s.aov_normal,
            aov_depth: s.aov_depth,
            transparent_background: s.transparent_background,
        }
    }
}

/// A screenshot request: capture resolution (physical pixels) plus the
/// GPU-side overlay toggles (DOM layers like markers never appear in a GPU
/// capture; compositing them is a JS concern).
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
struct ScreenshotOptsDto {
    width: u32,
    height: u32,
    overlays: ScreenshotOverlaysDto,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
struct ScreenshotOverlaysDto {
    grid: bool,
    axes: bool,
    validation: bool,
}

/// The Solarxy browser application: one WebGPU surface, the full renderer,
/// the multi-object scene, the scene environment, per-pane cameras, and
/// the headless engine.
#[wasm_bindgen]
// Four bools, and clippy is right to ask. They stay separate because they
// share nothing: `uv_use_preview` is a UV-pane source choice, `player_mode`
// is a session mode, and the two `*_dirty` flags are per-frame recompute
// latches. A sub-struct would put one name over four unrelated things and
// make every read longer without making any of them clearer.
#[allow(clippy::struct_excessive_bools)]
pub struct SolarxyApp {
    /// Kept for the asset-preview pane: a second surface (its own canvas)
    /// must come from the same instance as the device.
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The asset-preview pane's isolated render state, when open.
    preview: Option<PreviewState>,
    /// The (sRGB) format render pipelines target; the surface view is
    /// created with this format each frame (Chrome offers only non-sRGB
    /// surface formats, so it rides `view_formats`).
    render_format: wgpu::TextureFormat,
    renderer: Renderer,
    raster: solarxy_host::RasterBackend,
    env: SceneEnvironment,
    /// Makes `SceneOp::SetEnvironment` idempotent. The engine re-emits the
    /// whole environment on every rebuild and installing one convolves an
    /// irradiance cubemap, so this remembers what is already on the GPU.
    /// Invalidated by `set_environment_prepared` and `clear_environment`,
    /// which move the IBL behind the scene contract's back.
    environment: solarxy_renderer::environment::EnvironmentTracker,
    /// The bounds `env` was last built for (grid/floor/shadow fit).
    env_bounds: AABB,
    view: HostViewState,
    /// Each pane's own rendering intent, used when the pane is a free view. A
    /// pane looking through a camera composites with that camera's look
    /// instead; see `SolarxyApp::pane_look`.
    pane_looks: [PaneLook; 4],
    /// Which `camera` node each pane looks through (`None` = free view).
    look_through: [Option<NodeId>; 4],
    /// Whether a look-through pane is locked so navigation reframes the camera
    /// node. The type carries the rule that a lock means nothing on a pane
    /// that is not bound, so no call site here restates it.
    camera_locked: solarxy_host::cameras::CameraLocks,
    /// Transient: a locked look-through pane is mid-navigation, so the
    /// node-to-pane follow is suppressed until the gesture commits (it would
    /// otherwise fight live navigation). Not persisted.
    camera_editing: [bool; 4],
    engine: Engine,
    host_events: Vec<HostEvent>,
    /// The uncaptured-error queue the shared device hook fills; drained
    /// into `host_events` when the frontend takes them each frame.
    gpu_faults: solarxy_renderer::faults::GpuFaults,
    last_pane_rects: Vec<RectDto>,
    /// Device pixel ratio: JS pointer coordinates arrive in CSS px and are
    /// scaled into physical canvas px for pane hit-testing and picking.
    dpr: f32,
    pointer_buttons_down: u32,
    /// The viewport tool, plus any hover highlight and drag in flight. The drag
    /// loop runs entirely host-side; JS only ever calls `set_tool`.
    gizmo: GizmoState,
    /// Where the in-flight drag's result is addressed.
    ///
    /// Parallel to `gizmo.drag` rather than inside it. The drag solver lives in
    /// `solarxy-host`, which deliberately knows nothing about documents, so it
    /// solves a *pose* and this shell remembers which node the answer belongs
    /// to. The two are only ever ended together, through `take_gizmo_drag`, so
    /// they cannot drift apart into a drag that commits to nowhere.
    gizmo_addr: Option<GizmoAddr>,
    /// The live drag's delta text, rebuilt each pointer move and polled once per
    /// frame by the shell. `None` whenever nothing is being dragged.
    gizmo_readout: Option<String>,
    /// The last capability pushed to the frontend, so the event fires on
    /// change rather than every frame. `None` means nothing manipulable is
    /// selected, which is a distinct answer from an empty tool list.
    last_capability: Option<(Vec<&'static str>, Vec<&'static str>)>,
    /// The scene object tinted as selected in the viewports,
    /// or `None`.
    selected_object: Option<SceneObjectId>,
    /// Validate jobs drained from the engine but not yet handed to the
    /// worker: the geometry is packed to a transfer blob at drain time so
    /// `take_validate_jobs` moves plain bytes.
    pending_validate: Vec<PendingValidateJob>,
    pending_image: Vec<PendingImageJob>,
    pending_hdri: Vec<PendingHdriJob>,
    /// The graph context the node canvas currently shows (React mirrors it
    /// via `set_current_context`); the UV pane's selected-node source
    /// resolves against it.
    current_ctx: GraphContext,
    /// The UV pane's selected-node preview: a one-object scene holding the
    /// selected node's cooked geometry, uploaded on demand and deduped by
    /// attribute-`Arc` identity like the main scene.
    uv_scene: SceneObjects,
    /// Whether the UV pane draws the selected-node preview (a subflow
    /// selection with committed geometry) or falls back to the selected /
    /// first scene object.
    uv_use_preview: bool,
    /// Identity of the last UV source (node id + geometry Arc address);
    /// a change invalidates the overlap statistic.
    last_uv_source: Option<(u64, usize)>,
    /// The last (pct, pending) pushed as a `UvOverlap` host event.
    last_overlap: (Option<f32>, bool),
    /// Last physical-pixel pointer position (UV pan deltas).
    last_pointer: (f32, f32),
    /// The loaded HDRI environment's staged-asset identity (content hash +
    /// original name), for the `.slxy` environment section. `None` when the
    /// procedural sky is active.
    hdri: Option<HdriMeta>,
    /// Player mode: the host runs a published scene rather than an editing
    /// session. Suppresses the manipulator, picking and review markers, and
    /// locks the layout to one pane.
    ///
    /// A flag on the editor host rather than a second wasm target: a lean
    /// player crate would mean a second boot path and a second
    /// payload gate that could drift from this one, for a saving nobody has
    /// measured. The follow-up is recorded with an instruction to measure
    /// first.
    player_mode: bool,
    /// A screenshot request captured this frame (rendered at frame end).
    screenshot_request: Option<ScreenshotOptsDto>,
    /// A turntable-export frame request: (pane, absolute azimuth in degrees,
    /// opts). Rendered offscreen at frame end from a rotated clone of the
    /// pane's render-through camera, through the same capture slot as the
    /// screenshot. The frontend drives one frame at a time.
    turntable_request: Option<(usize, f32, ScreenshotOptsDto)>,
    /// The in-flight screenshot readback (one at a time).
    pending_screenshot: Option<solarxy_renderer::capture::PendingCapture>,
    /// The camera the running still shoots through, owned by the job rather
    /// than borrowed from a pane, so rendering never moves the view.
    still_camera: Option<solarxy_renderer::camera_state::CameraState>,
    /// The look that camera carries, resolved once when the job starts.
    ///
    /// The camera owns the look as of 0.8.2, so a still through camera X gets
    /// X's exposure, tone map and grade rather than whichever pane happened to
    /// be active. Resolved at start rather than per tile because a look that
    /// changed mid-render would band the picture at a tile boundary.
    still_look: CompositeLook,
    /// The running still render, if any.
    ///
    /// While this is `Some` the frame loop renders the job instead of the
    /// panes: the shared targets are sized to the tile for the job's duration,
    /// and a viewport rendering at layout size in the same frame would resize
    /// them back twice a frame for the length of the render.
    still: Option<solarxy_host::StillRenderJob>,
    /// The tracer, built the first time a traced still is asked for. A session
    /// that never renders one never pays for the pipelines.
    tracer: Option<solarxy_renderer::pathtrace::backend::PathBackend>,
    /// Whether the tracer's environment is behind the scene's.
    ///
    /// Set where the image behind the IBL actually changes, not where the
    /// environment op arrives: the engine re-emits that op on every rebuild,
    /// and installing on every one would upload the distribution once a cook.
    /// Read lazily when a traced render starts, so a session that never traces
    /// pays nothing for the tables.
    traced_env_dirty: bool,
    /// Finished tiles waiting for the frontend to take them.
    still_tiles: std::collections::VecDeque<solarxy_host::still::StillTile>,
    /// The picture so far, waiting for the frontend to take it. Separate from
    /// the tiles because a preview is painted and never saved.
    still_previews: std::collections::VecDeque<solarxy_host::still::StillPreview>,
    /// When the running still began, on the page's own timer. The elapsed a
    /// reader sees is measured from here rather than from the first tile, so it
    /// includes the setup a person also waited through.
    still_started_ms: f64,
    /// The auxiliary planes being assembled, when any were asked for.
    still_passes: Option<StillPasses>,
    /// What the render was *asked* for, as albedo, normal, depth.
    ///
    /// Kept beside the planes because the spec's two flags cannot answer it:
    /// albedo and normal share one store, so a spec that says `aux` does not
    /// say which of the two a person wanted, and a selector offering a pass
    /// nobody asked for would be offering something the file will not contain.
    still_pass_request: [bool; 3],
    /// Whether the engine drawing this still writes auxiliary passes at all.
    ///
    /// Capability rather than identity, resolved once when the render starts.
    /// A window asks this rather than which backend is running, which is the
    /// rule the backend contract states and the same one the terminal's
    /// selector follows.
    still_writes_aovs: bool,
    /// The floating-point image being assembled, when the running still is a
    /// float one. `None` for the ordinary eight-bit still, which is assembled
    /// on a canvas the browser owns and never needs a copy here.
    still_float: Option<solarxy_host::still::FloatImage>,
    /// Whether the traced preview runs its edge-aware filter.
    ///
    /// A preference rather than a per-pane setting, matching the two effects
    /// it sits beside in the panel: it describes how the preview is built,
    /// not what any one pane is showing.
    preview_denoise: bool,
    /// Per-pane camera identity the traced preview last accumulated under.
    /// A mismatch on encode resets that pane's accumulation, which is what
    /// makes the preview converge only while the pane is quiescent.
    traced_cam_keys: [Option<[f32; 13]>; 4],
    /// The environment scalars the tracer last integrated against; a
    /// change resets every traced pane, since the mean was of another sky.
    traced_env_params: (f32, f32),
    /// The last per-pane sample counts pushed to the frontend, so the
    /// counter event fires on change rather than every frame.
    last_pane_samples: [Option<(u32, u32)>; 4],
    /// Whether the normals/bounds visualization aggregate is stale
    /// (geometry changed, env rebuilt, or an overlay mode just turned on).
    viz_dirty: bool,
    /// Host-owned attribute visualization (session-only, scene-wide; never
    /// saved into `.slxy`, never in undo). The strip's toggles and the
    /// picked lane name.
    attr_viz: AttrVizState,
    /// Theme label colors (text, chip, dot) as last pushed by the frontend.
    /// Cached because the style is assembled from two independent halves
    /// (see `push_label_style`), and a size change must not blank the
    /// palette any more than a theme change must reset the size.
    label_colors: [[f32; 3]; 3],
    /// Whether the attribute-vector line buffer is stale (mirrors the
    /// `viz_dirty` sites, plus any `set_attr_viz`).
    attr_dirty: bool,
    /// The preference-backed display defaults (wireframe weight,
    /// background), pushed from the TS prefs store. Pane seeds, never a
    /// force-override: a loaded scene's saved per-pane settings win.
    display_defaults: DisplayDefaults,
}

/// Where a gizmo drag's result is written: the engine half of a drag, which
/// the pose-only solver in `solarxy-host` does not carry.
#[derive(Clone, Copy)]
struct GizmoAddr {
    ctx: GraphContext,
    node: NodeId,
}

/// The solver's view of an engine gizmo target: the pose, without the
/// addressing or the append bookkeeping.
/// Everything one pane's encode and composite read that is derived from
/// the host rather than handed in by the frame loop.
struct PaneInputs {
    background: ResolvedBackground,
    bounds: AABB,
    look: CompositeLook,
    scene_present: bool,
    outline: bool,
    grid_plane: Option<u32>,
    /// Whether this pane's 3D content goes to the tracer instead of the
    /// rasterizer this frame. Decided (and its housekeeping run) before
    /// the pane content is built, because the content can borrow the UV
    /// preview scene while the housekeeping needs the whole host.
    traced: bool,
}

/// What the traced preview converges to. High enough that a resting pane
/// keeps improving for minutes at one sample per frame, low enough that
/// the counter's target still means something.
const PREVIEW_TARGET_SAMPLES: u32 = 4096;

/// The largest floating-point still the browser will assemble, in pixels.
///
/// Not the eight-bit limit, which stays at the job's own 8192 edge. A float
/// save holds three `f32` a pixel in this module's heap while the render runs
/// and then encodes from them, so the peak is roughly forty bytes a pixel
/// against the canvas path's four, inside a thirty-two-bit address space that
/// is also holding the document, the tracer's buffers and the page. Sixteen
/// megapixels puts the peak near 290 MB, which a tab can be asked for. A
/// transparent render keeps its fourth channel and holds sixteen bytes a
/// pixel instead of twelve, which moves that peak to roughly 350 MB at the
/// same ceiling; the ceiling deliberately does not move for it, because a
/// limit that shifted with a checkbox would be two limits wearing one name.
///
/// The same shape as the screenshot path's four-megapixel ceiling, and for the
/// same reason: a limit with a stated number beats an allocation failure,
/// which on wasm takes the whole tab rather than the operation.
const MAX_FLOAT_STILL_PIXELS: u64 = 16_000_000;

/// The most memory the auxiliary planes may take, for one render.
///
/// Stated in bytes rather than pixels because that is what the limit is about:
/// the auxiliary store is sixteen bytes a pixel and the depth four, so twenty
/// together, and a ceiling written as a pixel count would have to be rewritten
/// the moment a fourth pass existed. At this budget a 4096 by 2304 still, which
/// is the largest this release measures, fits with room over.
///
/// The planes are held whole rather than per tile because they have to be:
/// the depth display normalizes over the range of the whole picture, so a plane
/// mapped tile by tile would band at every seam.
const MAX_PASS_PLANE_BYTES: u64 = 192 * 1024 * 1024;

/// Pixels as megapixels, for a message a person reads.
#[allow(clippy::cast_precision_loss)]
fn megapixels(pixels: u64) -> f64 {
    pixels as f64 / 1_000_000.0
}

/// The traced preview's settings: one sample per animation frame (the
/// pacing that keeps the page responsive), half resolution, and the
/// edge-aware filter, which defaults on because a one-sample frame is
/// unusable without it. Asserted before every preview encode rather than
/// held, since the still job authors its own settings on the same backend.
///
/// The filter is the one value a person can turn off, and the reason to is
/// judging what the tracer actually produced rather than what the filter
/// made of it, which matters most at the sample counts where the filter is
/// doing the most work.
fn preview_trace_settings(denoise: bool) -> TraceSettings {
    TraceSettings {
        samples: PREVIEW_TARGET_SAMPLES,
        chunk: 1,
        denoise,
        resolution_scale: 0.5,
        ..TraceSettings::default()
    }
}

/// The fields of a camera a traced accumulation is valid under. Aspect
/// included, because a resize reshapes every ray; the projection kind
/// rides as a discriminant.
fn camera_key(c: &Camera) -> [f32; 13] {
    [
        c.eye.x,
        c.eye.y,
        c.eye.z,
        c.target.x,
        c.target.y,
        c.target.z,
        c.up.x,
        c.up.y,
        c.up.z,
        c.fovy,
        c.aspect,
        c.ortho_scale,
        match c.projection {
            solarxy_core::preferences::ProjectionMode::Perspective => 0.0,
            solarxy_core::preferences::ProjectionMode::Orthographic => 1.0,
        },
    ]
}

fn gizmo_pose(t: &GizmoTarget) -> GizmoPose {
    GizmoPose {
        translate: t.translate,
        rotate: t.rotate,
        rotate_order: t.rotate_order,
        scale: t.scale,
        uniform_scale: t.uniform_scale,
        extent: t.extent,
        aim: t.aim,
        params: t.params,
        anchor: t.anchor,
        aim_anchor: t.aim_anchor,
        basis: t.basis,
        parent_basis: t.parent_basis,
        parent: t.parent,
    }
}

/// The parameter writes one solved drag value makes on one target: each key
/// the target declares for that drag, paired with the value for it.
///
/// Lives here rather than on `DragValue` because `ParamSource` is an engine
/// type and the solver's crate has no engine dependency. Pairing happens in
/// one place so preview, commit and rollback cannot disagree about which key
/// gets which number; the two sides are positional, and
/// `the_keys_and_the_values_of_a_drag_are_always_the_same_length` in the
/// solver keeps them the same length.
fn drag_writes(
    value: gizmo::DragValue,
    params: &TransformParams,
) -> Vec<(&'static str, ParamSource)> {
    let Some(keys) = value.param().keys(params) else {
        return Vec::new();
    };
    keys.iter()
        .zip(value.values().into_iter().flatten())
        .map(|(key, v)| {
            let value = match v {
                gizmo::DragScalarOrVec3::Vec3(v) => {
                    ParamValue::Vec3([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
                }
                gizmo::DragScalarOrVec3::Scalar(f) => ParamValue::Float(f64::from(f)),
            };
            (key, ParamSource::Literal(value))
        })
        .collect()
}

/// Identity of the loaded HDRI (its bytes live in the engine asset table).
#[derive(Clone)]
struct HdriMeta {
    hash: String,
    name: String,
}

/// The scene-object id reserved for the UV pane's selected-node preview
/// (far outside the engine's node-derived id space).
const UV_PREVIEW_ID: SceneObjectId = SceneObjectId(u64::MAX);

/// One stashed `ValidateGeometry` job awaiting worker dispatch.
struct PendingValidateJob {
    ctx: GraphContext,
    job_id: u64,
    blob: Vec<u8>,
    config_json: String,
    budget: Option<u32>,
}

/// A drained `DecodeImage` job awaiting [`SolarxyApp::take_image_jobs`]:
/// the frontend pulls the encoded bytes by hash (like the parse pump) and
/// decodes them in the import worker via `createImageBitmap`.
struct PendingImageJob {
    ctx: GraphContext,
    job_id: u64,
    hash: String,
    name: String,
}

/// A drained `DecodeHdrImage` job awaiting [`SolarxyApp::take_hdri_jobs`].
/// Same shape as an image job; it exists separately because the frontend
/// routes it to a different worker entry point (no browser codec reads
/// Radiance or `OpenEXR`, so the Rust decoder runs in the worker).
struct PendingHdriJob {
    ctx: GraphContext,
    job_id: u64,
    hash: String,
    name: String,
}

/// The asset-preview pane's isolated render state: its own surface
/// (a second canvas from the SAME instance/device), a throwaway `SceneObjects`
/// holding one parsed model, and an orbit camera. Never touches the document.
///
/// **The one place in this host that still drives the renderer's passes
/// directly rather than through a backend, and deliberately so.** It is not a
/// document pane: it holds a staged asset nobody has imported, and it runs a
/// reduced chain on purpose, shadow and main only, with bloom and ambient
/// occlusion off, because a preview is a shaded look rather than a beauty
/// frame. Routing it through the shared pane body would encode the gbuffer,
/// ambient-occlusion and bloom passes it exists to skip, which is a behaviour
/// change dressed as a cleanup. Left as it is, on purpose.
struct PreviewState {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    scene: solarxy_host::preview::PreviewScene,
}

fn map_button(button: u32) -> Option<PointerButton> {
    match button {
        0 => Some(PointerButton::Left),
        1 => Some(PointerButton::Middle),
        2 => Some(PointerButton::Right),
        _ => None,
    }
}

fn projection_name(mode: ProjectionMode) -> &'static str {
    match mode {
        ProjectionMode::Perspective => "perspective",
        ProjectionMode::Orthographic => "orthographic",
    }
}

/// Bridges the renderer camera to the `.slxy` orbit shape (target + yaw /
/// pitch / distance): `dir = eye - target`, `pitch = asin(dir.y)`,
/// `yaw = atan2(dir.x, dir.z)`.
fn camera_to_json(cam: &Camera) -> solarxy_scenefile::CameraJson {
    let offset = cam.eye - cam.target;
    let distance = offset.magnitude().max(1e-4);
    let dir = offset / distance;
    solarxy_scenefile::CameraJson {
        target: [cam.target.x, cam.target.y, cam.target.z],
        yaw: dir.x.atan2(dir.z),
        pitch: dir.y.clamp(-1.0, 1.0).asin(),
        distance,
        fov_y: cam.fovy.to_radians(),
        projection: projection_name(cam.projection).to_string(),
        ortho_scale: cam.ortho_scale,
    }
}

/// Copies a resolved `CameraDef` (from a `camera` node) into a viewport
/// camera, so a pane looking through the node shows exactly what it frames.
/// The pane's aspect is not touched here (it tracks the pane rect); the
/// framing gate uses the def's own aspect.
fn apply_camera_json(cam: &mut Camera, json: &solarxy_scenefile::CameraJson) {
    let target = Point3::new(json.target[0], json.target[1], json.target[2]);
    let cp = json.pitch.cos();
    let dir = Vector3::new(cp * json.yaw.sin(), json.pitch.sin(), cp * json.yaw.cos());
    cam.target = target;
    cam.eye = target + dir * json.distance.max(1e-4);
    // A hardcoded +Y up is degenerate for a scene saved in a top/bottom view
    // (look_at with forward parallel to up); the turntable up at the stored
    // angles is what the orbit maintains live.
    cam.up = turntable_up(json.yaw, json.pitch);
    if json.fov_y > 0.0 {
        cam.fovy = json.fov_y.to_degrees();
    }
    cam.projection = if json.projection == "orthographic" {
        ProjectionMode::Orthographic
    } else {
        ProjectionMode::Perspective
    };
    if json.ortho_scale > 0.0 {
        cam.ortho_scale = json.ortho_scale;
    }
}

/// Serializes a value to a `JsValue` via serde-wasm-bindgen, using the
/// json-compatible serializer so Rust maps (e.g. a node's `params`) become
/// plain JS objects rather than `Map`s, matching what the frontend expects.
fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&format!("serialize: {e}")))
}

/// The import-worker parse entry: a GPU-free wasm export the worker calls in
/// a second, headless instantiation of this same module. It parses `files`
/// (a JS array of `{ name, bytes }`, the primary model first, then any
/// sidecars) into a finished [`solarxy_kernel::GeometrySet`] and returns
/// `{ blob, validation }`: the geometry transfer blob (`Uint8Array`) plus
/// the implicit load validation as JSON (the same `validate_raw_model` the
/// desktop viewer runs at load). Never touches wgpu, so instantiating it in
/// a worker creates no device.
#[wasm_bindgen]
pub fn parse_model_job(
    format: String,
    options_json: String,
    files: JsValue,
) -> Result<JsValue, JsError> {
    let files = read_files(&files)?;
    let (name, bytes) = files
        .first()
        .ok_or_else(|| JsError::new("parse_model_job: no files provided"))?;
    let options: ImportOptions = serde_json::from_str(&options_json)
        .map_err(|e| JsError::new(&format!("bad import options: {e}")))?;

    // Rebuild a temporary asset table so the resolver can find sidecars by
    // name (content-addressed staging; ids are irrelevant here).
    let mut table = AssetTable::new();
    for (n, b) in &files {
        table.stage(n.clone(), String::new(), b.clone());
    }

    let (set, validation) =
        solarxy_graph::nodes::parse_model_validated(&format, bytes, name, &table, &options)
            .map_err(|e| JsError::new(&e))?;
    let validation_json = serde_json::to_string(&validation)
        .map_err(|e| JsError::new(&format!("serialize validation: {e}")))?;
    let out = js_sys::Object::new();
    js_sys::Reflect::set(
        &out,
        &JsValue::from_str("blob"),
        &js_sys::Uint8Array::from(transfer::pack(&set).as_slice()),
    )
    .map_err(|_| JsError::new("parse_model_job: reflect set failed"))?;
    js_sys::Reflect::set(
        &out,
        &JsValue::from_str("validation"),
        &JsValue::from_str(&validation_json),
    )
    .map_err(|_| JsError::new("parse_model_job: reflect set failed"))?;
    Ok(out.into())
}

/// The worker validation entry (the validate node above its inline
/// threshold): unpacks the geometry transfer blob, runs the configured
/// validation pipeline, and returns the full `ValidationResult` as JSON.
/// GPU-free, like `parse_model_job`.
#[wasm_bindgen]
pub fn validate_geometry_job(
    blob: Vec<u8>,
    config_json: String,
    budget: Option<u32>,
) -> Result<String, JsError> {
    let set =
        transfer::unpack(&blob).map_err(|e| JsError::new(&format!("bad geometry blob: {e}")))?;
    let config: ValidationConfig = serde_json::from_str(&config_json)
        .map_err(|e| JsError::new(&format!("bad validation config: {e}")))?;
    let raw = set.to_raw();
    let result =
        validate_raw_model_with_config(&raw, "", &config, &ValidationThresholds::default(), budget);
    serde_json::to_string(&result).map_err(|e| JsError::new(&format!("serialize result: {e}")))
}

/// The worker HDRI-preparation entry: runs the CPU stages of the IBL
/// build (decode, sanitize, irradiance convolution) off-thread and returns
/// the packed [`solarxy_renderer::ibl::PreparedHdri`] blob for
/// `set_environment_prepared`. GPU-free, like the other worker exports.
#[wasm_bindgen]
pub fn prepare_hdri_job(bytes: Vec<u8>, format: String) -> Result<js_sys::Uint8Array, JsError> {
    let prepared = solarxy_renderer::ibl::PreparedHdri::prepare(&bytes, &format)
        .map_err(|e| JsError::new(&format!("prepare HDRI: {e}")))?;
    Ok(js_sys::Uint8Array::from(prepared.pack().as_slice()))
}

/// The import-worker hierarchy-build entry: builds one acceleration structure
/// over a mesh and returns the packed transfer blob.
///
/// The fourth GPU-free worker export, and the reason this one exists is the
/// same as the first: wasm has no threads, and a build over a million
/// triangles is seconds rather than milliseconds, so running it on the main
/// thread would stall the frame loop for the length of the build. Native hosts
/// call `build_hierarchy_job` directly on their own thread; it is the same
/// function, so there is one build path rather than two.
///
/// `positions` is flat `xyz`. Both arrive as typed arrays and are copied into
/// this instance's heap by `wasm-bindgen`, which is unavoidable: the worker is
/// a second wasm instance with its own memory and nothing can be shared into
/// it.
#[wasm_bindgen]
pub fn build_bvh_job(positions: Vec<f32>, indices: Vec<u32>) -> js_sys::Uint8Array {
    js_sys::Uint8Array::from(
        solarxy_renderer::pathtrace::scene::build_hierarchy_job(&positions, &indices).as_slice(),
    )
}

/// Reads a JS array of `{ name: string, bytes: Uint8Array }` into owned
/// `(name, bytes)` pairs.
fn read_files(files: &JsValue) -> Result<Vec<(String, Vec<u8>)>, JsError> {
    let array: js_sys::Array = files
        .clone()
        .dyn_into()
        .map_err(|_| JsError::new("parse_model_job: files must be an array"))?;
    let mut out = Vec::with_capacity(array.length() as usize);
    for item in array.iter() {
        let name = js_sys::Reflect::get(&item, &JsValue::from_str("name"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let bytes = js_sys::Reflect::get(&item, &JsValue::from_str("bytes"))
            .ok()
            .and_then(|v| v.dyn_into::<js_sys::Uint8Array>().ok())
            .map(|u| u.to_vec())
            .unwrap_or_default();
        out.push((name, bytes));
    }
    Ok(out)
}

// ---- boundary DTOs (camelCase; the engine/scene-file types stay
// snake_case on disk, so these bridge to the JS convention) ----

/// Screen-edge slack shared by the review-marker DOM projection: a little
/// beyond the frustum so pins fade at the edge instead of popping exactly
/// on it.
const NDC_XY_SLACK: f32 = 1.05;
/// The wgpu clip-space depth range with the same slack; the z cull is
/// what rejects behind-camera points under orthographic projection,
/// where `clip.w` is a constant 1.
const NDC_Z_MIN: f32 = -0.05;
const NDC_Z_MAX: f32 = 1.05;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewStateDto {
    layout: ViewLayout,
    split_ratio: f32,
    active_pane: usize,
    cameras_linked: bool,
    pane_settings: [PaneDisplaySettings; 4],
    /// Each pane's own look, which the Look dialog edits. A pane looking
    /// through a camera shows that camera's look instead and edits it on
    /// the node.
    pane_looks: [PaneLook; 4],
    display: DisplaySettings,
    pane_projections: [String; 4],
    pane_rects: Vec<RectDto>,
    /// The `camera` node each pane looks through (id as a number), or `null`
    /// for a free view.
    pane_look_through: [Option<f64>; 4],
    /// Whether each look-through pane is locked (reframes the camera).
    pane_camera_locked: [bool; 4],
    /// The framing aspect of each pane's look-through camera (for the gate
    /// overlay); `null` when the pane is a free view.
    pane_gate_aspect: [Option<f32>; 4],
    /// The host-owned attribute-visualization state (the right strip
    /// mirrors this, like the tool mode).
    attr_viz: AttrVizState,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CameraCommandDto {
    kind: String,
    axis: String,
    mode: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CameraPoseDto {
    position: [f32; 3],
    target: [f32; 3],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportJobDto {
    ctx: GraphContext,
    job_id: f64,
    hash: String,
    name: String,
    format: String,
    options: ImportOptions,
    sidecars: Vec<AssetRefDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AssetRefDto {
    hash: String,
    name: String,
}

/// The parameter panel's per-row readout: a value, or why there is none.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedParamDto {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<solarxy_graph::params::ParamValue>,
    /// The value as the readout under the field prints it, or the error
    /// message when there is no value.
    ///
    /// Rides the value rather than being asked for separately: the field
    /// already makes this call and formatted the answer itself, so a
    /// query of its own would be a second crossing for a rule that had to
    /// stop existing twice anyway.
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SaveExtra {
    generator: String,
    canvas_viewports: BTreeMap<String, ViewportDto>,
    meta: MetaDto,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadResultDto {
    batch: EventBatch,
    warnings: Vec<String>,
    canvas_viewports: BTreeMap<String, ViewportDto>,
    meta: MetaDto,
    environment: EnvironmentDto,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentDto {
    ibl_mode: String,
    hdri_hash: Option<String>,
    hdri_name: Option<String>,
    /// Whether the loaded document holds an `environment` node.
    ///
    /// When it does, the node wins and the frontend must not restore the
    /// scene file's own environment section: the node's cook emits
    /// `SceneOp::SetEnvironment` and installing the sidecar's HDRI too
    /// would race it, with whichever finished last taking the viewport.
    /// The section stays the fallback for documents authored before the
    /// node existed, which is the whole point of keeping it.
    from_node: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct ViewportDto {
    x: f32,
    y: f32,
    zoom: f32,
}

impl From<ViewportDto> for solarxy_scenefile::CanvasViewportJson {
    fn from(v: ViewportDto) -> Self {
        Self {
            x: v.x,
            y: v.y,
            zoom: v.zoom,
        }
    }
}
impl From<solarxy_scenefile::CanvasViewportJson> for ViewportDto {
    fn from(v: solarxy_scenefile::CanvasViewportJson) -> Self {
        Self {
            x: v.x,
            y: v.y,
            zoom: v.zoom,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct MetaDto {
    name: String,
    description: String,
    project_id: String,
    created: String,
    modified: String,
}

impl From<MetaDto> for solarxy_scenefile::MetaJson {
    fn from(m: MetaDto) -> Self {
        Self {
            name: m.name,
            description: m.description,
            project_id: m.project_id,
            created: m.created,
            modified: m.modified,
        }
    }
}
impl From<solarxy_scenefile::MetaJson> for MetaDto {
    fn from(m: solarxy_scenefile::MetaJson) -> Self {
        Self {
            name: m.name,
            description: m.description,
            project_id: m.project_id,
            created: m.created,
            modified: m.modified,
        }
    }
}
