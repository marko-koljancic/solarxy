use std::sync::mpsc;

use wgpu::util::DeviceExt;

use super::*;

impl State {
    fn active_ibl(&self) -> &IblState {
        match self.renderer.ibl_res.ibl_mode {
            IblMode::Off => &self.renderer.ibl_res.ibl_fallback,
            IblMode::Diffuse | IblMode::Full => &self.renderer.ibl_res.ibl,
        }
    }

    pub(super) fn rebuild_light_bind_group(&mut self) {
        if let Some(scene) = &mut self.scene {
            scene.light_bind_group = match self.renderer.ibl_res.ibl_mode {
                IblMode::Off => create_light_bind_group(
                    &self.device,
                    &self.renderer.layouts,
                    &scene.light_buffer,
                    &self.renderer.ibl_res.ibl_fallback,
                    &self.renderer.ibl_res.brdf_lut,
                ),
                IblMode::Diffuse => create_light_bind_group_selective(
                    &self.device,
                    &self.renderer.layouts,
                    &scene.light_buffer,
                    &self.renderer.ibl_res.ibl,
                    &self.renderer.ibl_res.ibl_fallback,
                    &self.renderer.ibl_res.brdf_lut,
                ),
                IblMode::Full => create_light_bind_group(
                    &self.device,
                    &self.renderer.layouts,
                    &scene.light_buffer,
                    &self.renderer.ibl_res.ibl,
                    &self.renderer.ibl_res.brdf_lut,
                ),
            };
        }
    }

    pub(super) fn update_wireframe_params(&self) {
        self.write_wireframe_params_for(&self.view.pane_settings[0]);
    }

    pub(super) fn write_gradient_colors_for(&self, pds: &PaneDisplaySettings) {
        let (top, bottom) = pds.background_mode.sky_colors();
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
            color: pds.background_mode.wireframe_color(),
            line_width: pds.line_weight.width_px(),
            screen_width: self.renderer.target_width as f32,
            screen_height: self.renderer.target_height as f32,
            _pad: 0.0,
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
        let initial_grid_color = self.view.pane_settings[0].background_mode.grid_color();
        let shadow_map_size = self.preferences.rendering.shadow_map_size;
        let path = model_path.clone();

        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let placeholder_brdf = BrdfLut::fallback(&device, &queue);
            let result = ModelScene::new(
                model_path,
                &device,
                &queue,
                &layouts,
                &config,
                initial_grid_color,
                &placeholder_brdf,
                shadow_map_size,
            );
            let _ = tx.send(result);
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

            let (tw, th) = self.target_dimensions();
            self.resize_render_targets(tw, th);

            let aspect = width as f32 / height as f32;
            if let Some(scene) = &mut self.scene {
                scene.cam.resize(aspect);
            }
            if let Some(cam) = &mut self.view.secondary_cam {
                cam.resize(aspect);
            }
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

        if let Some(scene) = &self.scene {
            self.renderer.post.ssao.resize(
                &self.device,
                &self.renderer.layouts,
                &scene.cam.buffer,
                width,
                height,
            );
        } else {
            let dummy_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Dummy Camera Buffer for SSAO resize"),
                    contents: &[0u8; 288],
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            self.renderer.post.ssao.resize(
                &self.device,
                &self.renderer.layouts,
                &dummy_buf,
                width,
                height,
            );
        }
    }

    pub fn update(&mut self) {
        let poll = self.pending_load.as_ref().map(|p| p.receiver.try_recv());
        match poll {
            Some(Ok(Ok(mut new_scene))) => {
                let pending = self.pending_load.take().unwrap();
                let active_ibl = self.active_ibl();
                new_scene.light_bind_group = create_light_bind_group(
                    &self.device,
                    &self.renderer.layouts,
                    &new_scene.light_buffer,
                    active_ibl,
                    &self.renderer.ibl_res.brdf_lut,
                );
                let file_size = std::fs::metadata(&pending.path)
                    .map(|m| m.len())
                    .unwrap_or(0);
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
                self.gui.clear_loading_message();
                self.window
                    .set_title(&format!("Solarxy \u{2014} {}", pending.filename));
                preferences::add_recent_file(&mut self.preferences, &pending.path);
                self.scene = Some(new_scene);
                if let Some(scene) = &self.scene {
                    self.renderer.post.ssao.rebuild_bind_groups(
                        &self.device,
                        &self.renderer.layouts,
                        &scene.cam.buffer,
                    );
                }
                if let Some(scene) = &mut self.scene {
                    scene
                        .cam
                        .resize(self.config.width as f32 / self.config.height as f32);
                    scene
                        .cam
                        .set_projection(self.preferences.display.projection_mode);
                }

                self.view.secondary_cam = None;
                if self.view.display.layout != ViewLayout::Single
                    && let Some(scene) = &self.scene
                {
                    self.view.secondary_cam = Some(
                        scene
                            .cam
                            .clone_with_new_resources(&self.device, &self.renderer.layouts.camera),
                    );
                }

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
            Some(Ok(Err(e))) => {
                self.pending_load.take();
                self.gui.clear_loading_message();
                self.gui
                    .set_toast(&format!("Failed to load: {}", e), [0.6, 0.0, 0.0, 1.0]);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.pending_load.take();
                self.gui.clear_loading_message();
                self.gui
                    .set_toast("Loading thread crashed", [0.6, 0.0, 0.0, 1.0]);
            }
            _ => {}
        }

        let now = Instant::now();
        self.dt = (now - self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        self.view.active_pane = self.active_pane_index();

        if self.view.display.turntable_active {
            let speed = self.view.display.turntable_rpm * std::f32::consts::TAU / 60.0;
            let yaw = speed * self.dt;
            if let Some(scene) = &mut self.scene
                && (self.view.active_pane == 0 || self.view.cameras_linked)
                && !scene.cam.is_orbiting()
            {
                scene.cam.inject_orbit_yaw(yaw);
            }
            if (self.view.active_pane == 1 || self.view.cameras_linked)
                && let Some(cam) = &mut self.view.secondary_cam
                && !cam.is_orbiting()
            {
                cam.inject_orbit_yaw(yaw);
            }
        }

        if let Some(scene) = &mut self.scene {
            scene.cam.update(&self.queue, self.dt);
        }
        if let Some(cam) = &mut self.view.secondary_cam {
            cam.update(&self.queue, self.dt);
        }

        if let Some(scene) = &mut self.scene
            && !self.view.display.lights_locked
        {
            scene.lights_uniform = lights_from_camera(&scene.cam.camera, &scene.model.bounds);
            self.queue.write_buffer(
                &scene.light_buffer,
                0,
                bytemuck::cast_slice(&[scene.lights_uniform]),
            );
            let key_pos = scene.lights_uniform.lights[0].position;
            scene.shadow.update_light_vp(
                &self.queue,
                cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
                scene.model.bounds.center(),
                scene.model.bounds.diagonal() / 2.0,
            );
        }
    }
}
