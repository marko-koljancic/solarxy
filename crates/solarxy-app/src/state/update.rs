//! Per-frame state updates. Owns `rebuild_light_bind_group` — the **single
//! IBL chokepoint** triggered on HDRI drop, [`IblMode`] toggle (`I` /
//! `Shift+I`), and background change. Adding any IBL-derived uniform means
//! routing it through this function so Clay modes etc. update instantly
//! without waiting for the next camera-driven frame.

use std::sync::mpsc;

use super::*;

impl State {
    pub(super) fn active_ibl(&self) -> &IblState {
        match self.renderer.ibl_res.ibl_mode {
            IblMode::Off => &self.renderer.ibl_res.ibl_fallback,
            IblMode::Diffuse | IblMode::Full => &self.renderer.ibl_res.ibl,
        }
    }

    pub(super) fn rebuild_light_bind_group(&mut self) {
        // The skybox pass samples the active IBL's source equirect — track
        // it here so HDRI load / IBL swaps keep the visible sky in sync.
        self.renderer.skybox_bind_group = self.renderer.ibl_res.ibl.equirect.as_ref().map(|eq| {
            solarxy_renderer::skybox::create_skybox_bind_group(
                &self.device,
                &self.renderer.layouts.skybox,
                eq,
            )
        });

        let ibl_avg = self.active_ibl().irradiance_average;
        if let Some(scene) = &mut self.scene {
            scene.env.light_bind_group = match self.renderer.ibl_res.ibl_mode {
                IblMode::Off => create_light_bind_group(
                    &self.device,
                    &self.renderer.layouts,
                    &scene.env.light_buffer,
                    &self.renderer.ibl_res.ibl_fallback,
                    &self.renderer.ibl_res.brdf_lut,
                    &self.renderer.ibl_res.ltc,
                ),
                IblMode::Diffuse => create_light_bind_group_selective(
                    &self.device,
                    &self.renderer.layouts,
                    &scene.env.light_buffer,
                    &self.renderer.ibl_res.ibl,
                    &self.renderer.ibl_res.ibl_fallback,
                    &self.renderer.ibl_res.brdf_lut,
                    &self.renderer.ibl_res.ltc,
                ),
                IblMode::Full => create_light_bind_group(
                    &self.device,
                    &self.renderer.layouts,
                    &scene.env.light_buffer,
                    &self.renderer.ibl_res.ibl,
                    &self.renderer.ibl_res.brdf_lut,
                    &self.renderer.ibl_res.ltc,
                ),
            };

            scene.env.lights_uniform.ibl_avg_r = ibl_avg[0];
            scene.env.lights_uniform.ibl_avg_g = ibl_avg[1];
            scene.env.lights_uniform.ibl_avg_b = ibl_avg[2];
            let offset = std::mem::offset_of!(LightsUniform, ibl_avg_r) as u64;
            self.queue.write_buffer(
                &scene.env.light_buffer,
                offset,
                bytemuck::cast_slice(&ibl_avg),
            );
        }
    }

    pub(super) fn update_wireframe_params(&self) {
        self.write_wireframe_params_for(&self.view.pane_settings[0]);
    }

    pub(super) fn write_gradient_colors_for(&self, pds: &PaneDisplaySettings) {
        let (top, bottom) = self.resolve_background(pds).sky_colors();
        let data = GradientUniform {
            top_color: [top[0], top[1], top[2], 1.0],
            bottom_color: [bottom[0], bottom[1], bottom[2], 1.0],
            uv_y_offset: 0.0,
            uv_y_scale: 1.0,
            _pad: [0.0; 2],
        };
        self.queue.write_buffer(
            &self.renderer.wire._gradient_buffer,
            0,
            bytemuck::bytes_of(&data),
        );
    }

    pub(super) fn write_wireframe_params_for(&self, pds: &PaneDisplaySettings) {
        let params = WireframeParams {
            color: self.resolve_background(pds).wireframe_color(),
            line_width: pds.line_weight.width_px(),
            screen_width: self.renderer.target_width as f32,
            screen_height: self.renderer.target_height as f32,
            point_size: self.view.display.point_size,
        };
        self.queue.write_buffer(
            &self.renderer.wire.wireframe_params_buffer,
            0,
            bytemuck::bytes_of(&params),
        );
    }

    pub(super) fn spawn_load(&mut self, model_path: String) {
        let filename = std::path::Path::new(&model_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&model_path)
            .to_string();

        self.gui
            .set_loading_message(&format!("Loading {}...", filename));

        let device = self.device.clone();
        let queue = self.queue.clone();
        let layouts = Arc::clone(&self.renderer.layouts);
        let config = self.config.clone();
        let initial_grid_color = self
            .resolve_background(&self.view.pane_settings[0])
            .grid_color();
        let shadow_map_size = self.preferences.rendering.shadow_map_size;
        let path = model_path.clone();

        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let placeholder_brdf = BrdfLut::fallback(&device, &queue);
            // No fallback for the LTC tables: they are a fixed 64 KB blob
            // with nothing to degrade to, and this bind group is rebuilt
            // against the renderer's own copy as soon as the load lands.
            let ltc = solarxy_renderer::ltc::LtcLuts::load(&device, &queue);
            let result = ModelScene::new(
                model_path,
                &device,
                &queue,
                &layouts,
                &config,
                initial_grid_color,
                &placeholder_brdf,
                &ltc,
                shadow_map_size,
            );
            // The channel carries anyhow (binary-crate convention); the
            // renderer's typed error converts at the boundary.
            let _ = tx.send(result.map_err(anyhow::Error::from));
        });

        self.pending_load = Some(PendingLoad {
            receiver: rx,
            filename,
            path,
        });
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.is_surface_configured = true;

            self.gui.invalidate_viewport_rect();

            let (tw, th) = self.target_dimensions();
            self.resize_render_targets(tw, th);

            let aspect = tw as f32 / th as f32;
            for cam in self.view.cameras.iter_mut().flatten() {
                cam.resize(aspect);
            }
        }
    }

    /// Lazily create a [`CameraState`] for every pane slot the current
    /// layout uses. Idempotent — a slot that already holds a camera is
    /// skipped, so layout toggles preserve per-slot cameras within a
    /// session. No-op until a model is loaded (bounds frame the camera).
    /// Slot 0 is the perspective Single-layout camera; slots 1-3 seed to
    /// orthographic Top / Front / Left — a one-time convenience that the
    /// user re-orients with T / F / L.
    pub(super) fn ensure_pane_cameras(&mut self) {
        let Some(bounds) = self.scene.as_ref().map(|s| s.model.bounds) else {
            return;
        };
        let count = self.view.display.layout.pane_count();
        let (tw, th) = self.target_dimensions();
        let aspect = tw as f32 / th.max(1) as f32;
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
                0 => cam.set_projection(self.preferences.display.projection_mode),
                1 => cam.reset_to_bounds_axis(
                    &bounds,
                    cgmath::Vector3::unit_y(),
                    -cgmath::Vector3::unit_z(),
                ),
                2 => cam.reset_to_bounds_axis(
                    &bounds,
                    cgmath::Vector3::unit_z(),
                    cgmath::Vector3::unit_y(),
                ),
                _ => cam.reset_to_bounds_axis(
                    &bounds,
                    -cgmath::Vector3::unit_x(),
                    cgmath::Vector3::unit_y(),
                ),
            }
            self.view.cameras[i] = Some(cam);
        }
    }

    pub(super) fn resize_render_targets(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
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
        if self.view.pane_settings.iter().any(|p| p.show_uv_overlap) {
            self.renderer.uv_overlap.stats_dirty = true;
        }

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

    pub fn update(&mut self) {
        let hdri_poll = self.pending_hdri.as_ref().map(|p| p.receiver.try_recv());
        match hdri_poll {
            Some(Ok(Ok(new_ibl))) => {
                if let Some(pending) = self.pending_hdri.take() {
                    let filename = pending
                        .path
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or("HDRI")
                        .to_string();
                    let file_size = std::fs::metadata(&pending.path).map_or(0, |m| m.len());
                    let resolution = new_ibl
                        .equirect
                        .as_ref()
                        .map_or((0, 0), |e| (e.texture.width(), e.texture.height()));
                    self.gui.update_hdri_info(hdri_info::HdriInfo {
                        filename,
                        path: pending.path.display().to_string(),
                        resolution,
                        file_size,
                    });
                }
                self.renderer.ibl_res.ibl = new_ibl;
                self.renderer.ibl_res.ibl_mode = IblMode::Full;
                self.renderer.ibl_res.last_active_ibl_mode = IblMode::Full;
                self.rebuild_light_bind_group();
                self.gui.clear_loading_message();
                self.gui.set_toast("HDRI loaded", ToastSeverity::Success);
            }
            Some(Ok(Err(e))) => {
                self.pending_hdri.take();
                self.gui.clear_loading_message();
                self.gui
                    .set_toast(&format!("HDRI error: {}", e), ToastSeverity::Error);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.pending_hdri.take();
                self.gui.clear_loading_message();
                self.gui
                    .set_toast("HDRI load thread crashed", ToastSeverity::Error);
            }
            _ => {}
        }

        if let Some(pending) = self.pending_load.take() {
            match pending.receiver.try_recv() {
                Ok(Ok(mut new_scene)) => {
                    let active_ibl = self.active_ibl();
                    new_scene.env.light_bind_group = create_light_bind_group(
                        &self.device,
                        &self.renderer.layouts,
                        &new_scene.env.light_buffer,
                        active_ibl,
                        &self.renderer.ibl_res.brdf_lut,
                        &self.renderer.ibl_res.ltc,
                    );
                    let file_size = std::fs::metadata(&pending.path).map_or(0, |m| m.len());
                    let bounds_size = new_scene.model.bounds.size();
                    self.gui.update_model_info(
                        &pending.filename,
                        &pending.path,
                        file_size,
                        new_scene.model.meshes.len(),
                        new_scene.model.materials.len(),
                        &new_scene.stats,
                        [bounds_size.x, bounds_size.y, bounds_size.z],
                        new_scene.model.has_uvs,
                    );
                    // The Material Inspector's thumbnail cache is keyed by
                    // (material_index, role); drop it so the new model's
                    // textures aren't shadowed by stale entries.
                    self.gui.reset_material_inspector();
                    tracing::info!(
                        "Loaded model: {} ({} verts, {} tris, {} meshes)",
                        pending.path,
                        new_scene.stats.verts,
                        new_scene.stats.tris,
                        new_scene.model.meshes.len(),
                    );
                    self.gui.clear_loading_message();
                    self.window
                        .set_title(&format!("Solarxy \u{2014} {}", pending.filename));
                    preferences::add_recent_file(&mut self.preferences, &pending.path);
                    self.scene = Some(new_scene);
                    // Flush unsaved review notes for the outgoing model
                    // before its sidecar path is cleared by the reload.
                    if self.review.dirty {
                        self.save_review_sidecar();
                    }
                    self.load_review_for_model(&pending.path);
                    self.view.cameras = [None, None, None, None];
                    self.ensure_pane_cameras();

                    self.view.pane_settings[0].view_mode = self.preferences.display.view_mode;
                    self.view.pane_settings[0].prev_non_ghosted_mode = ViewMode::Shaded;
                    self.view.pane_settings[0].ghosted_wireframe = false;
                    self.view.pane_settings[0].normals_mode = self.preferences.display.normals_mode;
                    self.view.pane_settings[0].uv_mode = self.preferences.display.uv_mode;
                    self.view.pane_settings[0].inspection_mode = InspectionMode::Shaded;
                    self.view.pane_settings[0].texel_density_target = 1.0;
                    self.view.pane_settings[0].pane_mode = PaneMode::Scene3D;
                    self.view.pane_settings[0].uv_bg = UvMapBackground::Dark;
                    self.view.pane_settings[0].uv_offset = [0.0, 0.0];
                    self.view.pane_settings[0].uv_zoom = 1.0;
                    self.view.pane_settings[0].show_uv_overlap = false;
                    self.view.pane_settings[0].show_validation = false;
                    self.renderer.uv_overlap.overlap_pct = None;
                    self.renderer.uv_overlap.stats_dirty = false;
                    self.view.display.turntable_active = self.preferences.display.turntable_active;
                }
                Ok(Err(e)) => {
                    self.gui.clear_loading_message();
                    self.gui
                        .set_toast(&format!("Failed to load: {}", e), ToastSeverity::Error);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.pending_load = Some(pending);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.gui.clear_loading_message();
                    self.gui
                        .set_toast("Loading thread crashed", ToastSeverity::Error);
                }
            }
        }

        let now = Instant::now();
        self.dt = (now - self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        self.view.active_pane = self.active_pane_index();
        self.ensure_pane_cameras();

        if self.view.display.turntable_active {
            let speed = self.view.display.turntable_rpm * std::f32::consts::TAU / 60.0;
            let yaw = speed * self.dt;
            let linked = self.view.cameras_linked;
            let active = self.view.active_pane;
            for (i, slot) in self.view.cameras.iter_mut().enumerate() {
                if let Some(cam) = slot
                    && (i == active || linked)
                    && !cam.is_orbiting()
                {
                    cam.inject_orbit_yaw(yaw);
                }
            }
        }

        for cam in self.view.cameras.iter_mut().flatten() {
            cam.update(&self.queue, self.dt);
        }

        if !self.view.display.lights_locked {
            let ibl_avg = self.active_ibl().irradiance_average;
            let cam0 = self.view.cameras[0].as_ref().map(|c| c.camera);
            if let (Some(cam0), Some(scene)) = (cam0, &mut self.scene) {
                scene.env.lights_uniform = lights_from_camera(&cam0, &scene.model.bounds, ibl_avg);
                self.queue.write_buffer(
                    &scene.env.light_buffer,
                    0,
                    bytemuck::cast_slice(&[scene.env.lights_uniform]),
                );
                let key_pos = scene.env.lights_uniform.lights[0].position;
                scene.env.shadow.update_light_vp(
                    &self.queue,
                    cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
                    scene.model.bounds.center(),
                    scene.model.bounds.diagonal() / 2.0,
                );
            }
        }
    }
}
