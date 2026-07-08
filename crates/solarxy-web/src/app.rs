//! The `SolarxyApp` wasm-bindgen class: the browser host over the engine
//! and the forward renderer.
//!
//! The React frontend holds one instance: it dispatches `Command`s (in) and
//! receives `EventBatch`es (out), calls `frame` each rAF tick to cook under
//! a budget and render, and routes pointer gestures to the camera and
//! Rust-side picking. Cooked geometry never crosses into JavaScript.

use solarxy_core::raycast::screen_to_world_ray;
use solarxy_graph::{Command, Engine, EventBatch};
use wasm_bindgen::prelude::*;

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

        // Drain async jobs inline (imports are Phase 5; native-style resolve
        // keeps the protocol exercised without a worker).
        let jobs = self.engine.take_jobs();
        for (ctx, job, request) in jobs {
            let result = self.engine.resolve_job(&request);
            self.engine.submit_job_result(ctx, job, result);
        }

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
}

/// Serializes a value to a `JsValue` via serde-wasm-bindgen, using the
/// json-compatible serializer so Rust maps (e.g. a node's `params`) become
/// plain JS objects rather than `Map`s, matching what the frontend expects.
fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&format!("serialize: {e}")))
}
