//! `State::render`: per-frame entry point. Builds a per-pane camera,
//! invokes [`solarxy_renderer::frame::Renderer::render_pane`] for each pane,
//! and drives the egui sidebar/menu/HUD/console paint at the end.
//!
//! Reads `GuiSnapshot::from_state` then calls `apply_to_state` after the
//! sidebar has had a chance to mutate it; the resulting `SidebarChanges`
//! drives any expensive recomputations (background, wireframe, composite,
//! IBL).

use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::visualization::GridUniform;
use solarxy_core::preferences::{
    InspectionMode, MaterialOverride, PaneMode, ResolvedBackground, UvMapBackground,
};

use super::overlap::request_overlap_readback_impl;
use super::view_state::PaneDisplaySettings;
use super::{BackgroundModeExt, GradientUniform, Pane, State, WireframeParams, lights_from_camera};

impl State {
    /// Resolve a pane's background choice against the user
    /// custom-background registry into concrete colours for the renderer
    /// and IBL. A dangling `Custom` id falls back to the builtin Gradient.
    pub(super) fn resolve_background(&self, pds: &PaneDisplaySettings) -> ResolvedBackground {
        pds.background_mode
            .resolve(&self.preferences.view.custom_backgrounds)
    }

    /// Per-frame render entry point. Computes pane rectangles, dispatches
    /// per-pane scene/UV passes, paints the egui overlay (sidebar, menu,
    /// HUD, console, modals, toasts), and presents the swapchain frame.
    ///
    /// Wraps `GuiSnapshot::from_state` → sidebar mutation → `apply_to_state`
    /// each frame; the resulting [`crate::gui::SidebarChanges`] flags drive
    /// any expensive recomputations (background gradient rebuild, wireframe
    /// params upload, composite params upload, IBL bind-group rebuild).
    ///
    /// # Errors
    /// Returns `Err` if the surface texture is unavailable (e.g. the window
    /// was minimised between frames) or if the GPU device is lost.
    pub fn render(&mut self) -> anyhow::Result<()> {
        self.window.request_redraw();
        if !self.is_surface_configured {
            return Ok(());
        }

        let frame_ms = self.dt * 1000.0;
        self.gui.clear_expired_toasts();
        self.sync_render_target_dims();

        let output = self.surface.get_current_texture()?;
        let surface_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.poll_overlap_stats();

        let viewport_present = self.gui.viewport_tab_present();
        if !viewport_present {
            self.clear_surface(&surface_view);
            self.render_gui_overlay(&output, &[], false, frame_ms);
            output.present();
            return Ok(());
        }

        let panes = self.compute_panes();
        let is_split = panes.len() > 1;

        for (i, pane) in panes.iter().enumerate() {
            self.render_pane(i, pane, &surface_view, is_split);
        }

        self.render_gui_overlay(&output, &panes, is_split, frame_ms);
        output.present();
        Ok(())
    }

    /// Issue a single clear pass writing the dock background color into the
    /// surface. Used when the Viewport tab is hidden — egui still needs a
    /// fresh canvas to paint the docked panels into.
    fn clear_surface(&self, surface_view: &wgpu::TextureView) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Surface Clear (Viewport hidden)"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Surface Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.122,
                            g: 0.141,
                            b: 0.188,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
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
        // The 3D scene renders the full pane — the per-pane toolbar labels
        // float on top of it (3ds Max style), no reserved strip.
        let pane_aspect = pane.width / pane.height;

        let cam_data = self.view.cameras[i].as_ref().map(|c| c.camera);

        let pds = self.view.pane_settings[i];

        let Some(cam_data) = cam_data else {
            self.renderer
                .render_empty_pass(&mut encoder, self.resolve_background(&pds));
            self.composite_and_submit(encoder, surface_view, i, pane, is_split, false, false);
            return;
        };

        let is_uv_map = pds.pane_mode == PaneMode::UvMap;

        if is_uv_map {
            self.render_uv_map_pane(&mut encoder, pane_aspect, &pds);
        } else {
            if let Some(cam) = &mut self.view.cameras[i] {
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
        }

        self.composite_and_submit(encoder, surface_view, i, pane, is_split, is_uv_map, true);
    }

    fn render_overdraw_pane(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        i: usize,
        pane: &Pane,
        is_split: bool,
    ) {
        let cam_bg = self.view.cameras[i].as_ref().map(|c| &c.bind_group);
        let Some(scene) = &self.scene else {
            return;
        };
        let Some(cam_bg) = cam_bg else {
            return;
        };
        let pane_viewport = if is_split {
            Some([pane.x, pane.y, pane.width, pane.height])
        } else {
            None
        };
        self.renderer
            .render_overdraw_passes(encoder, scene, cam_bg, pane_viewport);
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
        let pane_inspection = self.view.pane_settings[i].inspection_mode;
        self.renderer.post.composite.write_params(
            &self.queue,
            pane_bloom,
            pane_ssao,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
            pane_inspection,
        );

        let _ = is_split;
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
                self.renderer
                    .render_empty_pass(encoder, self.resolve_background(pds));
            }
        } else {
            self.renderer
                .render_empty_pass(encoder, self.resolve_background(pds));
        }
    }

    fn write_3d_pane_uniforms(&self, i: usize, pds: &PaneDisplaySettings) {
        self.write_wireframe_params_for(pds);
        self.write_gradient_colors_for(pds);
        if let Some(scene) = &self.scene {
            let color = self.resolve_background(pds).grid_color();
            self.queue.write_buffer(
                &scene.vis.grid_uniform_buf,
                GridUniform::COLOR_OFFSET,
                bytemuck::cast_slice(&color),
            );
        }

        let cam_buf = self.view.cameras[i].as_ref().map(|c| &c.buffer);
        let depth_bounds = self.view.cameras[i]
            .as_ref()
            .zip(self.scene.as_ref())
            .map(|(c, s)| Self::compute_depth_bounds(&c.camera, &s.model.bounds));
        if let Some(buf) = cam_buf {
            let (depth_near, depth_far) = depth_bounds.unwrap_or((0.01, 100.0));
            let data: [u32; 8] = [
                pds.inspection_mode.as_u32(),
                pds.texel_density_target.to_bits(),
                pds.material_override.as_u32(),
                depth_near.to_bits(),
                depth_far.to_bits(),
                self.view.display.roughness_scale.to_bits(),
                self.view.display.metallic_scale.to_bits(),
                self.view.display.hdri_rotation.to_bits(),
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

        let cam_bg = self.view.cameras[i].as_ref().map(|c| &c.bind_group);
        if let (Some(scene), Some(cam_bg)) = (&self.scene, cam_bg) {
            if self.renderer.post.ssao_enabled {
                self.renderer.render_gbuffer_pass(encoder, scene, cam_bg);
            }
            self.renderer.render_main_pass(
                encoder,
                scene,
                cam_bg,
                cam_data,
                pds,
                self.resolve_background(pds),
            );
        } else {
            self.renderer
                .render_empty_pass(encoder, self.resolve_background(pds));
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

    /// Recompute the camera-relative light rig for a non-primary pane
    /// from `cam_data` before it renders, so each pane is lit from its
    /// own viewpoint. No-op when lights are locked. Pane 0 keeps the rig
    /// `update()` set from slot 0's camera.
    fn setup_pane_lighting(&mut self, cam_data: &Camera) {
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

        let divider = match (self.compute_divider_rect(), self.compute_divider_hit_rect()) {
            (Some(visible), Some(hit)) => Some(crate::gui::DividerInfo {
                visible,
                hit,
                layout: self.view.display.layout,
            }),
            _ => None,
        };

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        let ppp = self.window.scale_factor() as f32;
        let active_pane_rect = if is_split {
            panes.get(self.view.active_pane).map(|p| {
                egui::Rect::from_min_size(
                    egui::pos2(p.x / ppp, p.y / ppp),
                    egui::vec2(p.width / ppp, p.height / ppp),
                )
            })
        } else {
            None
        };

        let review_panes = self.build_review_panes(panes, ppp);

        let pane_rects: Vec<egui::Rect> = panes
            .iter()
            .map(|p| {
                egui::Rect::from_min_size(
                    egui::pos2(p.x / ppp, p.y / ppp),
                    egui::vec2(p.width / ppp, p.height / ppp),
                )
            })
            .collect();
        let default_projection = self.preferences.display.projection_mode;
        let pane_projections: [solarxy_core::preferences::ProjectionMode; 4] =
            std::array::from_fn(|i| {
                self.view.cameras[i]
                    .as_ref()
                    .map_or(default_projection, |c| c.camera.projection)
            });
        let mut projection_change = None;
        let mut properties_events = crate::gui::PropertiesEvents::default();
        let mut outliner_events = crate::gui::OutlinerEvents::default();

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

        let projection_mode = self.view.cameras[ap]
            .as_ref()
            .map_or(self.preferences.display.projection_mode, |c| {
                c.camera.projection
            });
        let snap_before = GuiSnapshot::from_state(
            pds,
            &self.view.display,
            &self.renderer.post,
            self.renderer.ibl_res.ibl_mode,
            self.view.cameras_linked,
            is_split,
            projection_mode,
        );
        let active_inspection = self.view.pane_settings[self.view.active_pane].inspection_mode;
        let active_pane_mode = self.view.pane_settings[self.view.active_pane].pane_mode;
        let hud = HudInfo {
            pane_label,
            cameras_linked: if is_split {
                Some(self.view.cameras_linked)
            } else {
                None
            },
            has_uvs: self.scene.as_ref().is_some_and(|s| s.model.has_uvs),
            overdraw_active: active_inspection == InspectionMode::Overdraw
                && active_pane_mode == PaneMode::Scene3D,
        };
        let validation_report = self.scene.as_ref().map(|s| &s.validation);

        let recent_files = self.preferences.history.recent_files.clone();
        let model = self.scene.as_ref().map(|s| &s.model);
        // `PaneToolbarData` is passed by value — `render_ui` consumes it,
        // releasing its `&mut self.view.pane_settings` borrow before
        // `apply_to_state` re-borrows the same field below.
        let hdri_available = self.renderer.ibl_res.ibl.equirect.is_some();
        let uv_overlap_pct = self.renderer.uv_overlap.overlap_pct;
        let pane_toolbar = crate::gui::PaneToolbarData {
            rects: &pane_rects,
            active: ap,
            pane_settings: &mut self.view.pane_settings,
            projections: pane_projections,
            projection_change: &mut projection_change,
            hdri_available,
            customs: &self.preferences.view.custom_backgrounds,
            uv_overlap_pct,
        };
        // The screenshot modal is suppressed on any capture frame so it
        // cannot land in the shot; a re-capture additionally forces every
        // review card open for that frame.
        let suppress_screenshot_modal = self.capture_requested;
        let force_expand_review = self.capture_requested && self.screenshot_expand_review;
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
            divider,
            active_pane_rect,
            &review_panes,
            &recent_files,
            &mut self.review,
            model,
            pane_toolbar,
            &mut properties_events,
            &mut outliner_events,
            &mut self.viewport_context_menu,
            force_expand_review,
            suppress_screenshot_modal,
        );

        if let Some((i, proj)) = projection_change
            && let Some(cam) = &mut self.view.cameras[i]
        {
            cam.set_projection(proj);
        }

        let changes = snap_after.apply_to_state(
            &snap_before,
            &mut self.view.pane_settings[ap],
            &mut self.view.display,
            &mut self.renderer.post,
            &mut self.renderer.ibl_res.ibl_mode,
            &mut self.view.cameras_linked,
        );

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

        // Properties-panel events: validation fly-to + HDRI load/clear.
        if let Some(idx) = properties_events.fly_to_issue {
            self.fly_to_validation_issue(idx);
        }
        if properties_events.clear_hdri {
            self.clear_hdri();
        }
        if properties_events.load_hdri {
            self.open_hdri_dialog();
        }

        // Outliner events: mesh / material visibility + camera framing.
        if let Some(action) = outliner_events.action {
            self.handle_outliner_action(action);
        }

        // Review panel: clicking a note row flies the camera to its anchor.
        if let Some(id) = self.review.focus_request.take() {
            self.focus_review_annotation(&id);
        }
        // Review panel Save button.
        if self.review.save_requested {
            self.review.save_requested = false;
            self.save_review_sidecar();
        }

        if let Some(new_prefs) = self.gui.take_committed_prefs() {
            let theme_changed = self.preferences.ui.theme != new_prefs.ui.theme;
            self.preferences = new_prefs;
            if theme_changed {
                self.gui.apply_theme_choice(self.preferences.ui.theme);
            }
            // The reviewer name is mirrored onto `ReviewState`; refresh it
            // so new annotations pick up the change without a model reload.
            self.review
                .author
                .clone_from(&self.preferences.review.author);
            let cap = self.preferences.ui.max_recent_files.max(1);
            if self.preferences.history.recent_files.len() > cap {
                self.preferences.history.recent_files.truncate(cap);
            }
            self.gui
                .set_toast("Preferences saved", crate::gui::ToastSeverity::Success);
        }

        let capture = if self.capture_requested {
            self.capture_requested = false;
            self.encode_active_pane_capture(panes, &output.texture, &mut encoder)
        } else {
            None
        };

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some((buffer, padded_row_bytes, width, height)) = capture
            && let Some(image) = self.read_capture(buffer, padded_row_bytes, width, height)
        {
            let filename = self.screenshot_filename();
            self.gui.set_screenshot_capture(
                image,
                filename,
                self.review.active,
                self.screenshot_expand_review,
            );
        }

        self.handle_screenshot_modal();
    }

    /// Build the per-pane data the egui review overlay needs: one
    /// `ReviewPaneOverlay` per `Scene3D` pane, pairing the pane's
    /// egui-logical rect with the pane camera's `view * proj` matrix.
    /// UV panes are skipped (markers never render on UV map panes).
    fn build_review_panes(&self, panes: &[Pane], ppp: f32) -> Vec<crate::gui::ReviewPaneOverlay> {
        let mut out = Vec::with_capacity(panes.len());
        for (i, pane) in panes.iter().enumerate() {
            let pds = self.view.pane_settings[i];
            if pds.pane_mode != PaneMode::Scene3D {
                continue;
            }
            // The 3D scene now fills the whole pane (the toolbar labels
            // float over it), so markers project against the full rect.
            let pane_aspect = if pane.height > 0.0 {
                pane.width / pane.height
            } else {
                1.0
            };
            let Some(mut cam) = self.view.cameras[i].as_ref().map(|c| c.camera) else {
                continue;
            };
            cam.aspect = pane_aspect;
            let view_proj = cam.build_view_projection_matrix();
            let egui_rect = egui::Rect::from_min_size(
                egui::pos2(pane.x / ppp, pane.y / ppp),
                egui::vec2(pane.width / ppp, pane.height / ppp),
            );
            out.push(crate::gui::ReviewPaneOverlay {
                egui_rect,
                view_proj,
            });
        }
        out
    }

    /// Recreate HDR + derived render targets to match the current
    /// Viewport-tab rect dims when those have changed since last frame.
    /// No-op steady-state — `resize_render_targets` has its own
    /// early-out when dims already match. Triggered each frame after
    /// the previous frame's egui pass populated `last_viewport_rect`;
    /// also a no-op when no rect is cached (full-surface fallback).
    fn sync_render_target_dims(&mut self) {
        let (target_w, target_h) = self.target_dimensions();
        if target_w == self.renderer.target_width && target_h == self.renderer.target_height {
            return;
        }
        self.resize_render_targets(target_w, target_h);
    }
}
