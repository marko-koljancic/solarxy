//! The `SolarxyApp` wasm-bindgen class: the browser host over the engine
//! and the full `solarxy-renderer` pipeline (phase 6; the phase-4 stopgap
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
use solarxy_core::raycast::screen_to_world_ray;
use solarxy_core::scene::{SceneDelta, SceneObjectId, SceneOp};
use solarxy_core::validation::{
    ValidationConfig, ValidationResult, ValidationThresholds, validate_raw_model_with_config,
};
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings, ViewLayout};
use solarxy_core::geometry::compute_bounds;
use solarxy_core::AABB;
use solarxy_graph::assets::AssetTable;
use solarxy_graph::cook::{ImportOptions, JobId, JobRequest, JobResult, ParsedModel};
use solarxy_graph::document::GraphContext;
use solarxy_graph::engine::SceneSidecar;
use solarxy_graph::{Command, Engine, EventBatch};
use solarxy_kernel::transfer;
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::frame::{DrawObject, GradientUniform, Renderer, RendererInit, WireframeParams};
use solarxy_renderer::geometry::build_normals_geometry;
use solarxy_renderer::model::NormalsGeometry;
use solarxy_renderer::input::PointerButton;
use solarxy_renderer::light::LightsUniform;
use solarxy_renderer::panes::{self, PaneRect};
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
        line_weight: LineWeight::Medium,
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
    }
}

/// Host-owned view state: the web mirror of the desktop `ViewState`.
struct WebViewState {
    display: DisplaySettings,
    active_pane: usize,
    cameras_linked: bool,
    cameras: [Option<CameraState>; 4],
    pane_settings: [PaneDisplaySettings; 4],
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
pub struct SolarxyApp {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
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
    /// The scene object tinted as selected in the viewports (decision 24),
    /// or `None`.
    selected_object: Option<SceneObjectId>,
    /// Validate jobs drained from the engine but not yet handed to the
    /// worker: the geometry is packed to a transfer blob at drain time so
    /// `take_validate_jobs` moves plain bytes.
    pending_validate: Vec<PendingValidateJob>,
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
    /// A screenshot request captured this frame (rendered at frame end).
    screenshot_request: Option<ScreenshotOptsDto>,
    /// The in-flight screenshot readback (one at a time).
    pending_screenshot: Option<solarxy_renderer::capture::PendingCapture>,
    /// Whether the normals/bounds visualization aggregate is stale
    /// (geometry changed, env rebuilt, or an overlay mode just turned on).
    viz_dirty: bool,
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
            wireframe_line_width: solarxy_core::preferences::LineWeight::Medium.width_px(),
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
            SHADOW_MAP_SIZE,
            vis,
        );
        env.light_bind_group = create_light_bind_group(
            &device,
            &renderer.layouts,
            &env.light_buffer,
            &renderer.ibl_res.ibl,
            &renderer.ibl_res.brdf_lut,
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
            surface,
            device,
            queue,
            config,
            render_format,
            renderer,
            scene_objects: SceneObjects::new(),
            env,
            env_bounds: bounds,
            view: WebViewState {
                display: default_display_settings(),
                active_pane: 0,
                cameras_linked: false,
                cameras: [None, None, None, None],
                pane_settings: [default_pane_settings(); 4],
            },
            engine,
            host_events: Vec::new(),
            last_pane_rects: Vec::new(),
            dpr: dpr as f32,
            pointer_buttons_down: 0,
            selected_object: None,
            pending_validate: Vec::new(),
            current_ctx: GraphContext::Root,
            uv_scene: SceneObjects::new(),
            uv_use_preview: false,
            last_uv_source: None,
            last_overlap: (None, false),
            last_pointer: (0.0, 0.0),
            hdri: None,
            screenshot_request: None,
            pending_screenshot: None,
            viz_dirty: true,
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
        // Cook the dirty set under a wall-clock budget.
        let deadline = web_now() + COOK_BUDGET_MS;
        let events = self.engine.cook(&mut || web_now() < deadline);

        // Apply the fresh scene delta to the multi-object scene.
        let delta = self.engine.take_scene_delta();
        if !delta.ops.is_empty() {
            self.viz_dirty = true;
            if let Err(e) =
                self.scene_objects
                    .apply(&self.device, &self.queue, &self.renderer.layouts, &delta)
            {
                log(&format!("scene delta apply failed: {e}"));
            }
        }

        self.sync_env_bounds();
        self.sync_visualization();
        self.sync_uv_preview();
        self.ensure_pane_cameras();
        let dt = (dt_ms / 1000.0).clamp(0.0, 0.1) as f32;
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

        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Resizes the surface and render targets.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
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

    /// Pointer button down. `button`: 0 left, 1 middle, 2 right.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: u32) {
        let p = (x * self.dpr, y * self.dpr);
        if self.pointer_buttons_down == 0 {
            self.set_hovered_pane(panes::hit_test_pane(&self.compute_panes(), p));
        }
        self.pointer_buttons_down |= 1 << button;
        let active = self.view.active_pane;
        if let Some(cam) = self.view.cameras[active].as_mut() {
            cam.handle_mouse_move(p.0, p.1);
            if let Some(btn) = map_button(button) {
                cam.handle_mouse_button(btn, true);
            }
        }
    }

    /// Pointer move; updates the hovered (active) pane while no drag is in
    /// flight, and feeds the active pane's camera controller.
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        let p = (x * self.dpr, y * self.dpr);
        let last = std::mem::replace(&mut self.last_pointer, p);
        if self.pointer_buttons_down == 0 {
            self.set_hovered_pane(panes::hit_test_pane(&self.compute_panes(), p));
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

    pub fn pointer_up(&mut self, button: u32) {
        self.pointer_buttons_down &= !(1 << button);
        let active = self.view.active_pane;
        if let Some(cam) = self.view.cameras[active].as_mut()
            && let Some(btn) = map_button(button)
        {
            cam.handle_mouse_button(btn, false);
        }
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
    }

    /// Picks the geo node under a canvas CSS pixel, pane-aware: the ray is
    /// built from the pane under the cursor with that pane's camera.
    /// Returns the node id as a number, or `undefined` on a miss.
    pub fn pick(&self, x: f32, y: f32) -> Option<f64> {
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
            cam.eye,
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
                cam.eye,
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

    /// Marker pin positions in PANE-RELATIVE CSS pixels (the DOM overlay
    /// clips one absolutely-positioned box per pane, so pins offset from
    /// their pane's origin), one entry per visible (marker x 3D pane) pair,
    /// resolved through each pane's camera (the desktop projection: clip ->
    /// NDC -> pane pixel, small NDC slack). Called once per animation frame
    /// by the host loop and applied to the DOM imperatively; markers absent
    /// from the list are hidden. UV panes carry no markers.
    pub fn review_markers(&self) -> Result<JsValue, JsError> {
        let markers = self.engine.review_markers_world();
        let mut out: Vec<MarkerScreenDto> = Vec::new();
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
                let ndc = (clip.x / clip.w, clip.y / clip.w);
                if ndc.0.abs() > 1.05 || ndc.1.abs() > 1.05 {
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

    /// Mirrors the graph context the node canvas currently shows (the UV
    /// pane's selected-node source resolves against it).
    pub fn set_current_context(&mut self, ctx: JsValue) -> Result<(), JsError> {
        self.current_ctx = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        Ok(())
    }

    /// Marks the scene object produced by `node` as selected (viewport
    /// outline tint, decision 24); `undefined`/null clears it.
    pub fn set_scene_selection(&mut self, node: Option<f64>) {
        self.selected_object = node.map(|n| SceneObjectId(n as u64));
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
    /// `{kind:"projection", mode:"perspective"|"orthographic"}`.
    pub fn camera_command(&mut self, pane: usize, cmd: JsValue) -> Result<(), JsError> {
        let cmd: CameraCommandDto = serde_wasm_bindgen::from_value(cmd)
            .map_err(|e| JsError::new(&format!("bad camera command: {e}")))?;
        let bounds = self.scene_bounds();
        let Some(cam) = self.view.cameras.get_mut(pane).and_then(|c| c.as_mut()) else {
            return Ok(());
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
        Ok(())
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
            ),
            IblMode::Diffuse => solarxy_renderer::scene::create_light_bind_group_selective(
                &self.device,
                &self.renderer.layouts,
                &self.env.light_buffer,
                &self.renderer.ibl_res.ibl,
                &self.renderer.ibl_res.ibl_fallback,
                &self.renderer.ibl_res.brdf_lut,
            ),
            IblMode::Full => create_light_bind_group(
                &self.device,
                &self.renderer.layouts,
                &self.env.light_buffer,
                &self.renderer.ibl_res.ibl,
                &self.renderer.ibl_res.brdf_lut,
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
            _pad: 0.0,
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
            SHADOW_MAP_SIZE,
            vis,
        );
        env.light_bind_group = create_light_bind_group(
            &self.device,
            &self.renderer.layouts,
            &env.light_buffer,
            self.active_ibl(),
            &self.renderer.ibl_res.brdf_lut,
        );
        self.env = env;
        self.env_bounds = bounds;
        // The rebuilt environment starts with empty per-mesh viz data; the
        // aggregate refills it when a pane wants overlays.
        self.viz_dirty = true;
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
        // The picking-sync selection highlight (decision 24): flag the
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
            _pad: 0.0,
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
        ViewStateDto {
            layout: self.view.display.layout,
            split_ratio: self.view.display.split_ratio,
            active_pane: self.view.active_pane,
            cameras_linked: self.view.cameras_linked,
            pane_settings: self.view.pane_settings,
            display: self.view.display,
            pane_projections: projections,
            pane_rects: self.pane_rects_css(),
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
                if let Ok(settings) = serde_json::from_value::<PaneDisplaySettings>(value) {
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
        }
        self.ensure_pane_cameras();
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

fn apply_camera_json(cam: &mut Camera, json: &solarxy_scenefile::CameraJson) {
    let target = Point3::new(json.target[0], json.target[1], json.target[2]);
    let cp = json.pitch.cos();
    let dir = Vector3::new(cp * json.yaw.sin(), json.pitch.sin(), cp * json.yaw.cos());
    cam.target = target;
    cam.eye = target + dir * json.distance.max(1e-4);
    cam.up = Vector3::unit_y();
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
