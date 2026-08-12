//! `State::render`: per-frame entry point. Computes the pane rectangles,
//! assembles each pane's parameters and hands them to `solarxy_host::render_pane`,
//! then drives the egui sidebar/menu/HUD/console paint at the end.
//!
//! The pane body itself is not here. It lives in `solarxy-host` beside the web
//! shell's copy of the same call, which is the point: what remains in this file
//! is the assembly only a desktop shell can do.
//!
//! Reads `GuiSnapshot::from_state` then calls `apply_to_state` after the
//! sidebar has had a chance to mutate it; the resulting `SidebarChanges`
//! drives any expensive recomputations (background, wireframe, composite,
//! IBL).

use solarxy_core::preferences::{InspectionMode, MaterialOverride, PaneMode, ResolvedBackground};
use solarxy_host::EncodedPane;
use solarxy_renderer::backend::{FrameCtx, PaneContent, RenderBackend, UvSource};
use solarxy_renderer::camera::Camera;

use super::view_state::PaneDisplaySettings;
use super::{Pane, State};

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
        // A running still owns the shared render targets; resizing them
        // back to the panes every frame would fight its per-tile sizing.
        if self.still.is_none() {
            self.sync_render_target_dims();
        }

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
                self.raster.apply(&self.device, &self.queue, &delta);
                // The backend collects upload failures rather than logging
                // them: it has no logging facility and this shell is the layer
                // that knows a console line is what is wanted here.
                for e in self.raster.take_errors() {
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

        // A running still renders instead of the panes: the job and the
        // viewport would otherwise fight over the shared render targets
        // at different sizes every frame. The GUI still draws, which is
        // where the modal's progress and cancel live.
        if self.still.is_some() {
            self.pump_still_render();
            self.clear_surface(&surface_view);
            self.render_gui_overlay(&output, &[], false, frame_ms);
            output.present();
            return Ok(());
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

    /// Assemble this pane's parameters and hand them to the shared body.
    ///
    /// What is left here is policy and assembly: the light-rig guard, which
    /// writes through `&mut self` and so cannot travel, and the draw list,
    /// which this shell builds differently because it has a file-loaded model
    /// the web shell does not.
    fn render_pane(
        &mut self,
        i: usize,
        pane: &Pane,
        surface_view: &wgpu::TextureView,
        is_split: bool,
    ) {
        let pds = self.view.pane_settings[i];
        let cam_data = self.view.cameras[i].as_ref().map(|c| c.camera);
        let is_uv_map = pds.pane_mode == PaneMode::UvMap;

        // Before the field borrows below, because it takes `&mut self`: the
        // authored-light install writes inside its own guard.
        if !is_uv_map
            && is_split
            && i >= 1
            && let Some(cam_data) = cam_data
        {
            self.setup_pane_lighting(&cam_data);
        }

        let background = self.resolve_background(&pds);
        let bounds = self.scene_bounds();
        // A pane looking through a camera composites with the shot's look
        // and its grading tables, resolved through the one precedence site;
        // a free pane keeps its own. `set_lut` dedupes on content, so the
        // common case (free panes, or every pane through one camera) costs
        // two comparisons per pane and rebuilds nothing.
        let cam_look: Option<solarxy_core::scene::CameraLook> = self.look_through[i.min(3)]
            .and_then(|id| {
                self.raster
                    .scene()
                    .cameras()
                    .and_then(|cams| cams.iter().find(|c| c.id == id))
                    .map(|c| c.look.clone())
            });
        let look = solarxy_renderer::composite::resolve_look(
            cam_look.as_ref(),
            &solarxy_core::view_config::PaneLook::from_tone(
                self.renderer.post.tone_mode,
                self.renderer.post.exposure,
            ),
        );
        let (lut_a, lut_b) = cam_look
            .as_ref()
            .map_or((None, None), |l| (l.lut_a.clone(), l.lut_b.clone()));
        self.renderer.set_lut(
            &self.device,
            &self.queue,
            solarxy_renderer::lut::LutSlot::A,
            lut_a.as_deref(),
        );
        self.renderer.set_lut(
            &self.device,
            &self.queue,
            solarxy_renderer::lut::LutSlot::B,
            lut_b.as_deref(),
        );
        let scene_present = self.scene_present();
        let outline = self.renderer.selection_style
            == solarxy_renderer::frame::SelectionStyle::Outline
            && self
                .selected_object
                .is_some_and(|id| self.raster.scene().draw_object(id).is_some());

        // Everything below borrows fields other than `raster`, so the backend
        // can be driven mutably: the file-loaded model and the UV source both
        // come from this shell's own state, and the backend assembles the rest
        // of the draw list from the scene it owns.
        let content = match cam_data {
            None => PaneContent::Empty,
            Some(_) if is_uv_map => PaneContent::Uv {
                source: self.scene.as_ref().map_or(UvSource::None, |s| {
                    UvSource::External(s.draw_object(&self.env.instance_buffer))
                }),
            },
            Some(cam_data) => PaneContent::Scene {
                extra: self
                    .scene
                    .as_ref()
                    .map(|s| s.draw_object(&self.env.instance_buffer)),
                selected: self.selected_object,
                cam_data,
                shadow: i == 0 || !self.view.display.lights_locked,
            },
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Pane Encoder"),
            });
        let target = self.renderer.targets.hdr_resolve_view.clone();
        let _outcome = self.raster.encode(
            &mut FrameCtx {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.renderer,
                encoder: &mut encoder,
                index: i,
                rect: *pane,
                is_split,
                pds: &pds,
                display: &self.view.display,
                background,
                camera: self.view.cameras[i].as_mut(),
                env: &self.env,
                bounds: Some(&bounds),
                // This shell does not steer the grid plane from the camera, so the
                // plane offset is left exactly as it was initialised.
                grid_plane: None,
                look,
                scene_present,
                outline,
                // An ordinary frame is a view in its own right, not a window on
                // a larger picture. Only the still render sets this.
                window: None,
                content,
            },
            &target,
        );
        let encoded = self.raster.encoded(i).unwrap_or(EncodedPane {
            is_uv_map: false,
            scene_present: false,
        });

        solarxy_host::composite_and_submit(
            &self.queue,
            &self.renderer,
            encoder,
            surface_view,
            &solarxy_host::PaneComposite {
                index: i,
                rect: *pane,
                look,
                inspection: pds.inspection_mode,
                is_uv_map: encoded.is_uv_map,
                scene_present: encoded.scene_present,
                outline,
                // Every desktop pane rasterizes; the shell's traced path is
                // the still render, which resolves this for itself.
                writes_occlusion: solarxy_host::RasterBackend::CAPS.writes_occlusion,
            },
        );
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
        self.scene.is_some() || self.raster.scene().draw_objects().next().is_some()
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
        let mut node_tree_events = crate::gui::NodeTreeEvents::default();

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
                objects: self.raster.scene(),
                names: &info.object_names,
            },
            (None, None) => crate::gui::OutlinerSource::Empty,
        };
        // Folded fresh each frame rather than cached on a delta, because
        // selection is part of what the tree draws and selection changes
        // without a delta. Skipped outright when the tab is closed, which
        // is the only case where the cost would be paid for nothing.
        let node_tree_source = match &self.engine {
            _ if !self.gui.node_tree_tab_present() => crate::gui::NodeTreeSource::Empty,
            Some(engine) => crate::gui::NodeTreeSource::Scene {
                doc: engine.document(),
                registry: engine.registry(),
            },
            // A model file is open, or nothing is. The panel distinguishes
            // the two: "no graph" and "nothing open" are different facts,
            // and a panel that conflates them reads as broken while the
            // viewport is plainly full of geometry.
            None if self.scene.is_some() => crate::gui::NodeTreeSource::ModelFile,
            None => crate::gui::NodeTreeSource::Empty,
        };

        let recent_files = self.preferences.history.recent_files.clone();
        let model = self.scene.as_ref().map(|s| &s.model);
        // `PaneToolbarData` is passed by value — `render_ui` consumes it,
        // releasing its `&mut self.view.pane_settings` borrow before
        // `apply_to_state` re-borrows the same field below.
        let hdri_available = self.renderer.ibl_res.ibl.equirect.is_some();
        let uv_overlap_pct = self.renderer.uv_overlap.overlap_pct;
        // The open scene's cameras, named the way the Node Tree names them,
        // for the toolbar's Look Through submenu. Empty with no engine open.
        let scene_cameras: Vec<(u64, String)> = match &self.engine {
            Some(engine) => self
                .raster
                .scene()
                .cameras()
                .map(|cams| {
                    let doc = engine.document();
                    let registry = engine.registry();
                    let root = doc.graph(solarxy_graph::document::GraphContext::Root).ok();
                    cams.iter()
                        .map(|c| {
                            let name = root
                                .as_ref()
                                .and_then(|g| {
                                    g.nodes()
                                        .find(|n| n.id.0 == c.id.0)
                                        .map(|n| solarxy_graph::naming::node_name(n, registry))
                                })
                                .unwrap_or_else(|| format!("Camera {}", c.id.0));
                            (c.id.0, name)
                        })
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let look_through_mirror: [Option<u64>; 4] =
            std::array::from_fn(|i| self.look_through[i].map(|id| id.0));
        let mut look_through_change = None;
        let pane_toolbar = crate::gui::PaneToolbarData {
            rects: &pane_rects,
            active: ap,
            pane_settings: &mut self.view.pane_settings,
            projections: pane_projections,
            projection_change: &mut projection_change,
            hdri_available,
            customs: &self.preferences.view.custom_backgrounds,
            uv_overlap_pct,
            cameras: &scene_cameras,
            look_through: look_through_mirror,
            look_through_change: &mut look_through_change,
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
            node_tree_source,
            pane_toolbar,
            &mut properties_events,
            &mut outliner_events,
            &mut node_tree_events,
            &mut self.viewport_context_menu,
            force_expand_review,
            suppress_screenshot_modal,
        );

        if let Some((i, proj)) = projection_change {
            // A projection picked on a bound pane takes the view over;
            // without the release the per-frame follow would silently
            // revert the choice on the next frame.
            self.release_look_through_pane(i);
            if let Some(cam) = &mut self.view.cameras[i] {
                cam.set_projection(proj);
            }
        }

        if let Some((i, change)) = look_through_change
            && i < 4
        {
            // Applied after `render_ui` like a projection change; the pose
            // lands on the next frame's follow, one frame after the click.
            self.look_through[i] = match change {
                crate::gui::LookThroughChange::Bind(id) => {
                    Some(solarxy_core::scene::SceneObjectId(id))
                }
                crate::gui::LookThroughChange::Free => None,
            };
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

        // Node Tree events: selection, engine-side and in the viewport.
        if let Some(action) = node_tree_events.action {
            self.handle_node_tree_action(action);
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
        self.handle_still_modal();
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
