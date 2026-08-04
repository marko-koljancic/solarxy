//! `State::render`: per-frame entry point. Builds a per-pane camera,
//! invokes [`solarxy_renderer::frame::Renderer::render_pane`] for each pane,
//! and drives the egui sidebar/menu/HUD/console paint at the end.
//!
//! Reads `GuiSnapshot::from_state` then calls `apply_to_state` after the
//! sidebar has had a chance to mutate it; the resulting `SidebarChanges`
//! drives any expensive recomputations (background, wireframe, composite,
//! IBL).

use solarxy_renderer::camera::Camera;
use solarxy_core::preferences::{
    InspectionMode, MaterialOverride, PaneMode, ResolvedBackground, UvMapBackground,
};

use super::overlap::request_overlap_readback_impl;
use super::view_state::PaneDisplaySettings;
use super::{GradientUniform, Pane, State, WireframeParams};

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
        self.poll_pending_capture();

        // Drain queued scene deltas into the multi-object scene before any
        // pane encodes (the engine's per-frame commit point, next milestone).
        if !self.pending_scene_deltas.is_empty() {
            for delta in std::mem::take(&mut self.pending_scene_deltas) {
                if let Err(e) = self.scene_objects.apply(
                    &self.device,
                    &self.queue,
                    &self.renderer.layouts,
                    &delta,
                ) {
                    tracing::error!("Scene delta apply failed: {e}");
                }
                self.apply_scene_environment(&delta);
            }
            // The panels read summed counters and one merged validation
            // report. Both are derived from what just landed, so they are
            // rebuilt here rather than per frame: a delta is the only thing
            // that can change either.
            self.refresh_engine_scene_info();
        }

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
            self.composite_and_submit(encoder, surface_view, i, pane, false, false);
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
                self.render_overdraw_pane(&mut encoder, i, *pane, is_split);
            } else {
                self.render_3d_passes(&mut encoder, i, &cam_data, &pds);
            }
        }

        self.composite_and_submit(
            encoder,
            surface_view,
            i,
            pane,
            is_uv_map,
            self.scene_present(),
        );
    }

    fn render_overdraw_pane(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        i: usize,
        pane: Pane,
        is_split: bool,
    ) {
        let Some(cam_bg) = self.view.cameras[i].as_ref().map(|c| &c.bind_group) else {
            return;
        };
        // No scene guard: the count target is cleared by the pass itself and
        // the show pass writes the zero-count colour, so a pane with nothing
        // in it reads black — which is what overdraw already shows wherever
        // geometry does not cover.
        let objects = self.draw_objects();
        solarxy_host::render_overdraw_pane(
            &self.renderer,
            encoder,
            &objects,
            cam_bg,
            pane,
            is_split,
        );
    }

    /// The frame's draw list: the file-loaded model when one is open, then
    /// every visible multi-object entry.
    ///
    /// Order is load-bearing, not incidental. Overdraw counts fragments in
    /// submission order, and the depth-equal overlays (edge wireframe,
    /// validation lines) resolve against whatever landed first, so the file
    /// model stays ahead of the delta-fed objects exactly as it did when it
    /// was the only entry that could come first.
    ///
    /// An empty list is a legitimate frame. The background, grid, floor and
    /// axes come from the environment, not from this list.
    fn draw_objects(&self) -> Vec<solarxy_renderer::frame::DrawObject<'_>> {
        let mut objects =
            Vec::with_capacity(usize::from(self.scene.is_some()) + self.scene_objects.len());
        if let Some(scene) = &self.scene {
            objects.push(scene.draw_object(&self.env.instance_buffer));
        }
        objects.extend(self.scene_objects.draw_objects());
        objects
    }

    /// Whether this frame has scene content: a file-loaded model, or at least
    /// one visible object in the multi-object scene.
    ///
    /// The composite pass folds in the bloom and ambient-occlusion textures
    /// only when this is true. It deliberately is not "does the pane have a
    /// camera": a pane with a camera and nothing in it renders the
    /// background, the grid and the floor, and blooming that would put a glow
    /// on a bare viewport nobody asked for.
    fn scene_present(&self) -> bool {
        self.scene.is_some() || self.scene_objects.draw_objects().next().is_some()
    }

    fn composite_and_submit(
        &self,
        encoder: wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        i: usize,
        pane: &Pane,
        is_uv_map: bool,
        scene_present: bool,
    ) {
        // This shell has no camera nodes yet, so every pane is a free view and
        // resolves to its own look. Equal field for field to the
        // `CompositeLook::from_tone` this used to build, which is what makes
        // adopting the shared path golden-neutral here.
        let look = solarxy_renderer::composite::resolve_look(
            None,
            &solarxy_core::view_config::PaneLook::from_tone(
                self.renderer.post.tone_mode,
                self.renderer.post.exposure,
            ),
        );
        solarxy_host::composite_and_submit(
            &self.queue,
            &self.renderer,
            encoder,
            surface_view,
            &solarxy_host::PaneComposite {
                index: i,
                rect: *pane,
                look,
                inspection: self.view.pane_settings[i].inspection_mode,
                is_uv_map,
                scene_present,
                // No selection concept in this shell yet, so the rim is never
                // blitted.
                outline: false,
            },
        );
    }

    fn render_uv_map_pane(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pane_aspect: f32,
        pds: &PaneDisplaySettings,
    ) {
        if let Some(scene) = &self.scene {
            if scene.model.has_uvs {
                let uv_object = scene.draw_object(&self.env.instance_buffer);
                self.renderer
                    .uv_cam
                    .write(&self.queue, pds.uv_offset, pds.uv_zoom, pane_aspect);
                let uv_wire = WireframeParams {
                    color: [0.8, 0.8, 0.8, 1.0],
                    line_width: pds.line_weight.width_px(),
                    screen_width: self.renderer.target_width as f32,
                    screen_height: self.renderer.target_height as f32,
                    // The UV pass draws no points; carrying the default keeps
                    // the shared uniform coherent for the next 3D pass.
                    point_size: solarxy_core::view_config::DEFAULT_POINT_SIZE,
                };
                self.queue.write_buffer(
                    &self.renderer.wire.wireframe_params_buffer,
                    0,
                    bytemuck::bytes_of(&uv_wire),
                );
                if pds.show_uv_overlap {
                    self.renderer.render_uv_overlap_count_pass(
                        encoder,
                        &uv_object,
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
                            &uv_object,
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
                    &uv_object,
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
        // Bound locally: `PaneUniforms` holds a borrow, so an inline
        // `Some(&self.scene_bounds())` would not outlive the statement.
        let bounds = self.scene_bounds();
        solarxy_host::write_pane_uniforms(
            &self.queue,
            &self.renderer,
            &solarxy_host::PaneUniforms {
                background: self.resolve_background(pds),
                pds,
                display: &self.view.display,
                camera: self.view.cameras[i].as_ref(),
                env: &self.env,
                bounds: Some(&bounds),
                // This shell does not steer the grid plane from the camera, so
                // the plane offset is left exactly as it was initialised.
                grid_plane: None,
            },
        );
    }

    fn render_3d_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        i: usize,
        cam_data: &Camera,
        pds: &PaneDisplaySettings,
    ) {
        let background = self.resolve_background(pds);
        // The camera is what binds this chain to a viewpoint, so without one
        // there is nothing sensible to encode. A pane in that state has
        // already been sent to the empty path by `render_pane`; the guard
        // stays because the invariant belongs next to the code that needs it.
        // The scene is a different matter: an empty draw list still renders
        // the background, grid, floor and axes off the environment.
        let Some(cam_bg) = self.view.cameras[i].as_ref().map(|c| &c.bind_group) else {
            self.renderer.render_empty_pass(encoder, background);
            return;
        };
        let objects = self.draw_objects();
        solarxy_host::render_3d_passes(
            &self.renderer,
            &self.queue,
            encoder,
            &solarxy_host::PaneScene {
                objects: &objects,
                env: &self.env,
                cam_bg,
                cam_data,
                pds,
                background,
                shadow: i == 0 || !self.view.display.lights_locked,
                selected: false,
            },
        );
    }

    /// Recompute the camera-relative light rig for a non-primary pane
    /// from `cam_data` before it renders, so each pane is lit from its
    /// own viewpoint. No-op when lights are locked. Pane 0 keeps the rig
    /// `update()` set from slot 0's camera.
    fn setup_pane_lighting(&mut self, cam_data: &Camera) {
        if self.install_authored_lights() {
            return;
        }
        if self.view.display.lights_locked {
            return;
        }
        let ibl_avg = solarxy_host::active_ibl(&self.renderer).irradiance_average;
        // Bound before the mutable borrow of the environment: the accessor
        // reads the whole of `self`, and the result is owned, so the shared
        // borrow ends here.
        let bounds = self.scene_bounds();
        solarxy_host::setup_pane_lighting(&self.queue, &mut self.env, cam_data, &bounds, ibl_avg);
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
        // Whichever root is open supplies the panels. The two are mutually
        // exclusive, so this is a choice rather than a merge; a file model
        // wins the tie only because it cannot occur.
        let validation = match (&self.scene, &self.engine_scene) {
            (Some(scene), _) => crate::gui::ValidationView {
                report: Some(&scene.validation),
                owners: &[],
            },
            (None, Some(info)) => crate::gui::ValidationView {
                report: Some(&info.validation.report),
                owners: &info.validation.labels,
            },
            (None, None) => crate::gui::ValidationView::default(),
        };
        let outliner_source = match (&self.scene, &self.engine_scene) {
            (Some(scene), _) => crate::gui::OutlinerSource::Model(&scene.model),
            (None, Some(info)) => crate::gui::OutlinerSource::Scene {
                objects: &self.scene_objects,
                names: &info.object_names,
            },
            (None, None) => crate::gui::OutlinerSource::Empty,
        };

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
            validation,
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
            outliner_source,
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

        // Arm the async readback; `poll_pending_capture` delivers the image
        // to the screenshot modal a frame or two later.
        if let Some((buffer, padded_row_bytes, width, height)) = capture {
            self.arm_pending_capture(buffer, padded_row_bytes, width, height);
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
