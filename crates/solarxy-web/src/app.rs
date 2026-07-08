//! The `SolarxyApp` wasm-bindgen class: the browser host over the engine
//! and the forward renderer.
//!
//! The React frontend holds one instance: it dispatches `Command`s (in) and
//! receives `EventBatch`es (out), calls `frame` each rAF tick to cook under
//! a budget and render, and routes pointer gestures to the camera and
//! Rust-side picking. Cooked geometry never crosses into JavaScript.

use std::collections::BTreeMap;

use solarxy_core::raycast::screen_to_world_ray;
use solarxy_graph::assets::AssetTable;
use solarxy_graph::cook::{ImportOptions, JobId, JobRequest, JobResult};
use solarxy_graph::document::GraphContext;
use solarxy_graph::engine::SceneSidecar;
use solarxy_graph::{Command, Engine, EventBatch};
use solarxy_kernel::transfer;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::camera::OrbitCamera;
use crate::render::WebRenderer;

/// The synchronous cook budget per frame, in milliseconds (about half a
/// 60fps frame, leaving headroom for render + the browser).
const COOK_BUDGET_MS: f64 = 6.0;

/// The current host time in milliseconds (`performance.now`).
fn web_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// The Solarxy browser application: one WebGPU surface, the forward
/// renderer, an orbit camera, and the headless engine.
#[wasm_bindgen]
pub struct SolarxyApp {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: WebRenderer,
    camera: OrbitCamera,
    engine: Engine,
}

#[wasm_bindgen]
impl SolarxyApp {
    /// Boots over a canvas: WebGPU surface/device/queue, the forward
    /// renderer, the camera, and the engine with the host clock installed.
    pub async fn create(canvas: web_sys::HtmlCanvasElement) -> Result<SolarxyApp, JsError> {
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

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = WebRenderer::new(&device, format, width, height);
        let camera = OrbitCamera {
            aspect: width as f32 / height.max(1) as f32,
            ..OrbitCamera::default()
        };

        let mut engine = Engine::new().map_err(|e| JsError::new(&format!("engine: {e}")))?;
        engine.set_clock(web_now);
        // Imports run off the main thread: cooks yield a ParseModel job the
        // frontend pumps to the import worker (`take_import_jobs` ->
        // `submit_parsed_model`), rather than parsing inline.
        engine.set_async_jobs(true);

        log(&format!(
            "solarxy-web: booted ({width}x{height}, {} node types)",
            engine.registry().len()
        ));

        Ok(SolarxyApp {
            surface,
            device,
            queue,
            config,
            renderer,
            camera,
            engine,
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

    /// Cooks under a frame budget, applies the scene delta, renders, and
    /// returns the cook `EventBatch` (status + stats) for the mirror.
    pub fn frame(&mut self, _dt_ms: f64) -> Result<JsValue, JsError> {
        // Cook the dirty set under a wall-clock budget.
        let deadline = web_now() + COOK_BUDGET_MS;
        let events = self.engine.cook(&mut || web_now() < deadline);

        // Import jobs spawned by this cook accumulate as Pending; the
        // frontend drains them to the worker via `take_import_jobs`.

        // Apply the fresh scene delta to the renderer.
        let delta = self.engine.take_scene_delta();
        self.renderer.apply_delta(&self.device, &self.queue, &delta);

        // Render.
        match self.surface.get_current_texture() {
            Ok(frame) => {
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                self.renderer
                    .render(&self.device, &self.queue, &view, &self.camera);
                frame.present();
            }
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
            }
            Err(e) => return Err(JsError::new(&format!("acquire: {e}"))),
        }

        let batch = EventBatch {
            revision: self.engine.revision(),
            events,
        };
        to_js(&batch)
    }

    /// Resizes the surface, camera aspect, and depth buffer.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.renderer.resize(&self.device, width, height);
        self.camera.aspect = width as f32 / height.max(1) as f32;
    }

    /// Orbits the camera by pointer deltas (pixels).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.camera.orbit(dx, dy);
    }

    /// Pans the camera target by pointer deltas (pixels).
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.camera.pan(dx, dy);
    }

    /// Dollies the camera by a wheel delta (positive zooms in).
    pub fn dolly(&mut self, amount: f32) {
        self.camera.dolly(amount);
    }

    /// Picks the geo node under a canvas pixel (Rust-side ray over cooked
    /// geometry). Returns the node id as a number, or `undefined` on a miss.
    pub fn pick(&self, x: f32, y: f32) -> Option<f64> {
        let ray = screen_to_world_ray(
            (x, y),
            (self.config.width as f32, self.config.height as f32),
            self.camera.view_proj(),
            self.camera.eye(),
        );
        let origin = [ray.origin.x, ray.origin.y, ray.origin.z];
        let dir = [ray.direction.x, ray.direction.y, ray.direction.z];
        self.engine.pick(origin, dir).map(|n| n.0 as f64)
    }

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

    /// Serializes the whole document (JSON autosave / explicit save).
    pub fn save_scene(&self) -> Result<JsValue, JsError> {
        to_js(&self.engine.save_document())
    }

    /// Replaces the whole document from a save file, returning the
    /// `DocumentReplaced` batch (the mirror then resnapshots).
    pub fn load_scene(&mut self, file: JsValue) -> Result<JsValue, JsError> {
        let file = serde_wasm_bindgen::from_value(file)
            .map_err(|e| JsError::new(&format!("bad scene: {e}")))?;
        let batch = self.engine.load_document(&file);
        to_js(&batch)
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
        self.renderer.object_count()
    }

    // ---- Phase 5: asset staging + the import-worker pump ----

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

    /// Drains the import jobs the last cook spawned into a JS array of
    /// `{ ctx, jobId, hash, name, format, options, sidecars }`. The frontend
    /// gathers each job's bytes, posts them to the import worker, and returns
    /// the result through `submit_parsed_model` / `submit_parse_error`.
    pub fn take_import_jobs(&mut self) -> Result<JsValue, JsError> {
        let manifest = self.engine.asset_manifest();
        let payloads: Vec<ImportJobDto> = self
            .engine
            .take_jobs()
            .into_iter()
            .map(|(ctx, job, req)| {
                let (asset, format, options) = match req {
                    JobRequest::ParseModel {
                        asset,
                        format,
                        options,
                    } => (asset, format, options),
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
                ImportJobDto {
                    ctx,
                    job_id: job.0 as f64,
                    hash: asset.0,
                    name,
                    format,
                    options,
                    sidecars,
                }
            })
            .collect();
        to_js(&payloads)
    }

    /// Commits a worker-parsed model (the transfer blob from
    /// `parse_model_job`) under the per-node generation guard, returning the
    /// cook `EventBatch`. A superseded result is dropped inside the engine.
    pub fn submit_parsed_model(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        blob: Vec<u8>,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let set =
            transfer::unpack(&blob).map_err(|e| JsError::new(&format!("bad model blob: {e}")))?;
        let events =
            self.engine
                .submit_job_result(ctx, JobId(job_id as u64), JobResult::Model(Ok(set)));
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

    // ---- Phase 5: .slxy save / load ----

    /// Builds `.slxy` archive bytes from the current document, its referenced
    /// assets, and the host `extra` (generator, canvas viewports, meta). The
    /// camera comes from this app's orbit camera.
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
        if let Some(pane) = sidecar.view.panes.first_mut() {
            pane.camera = self.camera_json();
        }
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
    /// applies the saved camera, and returns `{ batch, warnings,
    /// canvasViewports, meta }` for the mirror and the frontend view state.
    pub fn load_slxy(&mut self, bytes: Vec<u8>) -> Result<JsValue, JsError> {
        let loaded = self
            .engine
            .load_slxy(&bytes)
            .map_err(|e| JsError::new(&format!("load .slxy: {e}")))?;
        if let Some(pane) = loaded.sidecar.view.panes.first() {
            self.apply_camera_json(&pane.camera);
        }
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
        };
        to_js(&result)
    }

    fn camera_json(&self) -> solarxy_scenefile::CameraJson {
        let c = &self.camera;
        solarxy_scenefile::CameraJson {
            target: [c.target.x, c.target.y, c.target.z],
            yaw: c.yaw,
            pitch: c.pitch,
            distance: c.distance,
            fov_y: c.fov_y.0,
        }
    }

    fn apply_camera_json(&mut self, c: &solarxy_scenefile::CameraJson) {
        self.camera.target = cgmath::Point3::new(c.target[0], c.target[1], c.target[2]);
        self.camera.yaw = c.yaw;
        self.camera.pitch = c.pitch;
        self.camera.distance = c.distance;
        self.camera.fov_y = cgmath::Rad(c.fov_y);
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
/// sidecars) into a finished [`solarxy_kernel::GeometrySet`] and returns it
/// as a transfer blob (`Uint8Array`) to move back to the main instance.
/// Never touches wgpu, so instantiating it in a worker creates no device.
#[wasm_bindgen]
pub fn parse_model_job(
    format: String,
    options_json: String,
    files: JsValue,
) -> Result<js_sys::Uint8Array, JsError> {
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

    let set = solarxy_graph::nodes::parse_model(&format, bytes, name, &table, &options)
        .map_err(|e| JsError::new(&e))?;
    Ok(js_sys::Uint8Array::from(transfer::pack(&set).as_slice()))
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
