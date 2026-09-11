//! The asset preview pane: its own surface, its own throwaway scene.
//!
//! The scene, the camera rules and the render are `solarxy_host::preview`,
//! shared with the desktop; what is this shell's is the second canvas's
//! surface and the parse arriving from the import worker.

use super::*;
use solarxy_host::preview::PreviewScene;

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
        self.preview_render_set(canvas, &set)
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
        self.preview_render_set(canvas, &set)
    }

    /// Orbits the preview camera (canvas-px deltas) and re-renders.
    pub fn preview_orbit(&mut self, dx: f32, dy: f32) {
        if let Some(p) = self.preview.as_mut() {
            p.scene.orbit(dx, dy);
        }
        self.render_preview();
    }

    /// Dollies the preview camera and re-renders; positive zooms in.
    pub fn preview_zoom(&mut self, delta: f32) {
        if let Some(p) = self.preview.as_mut() {
            p.scene.zoom(delta);
        }
        self.render_preview();
    }

    /// Resizes the preview surface to the canvas's current physical size.
    pub fn preview_resize(&mut self, width: u32, height: u32) {
        if let Some(p) = self.preview.as_mut() {
            p.config.width = width.max(solarxy_host::preview::MIN_EDGE);
            p.config.height = height.max(solarxy_host::preview::MIN_EDGE);
            p.surface.configure(&self.device, &p.config);
            p.scene.resize(p.config.width, p.config.height);
        }
        self.render_preview();
    }

    /// Drops the preview (its surface, geometry, and camera).
    pub fn preview_close(&mut self) {
        self.preview = None;
    }
}

impl SolarxyApp {
    /// Uploads a parsed set to a throwaway preview scene on `canvas`,
    /// frames a camera on its bounds, and renders the first frame. Shared by
    /// `preview_open` (host parse) and `preview_open_parsed` (worker parse).
    pub(super) fn preview_render_set(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        set: &solarxy_kernel::GeometrySet,
    ) -> Result<(), JsError> {
        let width = canvas.width().max(solarxy_host::preview::MIN_EDGE);
        let height = canvas.height().max(solarxy_host::preview::MIN_EDGE);
        let surface = self
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsError::new(&format!("preview surface: {e}")))?;
        let mut config = self.config.clone();
        config.width = width;
        config.height = height;
        surface.configure(&self.device, &config);

        let scene = PreviewScene::new(
            &self.device,
            &self.queue,
            &self.renderer.layouts,
            set,
            width,
            height,
        )
        .map_err(|e| JsError::new(&format!("preview upload: {e}")))?;

        self.preview = Some(PreviewState {
            surface,
            config,
            scene,
        });
        self.render_preview();
        Ok(())
    }

    /// Renders one preview frame into the preview surface through the shared
    /// render chain at preview size; the next main frame's
    /// `sync_render_target_dims` restores the layout dimensions.
    pub(super) fn render_preview(&mut self) {
        // The background needs whole-&self access, so it is resolved before
        // the preview borrow; inside it only disjoint field borrows are used.
        let background = self.resolve_background(&solarxy_host::preview::preview_pane_settings());
        let Some(p) = self.preview.as_mut() else {
            return;
        };
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
        p.scene.render(
            &mut self.renderer,
            &self.env,
            &self.device,
            &self.queue,
            &view,
            background,
        );
        output.present();
    }
}
