use std::sync::mpsc;

use wgpu::util::DeviceExt;

use super::*;

impl State {
    fn active_ibl(&self) -> &IblState {
        match self.ibl_res.ibl_mode {
            IblMode::Off => &self.ibl_res.ibl_fallback,
            IblMode::Diffuse | IblMode::Full => &self.ibl_res.ibl,
        }
    }

    pub(super) fn rebuild_light_bind_group(&mut self) {
        if let Some(scene) = &mut self.scene {
            scene.light_bind_group = match self.ibl_res.ibl_mode {
                IblMode::Off => create_light_bind_group(
                    &self.device,
                    &self.layouts,
                    &scene.light_buffer,
                    &self.ibl_res.ibl_fallback,
                    &self.ibl_res.brdf_lut,
                ),
                IblMode::Diffuse => create_light_bind_group_selective(
                    &self.device,
                    &self.layouts,
                    &scene.light_buffer,
                    &self.ibl_res.ibl,
                    &self.ibl_res.ibl_fallback,
                    &self.ibl_res.brdf_lut,
                ),
                IblMode::Full => create_light_bind_group(
                    &self.device,
                    &self.layouts,
                    &scene.light_buffer,
                    &self.ibl_res.ibl,
                    &self.ibl_res.brdf_lut,
                ),
            };
        }
    }

    pub(super) fn update_wireframe_params(&self) {
        let params = WireframeParams {
            color: self.display.background_mode.wireframe_color(),
            line_width: self.display.line_weight.width_px(),
            screen_width: self.config.width as f32,
            screen_height: self.config.height as f32,
            _pad: 0.0,
        };
        self.queue.write_buffer(
            &self.wire.wireframe_params_buffer,
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
        let layouts = Arc::clone(&self.layouts);
        let config = self.config.clone();
        let initial_grid_color = self.display.background_mode.grid_color();
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
            self.targets.depth_texture = texture::Texture::create_depth_texture(
                &self.device,
                &self.config,
                "depth_texture",
                self.msaa_sample_count,
            );
            self.targets.msaa_hdr_view = texture::create_msaa_hdr_texture(
                &self.device,
                width,
                height,
                self.msaa_sample_count,
            );
            let (hdr_tex, hdr_view) =
                texture::create_hdr_resolve_texture(&self.device, width, height);
            self.targets._hdr_resolve_texture = hdr_tex;
            self.targets.hdr_resolve_view = hdr_view;
            self.post.bloom.resize(
                &self.device,
                &self.layouts,
                &self.targets.hdr_resolve_view,
                width,
                height,
            );
            self.post.composite.resize(
                &self.device,
                &self.layouts,
                &self.targets.hdr_resolve_view,
                &self.post.bloom.ping_view,
                &self.post.bloom.sampler,
            );
            if let Some(scene) = &self.scene {
                self.post.ssao.resize(
                    &self.device,
                    &self.layouts,
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
                self.post
                    .ssao
                    .resize(&self.device, &self.layouts, &dummy_buf, width, height);
            }
            self.is_surface_configured = true;
            self.update_wireframe_params();
            if let Some(scene) = &mut self.scene {
                scene.cam.resize(width as f32 / height as f32);
            }
        }
    }

    pub(super) fn update_grid_color(&self) {
        if let Some(scene) = &self.scene {
            let color = self.display.background_mode.grid_color();
            self.queue
                .write_buffer(&scene.vis.grid_uniform_buf, 4, bytemuck::cast_slice(&color));
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
                    &self.layouts,
                    &new_scene.light_buffer,
                    active_ibl,
                    &self.ibl_res.brdf_lut,
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
                    self.post.ssao.rebuild_bind_groups(
                        &self.device,
                        &self.layouts,
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
                self.display.view_mode = self.preferences.display.view_mode;
                self.display.prev_non_ghosted_mode = ViewMode::Shaded;
                self.display.ghosted_wireframe = false;
                self.display.normals_mode = self.preferences.display.normals_mode;
                self.display.uv_mode = self.preferences.display.uv_mode;
                self.display.turntable_active = self.preferences.display.turntable_active;
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

        if let Some(scene) = &mut self.scene {
            if self.display.turntable_active && !scene.cam.is_orbiting() {
                let speed = self.display.turntable_rpm * std::f32::consts::TAU / 60.0;
                scene.cam.inject_orbit_yaw(speed * self.dt);
            }
            scene.cam.update(&self.queue, self.dt);

            if !self.display.lights_locked {
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
}
