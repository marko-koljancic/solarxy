//! The asset preview pane: its own surface, its own throwaway scene.

use super::*;

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
    pub(super) fn preview_render_set(
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
    pub(super) fn render_preview(&mut self) {
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
            &CompositeLook::from_tone(self.renderer.post.tone_mode, self.renderer.post.exposure),
            &self.renderer.post.luts,
            pds.inspection_mode,
            false,
        );
        self.renderer.post.composite.render(
            &mut encoder,
            &self.renderer.pipelines,
            &view,
            false,
            &self.renderer.post.ssao,
            Some([0.0, 0.0, w as f32, h as f32]),
            true,
            None,
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}
