//! The `SolarxyApp` wasm-bindgen class: the browser host over the engine
//! and the full `solarxy-renderer` pipeline (the phase-4 stopgap
//! forward renderer is retired).
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
    BackgroundMode, IblMode, InspectionMode, PaneMode, ProjectionMode, ResolvedBackground,
    ToneMode, UvMapBackground,
};
use solarxy_core::raycast::{Ray, screen_to_world_ray};
use solarxy_core::scene::{SceneDelta, SceneObjectId, SceneOp};
use solarxy_core::validation::{
    ValidationConfig, ValidationResult, ValidationThresholds, validate_raw_model_with_config,
};
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings, ViewLayout};
use solarxy_core::geometry::compute_bounds;
use solarxy_core::AABB;
use solarxy_graph::assets::AssetTable;
use solarxy_graph::cook::{ImportOptions, JobId, JobRequest, JobResult, ParsedModel};
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::{EngineEvent, GizmoTarget, SceneSidecar};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::{Command, Engine, EventBatch};
use solarxy_kernel::transfer;
use solarxy_renderer::manipulator::{self, ManipulatorState};

use crate::attr_viz::{AttrColorMode, AttrVizState, ramp_color};
use crate::display_defaults::{self, DisplayDefaults};
use crate::gizmo::{self, GizmoState, ToolMode};
use solarxy_renderer::camera::{turntable_up, Camera, CameraUniform};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::frame::{DrawObject, GradientUniform, Renderer, RendererInit, WireframeParams};
use solarxy_renderer::geometry::build_normals_geometry;
use solarxy_renderer::model::{GizmoVertex, NormalsGeometry};
use solarxy_renderer::input::PointerButton;
use solarxy_renderer::light::LightsUniform;
use solarxy_renderer::panes::{self, PaneRect};
use solarxy_renderer::visualization::grid_plane_for;
use solarxy_renderer::scene::{create_light_bind_group, lights_from_camera, BackgroundModeExt};
use solarxy_renderer::scene_objects::SceneObjects;
use solarxy_renderer::texture;
use solarxy_renderer::visualization::VisualizationState;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// The synchronous cook budget per frame, in milliseconds (about half a
/// 60fps frame, leaving headroom for render + the browser).
const COOK_BUDGET_MS: f64 = 6.0;
const MSAA_SAMPLES: u32 = 4;
const SHADOW_MAP_SIZE: u32 = 2048;

/// The current host time in milliseconds (`performance.now`).
fn web_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// Placeholder scene bounds before anything cooks (frames the grid).
fn default_bounds() -> AABB {
    AABB {
        min: Point3::new(-2.0, -2.0, -2.0),
        max: Point3::new(2.0, 2.0, 2.0),
    }
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
        turntable_active: false,
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
        point_size: solarxy_core::view_config::DEFAULT_POINT_SIZE,
    }
}

/// Host-owned view state: the web mirror of the desktop `ViewState`.
struct WebViewState {
    display: DisplaySettings,
    active_pane: usize,
    cameras_linked: bool,
    cameras: [Option<CameraState>; 4],
    pane_settings: [PaneDisplaySettings; 4],
    /// Which `camera` node each pane looks through (`None` = free view).
    look_through: [Option<NodeId>; 4],
    /// Whether a look-through pane is locked so navigation reframes the camera
    /// node (Blender's Lock-Camera-to-View). Only meaningful when the same
    /// slot's `look_through` is `Some`.
    camera_locked: [bool; 4],
    /// Transient: a locked look-through pane is mid-navigation, so the
    /// node-to-pane follow is suppressed until the gesture commits (avoids the
    /// follow fighting live navigation). Not persisted.
    camera_editing: [bool; 4],
}

/// Async happenings the frontend drains once per frame.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
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
    scene_objects: SceneObjects,
    env: SceneEnvironment,
    /// The bounds `env` was last built for (grid/floor/shadow fit).
    env_bounds: AABB,
    view: WebViewState,
    engine: Engine,
    host_events: Vec<HostEvent>,
    last_pane_rects: Vec<RectDto>,
    /// Device pixel ratio: JS pointer coordinates arrive in CSS px and are
    /// scaled into physical canvas px for pane hit-testing and picking.
    dpr: f32,
    pointer_buttons_down: u32,
    /// The viewport tool, plus any hover highlight and drag in flight. The drag
    /// loop runs entirely host-side; JS only ever calls `set_tool`.
    gizmo: GizmoState,
    /// The live drag's delta text, rebuilt each pointer move and polled once per
    /// frame by the shell. `None` whenever nothing is being dragged.
    gizmo_readout: Option<String>,
    /// The scene object tinted as selected in the viewports,
    /// or `None`.
    selected_object: Option<SceneObjectId>,
    /// Validate jobs drained from the engine but not yet handed to the
    /// worker: the geometry is packed to a transfer blob at drain time so
    /// `take_validate_jobs` moves plain bytes.
    pending_validate: Vec<PendingValidateJob>,
    pending_image: Vec<PendingImageJob>,
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
    /// A flag on the editor host rather than a second wasm target (decision
    /// M-10): a lean player crate would mean a second boot path and a second
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
    /// Whether the normals/bounds visualization aggregate is stale
    /// (geometry changed, env rebuilt, or an overlay mode just turned on).
    viz_dirty: bool,
    /// Host-owned attribute visualization (session-only, scene-wide; never
    /// saved into `.slxy`, never in undo). The strip's toggles and the
    /// picked lane name.
    attr_viz: AttrVizState,
    /// Whether the attribute-vector line buffer is stale (mirrors the
    /// `viz_dirty` sites, plus any `set_attr_viz`).
    attr_dirty: bool,
    /// The preference-backed display defaults (wireframe weight,
    /// background), pushed from the TS prefs store. Pane seeds, never a
    /// force-override: a loaded scene's saved per-pane settings win.
    display_defaults: DisplayDefaults,
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

#[wasm_bindgen]
impl SolarxyApp {
    /// Boots over a canvas: WebGPU surface/device/queue, the full renderer
    /// (`uv_checker_png` is the checker texture asset the shell ships), the
    /// scene environment, and the engine with the host clock installed.
    #[allow(clippy::too_many_lines)] // linear boot sequence; splitting obscures it
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        uv_checker_png: Vec<u8>,
    ) -> Result<SolarxyApp, JsError> {
        let window = web_sys::window().ok_or_else(|| JsError::new("no window"))?;
        let dpr = window.device_pixel_ratio();
        let css_w = f64::from(canvas.client_width().max(1));
        let css_h = f64::from(canvas.client_height().max(1));
        let width = (css_w * dpr) as u32;
        let height = (css_h * dpr) as u32;
        canvas.set_width(width);
        canvas.set_height(height);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsError::new(&format!("create_surface: {e}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| JsError::new(&format!("request_adapter: {e}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("solarxy-web device"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| JsError::new(&format!("request_device: {e}")))?;

        // Chrome exposes only non-sRGB surface formats; render into an
        // sRGB view of the surface texture so the tone-mapped composite
        // output is gamma-encoded correctly (the phase-0 finding).
        let caps = surface.get_capabilities(&adapter);
        let base_format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let render_format = base_format.add_srgb_suffix();
        let view_formats = if render_format == base_format {
            vec![]
        } else {
            vec![render_format]
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: base_format,
            width,
            height,
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // The renderer builds its pipelines against the sRGB view format.
        let render_config = wgpu::SurfaceConfiguration {
            format: render_format,
            view_formats: vec![],
            ..config.clone()
        };
        let background = BackgroundMode::GRADIENT.resolve(&[]);
        let (sky_top, sky_bottom) = background.sky_colors();
        let init = RendererInit {
            msaa_sample_count: MSAA_SAMPLES,
            gradient_top: [0.35, 0.41, 0.47, 1.0],
            gradient_bottom: [0.66, 0.70, 0.72, 1.0],
            sky_top,
            sky_bottom,
            wireframe_color: background.wireframe_color(),
            // Only the seed for the renderer's first frame: every pane's real
            // weight arrives through `PaneDisplaySettings::line_weight` (see
            // the per-pane `width_px()` reads below). Taken from the shared
            // default rather than naming a variant, because this hardcoded
            // `Medium` silently disagreed with the desktop's persisted
            // `Light` default, so the same scene drew different wireframes in
            // the two shells out of the box.
            wireframe_line_width: solarxy_core::preferences::LineWeight::default().width_px(),
            bloom_enabled: false,
            ssao_enabled: false,
            tone_mode: ToneMode::AcesFilmic,
            exposure: 1.0,
            ibl_mode: IblMode::Full,
            uv_checker_png: &uv_checker_png,
        };
        let renderer = Renderer::new(&device, &queue, &render_config, &init)
            .map_err(|e| JsError::new(&format!("Renderer::new: {e}")))?;

        let bounds = default_bounds();
        let vis = VisualizationState::new_from_parts(
            &device,
            &renderer.layouts,
            &bounds,
            &[],
            None,
            background.grid_color(),
        );
        let mut env = SceneEnvironment::new(
            &device,
            &queue,
            &renderer.layouts,
            &bounds,
            width as f32 / height.max(1) as f32,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
            SHADOW_MAP_SIZE,
            vis,
        );
        env.light_bind_group = create_light_bind_group(
            &device,
            &renderer.layouts,
            &env.light_buffer,
            &renderer.ibl_res.ibl,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
        );

        let mut engine = Engine::new().map_err(|e| JsError::new(&format!("engine: {e}")))?;
        engine.set_clock(web_now);
        // Imports run off the main thread: cooks yield a ParseModel job the
        // frontend pumps to the import worker (`take_import_jobs` ->
        // `submit_parsed_model`), rather than parsing inline.
        engine.set_async_jobs(true);

        log(&format!(
            "solarxy-web: booted ({width}x{height}, {} node types, full renderer)",
            engine.registry().len()
        ));

        Ok(SolarxyApp {
            instance,
            surface,
            device,
            queue,
            config,
            render_format,
            renderer,
            preview: None,
            scene_objects: SceneObjects::new(),
            env,
            env_bounds: bounds,
            view: WebViewState {
                display: default_display_settings(),
                active_pane: 0,
                cameras_linked: false,
                cameras: [None, None, None, None],
                pane_settings: [default_pane_settings(); 4],
                look_through: [None; 4],
                camera_locked: [false; 4],
                camera_editing: [false; 4],
            },
            engine,
            host_events: Vec::new(),
            player_mode: false,
            last_pane_rects: Vec::new(),
            dpr: dpr as f32,
            pointer_buttons_down: 0,
            selected_object: None,
            pending_validate: Vec::new(),
            pending_image: Vec::new(),
            current_ctx: GraphContext::Root,
            uv_scene: SceneObjects::new(),
            uv_use_preview: false,
            last_uv_source: None,
            last_overlap: (None, false),
            last_pointer: (0.0, 0.0),
            hdri: None,
            screenshot_request: None,
            turntable_request: None,
            pending_screenshot: None,
            viz_dirty: true,
            attr_viz: AttrVizState::default(),
            attr_dirty: false,
            display_defaults: DisplayDefaults::default(),
            gizmo: GizmoState::default(),
            gizmo_readout: None,
        })
    }

    /// Applies one command, returning the `EventBatch` for the mirror.
    pub fn dispatch(&mut self, cmd: JsValue) -> Result<JsValue, JsError> {
        let command: Command = serde_wasm_bindgen::from_value(cmd)
            .map_err(|e| JsError::new(&format!("bad command: {e}")))?;
        let batch = self
            .engine
            .apply(command)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        to_js(&batch)
    }

    /// A transient param preview during a drag: no event, no undo, but it
    /// dirties the node so the next `frame` previews it. `ctx`/`value` are
    /// the same serde shapes as inside a `Command`.
    pub fn preview_param(
        &mut self,
        ctx: JsValue,
        node: f64,
        key: &str,
        value: JsValue,
    ) -> Result<(), JsError> {
        let ctx = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let value = serde_wasm_bindgen::from_value(value)
            .map_err(|e| JsError::new(&format!("bad value: {e}")))?;
        self.engine.preview_param(
            ctx,
            solarxy_graph::document::NodeId(node as u64),
            key,
            value,
        );
        Ok(())
    }

    /// Cooks under a frame budget, applies the scene delta, renders every
    /// pane, and returns the cook `EventBatch` (status + stats).
    pub fn frame(&mut self, dt_ms: f64) -> Result<JsValue, JsError> {
        // The clock advances BEFORE the cook, so this frame's geometry is
        // this frame's time. Fixed step (one tick is one frame), so a heavy
        // scene plays slowly rather than skipping and `$T` stays exactly
        // `frame / fps`. A stopped clock returns immediately.
        let mut events = self.engine.tick().events;

        // Cook the dirty set under a wall-clock budget.
        let deadline = web_now() + COOK_BUDGET_MS;
        events.extend(self.engine.cook(&mut || web_now() < deadline));

        // Apply the fresh scene delta to the multi-object scene.
        let delta = self.engine.take_scene_delta();
        if !delta.ops.is_empty() {
            self.viz_dirty = true;
            self.attr_dirty = true;
            if let Err(e) =
                self.scene_objects
                    .apply(&self.device, &self.queue, &self.renderer.layouts, &delta)
            {
                log(&format!("scene delta apply failed: {e}"));
            }
        }

        // The manipulator is pull-based: recompute what it should be, every
        // frame, from the engine's own view of the world. A selection change or
        // an undo therefore moves or removes it with no extra plumbing.
        // `view_dir` and `scale` are per-pane, so they are placeholders here:
        // `Renderer::write_manipulator` overwrites both before each pane's pass.
        let manip = if self.player_mode {
            // A published scene has nothing to manipulate.
            None
        } else {
            self.engine
                .gizmo_target(self.current_ctx)
                .and_then(|target| {
                    self.gizmo
                        .manipulator(&target, cgmath::Vector3::unit_z(), 1.0)
                })
        };
        self.renderer.set_manipulator(manip);

        self.sync_env_bounds();
        self.sync_visualization();
        self.sync_attr_channels();
        self.sync_uv_preview();
        self.ensure_pane_cameras();
        self.follow_look_through_cameras();
        // `f64::clamp` RETURNS NaN for a NaN input (every comparison with NaN is
        // false), so the clamp alone is not a guard. A non-finite delta reaching
        // a camera transition integrates straight into eye/target, and the next
        // projection panics inside cgmath with a NaN far plane. Callers are
        // supposed to pass a real frame delta; treat anything else as one frame.
        let dt_ms = if dt_ms.is_finite() { dt_ms } else { 16.0 };
        let dt = (dt_ms / 1000.0).clamp(0.0, 0.1) as f32;
        // Live turntable spin: a constant angular velocity on each pane
        // whose toggle is on. rpm is the global display setting; the spin is
        // session-temporary (reset on load) and drives the pane's scratch camera.
        let rpm = self.view.display.turntable_rpm;
        if rpm.abs() > 1e-6 {
            let yaw = rpm * std::f32::consts::TAU / 60.0 * dt;
            for i in 0..self.view.cameras.len() {
                if self.view.pane_settings[i].turntable_active
                    && let Some(cam) = self.view.cameras[i].as_mut()
                {
                    cam.inject_orbit_yaw(yaw);
                }
            }
        }
        for cam in self.view.cameras.iter_mut().flatten() {
            cam.update(&self.queue, dt);
        }
        self.update_lights();
        self.sync_render_target_dims();

        // Render every pane into the surface.
        let output = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return to_js(&EventBatch {
                    revision: self.engine.revision(),
                    events,
                });
            }
            Err(e) => return Err(JsError::new(&format!("acquire: {e}"))),
        };
        let surface_view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
        });

        let pane_rects = self.compute_panes();
        let is_split = pane_rects.len() > 1;
        for (i, pane) in pane_rects.iter().enumerate() {
            self.render_pane(i, *pane, &surface_view, is_split);
        }
        output.present();

        // Pump the overlap readback and mirror its progress to React.
        self.renderer.uv_overlap.poll_readback(&self.device);
        let overlap = (
            self.renderer.uv_overlap.overlap_pct,
            self.renderer.uv_overlap.readback_pending,
        );
        if overlap != self.last_overlap {
            self.last_overlap = overlap;
            self.host_events.push(HostEvent::UvOverlap {
                pct: overlap.0,
                pending: overlap.1,
            });
        }

        self.push_pane_rects_if_changed(&pane_rects);

        // A requested screenshot renders offscreen at capture resolution
        // after the on-screen frame (the next frame's target sync restores
        // the layout dimensions).
        if let Some(opts) = self.screenshot_request.take() {
            self.render_screenshot(&opts);
        }
        if let Some((pane, azimuth, opts)) = self.turntable_request.take() {
            self.render_turntable_frame(pane, azimuth, &opts);
        }

        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Resizes the surface and render targets. `dpr` is the LIVE device pixel
    /// ratio: it is not constant for the session (browser zoom and a move to a
    /// different-density monitor both change it), and every pointer coordinate,
    /// pane rect, and marker projection is scaled by it, so the shell re-reads
    /// it on each resize and pushes it through here.
    pub fn resize(&mut self, width: u32, height: u32, dpr: f32) {
        if width == 0 || height == 0 {
            return;
        }
        if dpr > 0.0 {
            self.dpr = dpr;
            // Label px metrics scale by dpr; keep them honest across
            // browser-zoom and monitor-density changes.
            self.renderer.write_label_dpr(&self.queue, dpr);
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.sync_render_target_dims();
        let aspect = self.renderer.target_width as f32 / self.renderer.target_height.max(1) as f32;
        for cam in self.view.cameras.iter_mut().flatten() {
            cam.resize(aspect);
        }
    }

    // ---- pointer routing (CSS px in; pane-aware) ----

    /// Selects the active viewport tool ("select" | "move" | "rotate" | "scale").
    /// The only JS surface the gizmo needs: the drag itself never crosses the
    /// boundary.
    pub fn set_tool(&mut self, tool: &str) -> Result<JsValue, JsError> {
        let next = ToolMode::parse(tool);
        if next == self.gizmo.tool {
            return Ok(JsValue::NULL);
        }
        // Switching tools mid-drag abandons the drag. Rolling it back rather
        // than dropping it is what keeps the preview lane from stranding a value
        // the document never agreed to.
        let rollback = match self.gizmo.drag.take() {
            Some(drag) => self.rollback_gizmo_drag(&drag)?,
            None => JsValue::NULL,
        };
        self.gizmo.tool = next;
        self.gizmo.hovered = None;
        self.gizmo_readout = None;
        Ok(rollback)
    }

    /// The gizmo's drag ergonomics, pushed from the TS prefs store.
    ///
    /// Pushed rather than polled because the drag loop never crosses back into
    /// JS: it would have to ask once per pointer move, which is exactly the
    /// traffic this design exists to avoid.
    pub fn set_gizmo_settings(
        &mut self,
        orientation: &str,
        snap_translate: f32,
        snap_rotate: f32,
        snap_scale: f32,
    ) {
        self.gizmo.settings = gizmo::GizmoSettings {
            orientation: gizmo::Orientation::parse(orientation),
            snap_translate: snap_translate.max(0.0),
            snap_rotate: snap_rotate.max(0.0),
            snap_scale: snap_scale.max(0.0),
        };
    }

    /// Enters (or leaves) player mode.
    ///
    /// Locks the layout to a single pane and clears any lingering editing
    /// affordance. The clock is NOT started here: whether a published scene
    /// autoplays is the document's `autoplay` setting, read by the player
    /// shell, so the same flag means the same thing whether the scene was
    /// exported or opened in the editor.
    pub fn set_player_mode(&mut self, on: bool) {
        self.player_mode = on;
        if on {
            self.view.display.layout = ViewLayout::Single;
            self.view.active_pane = 0;
            self.selected_object = None;
            self.gizmo_readout = None;
            self.renderer.set_manipulator(None);
            self.host_events.push(HostEvent::ViewChanged);
        }
    }

    /// The clock's current frame, polled by the player's transport readout.
    ///
    /// Polled rather than pushed for the same reason the gizmo readout is:
    /// under a playing clock a pushed value would cross the wasm boundary
    /// once per frame to update a number nobody is reading that closely.
    #[must_use]
    pub fn clock_frame(&self) -> f64 {
        self.engine.clock().frame as f64
    }

    /// Whether the clock is running right now.
    ///
    /// Polled beside [`SolarxyApp::clock_frame`] rather than tracked by the
    /// caller: a `once` range CLEARS `playing` when it reaches the end, so a
    /// shell holding its own boolean shows "Pause" over a clock that stopped
    /// by itself.
    #[must_use]
    pub fn clock_playing(&self) -> bool {
        self.engine.clock().playing
    }

    /// Whether a published scene should start playing, from the document's
    /// own runtime settings. The player shell reads this after load rather
    /// than the editor acting on it: autoplay in an authoring tool is a
    /// surprise, and in a viewer it is the point.
    #[must_use]
    pub fn autoplay(&self) -> bool {
        self.engine.clock().autoplay
    }

    /// The display defaults, pushed from the TS prefs store (the
    /// gizmo-settings pattern). The turntable rpm and the point size apply
    /// immediately: both are live session state that never serializes into
    /// `.slxy`. Wireframe weight and background are stored as the pane seed
    /// and, per `apply_*` flag, written into every pane: the boot push sets
    /// both flags; a mid-session preference save sets only the flags for
    /// fields that actually changed, so per-pane Display-menu overrides
    /// survive unrelated preference edits.
    pub fn set_display_defaults(
        &mut self,
        wireframe_weight: &str,
        background: &str,
        turntable_rpm: f32,
        point_size: f32,
        apply_wireframe: bool,
        apply_background: bool,
    ) {
        self.display_defaults = DisplayDefaults {
            line_weight: display_defaults::parse_line_weight(wireframe_weight),
            background: display_defaults::parse_background(background),
        };
        self.view.display.turntable_rpm = if turntable_rpm.is_finite() {
            turntable_rpm.clamp(1.0, 60.0)
        } else {
            6.0
        };
        self.view.display.point_size = if point_size.is_finite() {
            point_size.clamp(
                solarxy_core::view_config::MIN_POINT_SIZE,
                solarxy_core::view_config::MAX_POINT_SIZE,
            )
        } else {
            solarxy_core::view_config::DEFAULT_POINT_SIZE
        };
        let mut changed = false;
        for pds in &mut self.view.pane_settings {
            if apply_wireframe && pds.line_weight != self.display_defaults.line_weight {
                pds.line_weight = self.display_defaults.line_weight;
                changed = true;
            }
            if apply_background && pds.background_mode != self.display_defaults.background {
                pds.background_mode = self.display_defaults.background;
                changed = true;
            }
        }
        if changed {
            self.host_events.push(HostEvent::ViewChanged);
        }
    }

    /// The live drag readout ("X +1.250 m"), or `null` when nothing is dragging.
    ///
    /// POLLED once per frame, not pushed: `pointer_move` stays void so the hot
    /// path keeps costing zero boundary crossings, and the frame loop is already
    /// crossing anyway for the cook.
    #[must_use]
    pub fn gizmo_readout(&self) -> Option<String> {
        self.gizmo_readout.clone()
    }

    /// Pointer button down. `button`: 0 left, 1 middle, 2 right.
    ///
    /// Returns an `EventBatch` when the press STARTED a gizmo drag that mutated
    /// the document (the append path mints a transform node), else `null`.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: u32) -> Result<JsValue, JsError> {
        let p = (x * self.dpr, y * self.dpr);
        if self.pointer_buttons_down == 0 {
            self.set_hovered_pane(panes::hit_test_pane(&self.compute_panes(), p));
        }
        self.pointer_buttons_down |= 1 << button;

        // The gizmo gets first refusal on a LEFT press, and only on a left press:
        // middle and right always reach the camera, so orbit and pan can never be
        // stolen by a tool. In Select mode this whole branch is skipped and the
        // behaviour is bit-for-bit what it was.
        if button == 0
            && self.gizmo.tool.manipulates()
            && let Some(batch) = self.begin_gizmo_drag(p)?
        {
            return Ok(batch);
        }

        let active = self.view.active_pane;
        if let Some(cam) = self.view.cameras[active].as_mut() {
            cam.handle_mouse_move(p.0, p.1);
            if let Some(btn) = map_button(button) {
                cam.handle_mouse_button(btn, true);
            }
        }
        // On a locked look-through pane, this drag reframes the bound camera, so
        // suppress the node-to-pane follow for the duration of the gesture.
        if self.is_locked_look_through(active) {
            self.view.camera_editing[active] = true;
        }
        Ok(JsValue::NULL)
    }

    /// Pointer move; updates the hovered (active) pane while no drag is in
    /// flight, and feeds the active pane's camera controller.
    ///
    /// Deliberately returns nothing: this is the hot path, and a live gizmo drag
    /// streams straight into the engine's preview lane without crossing into JS
    /// at all.
    pub fn pointer_move(&mut self, x: f32, y: f32, mods: u8) {
        let p = (x * self.dpr, y * self.dpr);
        let last = std::mem::replace(&mut self.last_pointer, p);
        if self.pointer_buttons_down == 0 {
            self.set_hovered_pane(panes::hit_test_pane(&self.compute_panes(), p));
        }

        // A live drag owns the pointer entirely.
        if self.gizmo.drag.is_some() {
            self.update_gizmo_drag(p, mods);
            return;
        }
        // Otherwise, with a tool armed, keep the hover highlight fresh.
        if self.gizmo.tool.manipulates() && self.pointer_buttons_down == 0 {
            self.update_gizmo_hover(p);
        }

        let active = self.view.active_pane;
        if self.view.pane_settings[active].pane_mode == PaneMode::UvMap {
            // Drag pans the UV view (screen px scaled by the visible UV
            // span; the camera half-height is 0.6 / zoom).
            if self.pointer_buttons_down != 0 {
                let rects = self.compute_panes();
                let height = rects.get(active).map_or(1.0, |r| r.height.max(1.0));
                let pds = &mut self.view.pane_settings[active];
                let uv_per_px = (1.2 / pds.uv_zoom) / height;
                pds.uv_offset[0] -= (p.0 - last.0) * uv_per_px;
                pds.uv_offset[1] -= (p.1 - last.1) * uv_per_px;
                self.host_events.push(HostEvent::ViewChanged);
            }
            return;
        }
        if let Some(cam) = self.view.cameras[active].as_mut() {
            cam.handle_mouse_move(p.0, p.1);
        }
    }

    /// Updates the pointer-hovered active pane, mirroring the change to
    /// the frontend as a host event (the keyboard context reads the
    /// mirror, so it must track pointer routing).
    fn set_hovered_pane(&mut self, pane: usize) {
        if pane != self.view.active_pane {
            self.view.active_pane = pane;
            self.host_events.push(HostEvent::ActivePane { pane });
        }
    }

    /// Pointer button up. Returns the commit `EventBatch` when it ended a gizmo
    /// drag, else `null`.
    pub fn pointer_up(&mut self, button: u32) -> Result<JsValue, JsError> {
        self.pointer_buttons_down &= !(1 << button);

        if button == 0 && self.gizmo.drag.is_some() {
            return self.commit_gizmo_drag();
        }

        let active = self.view.active_pane;
        if let Some(cam) = self.view.cameras[active].as_mut()
            && let Some(btn) = map_button(button)
        {
            cam.handle_mouse_button(btn, false);
        }
        // A locked look-through reframe ends when the last button lifts: commit
        // the new camera pose to the node (one undo step) and let the follow
        // resume. The batch flows back so the parameter panel reflects the pose.
        if self.pointer_buttons_down == 0 && self.view.camera_editing[active] {
            self.view.camera_editing[active] = false;
            if let Some(batch) = self.commit_pane_camera_to_node(active) {
                return to_js(&batch);
            }
        }
        Ok(JsValue::NULL)
    }

    /// Escape during a drag: the document returns to where the drag started and
    /// the object snaps back. Returns the rollback `EventBatch`, or `null` when
    /// no drag was in flight.
    pub fn cancel_gizmo_drag(&mut self) -> Result<JsValue, JsError> {
        let Some(drag) = self.gizmo.drag.take() else {
            return Ok(JsValue::NULL);
        };
        self.rollback_gizmo_drag(&drag)
    }

    /// Wheel zoom on the active pane; positive zooms in.
    pub fn wheel(&mut self, delta: f32) {
        let active = self.view.active_pane;
        if self.view.pane_settings[active].pane_mode == PaneMode::UvMap {
            let pds = &mut self.view.pane_settings[active];
            pds.uv_zoom = (pds.uv_zoom * (1.0 + delta * 0.1)).clamp(0.1, 50.0);
            self.host_events.push(HostEvent::ViewChanged);
            return;
        }
        if let Some(cam) = self.view.cameras[active].as_mut() {
            cam.handle_scroll(delta);
        }
        // A dolly on a locked look-through pane reframes the bound camera. Wheel
        // has no return channel; committing bumps the revision, and the frame
        // loop's next batch carries it so the mirror self-heals (resnapshot on
        // the gap), keeping the node params in step with the pose.
        if self.is_locked_look_through(active) {
            let _ = self.commit_pane_camera_to_node(active);
        }
    }

    /// Picks the geo node under a canvas CSS pixel, pane-aware: the ray is
    /// built from the pane under the cursor with that pane's camera.
    /// Returns the node id as a number, or `undefined` on a miss.
    pub fn pick(&self, x: f32, y: f32) -> Option<f64> {
        if self.player_mode {
            return None;
        }
        let p = (x * self.dpr, y * self.dpr);
        let rects = self.compute_panes();
        let pane_idx = panes::hit_test_pane(&rects, p);
        let pane = rects.get(pane_idx)?;
        let mut cam = self.view.cameras[pane_idx].as_ref()?.camera;
        cam.aspect = pane.width / pane.height.max(1.0);
        let ray = screen_to_world_ray(
            (p.0 - pane.x, p.1 - pane.y),
            (pane.width, pane.height),
            cam.build_view_projection_matrix(),
        );
        let origin = [ray.origin.x, ray.origin.y, ray.origin.z];
        let dir = [ray.direction.x, ray.direction.y, ray.direction.z];
        self.engine.pick(origin, dir).map(|n| n.0 as f64)
    }

    /// [`SolarxyApp::pick`] with the full hit detail (mesh, face,
    /// barycentric, world point, pane): the anchor source for creating and
    /// re-placing review annotations. `undefined` on a miss.
    pub fn pick_detailed(&self, x: f32, y: f32) -> Result<JsValue, JsError> {
        let p = (x * self.dpr, y * self.dpr);
        let rects = self.compute_panes();
        let pane_idx = panes::hit_test_pane(&rects, p);
        let detail = rects.get(pane_idx).and_then(|pane| {
            let mut cam = self.view.cameras[pane_idx].as_ref()?.camera;
            cam.aspect = pane.width / pane.height.max(1.0);
            let ray = screen_to_world_ray(
                (p.0 - pane.x, p.1 - pane.y),
                (pane.width, pane.height),
                cam.build_view_projection_matrix(),
            );
            let origin = [ray.origin.x, ray.origin.y, ray.origin.z];
            let dir = [ray.direction.x, ray.direction.y, ray.direction.z];
            self.engine
                .pick_detailed(origin, dir)
                .map(|d| PickDetailDto {
                    node: d.node.0 as f64,
                    mesh: d.mesh,
                    face: d.face,
                    barycentric: d.barycentric,
                    world_pos: d.world_pos,
                    distance: d.distance,
                    pane: pane_idx,
                })
        });
        to_js(&detail)
    }

    /// The annotation set with runtime staleness (the review store's
    /// structure channel): re-read by the frontend on every `reviewChanged`
    /// event. Positions are the separate per-frame channel
    /// ([`SolarxyApp::review_markers`]).
    pub fn review_annotations(&self) -> Result<JsValue, JsError> {
        let annotations: Vec<solarxy_graph::engine::AnnotationSnapshot> = self
            .engine
            .document()
            .review()
            .iter()
            .map(|a| solarxy_graph::engine::AnnotationSnapshot {
                needs_reanchor: self.engine.annotation_stale(a.id),
                annotation: a.clone(),
            })
            .collect();
        to_js(&annotations)
    }

    /// The lane inventory of a node's cooked geometry (names, types,
    /// counts, both domains), or `null` while nothing is committed. Feeds
    /// the attribute-name pickers and the Attributes pane header; values
    /// page separately through [`SolarxyApp::attribute_table`].
    pub fn attribute_summary(&self, node: f64) -> Result<JsValue, JsError> {
        to_js(&self.engine.attribute_summary(NodeId(node as u64)))
    }

    /// The last completed cook's warnings for one node (a plain string
    /// array; empty when the cook was quiet). Pull-read by the node info
    /// card when it opens or the node's cook status changes.
    pub fn cook_warnings(&self, node: f64) -> Result<JsValue, JsError> {
        to_js(&self.engine.cook_warnings(NodeId(node as u64)))
    }

    /// One param's current value as the panel should display it, or the
    /// message explaining why it has none.
    ///
    /// Pull-read per row rather than pushed: under playback a per-cook
    /// resolved value would be one event per expression per frame across
    /// this boundary.
    pub fn resolved_param(&self, ctx: JsValue, node: f64, key: String) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        match self.engine.resolved_param(ctx, NodeId(node as u64), &key) {
            Ok(value) => to_js(&ResolvedParamDto {
                ok: true,
                value: Some(value),
                error: None,
            }),
            Err(error) => to_js(&ResolvedParamDto {
                ok: false,
                value: None,
                error: Some(error),
            }),
        }
    }

    /// One window of a node's cooked attribute values
    /// (`domain` is `"point"` or `"primitive"`). Only the requested page
    /// crosses the boundary; the geometry stays in wasm.
    pub fn attribute_table(
        &self,
        node: f64,
        domain: String,
        offset: u32,
        limit: u32,
    ) -> Result<JsValue, JsError> {
        let domain = match domain.as_str() {
            "primitive" => solarxy_kernel::AttributeDomain::Primitive,
            _ => solarxy_kernel::AttributeDomain::Point,
        };
        to_js(
            &self
                .engine
                .attribute_page(NodeId(node as u64), domain, offset, limit),
        )
    }

    /// Replaces the host-owned attribute-visualization state (the right
    /// strip's toggles and picked lane). Session-only: never saved, never
    /// in undo. Returns the full view state, the mutator convention.
    pub fn set_attr_viz(&mut self, state: JsValue) -> Result<JsValue, JsError> {
        let next: AttrVizState = serde_wasm_bindgen::from_value(state)
            .map_err(|e| JsError::new(&format!("attrViz: {e}")))?;
        if next != self.attr_viz {
            self.attr_viz = next;
            self.attr_dirty = true;
        }
        to_js(&self.view_state_dto())
    }

    /// Marker pin positions in PANE-RELATIVE CSS pixels (the DOM overlay
    /// clips one absolutely-positioned box per pane, so pins offset from
    /// their pane's origin), one entry per visible (marker x 3D pane) pair,
    /// resolved through each pane's camera (the desktop projection: clip ->
    /// NDC -> pane pixel, small NDC slack). Called once per animation frame
    /// by the host loop and applied to the DOM imperatively; markers absent
    /// from the list are hidden. UV panes carry no markers.
    pub fn review_markers(&self) -> Result<JsValue, JsError> {
        let mut out: Vec<MarkerScreenDto> = Vec::new();
        if self.player_mode {
            // Review notes are an authoring conversation, not part of the
            // published scene.
            return to_js(&out);
        }
        let markers = self.engine.review_markers_world();
        if markers.is_empty() {
            return to_js(&out);
        }
        let rects = self.compute_panes();
        for (i, pane) in rects.iter().enumerate() {
            if self.view.pane_settings[i].pane_mode == PaneMode::UvMap || pane.height <= 0.0 {
                continue;
            }
            let Some(cam_state) = self.view.cameras[i].as_ref() else {
                continue;
            };
            let mut cam = cam_state.camera;
            cam.aspect = pane.width / pane.height.max(1.0);
            let vp = cam.build_view_projection_matrix();
            for m in &markers {
                let Some(world) = m.world else { continue };
                let clip = vp * cgmath::Vector4::new(world[0], world[1], world[2], 1.0);
                if clip.w <= 0.0 {
                    continue;
                }
                let ndc = (clip.x / clip.w, clip.y / clip.w, clip.z / clip.w);
                // Same culls as the attribute pins; the z range is what
                // rejects behind-camera markers under orthographic
                // projection (clip.w is a constant 1 there).
                if ndc.0.abs() > NDC_XY_SLACK
                    || ndc.1.abs() > NDC_XY_SLACK
                    || !(NDC_Z_MIN..=NDC_Z_MAX).contains(&ndc.2)
                {
                    continue;
                }
                out.push(MarkerScreenDto {
                    id: m.id.0 as f64,
                    pane: i,
                    x: (ndc.0 + 1.0) * 0.5 * pane.width / self.dpr,
                    y: (1.0 - ndc.1) * 0.5 * pane.height / self.dpr,
                });
            }
        }
        to_js(&out)
    }

    /// Requests a screenshot of the active pane, rendered offscreen at the
    /// given resolution at the end of the current frame. One capture at a
    /// time; poll with [`SolarxyApp::poll_screenshot`].
    pub fn request_screenshot(&mut self, opts: JsValue) -> Result<(), JsError> {
        // The capture resizes the shared MSAA HDR chain to capture
        // resolution for one frame, so VRAM cost is ~4x the pixel count.
        // Empirically (Chrome/Apple GPU) captures in the 7-8M px range
        // lose the device NONDETERMINISTICALLY, and web wgpu has no
        // device-loss recovery yet, so the budget stays far below the
        // failure zone: a modest supersample of typical panes. True
        // hi-res needs tiled off-axis capture (logged follow-up); the
        // result reports the effective size either way.
        const MAX_CAPTURE_PIXELS: u64 = 4_000_000;
        if self.screenshot_request.is_some() || self.pending_screenshot.is_some() {
            return Err(JsError::new("a screenshot is already in flight"));
        }
        let mut opts: ScreenshotOptsDto = serde_wasm_bindgen::from_value(opts)
            .map_err(|e| JsError::new(&format!("bad opts: {e}")))?;
        let max = self.device.limits().max_texture_dimension_2d;
        opts.width = opts.width.clamp(16, max);
        opts.height = opts.height.clamp(16, max);
        let pixels = u64::from(opts.width) * u64::from(opts.height);
        if pixels > MAX_CAPTURE_PIXELS {
            #[allow(clippy::cast_precision_loss)]
            let scale = ((MAX_CAPTURE_PIXELS as f64) / (pixels as f64)).sqrt();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                opts.width = ((f64::from(opts.width) * scale) as u32).max(16);
                opts.height = ((f64::from(opts.height) * scale) as u32).max(16);
            }
        }
        self.screenshot_request = Some(opts);
        Ok(())
    }

    /// Requests one turntable-export frame: pane `pane` rendered offscreen from
    /// its render-through camera rotated by `azimuth_deg`, at the given opts.
    /// Uses the same single capture slot as the screenshot; the frontend drives
    /// one azimuth at a time (poll with `poll_screenshot`). Deterministic: it
    /// renders a rotated clone, never disturbing the live view.
    pub fn request_turntable_frame(
        &mut self,
        pane: usize,
        azimuth_deg: f32,
        opts: JsValue,
    ) -> Result<(), JsError> {
        const MAX_CAPTURE_PIXELS: u64 = 4_000_000;
        if self.screenshot_request.is_some()
            || self.turntable_request.is_some()
            || self.pending_screenshot.is_some()
        {
            return Err(JsError::new("a capture is already in flight"));
        }
        let mut opts: ScreenshotOptsDto = serde_wasm_bindgen::from_value(opts)
            .map_err(|e| JsError::new(&format!("bad opts: {e}")))?;
        let max = self.device.limits().max_texture_dimension_2d;
        opts.width = opts.width.clamp(16, max);
        opts.height = opts.height.clamp(16, max);
        let pixels = u64::from(opts.width) * u64::from(opts.height);
        if pixels > MAX_CAPTURE_PIXELS {
            #[allow(clippy::cast_precision_loss)]
            let scale = ((MAX_CAPTURE_PIXELS as f64) / (pixels as f64)).sqrt();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                opts.width = ((f64::from(opts.width) * scale) as u32).max(16);
                opts.height = ((f64::from(opts.height) * scale) as u32).max(16);
            }
        }
        self.turntable_request = Some((pane.min(3), azimuth_deg, opts));
        Ok(())
    }

    /// Polls the in-flight capture. `undefined` while pending (or when no
    /// capture is in flight); on completion returns
    /// `{ width, height, pixels: Uint8Array }` (tightly-packed RGBA8).
    pub fn poll_screenshot(&mut self) -> Result<JsValue, JsError> {
        use solarxy_renderer::capture::CapturePoll;
        let Some(pending) = &self.pending_screenshot else {
            return Ok(JsValue::UNDEFINED);
        };
        match pending.poll(&self.device, self.render_format) {
            CapturePoll::Pending => Ok(JsValue::UNDEFINED),
            CapturePoll::Failed => {
                self.pending_screenshot = None;
                Err(JsError::new("screenshot readback failed"))
            }
            CapturePoll::Ready(pixels) => {
                let (width, height) = (pending.width, pending.height);
                self.pending_screenshot = None;
                let obj = js_sys::Object::new();
                let set = |k: &str, v: &JsValue| {
                    let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
                };
                set("width", &JsValue::from_f64(f64::from(width)));
                set("height", &JsValue::from_f64(f64::from(height)));
                set(
                    "pixels",
                    &JsValue::from(js_sys::Uint8Array::from(pixels.as_slice())),
                );
                Ok(obj.into())
            }
        }
    }

    /// The displayed image of a texture network, for the
    /// texture viewer pane: `{ width, height, pixels }` (RGBA8) or
    /// `undefined` when the network publishes nothing. The pixel copy is
    /// display-only and pull-based, so cooked images still never ride the
    /// event stream; the viewer fetches on cook changes.
    pub fn texture_preview(&self, owner: f64) -> JsValue {
        let Some(img) = self
            .engine
            .display_image(solarxy_graph::document::NodeId(owner as u64))
        else {
            return JsValue::UNDEFINED;
        };
        let obj = js_sys::Object::new();
        let set = |k: &str, v: &JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
        };
        set("width", &JsValue::from_f64(f64::from(img.width)));
        set("height", &JsValue::from_f64(f64::from(img.height)));
        set(
            "pixels",
            &JsValue::from(js_sys::Uint8ClampedArray::from(img.pixels.as_slice())),
        );
        obj.into()
    }

    /// Executes an export node's Action param: the engine
    /// encodes the committed output, and the returned
    /// `{ filename, mime, bytes }` goes to the frontend's save path (the
    /// File System Access flow `.slxy` already uses).
    pub fn invoke_action(&self, ctx: JsValue, node: f64, key: String) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let result = self
            .engine
            .invoke_action(ctx, NodeId(node as u64), &key)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let obj = js_sys::Object::new();
        let set = |k: &str, v: &JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
        };
        set("filename", &JsValue::from_str(&result.filename));
        set("mime", &JsValue::from_str(&result.mime));
        set(
            "bytes",
            &JsValue::from(js_sys::Uint8Array::from(result.bytes.as_slice())),
        );
        Ok(obj.into())
    }

    /// Renders the active pane offscreen at capture resolution and encodes
    /// the readback copy. The pane's display settings are copied with the
    /// requested overlay toggles applied; the composite always clears (a
    /// fresh texture has no prior pane to load).
    fn render_screenshot(&mut self, opts: &ScreenshotOptsDto) {
        let (w, h) = (opts.width, opts.height);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Screenshot Target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.render_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.set_target_dims(w, h);
        let pane_idx = self.view.active_pane;
        let full = PaneRect {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        };
        let mut pds = self.view.pane_settings[pane_idx];
        if !opts.overlays.grid {
            pds.show_grid = false;
        }
        if !opts.overlays.axes {
            pds.show_axis_gizmo = false;
            pds.show_local_axes = false;
        }
        if !opts.overlays.validation {
            pds.show_validation = false;
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Screenshot Encoder"),
            });
        let aspect = full.width / full.height.max(1.0);
        let cam_data = self.view.cameras[pane_idx].as_ref().map(|c| c.camera);
        let mut is_uv = false;
        let mut scene_present = true;
        if pds.pane_mode == PaneMode::UvMap {
            self.render_uv_map_pane(&mut encoder, aspect, &pds);
            is_uv = true;
        } else if let Some(cam_data) = cam_data {
            if let Some(cam) = self.view.cameras[pane_idx].as_mut() {
                cam.write_with_aspect(&self.queue, aspect);
            }
            self.write_3d_pane_uniforms(pane_idx, &pds);
            if pds.inspection_mode == InspectionMode::Overdraw {
                self.render_overdraw_pane(&mut encoder, pane_idx, full, false);
            } else {
                self.render_3d_passes(&mut encoder, pane_idx, &cam_data, &pds);
            }
        } else {
            self.renderer
                .render_empty_pass(&mut encoder, self.resolve_background(&pds));
            scene_present = false;
        }

        // Composite into the offscreen target: full-rect viewport, always
        // cleared (unlike the per-pane path, which clears only pane 0).
        let bloom = self.renderer.post.bloom_enabled && !is_uv && scene_present;
        let ssao = self.renderer.post.ssao_enabled && !is_uv && scene_present;
        self.renderer.post.composite.write_params(
            &self.queue,
            bloom,
            ssao,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
            pds.inspection_mode,
        );
        self.renderer.post.composite.render(
            &mut encoder,
            &self.renderer.pipelines,
            &view,
            ssao,
            &self.renderer.post.ssao,
            Some([full.x, full.y, full.width, full.height]),
            true,
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        // The readback copy rides its own submission after the composite.
        let mut copy_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Screenshot Copy Encoder"),
                });
        let (buffer, padded) = solarxy_renderer::capture::encode_capture(
            &self.device,
            &mut copy_encoder,
            &texture,
            (0, 0, w, h),
        );
        self.queue.submit(std::iter::once(copy_encoder.finish()));
        self.pending_screenshot = Some(solarxy_renderer::capture::PendingCapture::arm(
            buffer, padded, w, h,
        ));
    }

    /// The camera a pane renders through: its bound camera's saved `CameraDef`
    /// if any, else its scratch camera. The base pose for a turntable sweep.
    fn render_through_camera(&self, pane: usize) -> Option<Camera> {
        let scratch = self
            .view
            .cameras
            .get(pane)
            .and_then(|c| c.as_ref())
            .map(|c| c.camera)?;
        if let Some(node) = self.view.look_through.get(pane).copied().flatten()
            && let Some(def) = self
                .scene_objects
                .cameras()
                .and_then(|cams| cams.iter().find(|c| c.id == SceneObjectId(node.0)))
        {
            let mut cam = scratch;
            apply_camera_def(&mut cam, def);
            return Some(cam);
        }
        Some(scratch)
    }

    /// Renders one turntable frame: the render-through camera rotated by
    /// `azimuth_deg`, offscreen at capture resolution, into the capture slot.
    /// The pane's live camera is swapped in and restored within this call, so a
    /// deterministic sweep never depends on or disturbs the live view / follow.
    fn render_turntable_frame(&mut self, pane: usize, azimuth_deg: f32, opts: &ScreenshotOptsDto) {
        let Some(mut cam) = self.render_through_camera(pane) else {
            return;
        };
        orbit_camera_yaw(&mut cam, azimuth_deg.to_radians());
        let saved = self.view.cameras[pane].as_ref().map(|c| c.camera);
        if let Some(cs) = self.view.cameras[pane].as_mut() {
            cs.camera = cam;
        }
        let prev_active = self.view.active_pane;
        self.view.active_pane = pane;
        self.render_screenshot(opts);
        self.view.active_pane = prev_active;
        if let (Some(saved), Some(cs)) = (saved, self.view.cameras[pane].as_mut()) {
            cs.camera = saved;
        }
    }

    /// Mirrors the graph context the node canvas currently shows (the UV
    /// pane's selected-node source resolves against it).
    pub fn set_current_context(&mut self, ctx: JsValue) -> Result<(), JsError> {
        self.current_ctx = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        Ok(())
    }

    /// Marks the scene object produced by `node` as selected (viewport
    /// outline tint); `undefined`/null clears it.
    pub fn set_scene_selection(&mut self, node: Option<f64>) {
        self.selected_object = node.map(|n| SceneObjectId(n as u64));
    }

    /// Applies the selection-highlight preference: `style` is
    /// `"outline"`, `"tint"`, or `"none"`; color is linear RGBA; `width`
    /// is the rim width in pixels (clamped 1..16 renderer-side). The
    /// legacy tint reuses the same color at its fixed 0.35 alpha.
    pub fn set_selection_highlight(
        &mut self,
        style: String,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
        width: f32,
    ) {
        use solarxy_renderer::frame::SelectionStyle;
        let style = match style.as_str() {
            "tint" => SelectionStyle::Tint,
            "none" => SelectionStyle::None,
            _ => SelectionStyle::Outline,
        };
        self.renderer
            .set_selection_highlight(&self.queue, style, [r, g, b, a], width);
    }

    /// Pushes the attribute-label theme colors (linear RGB, converted from
    /// the CSS tokens frontend-side like the selection highlight): text,
    /// background chip, anchor dot. Called at boot and on theme change.
    #[allow(clippy::too_many_arguments)]
    pub fn set_label_colors(
        &mut self,
        text_r: f32,
        text_g: f32,
        text_b: f32,
        chip_r: f32,
        chip_g: f32,
        chip_b: f32,
        dot_r: f32,
        dot_g: f32,
        dot_b: f32,
    ) {
        let style = solarxy_renderer::labels::LabelStyle {
            text: [text_r, text_g, text_b],
            chip: [chip_r, chip_g, chip_b],
            dot: [dot_r, dot_g, dot_b],
            dpr: self.dpr,
        };
        self.renderer.write_label_style(&self.queue, &style);
    }

    // ---- view-state boundary (host-owned; React mirrors) ----

    /// The full view state for the mirror.
    pub fn view_state(&self) -> Result<JsValue, JsError> {
        to_js(&self.view_state_dto())
    }

    /// Sets the pane layout (F1-F5). `layout` is the camelCase name
    /// (`"single"`, `"splitVertical"`, ...). Returns the new view state.
    pub fn set_view_layout(&mut self, layout: JsValue) -> Result<JsValue, JsError> {
        let layout: ViewLayout = serde_wasm_bindgen::from_value(layout)
            .map_err(|e| JsError::new(&format!("bad layout: {e}")))?;
        self.view.display.layout = layout;
        if self.view.active_pane >= layout.pane_count() {
            self.view.active_pane = 0;
        }
        self.ensure_pane_cameras();
        self.view_state()
    }

    /// Sets the two-pane divider ratio (clamped 0.05-0.95).
    pub fn set_split_ratio(&mut self, ratio: f32) -> Result<JsValue, JsError> {
        self.view.display.split_ratio = DisplaySettings::clamp_split_ratio(ratio);
        self.view_state()
    }

    pub fn set_active_pane(&mut self, pane: usize) -> Result<JsValue, JsError> {
        if pane < self.view.display.layout.pane_count() {
            self.view.active_pane = pane;
        }
        self.view_state()
    }

    /// Binds pane `pane` to look through the `camera` node id, or clears to a
    /// free view when `camera` is negative / non-finite. Returns the view state.
    pub fn set_pane_camera(&mut self, pane: usize, camera: f64) -> Result<JsValue, JsError> {
        if pane < 4 {
            self.view.look_through[pane] = if camera.is_finite() && camera >= 0.0 {
                Some(NodeId(camera as u64))
            } else {
                None
            };
            if self.view.look_through[pane].is_none() {
                self.view.camera_locked[pane] = false;
            }
            self.view.camera_editing[pane] = false;
        }
        self.view_state()
    }

    /// Toggles lock-camera-to-view for a look-through pane (Blender semantics:
    /// navigation reframes the bound camera). No effect on a free view.
    pub fn set_pane_camera_lock(&mut self, pane: usize, locked: bool) -> Result<JsValue, JsError> {
        if pane < 4 && self.view.look_through[pane].is_some() {
            self.view.camera_locked[pane] = locked;
            self.view.camera_editing[pane] = false;
        }
        self.view_state()
    }

    /// Jumps a pane's (free) view to a camera node's saved pose without binding
    /// or locking it (the bookmark action). Returns the view state.
    pub fn jump_to_camera(&mut self, pane: usize, camera: f64) -> Result<JsValue, JsError> {
        if pane < 4 && camera.is_finite() && camera >= 0.0 {
            let id = SceneObjectId(camera as u64);
            let def = self
                .scene_objects
                .cameras()
                .and_then(|cams| cams.iter().find(|c| c.id == id).cloned());
            if let (Some(def), Some(cam)) = (def, self.view.cameras[pane].as_mut()) {
                apply_camera_def(&mut cam.camera, &def);
            }
        }
        self.view_state()
    }

    /// The current pose (eye + target) of a pane's camera, so the frontend can
    /// author a new `camera` node framed on the current view (create-from-view
    /// is a frontend-orchestrated `AddNode` + `SetParam`, keeping the
    /// mirror-and-command model intact).
    pub fn pane_camera_pose(&self, pane: usize) -> Result<JsValue, JsError> {
        let (position, target) = self.view.cameras.get(pane).and_then(|c| c.as_ref()).map_or(
            ([7.0, 5.0, 7.0], [0.0, 0.0, 0.0]),
            |c| {
                (
                    [c.camera.eye.x, c.camera.eye.y, c.camera.eye.z],
                    [c.camera.target.x, c.camera.target.y, c.camera.target.z],
                )
            },
        );
        to_js(&CameraPoseDto { position, target })
    }

    /// Flies the active pane's camera to frame the mesh a validation issue
    /// lives on (report-panel row click; the desktop Properties fly-to) and
    /// enables that pane's validation overlay so the defect is visible.
    /// `object` is the owning geo node's id (= scene object id); `source`
    /// the node whose report the panel is showing (its engine-cached
    /// result is authoritative, which may differ from the object's
    /// effective overlay validation); `issue` the row index. Returns the
    /// view state.
    pub fn fly_to_issue(
        &mut self,
        object: f64,
        source: f64,
        issue: usize,
    ) -> Result<JsValue, JsError> {
        let id = SceneObjectId(object as u64);
        let source = solarxy_graph::document::NodeId(source as u64);
        let aabb = self.engine.validation(source).and_then(|v| {
            let issue = v.report.issues.get(issue)?;
            let obj = self.scene_objects.get(id)?;
            let raw_to_gpu = self.scene_objects.raw_to_gpu(id)?;
            solarxy_renderer::validation::resolve_issue_aabb(&issue.scope, &obj.model, raw_to_gpu)
        });
        if let Some(aabb) = aabb {
            let pane = self.view.active_pane;
            if let Some(settings) = self.view.pane_settings.get_mut(pane) {
                settings.show_validation = true;
            }
            if let Some(cam) = self.view.cameras.get_mut(pane).and_then(|c| c.as_mut()) {
                cam.reset_to_bounds(&aabb);
            }
        }
        self.view_state()
    }

    /// Replaces one pane's display settings with the full settings object.
    pub fn set_pane_settings(
        &mut self,
        pane: usize,
        settings: JsValue,
    ) -> Result<JsValue, JsError> {
        let settings: PaneDisplaySettings = serde_wasm_bindgen::from_value(settings)
            .map_err(|e| JsError::new(&format!("bad pane settings: {e}")))?;
        if let Some(slot) = self.view.pane_settings.get_mut(pane) {
            // Turning the overlap display on arms a fresh statistic run
            // (the desktop `O`-toggle behavior).
            if settings.show_uv_overlap && !slot.show_uv_overlap {
                self.renderer.uv_overlap.overlap_pct = None;
                self.renderer.uv_overlap.stats_dirty = true;
            }
            // A newly enabled normals/bounds overlay may need the (lazily
            // built) visualization aggregate.
            if settings.normals_mode != slot.normals_mode
                || settings.bounds_mode != slot.bounds_mode
            {
                self.viz_dirty = true;
            }
            *slot = settings;
        }
        self.view_state()
    }

    /// Replaces the global display settings (layout, split, turntable,
    /// lights lock, material scales, HDRI rotation).
    pub fn set_display_settings(&mut self, settings: JsValue) -> Result<JsValue, JsError> {
        let settings: DisplaySettings = serde_wasm_bindgen::from_value(settings)
            .map_err(|e| JsError::new(&format!("bad display settings: {e}")))?;
        self.view.display = settings;
        self.ensure_pane_cameras();
        self.view_state()
    }

    /// A camera command on a pane: `{kind:"fit"}`, `{kind:"view",
    /// axis:"top"|"bottom"|"front"|"back"|"left"|"right"}`, or
    /// `{kind:"projection", mode:"perspective"|"orthographic"}`. Returns the
    /// refreshed [`ViewStateDto`] like every other view mutator -- a view
    /// preset flips the pane to orthographic, and without the mirror update
    /// the toolbar's Persp/Ortho label kept showing the stale mode.
    pub fn camera_command(&mut self, pane: usize, cmd: JsValue) -> Result<JsValue, JsError> {
        let cmd: CameraCommandDto = serde_wasm_bindgen::from_value(cmd)
            .map_err(|e| JsError::new(&format!("bad camera command: {e}")))?;
        let bounds = self.scene_bounds();
        let Some(cam) = self.view.cameras.get_mut(pane).and_then(|c| c.as_mut()) else {
            return self.view_state();
        };
        match cmd.kind.as_str() {
            "fit" => cam.reset_to_bounds(&bounds),
            "view" => {
                let (dir, up) = match cmd.axis.as_str() {
                    "top" => (Vector3::unit_y(), -Vector3::unit_z()),
                    "bottom" => (-Vector3::unit_y(), Vector3::unit_z()),
                    "front" => (Vector3::unit_z(), Vector3::unit_y()),
                    "back" => (-Vector3::unit_z(), Vector3::unit_y()),
                    "left" => (-Vector3::unit_x(), Vector3::unit_y()),
                    "right" => (Vector3::unit_x(), Vector3::unit_y()),
                    other => return Err(JsError::new(&format!("bad view axis: {other}"))),
                };
                cam.reset_to_bounds_axis(&bounds, dir, up);
            }
            "projection" => {
                let mode = if cmd.mode == "orthographic" {
                    ProjectionMode::Orthographic
                } else {
                    ProjectionMode::Perspective
                };
                cam.set_projection(mode);
            }
            other => return Err(JsError::new(&format!("bad camera command: {other}"))),
        }
        self.view_state()
    }

    /// The current pane rectangles in CSS pixels (DOM toolbar positioning).
    pub fn pane_rects(&self) -> Result<JsValue, JsError> {
        to_js(&self.pane_rects_css())
    }

    /// Drains queued host events (pane-rect changes, async results).
    pub fn take_host_events(&mut self) -> Result<JsValue, JsError> {
        let events = std::mem::take(&mut self.host_events);
        to_js(&events)
    }

    // ---- mirror / persistence boundary (unchanged surfaces) ----

    /// The full document mirror (recovery after desync / structural undo).
    pub fn snapshot(&self) -> Result<JsValue, JsError> {
        to_js(&self.engine.snapshot())
    }

    /// The static registry snapshot (fetched once; drives palette + panel).
    pub fn registry_snapshot(&self) -> Result<JsValue, JsError> {
        to_js(&self.engine.registry_snapshot())
    }

    /// Captures a clipboard fragment of the given nodes (the frontend
    /// serializes it to `application/x-solarxy-nodes`). `ctx` is a
    /// `GraphContext`; `ids` a number array.
    pub fn copy_nodes(&self, ctx: JsValue, ids: Vec<f64>) -> Result<JsValue, JsError> {
        let ctx = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let ids: Vec<solarxy_graph::document::NodeId> = ids
            .into_iter()
            .map(|n| solarxy_graph::document::NodeId(n as u64))
            .collect();
        to_js(&self.engine.copy_nodes(ctx, &ids))
    }

    /// The ids of currently stale (dirty) nodes, for manual-mode badges and
    /// the header stale count.
    pub fn stale_nodes(&self) -> Vec<f64> {
        self.engine
            .dirty_nodes()
            .into_iter()
            .map(|n| n.0 as f64)
            .collect()
    }

    /// The number of registered node types (a boot smoke check).
    pub fn node_type_count(&self) -> usize {
        self.engine.registry().len()
    }

    /// The number of rendered objects (a smoke check that cooked geometry
    /// reached the GPU).
    pub fn object_count(&self) -> usize {
        self.scene_objects.draw_objects().count()
    }

    // ---- asset staging + the import-worker pump ----

    /// Stages asset bytes into the engine, returning the content id (its
    /// SHA-256 hex) the import node's `file` param references. The `_sha256`
    /// the caller computed for its OPFS cache is not trusted; the engine
    /// recomputes and the returned id is authoritative.
    pub fn stage_asset(
        &mut self,
        name: String,
        mime: String,
        _sha256: String,
        bytes: Vec<u8>,
    ) -> String {
        self.engine.stage_asset(name, mime, bytes).0
    }

    /// The staged bytes for an asset id, as a `Uint8Array`, or `undefined`.
    /// Lets the frontend feed the worker after a scene load, when its own
    /// JS-side byte cache is cold.
    pub fn asset_bytes(&self, hash: String) -> Option<js_sys::Uint8Array> {
        self.engine
            .asset_bytes(&solarxy_graph::params::AssetId(hash))
            .map(js_sys::Uint8Array::from)
    }

    /// Every staged asset as `[{ hash, name }]`. The sidecar preflight
    /// diffs a model's referenced companions against it, so the check is
    /// authoritative across reloads and `.slxy` restores (a JS-side cache
    /// of staged names would go cold on both).
    pub fn asset_manifest(&self) -> Result<JsValue, JsError> {
        let manifest: Vec<AssetRefDto> = self
            .engine
            .asset_manifest()
            .iter()
            .map(|(h, n)| AssetRefDto {
                hash: h.clone(),
                name: n.clone(),
            })
            .collect();
        to_js(&manifest)
    }

    /// Drains the import jobs the last cook spawned into a JS array of
    /// `{ ctx, jobId, hash, name, format, options, sidecars }`. The frontend
    /// gathers each job's bytes, posts them to the import worker, and returns
    /// the result through `submit_parsed_model` / `submit_parse_error`.
    /// `ValidateGeometry` jobs drained alongside are stashed for
    /// [`SolarxyApp::take_validate_jobs`].
    pub fn take_import_jobs(&mut self) -> Result<JsValue, JsError> {
        let manifest = self.engine.asset_manifest();
        let mut payloads: Vec<ImportJobDto> = Vec::new();
        for (ctx, job, req) in self.engine.take_jobs() {
            let (asset, format, options) = match req {
                JobRequest::ParseModel {
                    asset,
                    format,
                    options,
                } => (asset, format, options),
                JobRequest::ValidateGeometry {
                    geometry,
                    config,
                    budget,
                } => {
                    // Pack the geometry once, at drain time; the frontend
                    // moves plain bytes to the worker.
                    let config_json = serde_json::to_string(&config)
                        .map_err(|e| JsError::new(&format!("serialize config: {e}")))?;
                    self.pending_validate.push(PendingValidateJob {
                        ctx,
                        job_id: job.0,
                        blob: transfer::pack(&geometry),
                        config_json,
                        budget,
                    });
                    continue;
                }
                JobRequest::DecodeImage { asset } => {
                    let name = manifest
                        .iter()
                        .find(|(h, _)| *h == asset.0)
                        .map_or_else(String::new, |(_, n)| n.clone());
                    self.pending_image.push(PendingImageJob {
                        ctx,
                        job_id: job.0,
                        hash: asset.0,
                        name,
                    });
                    continue;
                }
            };
            let name = manifest
                .iter()
                .find(|(h, _)| *h == asset.0)
                .map_or_else(String::new, |(_, n)| n.clone());
            // OBJ/glTF resolve companions (mtl, bin, textures) by name;
            // hand the worker the other staged files as candidate
            // sidecars. Self-contained STL/PLY need none.
            let sidecars = if matches!(format.as_str(), "obj" | "gltf" | "glb") {
                manifest
                    .iter()
                    .filter(|(h, _)| *h != asset.0)
                    .map(|(h, n)| AssetRefDto {
                        hash: h.clone(),
                        name: n.clone(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            payloads.push(ImportJobDto {
                ctx,
                job_id: job.0 as f64,
                hash: asset.0,
                name,
                format,
                options,
                sidecars,
            });
        }
        to_js(&payloads)
    }

    /// Drains the stashed geometry-validation jobs into a JS array of
    /// `{ ctx, jobId, blob, config, budget }` (`blob` is a `Uint8Array`
    /// transfer blob of the geometry; `config` a JSON `ValidationConfig`).
    /// The frontend posts each to the worker's `validate_geometry_job` and
    /// returns the result through `submit_validation_result` /
    /// `submit_validation_error`. Call after `take_import_jobs` (which
    /// performs the drain from the engine).
    pub fn take_validate_jobs(&mut self) -> Result<JsValue, JsError> {
        let out = js_sys::Array::new();
        for job in self.pending_validate.drain(..) {
            let o = js_sys::Object::new();
            let set = |key: &str, value: &JsValue| {
                js_sys::Reflect::set(&o, &JsValue::from_str(key), value)
                    .map_err(|_| JsError::new("take_validate_jobs: reflect set failed"))
                    .map(|_| ())
            };
            set("ctx", &to_js(&job.ctx)?)?;
            set("jobId", &JsValue::from_f64(job.job_id as f64))?;
            set("blob", &js_sys::Uint8Array::from(job.blob.as_slice()))?;
            set("config", &JsValue::from_str(&job.config_json))?;
            set(
                "budget",
                &job.budget
                    .map_or(JsValue::UNDEFINED, |b| JsValue::from_f64(f64::from(b))),
            )?;
            out.push(&o);
        }
        Ok(out.into())
    }

    /// Drains the stashed image-decode jobs into a JS array of
    /// `{ ctx, jobId, hash, name }`. The frontend pulls the encoded bytes
    /// by hash (`asset_bytes`), posts them to the worker's decode-image
    /// path (`createImageBitmap`), and returns the RGBA result through
    /// `submit_decoded_image` / `submit_image_error`. Call after
    /// `take_import_jobs` (which performs the drain from the engine).
    pub fn take_image_jobs(&mut self) -> Result<JsValue, JsError> {
        let out = js_sys::Array::new();
        for job in self.pending_image.drain(..) {
            let o = js_sys::Object::new();
            let set = |key: &str, value: &JsValue| {
                js_sys::Reflect::set(&o, &JsValue::from_str(key), value)
                    .map_err(|_| JsError::new("take_image_jobs: reflect set failed"))
                    .map(|_| ())
            };
            set("ctx", &to_js(&job.ctx)?)?;
            set("jobId", &JsValue::from_f64(job.job_id as f64))?;
            set("hash", &JsValue::from_str(&job.hash))?;
            set("name", &JsValue::from_str(&job.name))?;
            out.push(&o);
        }
        Ok(out.into())
    }

    /// Commits a worker-decoded image (raw RGBA8 plus dimensions) under
    /// the per-node generation guard, returning the cook `EventBatch`.
    /// The content hash is stamped here, Rust-side, so every producer
    /// (native decode, worker decode, transfer unpack) yields identical
    /// hashes for identical pixels.
    pub fn submit_decoded_image(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let expected = (width as usize) * (height as usize) * 4;
        if pixels.len() != expected {
            return Err(JsError::new(&format!(
                "decoded image is {} bytes, expected {expected} ({width}x{height} RGBA)",
                pixels.len()
            )));
        }
        let image = std::sync::Arc::new(solarxy_core::RawImageData::new(pixels, width, height));
        let events =
            self.engine
                .submit_job_result(ctx, JobId(job_id as u64), JobResult::Image(Ok(image)));
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Reports a worker image-decode failure: the `import_image` node badges
    /// the error while keep-last-good holds the previous image.
    pub fn submit_image_error(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        message: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Image(Err(message)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Commits a worker validation result (the JSON `ValidationResult` from
    /// `validate_geometry_job`) under the generation guard, returning the
    /// cook `EventBatch`.
    pub fn submit_validation_result(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        result_json: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let result: ValidationResult = serde_json::from_str(&result_json)
            .map_err(|e| JsError::new(&format!("bad validation result: {e}")))?;
        let events =
            self.engine
                .submit_job_result(ctx, JobId(job_id as u64), JobResult::Report(Ok(result)));
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Reports a worker validation failure for a job (the validate node
    /// badges the error, keep-last-good holds its previous outputs).
    pub fn submit_validation_error(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        message: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Report(Err(message)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Commits a worker-parsed model (the transfer blob from
    /// `parse_model_job`, plus its implicit load-validation JSON) under the
    /// per-node generation guard, returning the cook `EventBatch`. A
    /// superseded result is dropped inside the engine.
    pub fn submit_parsed_model(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        blob: Vec<u8>,
        validation_json: Option<String>,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let set =
            transfer::unpack(&blob).map_err(|e| JsError::new(&format!("bad model blob: {e}")))?;
        let validation: Option<ValidationResult> = match validation_json {
            Some(json) => Some(
                serde_json::from_str(&json)
                    .map_err(|e| JsError::new(&format!("bad validation payload: {e}")))?,
            ),
            None => None,
        };
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Model(Ok(ParsedModel { set, validation })),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Reports a worker parse failure for a job: the import node badges the
    /// error while keep-last-good holds the last valid geometry.
    pub fn submit_parse_error(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        message: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Model(Err(message)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    // ---- .slxy save / load ----

    /// Builds `.slxy` archive bytes from the current document, its referenced
    /// assets, and the host `extra` (generator, canvas viewports, meta). The
    /// full view state (layout, split, all four pane cameras + display
    /// settings) rides the sidecar.
    /// Installs a prepared HDRI environment: unpacks the worker's
    /// `prepare_hdri_job` blob, finishes the IBL on the GPU, points the
    /// skybox at the new equirect, and rebuilds the light bind group (the
    /// desktop `rebuild_light_bind_group` chokepoint). `hash`/`name`
    /// identify the staged HDRI asset for the `.slxy` environment section.
    pub fn set_environment_prepared(
        &mut self,
        hash: String,
        name: String,
        prepared: Vec<u8>,
    ) -> Result<(), JsError> {
        let prepared = solarxy_renderer::ibl::PreparedHdri::unpack(&prepared)
            .map_err(|e| JsError::new(&format!("bad prepared HDRI: {e}")))?;
        self.renderer.ibl_res.ibl =
            solarxy_renderer::ibl::IblState::from_prepared(&self.device, &self.queue, &prepared);
        self.hdri = Some(HdriMeta { hash, name });
        self.rebuild_light_bind_group();
        Ok(())
    }

    /// Clears the HDRI back to the procedural sky derived from the primary
    /// pane's background (the desktop clear-HDRI behavior).
    pub fn clear_environment(&mut self) {
        let (top, bottom) = self
            .resolve_background(&self.view.pane_settings[0])
            .sky_colors();
        self.renderer.ibl_res.ibl = solarxy_renderer::ibl::IblState::from_sky_colors(
            &self.device,
            &self.queue,
            top,
            bottom,
        );
        self.hdri = None;
        self.rebuild_light_bind_group();
    }

    /// Sets the IBL contribution mode (`"off"` / `"diffuse"` / `"full"`).
    pub fn set_ibl_mode(&mut self, mode: String) {
        self.renderer.ibl_res.ibl_mode = match mode.to_ascii_lowercase().as_str() {
            "off" => IblMode::Off,
            "diffuse" => IblMode::Diffuse,
            _ => IblMode::Full,
        };
        self.rebuild_light_bind_group();
    }

    /// The current environment as a DTO (`{ iblMode, hdriHash, hdriName }`)
    /// for the frontend panel.
    pub fn environment_state(&self) -> Result<JsValue, JsError> {
        to_js(&EnvironmentDto {
            ibl_mode: match self.renderer.ibl_res.ibl_mode {
                IblMode::Off => "off",
                IblMode::Diffuse => "diffuse",
                IblMode::Full => "full",
            }
            .to_string(),
            hdri_hash: self.hdri.as_ref().map(|h| h.hash.clone()),
            hdri_name: self.hdri.as_ref().map(|h| h.name.clone()),
        })
    }

    pub fn save_slxy(&self, extra: JsValue) -> Result<Vec<u8>, JsError> {
        let extra: SaveExtra = serde_wasm_bindgen::from_value(extra).unwrap_or_default();
        let mut sidecar = SceneSidecar {
            generator: if extra.generator.is_empty() {
                "solarxy-web".to_string()
            } else {
                extra.generator
            },
            ..SceneSidecar::default()
        };
        sidecar.view = self.view_json();
        sidecar.environment = self.environment_json();
        sidecar.canvas_viewports = extra
            .canvas_viewports
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();
        sidecar.meta = extra.meta.into();
        self.engine
            .save_slxy(&sidecar)
            .map_err(|e| JsError::new(&format!("save .slxy: {e}")))
    }

    /// Replaces the document from `.slxy` bytes: stages the embedded assets,
    /// applies the saved view state (layout, split, pane cameras + display
    /// settings), and returns `{ batch, warnings, canvasViewports, meta }`
    /// for the mirror and the frontend view state.
    pub fn load_slxy(&mut self, bytes: Vec<u8>) -> Result<JsValue, JsError> {
        let loaded = self
            .engine
            .load_slxy(&bytes)
            .map_err(|e| JsError::new(&format!("load .slxy: {e}")))?;
        self.apply_view_json(&loaded.sidecar.view);
        // The saved IBL mode applies immediately; the HDRI itself needs the
        // worker's CPU stages, so the frontend re-prepares it from the
        // restored asset bytes and calls `set_environment_prepared`.
        let env = &loaded.sidecar.environment;
        if !env.ibl_mode.is_empty() {
            self.set_ibl_mode(env.ibl_mode.clone());
        }
        if let Some(rotation) = env
            .background
            .get("hdriRotation")
            .and_then(serde_json::Value::as_f64)
        {
            self.view.display.hdri_rotation = rotation as f32;
        }
        self.hdri = None;
        let environment = EnvironmentDto {
            ibl_mode: env.ibl_mode.clone(),
            hdri_hash: env.hdri_asset.clone(),
            hdri_name: None,
        };
        let result = LoadResultDto {
            batch: loaded.batch,
            warnings: loaded.warnings,
            canvas_viewports: loaded
                .sidecar
                .canvas_viewports
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            meta: loaded.sidecar.meta.into(),
            environment,
        };
        to_js(&result)
    }
}

// The gizmo drag loop. Lives entirely in Rust: `pointer_move` runs at pointer
// rate, and a boundary crossing per move would be a waste.
impl SolarxyApp {
    /// The pane under a point, its camera, and a world ray through it. Exactly
    /// the recipe `pick` uses, so the gizmo grabs what the user sees.
    fn pane_ray(&self, p: (f32, f32)) -> Option<(usize, PaneRect, Camera, Ray)> {
        let rects = self.compute_panes();
        let idx = panes::hit_test_pane(&rects, p);
        let pane = *rects.get(idx)?;
        // A UV pane has no 3D scene to manipulate.
        if self.view.pane_settings[idx].pane_mode == PaneMode::UvMap {
            return None;
        }
        let mut cam = self.view.cameras[idx].as_ref()?.camera;
        cam.aspect = pane.width / pane.height.max(1.0);
        let ray = screen_to_world_ray(
            (p.0 - pane.x, p.1 - pane.y),
            (pane.width, pane.height),
            cam.build_view_projection_matrix(),
        );
        Some((idx, pane, cam, ray))
    }

    /// The manipulator as it stands under a given pointer position: the engine's
    /// target, scaled for that pane. `None` when no gizmo is showing there.
    fn manipulator_at(
        &self,
        p: (f32, f32),
    ) -> Option<(GizmoTarget, ManipulatorState, Camera, Ray, f32)> {
        let target = self.engine.gizmo_target(self.current_ctx)?;
        let (_, pane, cam, ray) = self.pane_ray(p)?;
        let mut state = self.gizmo.manipulator(&target, cam.forward(), 1.0)?;
        // CSS px, not physical: `GIZMO_PX` and `HIT_PX` are the sizes the USER
        // sees, and pane rects are physical. Divide by the dpr or the gizmo comes
        // out half-size on a retina display (it did).
        let world_per_px = cam.world_per_pixel(state.origin(), pane.height / self.dpr);
        state.scale = manipulator::GIZMO_PX * world_per_px;
        Some((target, state, cam, ray, world_per_px))
    }

    fn update_gizmo_hover(&mut self, p: (f32, f32)) {
        self.gizmo.hovered = self
            .manipulator_at(p)
            .and_then(|(_, state, _, ray, wpp)| gizmo::hit_test(&ray, &state, wpp));
    }

    /// Left press with a tool armed: grab a handle, if one is under the cursor.
    ///
    /// On the APPEND path this mints a transform node before the drag can preview
    /// anything, which is why it happens inside the drag's transaction: the node
    /// and the move then undo together, in one step.
    fn begin_gizmo_drag(&mut self, p: (f32, f32)) -> Result<Option<JsValue>, JsError> {
        let Some((target, state, _, ray, wpp)) = self.manipulator_at(p) else {
            return Ok(None);
        };
        let Some(handle) = gizmo::hit_test(&ray, &state, wpp) else {
            return Ok(None); // a miss falls through to the camera, as before
        };

        let mut events = Vec::new();
        let begin = self
            .engine
            .apply(Command::BeginTransaction {
                label: self.gizmo.tool.undo_label().to_string(),
            })
            .map_err(|e| JsError::new(&format!("{e}")))?;
        events.extend(begin.events);

        // Resolve the real target. On the reuse path this is a no-op that simply
        // reports the tail transform; on the append path it creates one.
        let mut target = target;
        if target.append_pending {
            let GraphContext::Subflow(geo) = target.ctx else {
                return Ok(None);
            };
            let batch = self
                .engine
                .apply(Command::EnsureTransformTarget { geo })
                .map_err(|e| JsError::new(&format!("{e}")))?;
            // The paired event is the ONLY channel carrying the id (the reuse
            // path emits no NodeAdded).
            let node = batch.events.iter().find_map(|ev| match ev {
                EngineEvent::TransformTargetReady { node, .. } => Some(*node),
                _ => None,
            });
            let Some(node) = node else {
                return Ok(None);
            };
            events.extend(batch.events);

            // Re-resolve against the node the engine just minted: a fresh
            // transform is at identity, and reading its real params beats
            // hand-patching the struct field by field (which is how the old code
            // did it, and how it would have quietly kept a stale rotate).
            let Some(fresh) = self.engine.gizmo_target(target.ctx) else {
                return Ok(None);
            };
            debug_assert_eq!(fresh.node, node, "the engine minted a different node");
            target = fresh;
        }

        let Some(drag) = gizmo::begin_drag(&ray, &state, target, handle) else {
            return Ok(None);
        };
        self.gizmo.drag = Some(drag);
        self.gizmo.hovered = Some(handle);

        let revision = self.engine.revision();
        Ok(Some(to_js(&EventBatch { revision, events })?))
    }

    /// The manipulator as the LIVE drag sees it: rebuilt at the drag's stored
    /// anchor, not at a freshly resolved target.
    ///
    /// That distinction is load-bearing. Re-resolving mid-drag would move the
    /// gizmo's own origin under the maths (the object is moving, after all), and
    /// the object would accelerate away from the cursor.
    fn drag_state(&self, drag: &gizmo::Drag, p: (f32, f32)) -> Option<(ManipulatorState, Ray)> {
        let (_, pane, cam, ray) = self.pane_ray(p)?;
        let tool = self.gizmo.tool.manipulator_tool()?;
        let mut state = self.gizmo.manipulator(&drag.target, cam.forward(), 1.0)?;
        state.tool = tool;
        state.active = Some(drag.handle);
        state.scale =
            manipulator::GIZMO_PX * cam.world_per_pixel(state.origin(), pane.height / self.dpr);
        Some((state, ray))
    }

    /// Pointer move during a drag: solve, and stream into the preview lane. No
    /// document write, no undo entry, no event, no JS traffic.
    fn update_gizmo_drag(&mut self, p: (f32, f32), mods: u8) {
        let Some(mut drag) = self.gizmo.drag else {
            return;
        };
        let Some((state, ray)) = self.drag_state(&drag, p) else {
            return;
        };

        let settings = self.gizmo.settings;
        let Some((value, wrap)) = gizmo::solve_drag(&ray, &state, &drag, &settings, mods) else {
            return; // degenerate view angle: hold still rather than jump
        };

        // The rotate solve accumulates across the +/- pi seam, so its wrap state
        // has to ride back onto the drag or a sweep past 180 degrees would snap
        // back the other way.
        if let Some((last_raw, turns)) = wrap
            && let gizmo::DragGrab::Rotate {
                axis, start_vec, ..
            } = drag.grab
        {
            drag.grab = gizmo::DragGrab::Rotate {
                axis,
                start_vec,
                last_raw,
                turns,
            };
        }
        self.gizmo.drag = Some(drag);
        self.gizmo_readout = value.readout(drag.start);

        self.engine.preview_param(
            drag.target.ctx,
            drag.target.node,
            drag.param.key(),
            value.to_param_source(),
        );
    }

    /// Release: commit the dragged value as ONE authoritative `SetParam` inside the
    /// open transaction, then close it. That is the whole "one undo step per
    /// drag" contract -- the `SetParam` also clears the preview.
    fn commit_gizmo_drag(&mut self) -> Result<JsValue, JsError> {
        let Some(drag) = self.gizmo.drag.take() else {
            return Ok(JsValue::NULL);
        };
        self.gizmo_readout = None;
        let mut events = Vec::new();

        // Whatever the preview lane last resolved to IS the final value. Asked
        // through the drag's own `DragParam`, so the commit cannot read a
        // different param than the drag wrote.
        let final_value = self
            .engine
            .gizmo_target(drag.target.ctx)
            .map_or(drag.start, |t| drag.param.read(&t));

        // A click on a handle that never moved is not an edit. Committing it
        // would push an undo step that visibly does nothing (and, on the append
        // path, would leave a transform node behind for a click). Roll it back
        // instead, which is exactly what Escape does.
        if !final_value.differs_from(drag.start) {
            return self.rollback_gizmo_drag(&drag);
        }

        let set = self
            .engine
            .apply(Command::SetParam {
                ctx: drag.target.ctx,
                node: drag.target.node,
                key: drag.param.key().to_string(),
                value: final_value.to_param_source(),
            })
            .map_err(|e| JsError::new(&format!("{e}")))?;
        events.extend(set.events);

        let end = self
            .engine
            .apply(Command::EndTransaction)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        events.extend(end.events);

        let revision = self.engine.revision();
        to_js(&EventBatch { revision, events })
    }

    /// Unwinds a drag without committing: the document returns to where the drag
    /// started and the object snaps back.
    ///
    /// Two halves, and BOTH are needed. The transaction rollback undoes the
    /// document (an appended transform node); clearing the preview releases the
    /// transient value the drag was streaming. Skip the second and the viewport
    /// would keep asserting the dragged pose forever, disagreeing with the
    /// parameter panel. The key comes from the drag's own `DragParam`, so a
    /// rotate cancel can never clear a translate.
    fn rollback_gizmo_drag(&mut self, drag: &gizmo::Drag) -> Result<JsValue, JsError> {
        self.gizmo_readout = None;
        self.engine
            .clear_preview(drag.target.ctx, drag.target.node, drag.param.key());
        let batch = self
            .engine
            .apply(Command::CancelTransaction)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        to_js(&batch)
    }
}

// Internal orchestration: the web port of the desktop per-pane render loop.
impl SolarxyApp {
    fn compute_panes(&self) -> Vec<PaneRect> {
        panes::compute_panes(
            self.view.display.layout,
            self.view.display.split_ratio,
            (0.0, 0.0),
            (self.config.width as f32, self.config.height as f32),
        )
    }

    fn pane_rects_css(&self) -> Vec<RectDto> {
        self.compute_panes()
            .iter()
            .map(|p| RectDto {
                x: p.x / self.dpr,
                y: p.y / self.dpr,
                width: p.width / self.dpr,
                height: p.height / self.dpr,
            })
            .collect()
    }

    fn push_pane_rects_if_changed(&mut self, _physical: &[PaneRect]) {
        let css = self.pane_rects_css();
        if css != self.last_pane_rects {
            self.last_pane_rects.clone_from(&css);
            self.host_events.push(HostEvent::PaneRects { rects: css });
        }
    }

    /// The desktop `rebuild_light_bind_group` chokepoint, ported: retargets
    /// the skybox at the active IBL's equirect, rebuilds the light bind
    /// group per the IBL mode (full / diffuse-only / fallback), and pushes
    /// the IBL-derived ambient average so clay modes update instantly.
    fn rebuild_light_bind_group(&mut self) {
        self.renderer.skybox_bind_group = self.renderer.ibl_res.ibl.equirect.as_ref().map(|eq| {
            solarxy_renderer::skybox::create_skybox_bind_group(
                &self.device,
                &self.renderer.layouts.skybox,
                eq,
            )
        });

        let ibl_avg = self.active_ibl().irradiance_average;
        self.env.light_bind_group = match self.renderer.ibl_res.ibl_mode {
            IblMode::Off => create_light_bind_group(
                &self.device,
                &self.renderer.layouts,
                &self.env.light_buffer,
                &self.renderer.ibl_res.ibl_fallback,
                &self.renderer.ibl_res.brdf_lut,
                &self.renderer.ibl_res.ltc,
            ),
            IblMode::Diffuse => solarxy_renderer::scene::create_light_bind_group_selective(
                &self.device,
                &self.renderer.layouts,
                &self.env.light_buffer,
                &self.renderer.ibl_res.ibl,
                &self.renderer.ibl_res.ibl_fallback,
                &self.renderer.ibl_res.brdf_lut,
                &self.renderer.ibl_res.ltc,
            ),
            IblMode::Full => create_light_bind_group(
                &self.device,
                &self.renderer.layouts,
                &self.env.light_buffer,
                &self.renderer.ibl_res.ibl,
                &self.renderer.ibl_res.brdf_lut,
                &self.renderer.ibl_res.ltc,
            ),
        };
        self.env.lights_uniform.ibl_avg_r = ibl_avg[0];
        self.env.lights_uniform.ibl_avg_g = ibl_avg[1];
        self.env.lights_uniform.ibl_avg_b = ibl_avg[2];
        self.queue.write_buffer(
            &self.env.light_buffer,
            0,
            bytemuck::bytes_of(&self.env.lights_uniform),
        );
    }

    /// The `.slxy` environment section from the host state. The scene-wide
    /// HDRI rotation rides the free-form `background` object (the global
    /// `DisplaySettings` is otherwise host-session state).
    fn environment_json(&self) -> solarxy_scenefile::EnvironmentJson {
        let mut background = std::collections::BTreeMap::new();
        background.insert(
            "hdriRotation".to_string(),
            serde_json::json!(self.view.display.hdri_rotation),
        );
        solarxy_scenefile::EnvironmentJson {
            ibl_mode: match self.renderer.ibl_res.ibl_mode {
                IblMode::Off => "off",
                IblMode::Diffuse => "diffuse",
                IblMode::Full => "full",
            }
            .to_string(),
            hdri_asset: self.hdri.as_ref().map(|h| h.hash.clone()),
            background,
        }
    }

    /// The scene's visible bounds, or the placeholder before anything cooks.
    fn scene_bounds(&self) -> AABB {
        self.scene_objects
            .visible_bounds()
            .unwrap_or(self.env_bounds)
    }

    fn active_ibl(&self) -> &solarxy_renderer::ibl::IblState {
        match self.renderer.ibl_res.ibl_mode {
            IblMode::Off => &self.renderer.ibl_res.ibl_fallback,
            IblMode::Diffuse | IblMode::Full => &self.renderer.ibl_res.ibl,
        }
    }

    /// Keeps the UV pane's source current: the selected node's committed
    /// geometry (uploaded into the one-object preview scene, deduped by
    /// `Arc` identity), else the selected / first scene object. A source
    /// change invalidates the overlap statistic.
    fn sync_uv_preview(&mut self) {
        if !self
            .view
            .pane_settings
            .iter()
            .any(|p| p.pane_mode == PaneMode::UvMap)
        {
            return;
        }
        let source = self
            .engine
            .selected_geometry(self.current_ctx)
            .map(|(node, set)| {
                (
                    node.0,
                    std::sync::Arc::as_ptr(set).cast::<()>() as usize,
                    std::sync::Arc::clone(set),
                )
            });
        self.uv_use_preview = source.is_some();
        let identity = match &source {
            Some((node, addr, _)) => Some((*node, *addr)),
            None => self
                .selected_object
                .or_else(|| self.scene_objects.iter().next().map(|(id, _)| *id))
                .map(|id| (id.0, 0)),
        };
        if identity != self.last_uv_source {
            self.last_uv_source = identity;
            if self.view.pane_settings.iter().any(|p| p.show_uv_overlap) {
                self.renderer.uv_overlap.overlap_pct = None;
                self.renderer.uv_overlap.stats_dirty = true;
            }
        }
        if let Some((_, _, set)) = source {
            let mut delta = SceneDelta::default();
            delta.push(SceneOp::UpsertGeometry {
                id: UV_PREVIEW_ID,
                geometry: std::sync::Arc::new(set.to_cooked()),
            });
            if let Err(e) =
                self.uv_scene
                    .apply(&self.device, &self.queue, &self.renderer.layouts, &delta)
            {
                log(&format!("uv preview upload failed: {e}"));
            }
        }
    }

    /// Renders one UV pane: the UV-space checker/wire pass, plus the
    /// overlap count pass and its one-shot stats readback when enabled
    /// (the desktop `render_uv_map_pane` recipe). A source without real
    /// UVs, or no source, renders the pane background only.
    fn render_uv_map_pane(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pane_aspect: f32,
        pds: &PaneDisplaySettings,
    ) {
        self.renderer
            .uv_cam
            .write(&self.queue, pds.uv_offset, pds.uv_zoom, pane_aspect);
        let uv_wire = WireframeParams {
            color: [0.8, 0.8, 0.8, 1.0],
            line_width: pds.line_weight.width_px(),
            screen_width: self.renderer.target_width as f32,
            screen_height: self.renderer.target_height as f32,
            // The UV pass draws no points, but this write clobbers the
            // shared uniform, so it carries the real size for the next 3D
            // pass rather than a zero that would make points vanish.
            point_size: self.view.display.point_size,
        };
        self.queue.write_buffer(
            &self.renderer.wire.wireframe_params_buffer,
            0,
            bytemuck::bytes_of(&uv_wire),
        );
        if pds.uv_bg == UvMapBackground::Dark {
            let dark = GradientUniform {
                top_color: [0.10, 0.10, 0.10, 1.0],
                bottom_color: [0.10, 0.10, 0.10, 1.0],
                uv_y_offset: 0.0,
                uv_y_scale: 1.0,
                _pad: [0.0; 2],
            };
            self.queue.write_buffer(
                &self.renderer.wire._gradient_buffer,
                0,
                bytemuck::bytes_of(&dark),
            );
        }
        let stats_needed = pds.show_uv_overlap
            && self.renderer.uv_overlap.stats_dirty
            && !self.renderer.uv_overlap.readback_pending;

        let bg = self.resolve_background(pds);
        let uv_object = if self.uv_use_preview {
            self.uv_scene.draw_object(UV_PREVIEW_ID)
        } else {
            self.selected_object
                .and_then(|id| self.scene_objects.draw_object(id))
                .or_else(|| self.scene_objects.draw_objects().next())
        };
        let Some(uv_object) = uv_object.filter(|o| o.model.has_uvs) else {
            self.renderer.render_empty_pass(encoder, bg);
            return;
        };

        if pds.show_uv_overlap {
            self.renderer.render_uv_overlap_count_pass(
                encoder,
                &uv_object,
                &self.renderer.uv_cam.bind_group,
                &self.renderer.uv_overlap.count_view,
            );
            if stats_needed {
                // One-shot statistics render at the identity UV camera,
                // then restore the pane view.
                self.renderer
                    .uv_cam
                    .write(&self.queue, [0.0, 0.0], 1.0, 1.0);
                self.renderer.render_uv_overlap_count_pass(
                    encoder,
                    &uv_object,
                    &self.renderer.uv_cam.bind_group,
                    &self.renderer.uv_overlap.stats_view,
                );
                self.renderer
                    .uv_overlap
                    .request_readback(&self.device, encoder);
                self.renderer
                    .uv_cam
                    .write(&self.queue, pds.uv_offset, pds.uv_zoom, pane_aspect);
            }
        }
        self.renderer.render_uv_map_pass(
            encoder,
            &uv_object,
            &self.renderer.uv_cam.bind_group,
            pds,
        );
    }

    fn resolve_background(&self, pds: &PaneDisplaySettings) -> ResolvedBackground {
        // The web has no user custom-background registry yet.
        pds.background_mode.resolve(&[])
    }

    /// Rebuilds the bounds-derived environment (grid/floor scale, shadow
    /// frustum, light-rig fit) when the scene bounds move materially.
    /// Whether any active-layout 3D pane wants the per-mesh visualization
    /// overlays (normal arrows, per-mesh bounds boxes).
    fn viz_overlays_wanted(&self) -> bool {
        use solarxy_core::preferences::{NormalsMode, PaneMode};
        use solarxy_core::view_config::BoundsMode;
        let count = self.view.display.layout.pane_count();
        self.view.pane_settings[..count].iter().any(|pds| {
            pds.pane_mode != PaneMode::UvMap
                && (pds.normals_mode != NormalsMode::Off || pds.bounds_mode == BoundsMode::PerMesh)
        })
    }

    /// Rebuilds `env.vis` from every displayed geometry when the aggregate
    /// is stale and a pane actually shows it: world-baked normal lines
    /// (positions via the object matrix, directions via its
    /// inverse-transpose) and per-mesh world AABBs, flattened in draw order
    /// (the renderer zips segments against the flattened scene meshes).
    /// Lights/shadow are untouched -- only the visualization member swaps.
    fn sync_visualization(&mut self) {
        if !self.viz_dirty || !self.viz_overlays_wanted() {
            return;
        }
        let Some(bounds) = self.scene_objects.visible_bounds() else {
            return;
        };
        self.viz_dirty = false;
        let (mesh_bounds, normals) = self.build_viz_aggregate();
        let grid_color = self
            .resolve_background(&self.view.pane_settings[0])
            .grid_color();
        self.env.vis = VisualizationState::new_from_parts(
            &self.device,
            &self.renderer.layouts,
            &bounds,
            &mesh_bounds,
            Some(&normals),
            grid_color,
        );
        // The rebuilt state's attr channel is empty; refill it.
        self.attr_dirty = true;
    }

    /// Rebuilds (or clears) BOTH attribute channels (vector lines and GPU
    /// labels) when they are stale, then reports the sampling facts. One
    /// consumer of `attr_dirty` by construction: splitting the channels
    /// over two consumers would starve whichever ran second. Independent of
    /// `sync_visualization`: the overlays draw whenever the strip enables
    /// them, with or without the normals/bounds overlays.
    fn sync_attr_channels(&mut self) {
        if !self.attr_dirty {
            return;
        }
        self.attr_dirty = false;

        if self.attr_viz.vectors && self.attr_viz.name.is_some() {
            let lines = self.build_attr_vector_lines();
            self.env.vis.set_attr_lines(&self.device, &lines);
        } else if self.env.vis.attr_lines_count > 0 {
            self.env.vis.set_attr_lines(&self.device, &[]);
        }

        let (capacity, total) = self.rebuild_attr_labels();
        self.host_events.push(HostEvent::AttrPinStats {
            capacity,
            total: total as f64,
        });
    }

    /// Rebuilds the GPU label set from a deterministic stride sample of
    /// every displayed geometry's points (all of them up to the budget):
    /// world-space anchors plus per-label glyph words, uploaded once here
    /// and projected in the vertex shader thereafter. Returns
    /// `(capacity, total displayed points)` for the sampling notice.
    fn rebuild_attr_labels(&mut self) -> (u32, usize) {
        use cgmath::{Matrix4, Transform};
        if !self.attr_viz.pins_wanted() {
            self.renderer
                .set_attr_labels(&self.device, &self.queue, &[], &[]);
            return (0, 0);
        }
        let lane = self
            .attr_viz
            .name
            .as_deref()
            .filter(|_| self.attr_viz.labels);
        let geos = self.engine.display_geometries();
        let total: usize = geos
            .iter()
            .flat_map(|(_, set, _)| set.meshes.iter())
            .map(solarxy_kernel::KernelMesh::vertex_count)
            .sum();
        if total == 0 {
            self.renderer
                .set_attr_labels(&self.device, &self.queue, &[], &[]);
            return (0, 0);
        }
        let cap = self.attr_viz.effective_cap(total);
        let stride = total.div_ceil(cap).max(1);

        let mut candidates: Vec<crate::attr_labels::LabelCandidate> = Vec::with_capacity(cap);
        let mut global = 0usize;
        for (_node, set, m) in &geos {
            let matrix = Matrix4::from(*m);
            let mut ptnum: u64 = 0;
            for mesh in &set.meshes {
                let len = mesh.vertex_count();
                let values =
                    lane.and_then(|n| solarxy_graph::engine::attr_table::resolve_lane(mesh, n));
                let first = global.next_multiple_of(stride);
                let mut g = first;
                while g < global + len {
                    let i = g - global;
                    let tp = matrix.transform_point(Point3::from(mesh.positions[i]));
                    candidates.push(crate::attr_labels::LabelCandidate {
                        world: [tp.x, tp.y, tp.z],
                        ptnum: ptnum + i as u64,
                        value: values.map(|l| l.components(i).unwrap_or_default()),
                    });
                    g += stride;
                }
                ptnum += len as u64;
                global += len;
            }
        }
        let (instances, words) = crate::attr_labels::build_labels(
            &candidates,
            self.attr_viz.labels,
            self.attr_viz.points,
        );
        self.renderer
            .set_attr_labels(&self.device, &self.queue, &instances, &words);
        (cap as u32, total)
    }

    /// World-space arrow segments for the picked point lane (vec3, or the
    /// xyz of vec4; map lane or the fixed `N` buffer), over
    /// every displayed geometry: positions through the object matrix,
    /// directions through the normal matrix for the reserved `N` lane
    /// (bivector semantics under nonuniform scale) and the plain linear
    /// part for everything else. Length is the bounds-derived factor
    /// times the strip's scale multiplier, over the value (or its unit
    /// direction under normalize); color is the uniform pick, or the
    /// cold-to-warm ramp over this frame's magnitude range.
    fn build_attr_vector_lines(&self) -> Vec<GizmoVertex> {
        use cgmath::{InnerSpace, Matrix3, Matrix4, SquareMatrix, Transform};
        let Some(name) = self.attr_viz.name.as_deref() else {
            return Vec::new();
        };
        let is_normal_lane = name == solarxy_kernel::reserved::NORMAL;
        let multiplier = self.attr_viz.scale_multiplier();
        let normalize = self.attr_viz.normalize;

        // First pass: world-space segments plus each arrow's magnitude
        // (pre-normalization), so the ramp can span the real range.
        let mut segments: Vec<([f32; 3], [f32; 3], f32)> = Vec::new();
        for (_node, set, m) in self.engine.display_geometries() {
            let matrix = Matrix4::from(m);
            let linear = Matrix3::from_cols(
                matrix.x.truncate(),
                matrix.y.truncate(),
                matrix.z.truncate(),
            );
            let dir_matrix = if is_normal_lane {
                linear
                    .invert()
                    .map_or(linear, |inv| cgmath::Matrix::transpose(&inv))
            } else {
                linear
            };
            let scale = {
                let d = set.bounds.diagonal();
                if d > 1e-10 { d * 0.05 } else { 0.1 }
            } * multiplier;
            for mesh in &set.meshes {
                // Vec3 and vec4 (xyz) lanes draw, map or fixed-buffer N;
                // float/vec2 lanes have no spatial reading and skip.
                let Some(lane) = solarxy_graph::engine::attr_table::resolve_lane(mesh, name) else {
                    continue;
                };
                for (i, p) in mesh.positions.iter().enumerate() {
                    let Some(v) = lane.direction(i) else { continue };
                    let tp = matrix.transform_point(Point3::from(*p));
                    let mut dir = dir_matrix * Vector3::from(v);
                    let magnitude = dir.magnitude();
                    if normalize {
                        if magnitude <= 1e-10 {
                            continue;
                        }
                        dir /= magnitude;
                    }
                    segments.push((
                        [tp.x, tp.y, tp.z],
                        [
                            tp.x + dir.x * scale,
                            tp.y + dir.y * scale,
                            tp.z + dir.z * scale,
                        ],
                        magnitude,
                    ));
                }
            }
        }

        // Second pass: colors. Flat per arrow (both vertices alike) so
        // direction stays readable under the ramp.
        let color_for: Box<dyn Fn(f32) -> [f32; 3]> = match self.attr_viz.color_mode {
            AttrColorMode::Uniform => {
                let c = self.attr_viz.color;
                Box::new(move |_| c)
            }
            AttrColorMode::Ramp => {
                let (min, max) = segments
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), (_, _, m)| {
                        (lo.min(*m), hi.max(*m))
                    });
                if max - min <= 1e-10 {
                    // A degenerate range has nothing to rank; fall back
                    // to the uniform color.
                    let c = self.attr_viz.color;
                    Box::new(move |_| c)
                } else {
                    let preset = self.attr_viz.ramp_preset;
                    Box::new(move |m: f32| {
                        let t = ((m - min) / (max - min)).clamp(0.0, 1.0);
                        ramp_color(preset, t)
                    })
                }
            }
        };
        segments
            .into_iter()
            .flat_map(|(a, b, magnitude)| {
                let color = color_for(magnitude);
                [
                    GizmoVertex { position: a, color },
                    GizmoVertex { position: b, color },
                ]
            })
            .collect()
    }

    /// The world-space visualization aggregate over
    /// `Engine::display_geometries` (ascending geo id = the renderer's
    /// draw order).
    fn build_viz_aggregate(&self) -> (Vec<AABB>, NormalsGeometry) {
        use cgmath::{Matrix3, Matrix4, SquareMatrix, Transform};
        let mut mesh_bounds: Vec<AABB> = Vec::new();
        let mut agg = NormalsGeometry {
            vertex_lines: Vec::new(),
            face_lines: Vec::new(),
            vertex_segments: Vec::new(),
            face_segments: Vec::new(),
        };
        for (_node, set, m) in self.engine.display_geometries() {
            let matrix = Matrix4::from(m);
            // Normal matrix: inverse-transpose of the upper 3x3 (the geo
            // transform allows nonuniform scale).
            let normal_matrix = Matrix3::from_cols(
                matrix.x.truncate(),
                matrix.y.truncate(),
                matrix.z.truncate(),
            )
            .invert()
            .map(|inv| cgmath::Matrix::transpose(&inv));
            for mesh in &set.meshes {
                let world: Vec<[f32; 3]> = mesh
                    .positions
                    .iter()
                    .map(|p| {
                        let tp = matrix.transform_point(Point3::from(*p));
                        [tp.x, tp.y, tp.z]
                    })
                    .collect();
                let bounds = compute_bounds(&world);
                let world_normals: Vec<[f32; 3]> = match (&mesh.normals, normal_matrix) {
                    (Some(ns), Some(nm)) => ns
                        .iter()
                        .map(|n| {
                            let v = nm * Vector3::from(*n);
                            let v = if v.magnitude2() > 1e-12 {
                                v.normalize()
                            } else {
                                v
                            };
                            [v.x, v.y, v.z]
                        })
                        .collect(),
                    // A singular matrix (zero scale) has no usable normal
                    // transform; fall back to object-space directions.
                    (Some(ns), None) => ns.to_vec(),
                    // No stored normals: vertex arrows are empty, face
                    // arrows still derive from the world positions.
                    (None, _) => Vec::new(),
                };
                let (v_lines, f_lines) =
                    build_normals_geometry(&world, &world_normals, &mesh.indices, &bounds);
                let v_start = agg.vertex_lines.len() as u32;
                agg.vertex_lines.extend(v_lines);
                agg.vertex_segments
                    .push(v_start..agg.vertex_lines.len() as u32);
                let f_start = agg.face_lines.len() as u32;
                agg.face_lines.extend(f_lines);
                agg.face_segments.push(f_start..agg.face_lines.len() as u32);
                mesh_bounds.push(bounds);
            }
        }
        (mesh_bounds, agg)
    }

    fn sync_env_bounds(&mut self) {
        // Keep the ground environment (grid, floor, shadow frustum) world-fixed
        // during interactive edits. A subflow gizmo drag writes a `transform`
        // node that BAKES into the cooked points, so the visible bounds churn
        // every frame; refitting here would rescale/slide the grid and floor
        // under the gizmo. Any interaction streams through the preview lane, so
        // we skip the refit while a preview is in flight and let it settle once
        // when the edit commits (the preview clears on the authoritative write).
        if self.engine.has_active_previews() {
            return;
        }
        // The same reasoning one guard up, for the scene clock. An animated
        // scene bakes new point positions every frame, so the visible bounds
        // churn continuously and refitting here would make the grid, floor
        // and shadow frustum breathe in time with the animation. The world is
        // not something playback is allowed to rescale. Playback stopping
        // needs no bookkeeping: `frame()` calls this unconditionally, so the
        // first non-playing tick settles the environment once.
        if self.engine.clock().playing {
            return;
        }
        let Some(bounds) = self.scene_objects.visible_bounds() else {
            return;
        };
        let eps = (self.env_bounds.diagonal() * 1e-3).max(1e-6);
        let close = |a: Point3<f32>, b: Point3<f32>| {
            (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps && (a.z - b.z).abs() < eps
        };
        if close(bounds.min, self.env_bounds.min) && close(bounds.max, self.env_bounds.max) {
            return;
        }
        let grid_color = self
            .resolve_background(&self.view.pane_settings[0])
            .grid_color();
        let vis = VisualizationState::new_from_parts(
            &self.device,
            &self.renderer.layouts,
            &bounds,
            &[],
            None,
            grid_color,
        );
        let aspect = self.renderer.target_width as f32 / self.renderer.target_height.max(1) as f32;
        let mut env = SceneEnvironment::new(
            &self.device,
            &self.queue,
            &self.renderer.layouts,
            &bounds,
            aspect,
            &self.renderer.ibl_res.brdf_lut,
            &self.renderer.ibl_res.ltc,
            SHADOW_MAP_SIZE,
            vis,
        );
        env.light_bind_group = create_light_bind_group(
            &self.device,
            &self.renderer.layouts,
            &env.light_buffer,
            self.active_ibl(),
            &self.renderer.ibl_res.brdf_lut,
            &self.renderer.ibl_res.ltc,
        );
        self.env = env;
        self.env_bounds = bounds;
        // The rebuilt environment starts with empty per-mesh viz data; the
        // aggregate refills it when a pane wants overlays (and the attr
        // channel refills on its own dirty pass).
        self.viz_dirty = true;
        self.attr_dirty = true;
    }

    /// Drives each look-through pane's camera from its bound `camera` node, so
    /// param-panel edits (and non-navigating panes) always show the node's
    /// saved pose. Suppressed for a pane mid-navigation (`camera_editing`) so
    /// the follow never fights live orbit/pan on a locked pane.
    fn follow_look_through_cameras(&mut self) {
        // Snapshot the cloned defs first, ending the scene_objects borrow before
        // the pane cameras are mutated.
        let updates: Vec<(usize, solarxy_core::scene::CameraDef)> = {
            let Some(cams) = self.scene_objects.cameras() else {
                return;
            };
            (0..4)
                .filter_map(|i| {
                    let node = self.view.look_through[i]?;
                    // Suppressed while navigating, or while a turntable spins
                    // this pane's scratch camera.
                    if self.view.camera_editing[i] || self.view.pane_settings[i].turntable_active {
                        return None;
                    }
                    cams.iter()
                        .find(|c| c.id == SceneObjectId(node.0))
                        .map(|def| (i, def.clone()))
                })
                .collect()
        };
        for (i, def) in updates {
            if let Some(cam) = self.view.cameras[i].as_mut() {
                apply_camera_def(&mut cam.camera, &def);
            }
        }
    }

    /// Whether pane `pane` is a locked look-through pane (navigation reframes
    /// its bound camera node).
    fn is_locked_look_through(&self, pane: usize) -> bool {
        pane < 4 && self.view.look_through[pane].is_some() && self.view.camera_locked[pane]
    }

    /// Writes a locked look-through pane's current camera pose back to its bound
    /// `camera` node as one undo step, returning the merged event batch so the
    /// frontend mirror reflects the new params (position + target).
    fn commit_pane_camera_to_node(&mut self, pane: usize) -> Option<EventBatch> {
        let node = self.view.look_through.get(pane).copied().flatten()?;
        let (eye, target) = {
            let cam = self.view.cameras[pane].as_ref()?;
            (cam.camera.eye, cam.camera.target)
        };
        let cmds = [
            Command::BeginTransaction {
                label: "Frame Camera".to_string(),
            },
            Command::SetParam {
                ctx: GraphContext::Root,
                node,
                key: "position".to_string(),
                value: ParamSource::Literal(ParamValue::Vec3([
                    f64::from(eye.x),
                    f64::from(eye.y),
                    f64::from(eye.z),
                ])),
            },
            Command::SetParam {
                ctx: GraphContext::Root,
                node,
                key: "target".to_string(),
                value: ParamSource::Literal(ParamValue::Vec3([
                    f64::from(target.x),
                    f64::from(target.y),
                    f64::from(target.z),
                ])),
            },
            Command::EndTransaction,
        ];
        let mut events = Vec::new();
        let mut revision = self.engine.revision();
        for cmd in cmds {
            if let Ok(batch) = self.engine.apply(cmd) {
                revision = batch.revision;
                events.extend(batch.events);
            }
        }
        Some(EventBatch { revision, events })
    }

    /// Uploads the camera gizmos for pane `i`, hiding the camera the pane is
    /// looking through. Cloned first so the `scene_objects` borrow ends before
    /// the mutable renderer write.
    fn write_pane_camera_helpers(&mut self, i: usize) {
        let skip = self.view.look_through[i].map(|n| SceneObjectId(n.0));
        let cams: Vec<solarxy_core::scene::CameraDef> = self
            .scene_objects
            .cameras()
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        self.renderer.write_camera_helpers(&self.queue, &cams, skip);
    }

    /// Lazily creates a `CameraState` for every pane slot the layout uses
    /// (the desktop recipe: slot 0 primary perspective; slots 1-3 cloned
    /// then reset to Top / Front / Left).
    fn ensure_pane_cameras(&mut self) {
        let bounds = self.scene_bounds();
        let count = self.view.display.layout.pane_count();
        let aspect = self.renderer.target_width as f32 / self.renderer.target_height.max(1) as f32;
        for i in 0..count {
            if self.view.cameras[i].is_some() {
                continue;
            }
            let mut cam = if i == 0 {
                CameraState::new(&self.device, &self.renderer.layouts.camera, &bounds, aspect)
            } else if let Some(src) = self.view.cameras[0].as_ref() {
                src.clone_with_new_resources(&self.device, &self.renderer.layouts.camera)
            } else {
                continue;
            };
            match i {
                0 => {}
                1 => cam.reset_to_bounds_axis(&bounds, Vector3::unit_y(), -Vector3::unit_z()),
                2 => cam.reset_to_bounds_axis(&bounds, Vector3::unit_z(), Vector3::unit_y()),
                _ => cam.reset_to_bounds_axis(&bounds, -Vector3::unit_x(), Vector3::unit_y()),
            }
            self.view.cameras[i] = Some(cam);
        }
    }

    /// Per-frame lighting: engine light nodes (root additive lights) drive
    /// the 8-light array when present; otherwise the synthesized viewer rig
    /// follows the primary camera (desktop parity).
    fn update_lights(&mut self) {
        let bounds = self.scene_bounds();
        let ibl_avg = self.active_ibl().irradiance_average;

        // The helpers ride the same light list the shading does, so a helper can
        // never describe a light the renderer is not actually using. Sized in
        // world units, so unlike the manipulator this is once per frame, not
        // once per pane.
        match self.scene_objects.lights() {
            Some(defs) => self.renderer.write_light_helpers(&self.queue, defs),
            None => self.renderer.write_light_helpers(&self.queue, &[]),
        }

        if let Some(defs) = self.scene_objects.lights() {
            self.env.lights_uniform =
                LightsUniform::from_defs(defs, bounds.diagonal() * 0.04, ibl_avg);
        } else if !self.view.display.lights_locked {
            let Some(cam0) = self.view.cameras[0].as_ref().map(|c| c.camera) else {
                return;
            };
            self.env.lights_uniform = lights_from_camera(&cam0, &bounds, ibl_avg);
        } else {
            return;
        }
        self.queue.write_buffer(
            &self.env.light_buffer,
            0,
            bytemuck::cast_slice(&[self.env.lights_uniform]),
        );
        // The shadow map follows THE flagged caster (the engine's
        // exclusive-caster rule guarantees at most one), not blindly the
        // first entry; the synthesized viewer rig keeps its key at entry 0
        // flagged, so its behavior is unchanged.
        let count =
            (self.env.lights_uniform.count as usize).min(self.env.lights_uniform.lights.len());
        let caster = self.env.lights_uniform.lights[..count]
            .iter()
            .position(|l| l.shadowed > 0.5)
            .unwrap_or(0);
        let key = self.env.lights_uniform.lights[caster].position;
        let key_pos = if key.iter().all(|c| c.abs() < f32::EPSILON) {
            // A positionless (directional) key: synthesize a shadow eye
            // along its direction outside the bounds.
            let d = self.env.lights_uniform.lights[caster].direction;
            bounds.center() - Vector3::new(d[0], d[1], d[2]) * bounds.diagonal()
        } else {
            Point3::new(key[0], key[1], key[2])
        };
        self.env.shadow.update_light_vp(
            &self.queue,
            key_pos,
            bounds.center(),
            bounds.diagonal() / 2.0,
        );
    }

    /// Resizes the shared HDR target (and derived buffers) to the largest
    /// pane of the current layout. Port of the desktop
    /// `resize_render_targets` + `sync_render_target_dims`.
    fn sync_render_target_dims(&mut self) {
        let (width, height) = panes::compute_target_dimensions(
            self.view.display.layout,
            self.config.width,
            self.config.height,
        );
        if width == 0 || height == 0 {
            return;
        }
        self.set_target_dims(width, height);
    }

    /// Resizes the shared render targets to exact dimensions (the layout
    /// sync above, and the screenshot path's capture-resolution render;
    /// restoration after a capture is the next frame's sync call).
    fn set_target_dims(&mut self, width: u32, height: u32) {
        if width == self.renderer.target_width && height == self.renderer.target_height {
            return;
        }
        self.renderer.target_width = width;
        self.renderer.target_height = height;
        self.renderer.targets.depth_texture = texture::Texture::create_depth_texture(
            &self.device,
            width,
            height,
            "depth_texture",
            self.renderer.msaa_sample_count,
        );
        self.renderer.targets.msaa_hdr_view = texture::create_msaa_hdr_texture(
            &self.device,
            width,
            height,
            self.renderer.msaa_sample_count,
        );
        let (hdr_tex, hdr_view) = texture::create_hdr_resolve_texture(&self.device, width, height);
        self.renderer.targets._hdr_resolve_texture = hdr_tex;
        self.renderer.targets.hdr_resolve_view = hdr_view;
        self.renderer.post.bloom.resize(
            &self.device,
            &self.renderer.layouts,
            &self.renderer.targets.hdr_resolve_view,
            width,
            height,
        );
        self.renderer.post.composite.resize(
            &self.device,
            &self.renderer.layouts,
            &self.renderer.targets.hdr_resolve_view,
            &self.renderer.post.bloom.ping_view,
            &self.renderer.post.bloom.sampler,
        );
        let (ct, cv) = texture::create_overlap_count_texture(&self.device, width, height, false);
        self.renderer.uv_overlap.count_texture = ct;
        self.renderer.uv_overlap.count_view = cv;
        self.renderer.uv_overlap.overlay_bind_group =
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("UV Overlap Overlay Bind Group"),
                layout: &self.renderer.layouts.uv_overlap_read,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &self.renderer.uv_overlap.count_view,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.renderer.uv_overlap.sampler),
                    },
                ],
            });
        self.renderer
            .post
            .ssao
            .resize(&self.device, &self.renderer.layouts, width, height);
        self.renderer
            .overdraw
            .resize(&self.device, &self.renderer.layouts, width, height);
        let layouts = std::sync::Arc::clone(&self.renderer.layouts);
        self.renderer
            .outline
            .resize(&self.device, &layouts, width, height);
    }

    /// Renders one pane: 3D passes (or overdraw / empty) into the shared
    /// HDR target, then composites into the pane's surface rect.
    fn render_pane(
        &mut self,
        i: usize,
        pane: PaneRect,
        surface_view: &wgpu::TextureView,
        is_split: bool,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Pane Encoder"),
            });
        let pane_aspect = pane.width / pane.height.max(1.0);
        let pds = self.view.pane_settings[i];
        let cam_data = self.view.cameras[i].as_ref().map(|c| c.camera);

        let Some(cam_data) = cam_data else {
            self.renderer
                .render_empty_pass(&mut encoder, self.resolve_background(&pds));
            self.composite_and_submit(encoder, surface_view, i, pane, false, false);
            return;
        };

        if pds.pane_mode == PaneMode::UvMap {
            self.render_uv_map_pane(&mut encoder, pane_aspect, &pds);
            self.composite_and_submit(encoder, surface_view, i, pane, true, true);
            return;
        }

        if let Some(cam) = self.view.cameras[i].as_mut() {
            cam.write_with_aspect(&self.queue, pane_aspect);
        }
        // The gizmo's world size is per-pane (a pane's camera and height decide
        // how many world units a pixel is), so it is re-written before each
        // pane's pass rather than once per frame.
        self.renderer
            .write_manipulator(&self.queue, &cam_data, pane.height / self.dpr);
        self.write_pane_camera_helpers(i);
        if is_split && i >= 1 {
            self.setup_pane_lighting(&cam_data);
        }
        self.write_3d_pane_uniforms(i, &pds);

        if pds.inspection_mode == InspectionMode::Overdraw {
            self.render_overdraw_pane(&mut encoder, i, pane, is_split);
        } else {
            self.render_3d_passes(&mut encoder, i, &cam_data, &pds);
        }
        self.composite_and_submit(encoder, surface_view, i, pane, false, true);
    }

    fn composite_and_submit(
        &self,
        mut encoder: wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        i: usize,
        pane: PaneRect,
        is_uv_map: bool,
        scene_present: bool,
    ) {
        let pane_bloom = self.renderer.post.bloom_enabled && !is_uv_map && scene_present;
        let pane_ssao = self.renderer.post.ssao_enabled && !is_uv_map && scene_present;
        let pane_inspection = self.view.pane_settings[i].inspection_mode;
        self.renderer.post.composite.write_params(
            &self.queue,
            pane_bloom,
            pane_ssao,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
            pane_inspection,
        );
        let viewport = Some([pane.x, pane.y, pane.width, pane.height]);
        self.renderer.post.composite.render(
            &mut encoder,
            &self.renderer.pipelines,
            surface_view,
            pane_ssao,
            &self.renderer.post.ssao,
            viewport,
            i == 0,
        );
        // The selection-outline rim lands after tone mapping,
        // so it never blooms and AO never darkens it.
        if scene_present
            && !is_uv_map
            && self.renderer.selection_style == solarxy_renderer::frame::SelectionStyle::Outline
            && self.selected_object.is_some()
            && self
                .selected_object
                .is_some_and(|id| self.scene_objects.draw_object(id).is_some())
            && self.view.pane_settings[i].inspection_mode != InspectionMode::Overdraw
        {
            self.renderer
                .composite_selection_outline(&mut encoder, surface_view, viewport);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    fn render_overdraw_pane(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        i: usize,
        pane: PaneRect,
        is_split: bool,
    ) {
        let Some(cam_bg) = self.view.cameras[i].as_ref().map(|c| &c.bind_group) else {
            return;
        };
        let pane_viewport = if is_split {
            Some([pane.x, pane.y, pane.width, pane.height])
        } else {
            None
        };
        let objects: Vec<DrawObject<'_>> = self.scene_objects.draw_objects().collect();
        self.renderer
            .render_overdraw_passes(encoder, &objects, cam_bg, pane_viewport);
    }

    fn render_3d_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        i: usize,
        cam_data: &Camera,
        pds: &PaneDisplaySettings,
    ) {
        let mut objects: Vec<DrawObject<'_>> = self.scene_objects.draw_objects().collect();
        // The picking-sync selection highlight: flag the
        // selected node's object so the main pass draws its accent tint.
        if let Some(id) = self.selected_object
            && let Some(selected) = self.scene_objects.draw_object(id)
        {
            for o in &mut objects {
                if std::ptr::eq(o.model, selected.model) {
                    o.selected = true;
                }
            }
        }

        if i == 0 || !self.view.display.lights_locked {
            self.renderer
                .render_shadow_pass(encoder, &self.env, &objects);
        }

        let Some(cam_bg) = self.view.cameras[i].as_ref().map(|c| &c.bind_group) else {
            self.renderer
                .render_empty_pass(encoder, self.resolve_background(pds));
            return;
        };

        if self.renderer.post.ssao_enabled {
            self.renderer.render_gbuffer_pass(encoder, &objects, cam_bg);
        }
        self.renderer.render_main_pass(
            encoder,
            &self.env,
            &objects,
            cam_bg,
            cam_data,
            pds,
            self.resolve_background(pds),
        );

        // Selection outline: the offscreen mask + jump-flood
        // stages run here; the rim blits onto the swapchain after the
        // composite pass (composite_and_submit reads has_selection).
        if self.renderer.selection_style == solarxy_renderer::frame::SelectionStyle::Outline
            && objects.iter().any(|o| o.selected)
        {
            self.renderer
                .render_selection_outline(encoder, &objects, cam_bg);
        }

        if self.renderer.post.ssao_enabled {
            self.renderer.render_ssao_passes(encoder, cam_bg);
        }
        if self.renderer.post.bloom_enabled {
            self.renderer.post.bloom.render(
                encoder,
                &self.renderer.pipelines,
                &self.queue,
                self.renderer.target_width,
                self.renderer.target_height,
            );
        }
    }

    /// Recomputes the camera-relative light rig for a non-primary pane
    /// (only meaningful for the synthesized viewer rig; engine light nodes
    /// are world-fixed).
    fn setup_pane_lighting(&mut self, cam_data: &Camera) {
        if self.view.display.lights_locked || self.scene_objects.lights().is_some() {
            return;
        }
        let bounds = self.scene_bounds();
        let ibl_avg = self.active_ibl().irradiance_average;
        self.env.lights_uniform = lights_from_camera(cam_data, &bounds, ibl_avg);
        self.queue.write_buffer(
            &self.env.light_buffer,
            0,
            bytemuck::cast_slice(&[self.env.lights_uniform]),
        );
        let key_pos = self.env.lights_uniform.lights[0].position;
        self.env.shadow.update_light_vp(
            &self.queue,
            Point3::new(key_pos[0], key_pos[1], key_pos[2]),
            bounds.center(),
            bounds.diagonal() / 2.0,
        );
    }

    /// Per-pane uniform writes ahead of the 3D passes: grid color,
    /// wireframe params, gradient colors, and the camera inspection block.
    fn write_3d_pane_uniforms(&self, i: usize, pds: &PaneDisplaySettings) {
        let background = self.resolve_background(pds);

        let wire = WireframeParams {
            color: background.wireframe_color(),
            line_width: pds.line_weight.width_px(),
            screen_width: self.renderer.target_width as f32,
            screen_height: self.renderer.target_height as f32,
            point_size: self.view.display.point_size,
        };
        self.queue.write_buffer(
            &self.renderer.wire.wireframe_params_buffer,
            0,
            bytemuck::bytes_of(&wire),
        );

        let (top, bottom) = background.sky_colors();
        let gradient = GradientUniform {
            top_color: [top[0], top[1], top[2], 1.0],
            bottom_color: [bottom[0], bottom[1], bottom[2], 1.0],
            uv_y_offset: 0.0,
            uv_y_scale: 1.0,
            _pad: [0.0; 2],
        };
        self.queue.write_buffer(
            &self.renderer.wire._gradient_buffer,
            0,
            bytemuck::bytes_of(&gradient),
        );

        let grid = background.grid_color();
        self.queue.write_buffer(
            &self.env.vis.grid_uniform_buf,
            solarxy_renderer::visualization::GridUniform::COLOR_OFFSET,
            bytemuck::cast_slice(&grid),
        );
        // The grid plane follows the pane camera: perspective keeps the XZ
        // ground; an orthographic axis elevation (front/side) gets a view-plane
        // grid so it is not seen edge-on. Keyed off the transition destination
        // so a view-preset animation switches plane once, at click time, not
        // partway through the lerp. Shared buffer, written per pane before
        // that pane's grid pass, exactly like the color above.
        let plane: u32 = self.view.cameras[i]
            .as_ref()
            .map_or(0, |c| grid_plane_for(&c.destination_camera()));
        self.queue.write_buffer(
            &self.env.vis.grid_uniform_buf,
            solarxy_renderer::visualization::GridUniform::PLANE_OFFSET,
            bytemuck::bytes_of(&plane),
        );

        if let Some(cam) = self.view.cameras[i].as_ref() {
            let (near, far) = compute_depth_bounds(&cam.camera, &self.scene_bounds());
            let data: [u32; 8] = [
                pds.inspection_mode.as_u32(),
                pds.texel_density_target.to_bits(),
                pds.material_override.as_u32(),
                near.to_bits(),
                far.to_bits(),
                self.view.display.roughness_scale.to_bits(),
                self.view.display.metallic_scale.to_bits(),
                self.view.display.hdri_rotation.to_bits(),
            ];
            self.queue.write_buffer(
                &cam.buffer,
                CameraUniform::INSPECTION_OFFSET,
                bytemuck::cast_slice(&data),
            );
        }
    }

    fn view_state_dto(&self) -> ViewStateDto {
        let default_projection = ProjectionMode::Perspective;
        let projections = std::array::from_fn(|i| {
            projection_name(
                self.view.cameras[i]
                    .as_ref()
                    .map_or(default_projection, |c| c.camera.projection),
            )
            .to_string()
        });
        let cams = self.scene_objects.cameras();
        let pane_look_through =
            std::array::from_fn(|i| self.view.look_through[i].map(|n| n.0 as f64));
        let pane_gate_aspect = std::array::from_fn(|i| {
            let node = self.view.look_through[i]?;
            cams?
                .iter()
                .find(|c| c.id == SceneObjectId(node.0))
                .map(|c| c.aspect)
        });
        ViewStateDto {
            layout: self.view.display.layout,
            split_ratio: self.view.display.split_ratio,
            active_pane: self.view.active_pane,
            cameras_linked: self.view.cameras_linked,
            pane_settings: self.view.pane_settings,
            display: self.view.display,
            pane_projections: projections,
            pane_rects: self.pane_rects_css(),
            pane_look_through,
            pane_camera_locked: self.view.camera_locked,
            pane_gate_aspect,
            attr_viz: self.attr_viz.clone(),
        }
    }

    // ---- .slxy view sidecar bridge ----

    fn view_json(&self) -> solarxy_scenefile::ViewJson {
        let layout = serde_json::to_value(self.view.display.layout)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "single".to_string());
        let panes_json = (0..4)
            .map(|i| {
                let camera = self.view.cameras[i]
                    .as_ref()
                    .map_or_else(solarxy_scenefile::CameraJson::default, |c| {
                        camera_to_json(&c.camera)
                    });
                let display = serde_json::to_value(self.view.pane_settings[i])
                    .ok()
                    .and_then(|v| {
                        if let serde_json::Value::Object(map) = v {
                            Some(map.into_iter().collect())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                solarxy_scenefile::PaneJson {
                    camera,
                    display,
                    look_through: self.view.look_through[i].map(|n| n.0),
                    camera_locked: self.view.camera_locked[i],
                    ..solarxy_scenefile::PaneJson::default()
                }
            })
            .collect();
        solarxy_scenefile::ViewJson {
            layout,
            active_pane: self.view.active_pane as u32,
            split_ratio: self.view.display.split_ratio,
            panes: panes_json,
        }
    }

    fn apply_view_json(&mut self, view: &solarxy_scenefile::ViewJson) {
        if let Ok(layout) =
            serde_json::from_value::<ViewLayout>(serde_json::Value::String(view.layout.clone()))
        {
            self.view.display.layout = layout;
        }
        self.view.display.split_ratio = DisplaySettings::clamp_split_ratio(view.split_ratio);
        self.view.active_pane =
            (view.active_pane as usize).min(self.view.display.layout.pane_count() - 1);

        for (i, pane) in view.panes.iter().take(4).enumerate() {
            if !pane.display.is_empty() {
                let value = serde_json::Value::Object(pane.display.clone().into_iter().collect());
                if let Ok(mut settings) = serde_json::from_value::<PaneDisplaySettings>(value) {
                    // Viewport shading overrides and the turntable spin are
                    // session-temporary (items 7, 9): never restored from a
                    // saved scene, so a reopened scene starts Textured and still.
                    settings.material_override = solarxy_core::preferences::MaterialOverride::None;
                    settings.turntable_active = false;
                    self.view.pane_settings[i] = settings;
                }
            }
            if pane.camera.distance > 0.0 {
                let bounds = self.scene_bounds();
                let aspect =
                    self.renderer.target_width as f32 / self.renderer.target_height.max(1) as f32;
                let cam_state = self.view.cameras[i].get_or_insert_with(|| {
                    CameraState::new(&self.device, &self.renderer.layouts.camera, &bounds, aspect)
                });
                apply_camera_json(&mut cam_state.camera, &pane.camera);
            }
            // Restore the look-through binding + lock. The follow will
            // drive the pane from the node once it cooks.
            self.view.look_through[i] = pane.look_through.map(NodeId);
            self.view.camera_locked[i] = pane.camera_locked;
            self.view.camera_editing[i] = false;
        }
        self.ensure_pane_cameras();
    }
}

/// The asset-preview pane's isolated render state: its own surface
/// (a second canvas from the SAME instance/device), a throwaway `SceneObjects`
/// holding one parsed model, and an orbit camera. Never touches the document.
struct PreviewState {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    objects: SceneObjects,
    camera: CameraState,
}

/// The asset-preview pane: a live 3D orbit view of one staged model,
/// rendered ON DEMAND (open / orbit / zoom / resize), never in the frame loop,
/// so an idle preview costs nothing. Each render borrows the shared HDR chain
/// at preview size (the screenshot pattern); the next main frame's target sync
/// restores it.
#[wasm_bindgen]
impl SolarxyApp {
    /// Opens (or replaces) the model preview on the given canvas: parses the
    /// staged asset through the same `parse_model` path the import cooks use,
    /// frames a camera on its bounds, and renders the first frame.
    pub fn preview_open(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        hash: String,
        name: String,
    ) -> Result<(), JsError> {
        let format = name
            .rsplit('.')
            .next()
            .map(str::to_lowercase)
            .unwrap_or_default();
        let id = solarxy_graph::params::AssetId(hash);
        let Some(bytes) = self.engine.asset_bytes(&id).map(<[_]>::to_vec) else {
            return Err(JsError::new("asset is not staged"));
        };
        let options = ImportOptions {
            scale: 1.0,
            center_to_origin: false,
            recompute_normals: None,
            preserve_materials: None,
            vertex_colors: None,
        };
        let set = solarxy_graph::nodes::parse_model(
            &format,
            &bytes,
            &name,
            self.engine.asset_table(),
            &options,
        )
        .map_err(|e| JsError::new(&format!("preview parse failed: {e}")))?;
        self.preview_render_set(canvas, set)
    }

    /// Opens the model preview from a geometry blob the import worker parsed
    /// off the main thread (the same `transfer` blob a normal import commits
    /// through `submit_parsed_model`). This is the hitch-free path: the
    /// blocking `parse_model` runs in the worker, not on the main thread.
    pub fn preview_open_parsed(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        blob: Vec<u8>,
    ) -> Result<(), JsError> {
        let set =
            transfer::unpack(&blob).map_err(|e| JsError::new(&format!("preview blob: {e}")))?;
        self.preview_render_set(canvas, set)
    }

    /// Orbits the preview camera (canvas-px deltas) and re-renders.
    pub fn preview_orbit(&mut self, dx: f32, dy: f32) {
        if let Some(p) = self.preview.as_mut() {
            let cam = &mut p.camera.camera;
            orbit_camera_yaw(cam, dx * -0.008);
            // Pitch: rotate eye about the target's horizontal axis, clamped so
            // the orbit never flips over the pole.
            let offset = cam.eye - cam.target;
            let dist = offset.magnitude().max(1e-4);
            let pitch = (offset.y / dist).clamp(-1.0, 1.0).asin();
            let new_pitch = (pitch + dy * 0.008).clamp(-1.45, 1.45);
            let horiz = (offset.x * offset.x + offset.z * offset.z).sqrt().max(1e-4);
            let scale = (dist * new_pitch.cos()) / horiz;
            cam.eye = cam.target
                + Vector3::new(offset.x * scale, dist * new_pitch.sin(), offset.z * scale);
        }
        self.render_preview();
    }

    /// Dollies the preview camera and re-renders; positive zooms in.
    pub fn preview_zoom(&mut self, delta: f32) {
        if let Some(p) = self.preview.as_mut() {
            let cam = &mut p.camera.camera;
            let offset = cam.eye - cam.target;
            cam.eye = cam.target + offset * (-delta * 0.1).exp();
        }
        self.render_preview();
    }

    /// Resizes the preview surface to the canvas's current physical size.
    pub fn preview_resize(&mut self, width: u32, height: u32) {
        if let Some(p) = self.preview.as_mut() {
            p.config.width = width.max(16);
            p.config.height = height.max(16);
            p.surface.configure(&self.device, &p.config);
        }
        self.render_preview();
    }

    /// Drops the preview (its surface, geometry, and camera).
    pub fn preview_close(&mut self) {
        self.preview = None;
    }
}

impl SolarxyApp {
    /// Uploads a parsed set to a throwaway preview surface on `canvas`,
    /// frames a camera on its bounds, and renders the first frame. Shared by
    /// `preview_open` (host parse) and `preview_open_parsed` (worker parse).
    fn preview_render_set(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        set: solarxy_kernel::GeometrySet,
    ) -> Result<(), JsError> {
        let cooked = std::sync::Arc::new(set.to_cooked());

        let width = canvas.width().max(16);
        let height = canvas.height().max(16);
        let surface = self
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsError::new(&format!("preview surface: {e}")))?;
        let mut config = self.config.clone();
        config.width = width;
        config.height = height;
        surface.configure(&self.device, &config);

        let mut objects = SceneObjects::new();
        let delta = SceneDelta {
            ops: vec![SceneOp::UpsertGeometry {
                id: SceneObjectId(0),
                geometry: cooked,
            }],
        };
        objects
            .apply(&self.device, &self.queue, &self.renderer.layouts, &delta)
            .map_err(|e| JsError::new(&format!("preview upload: {e}")))?;
        let bounds = objects.visible_bounds().unwrap_or_else(default_bounds);
        let aspect = width as f32 / height.max(1) as f32;
        let camera = CameraState::new(&self.device, &self.renderer.layouts.camera, &bounds, aspect);

        self.preview = Some(PreviewState {
            surface,
            config,
            objects,
            camera,
        });
        self.render_preview();
        Ok(())
    }

    /// Renders one preview frame into the preview surface, reusing the shared
    /// render chain at preview size (the screenshot pattern; the next main
    /// frame's `sync_render_target_dims` restores the layout dimensions).
    fn render_preview(&mut self) {
        let Some((w, h)) = self
            .preview
            .as_ref()
            .map(|p| (p.config.width, p.config.height))
        else {
            return;
        };
        // Everything that needs whole-&self access happens BEFORE the preview
        // borrow; inside it only disjoint field borrows are used.
        self.set_target_dims(w, h);
        let mut pds = default_pane_settings();
        pds.show_grid = false;
        pds.show_axis_gizmo = false;
        let background = self.resolve_background(&pds);

        let Some(p) = self.preview.as_mut() else {
            return;
        };
        let aspect = w as f32 / h.max(1) as f32;
        p.camera.write_with_aspect(&self.queue, aspect);

        let output = match p.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                p.surface.configure(&self.device, &p.config);
                return;
            }
            Err(_) => return,
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
        });

        let objects: Vec<solarxy_renderer::frame::DrawObject<'_>> =
            p.objects.draw_objects().collect();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Preview Encoder"),
            });
        // Shadow the preview content itself, so the map matches what is drawn.
        self.renderer
            .render_shadow_pass(&mut encoder, &self.env, &objects);
        self.renderer.render_main_pass(
            &mut encoder,
            &self.env,
            &objects,
            &p.camera.bind_group,
            &p.camera.camera,
            &pds,
            background,
        );
        // Composite without bloom/SSAO: a preview is a shaded look, not a
        // post-processed beauty frame.
        self.renderer.post.composite.write_params(
            &self.queue,
            false,
            false,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
            pds.inspection_mode,
        );
        self.renderer.post.composite.render(
            &mut encoder,
            &self.renderer.pipelines,
            &view,
            false,
            &self.renderer.post.ssao,
            Some([0.0, 0.0, w as f32, h as f32]),
            true,
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}

fn map_button(button: u32) -> Option<PointerButton> {
    match button {
        0 => Some(PointerButton::Left),
        1 => Some(PointerButton::Middle),
        2 => Some(PointerButton::Right),
        _ => None,
    }
}

/// Orbits a camera's eye around its target about the world-up (Y) axis by
/// `yaw` radians: the turntable rotation for a deterministic export sweep.
fn orbit_camera_yaw(cam: &mut Camera, yaw: f32) {
    let offset = cam.eye - cam.target;
    let (s, c) = yaw.sin_cos();
    let x = offset.x * c + offset.z * s;
    let z = -offset.x * s + offset.z * c;
    cam.eye = cam.target + Vector3::new(x, offset.y, z);
}

fn projection_name(mode: ProjectionMode) -> &'static str {
    match mode {
        ProjectionMode::Perspective => "perspective",
        ProjectionMode::Orthographic => "orthographic",
    }
}

/// Same math as the desktop `compute_depth_bounds`.
fn compute_depth_bounds(camera: &Camera, bounds: &AABB) -> (f32, f32) {
    let view = camera.build_view_matrix();
    let mut z_min = f32::INFINITY;
    let mut z_max = f32::NEG_INFINITY;
    for corner in &bounds.corners() {
        let vp = view * corner.to_homogeneous();
        let z = -vp.z;
        z_min = z_min.min(z);
        z_max = z_max.max(z);
    }
    z_min = z_min.max(0.001);
    if z_max <= z_min {
        z_max = z_min + 1.0;
    }
    (z_min, z_max)
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
fn apply_camera_def(cam: &mut Camera, def: &solarxy_core::scene::CameraDef) {
    use solarxy_core::scene::CameraKind;
    cam.eye = Point3::new(def.position[0], def.position[1], def.position[2]);
    cam.target = Point3::new(def.target[0], def.target[1], def.target[2]);
    cam.up = Vector3::new(def.up[0], def.up[1], def.up[2]);
    if def.fov_y > 0.0 {
        cam.fovy = def.fov_y.to_degrees();
    }
    cam.projection = match def.kind {
        CameraKind::Orthographic => ProjectionMode::Orthographic,
        _ => ProjectionMode::Perspective,
    };
    if def.ortho_scale > 0.0 {
        cam.ortho_scale = def.ortho_scale;
    }
}

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
