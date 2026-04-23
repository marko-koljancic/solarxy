use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::visualization::GridUniform;
use solarxy_core::preferences::{MaterialOverride, PaneMode, UvMapBackground};

use super::overlap::request_overlap_readback_impl;
use super::view_state::PaneDisplaySettings;
use super::{BackgroundModeExt, GradientUniform, Pane, State, WireframeParams, lights_from_camera};

impl State {
    pub fn render(&mut self) -> anyhow::Result<()> {
        self.window.request_redraw();
        if !self.is_surface_configured {
            return Ok(());
        }

        let frame_ms = self.dt * 1000.0;
        self.gui.clear_expired_toast();

        let output = self.surface.get_current_texture()?;
        let surface_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.poll_overlap_stats();

        let panes = self.compute_panes();
        let is_split = panes.len() > 1;

        for (i, pane) in panes.iter().enumerate() {
            self.render_pane(i, pane, &surface_view, is_split);
        }

        self.render_gui_overlay(&output, &panes, is_split, frame_ms);
        output.present();
        Ok(())
    }

    fn render_pane(
        &mut self,
        i: usize,
        pane: &Pane,
        surface_view: &wgpu::TextureView,
        is_split: bool,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Pane Encoder"),
            });
        let pane_aspect = pane.width / pane.height;

        let cam_data = if i == 0 {
            self.scene.as_ref().map(|s| s.cam.camera)
        } else {
            self.view
                .secondary_cam
                .as_ref()
                .map(|c| c.camera)
                .or(self.scene.as_ref().map(|s| s.cam.camera))
        };

        let pds = self.view.pane_settings[i.min(1)];

        let Some(cam_data) = cam_data else {
            self.renderer.render_empty_pass(&mut encoder, &pds);
            self.composite_and_submit(encoder, surface_view, i, pane, is_split, false, false);
            return;
        };

        let is_uv_map = pds.pane_mode == PaneMode::UvMap;

        if is_uv_map {
            self.render_uv_map_pane(&mut encoder, pane_aspect, &pds);
        } else {
            if i == 0 {
                if let Some(scene) = &mut self.scene {
                    scene.cam.write_with_aspect(&self.queue, pane_aspect);
                }
            } else if let Some(sec) = &mut self.view.secondary_cam {
                sec.write_with_aspect(&self.queue, pane_aspect);
            }

            if is_split && i == 1 {
                self.setup_split_secondary(&cam_data);
            }

            self.write_3d_pane_uniforms(i, &pds);
            self.render_3d_passes(&mut encoder, i, &cam_data, &pds);
        }

        self.composite_and_submit(encoder, surface_view, i, pane, is_split, is_uv_map, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_and_submit(
        &self,
        mut encoder: wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        i: usize,
        pane: &Pane,
        is_split: bool,
        is_uv_map: bool,
        scene_present: bool,
    ) {
        let pane_bloom = self.renderer.post.bloom_enabled && !is_uv_map && scene_present;
        let pane_ssao = self.renderer.post.ssao_enabled && !is_uv_map && scene_present;
        self.renderer.post.composite.write_params(
            &self.queue,
            pane_bloom,
            pane_ssao,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
        );
        let viewport = if is_split {
            Some([pane.x, pane.y, pane.width, pane.height])
        } else {
            None
        };
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

    fn render_uv_map_pane(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pane_aspect: f32,
        pds: &PaneDisplaySettings,
    ) {
        if let Some(scene) = &self.scene {
            if scene.model.has_uvs {
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
                if pds.show_uv_overlap {
                    self.renderer.render_uv_overlap_count_pass(
                        encoder,
                        scene,
                        &self.renderer.uv_cam.bind_group,
                        &self.renderer.uv_overlap.count_view,
                    );
                    if self.renderer.uv_overlap.stats_dirty
                        && !self.renderer.uv_overlap.readback_pending
                    {
                        self.renderer
                            .uv_cam
                            .write(&self.queue, [0.0, 0.0], 1.0, 1.0);
                        self.renderer.render_uv_overlap_count_pass(
                            encoder,
                            scene,
                            &self.renderer.uv_cam.bind_group,
                            &self.renderer.uv_overlap.stats_view,
                        );
                        request_overlap_readback_impl(
                            &self.device,
                            &mut self.renderer.uv_overlap,
                            encoder,
                        );
                        self.renderer.uv_cam.write(
                            &self.queue,
                            pds.uv_offset,
                            pds.uv_zoom,
                            pane_aspect,
                        );
                    }
                }
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
                self.renderer.render_uv_map_pass(
                    encoder,
                    scene,
                    &self.renderer.uv_cam.bind_group,
                    pds,
                );
            } else {
                self.renderer.render_empty_pass(encoder, pds);
            }
        } else {
            self.renderer.render_empty_pass(encoder, pds);
        }
    }

    fn write_3d_pane_uniforms(&self, i: usize, pds: &PaneDisplaySettings) {
        self.write_wireframe_params_for(pds);
        self.write_gradient_colors_for(pds);
        if let Some(scene) = &self.scene {
            let color = pds.background_mode.grid_color();
            self.queue.write_buffer(
                &scene.vis.grid_uniform_buf,
                GridUniform::COLOR_OFFSET,
                bytemuck::cast_slice(&color),
            );
        }

        let (cam_buf, depth_bounds) = if i == 0 {
            (
                self.scene.as_ref().map(|s| &s.cam.buffer),
                self.scene
                    .as_ref()
                    .map(|s| Self::compute_depth_bounds(&s.cam.camera, &s.model.bounds)),
            )
        } else {
            (
                self.view.secondary_cam.as_ref().map(|c| &c.buffer),
                self.view
                    .secondary_cam
                    .as_ref()
                    .zip(self.scene.as_ref())
                    .map(|(c, s)| Self::compute_depth_bounds(&c.camera, &s.model.bounds)),
            )
        };
        if let Some(buf) = cam_buf {
            let (depth_near, depth_far) = depth_bounds.unwrap_or((0.01, 100.0));
            let data: [u32; 7] = [
                pds.inspection_mode.as_u32(),
                pds.texel_density_target.to_bits(),
                pds.material_override.as_u32(),
                depth_near.to_bits(),
                depth_far.to_bits(),
                self.view.display.roughness_scale.to_bits(),
                self.view.display.metallic_scale.to_bits(),
            ];
            self.queue.write_buffer(
                buf,
                CameraUniform::INSPECTION_OFFSET,
                bytemuck::cast_slice(&data),
            );
        }
    }

    fn compute_depth_bounds(
        camera: &solarxy_renderer::camera::Camera,
        bounds: &solarxy_core::AABB,
    ) -> (f32, f32) {
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

    fn render_3d_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        i: usize,
        cam_data: &Camera,
        pds: &PaneDisplaySettings,
    ) {
        if (i == 0 || !self.view.display.lights_locked)
            && let Some(scene) = &self.scene
        {
            self.renderer.render_shadow_pass(encoder, scene);
        }

        let cam_bg = if i == 0 {
            self.scene.as_ref().map(|s| &s.cam.bind_group)
        } else {
            self.view
                .secondary_cam
                .as_ref()
                .map(|c| &c.bind_group)
                .or(self.scene.as_ref().map(|s| &s.cam.bind_group))
        };
        if let (Some(scene), Some(cam_bg)) = (&self.scene, cam_bg) {
            if self.renderer.post.ssao_enabled {
                self.renderer.render_gbuffer_pass(encoder, scene, cam_bg);
            }
            self.renderer
                .render_main_pass(encoder, scene, cam_bg, cam_data, pds);
        } else {
            self.renderer.render_empty_pass(encoder, pds);
        }

        if self.renderer.post.ssao_enabled
            && let Some(cam_bg) = cam_bg
        {
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

    fn setup_split_secondary(&mut self, cam_data: &Camera) {
        if !self.view.display.lights_locked {
            let ibl_avg = self.active_ibl().irradiance_average;
            if let Some(scene) = &mut self.scene {
                scene.lights_uniform = lights_from_camera(cam_data, &scene.model.bounds, ibl_avg);
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

    fn render_gui_overlay(
        &mut self,
        output: &wgpu::SurfaceTexture,
        panes: &[Pane],
        is_split: bool,
        frame_ms: f32,
    ) {
        use crate::gui::{GuiSnapshot, HudInfo};

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("UI Encoder"),
            });

        let divider_rect = self.compute_divider_rect();

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        let active_pane_rect = if is_split {
            let ppp = self.window.scale_factor() as f32;
            panes.get(self.view.active_pane).map(|p| {
                egui::Rect::from_min_size(
                    egui::pos2(p.x / ppp, p.y / ppp),
                    egui::vec2(p.width / ppp, p.height / ppp),
                )
            })
        } else {
            None
        };

        let ap = self.view.active_pane;
        let pds = &self.view.pane_settings[ap];

        let pane_label = {
            let pane_mode_str = pds.pane_mode.to_string();
            let mut label = if is_split {
                let mode_detail = if pds.pane_mode == PaneMode::Scene3D {
                    format!("{} \u{00b7} {}", pane_mode_str, pds.view_mode)
                } else {
                    pane_mode_str
                };
                format!("Pane {} \u{00b7} {}", ap + 1, mode_detail)
            } else if pds.pane_mode == PaneMode::Scene3D {
                format!("{} \u{00b7} {}", pane_mode_str, pds.view_mode)
            } else {
                pane_mode_str
            };
            if pds.material_override != MaterialOverride::None {
                label = format!("{} \u{00b7} {}", label, pds.material_override);
            }
            label
        };

        let projection_mode = {
            let pref = self.preferences.display.projection_mode;
            if ap == 1 {
                self.view
                    .secondary_cam
                    .as_ref()
                    .map_or(pref, |c| c.camera.projection)
            } else {
                self.scene
                    .as_ref()
                    .map_or(pref, |s| s.cam.camera.projection)
            }
        };
        let snap_before = GuiSnapshot::from_state(
            pds,
            &self.view.display,
            &self.renderer.post,
            self.renderer.ibl_res.ibl_mode,
            self.view.cameras_linked,
            is_split,
            projection_mode,
        );
        let hud = HudInfo {
            pane_label,
            cameras_linked: if is_split {
                Some(self.view.cameras_linked)
            } else {
                None
            },
            has_uvs: self.scene.as_ref().is_some_and(|s| s.model.has_uvs),
            uv_overlap_pct: self.renderer.uv_overlap.overlap_pct,
        };
        let validation_report = self.scene.as_ref().map(|s| &s.validation);

        let recent_files = self.preferences.history.recent_files.clone();
        let (snap_after, actions) = self.gui.render_ui(
            snap_before,
            &hud,
            validation_report,
            &self.device,
            &self.queue,
            &mut encoder,
            &self.window,
            &output.texture,
            screen,
            frame_ms,
            divider_rect,
            active_pane_rect,
            &recent_files,
        );

        let changes = snap_after.diff(&snap_before);
        snap_after.write_back_pane(&mut self.view.pane_settings[ap]);
        snap_after.write_back_display(&mut self.view.display);
        snap_after.write_back_post(&mut self.renderer.post);
        self.renderer.ibl_res.ibl_mode = snap_after.ibl_mode;
        self.view.cameras_linked = snap_after.cameras_linked;

        if changes.background_changed {
            self.apply_background_change();
        } else if changes.wireframe_params_changed {
            self.update_wireframe_params();
        }
        if changes.composite_params_changed {
            self.apply_composite_params();
        }
        if changes.ibl_changed {
            self.apply_ibl_change();
        }

        self.handle_menu_actions(actions);

        if let Some(new_prefs) = self.gui.take_committed_prefs() {
            self.preferences = new_prefs;
            let cap = self.preferences.ui.max_recent_files.max(1);
            if self.preferences.history.recent_files.len() > cap {
                self.preferences.history.recent_files.truncate(cap);
            }
            self.gui
                .set_toast("Preferences saved", crate::gui::ToastSeverity::Success);
        }

        let capture_buffer = if self.capture_requested {
            self.capture_requested = false;
            Some(self.encode_capture(&output.texture, &mut encoder))
        } else {
            None
        };

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some((buffer, padded_row_bytes, width, height)) = capture_buffer {
            self.save_capture(buffer, padded_row_bytes, width, height);
        }
    }
}
