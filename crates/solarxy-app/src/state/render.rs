//! `State::render`: the per-frame entry point. Computes the pane rectangles,
//! assembles each pane's parameters in `render_pane`, hands the pass chain
//! and the composite to `solarxy_host` (`setup_pane_lighting`,
//! `composite_and_submit`), and runs the one interface pass at the end.
//!
//! The shared body lives in `solarxy-host`, which both shells drive; what
//! remains in this file is the assembly only a desktop shell can do.
//!
//! Hands the panels a read-only view of the display settings and drains the
//! intents they raise afterwards; the drain is what triggers any expensive
//! recomputation (background, wireframe, composite, IBL).

use std::collections::BTreeMap;

use solarxy_core::preferences::{InspectionMode, MaterialOverride, PaneMode, ResolvedBackground};
use solarxy_graph::document::{GraphContext, NodeId};
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

        self.apply_pending_scene_deltas();

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
        let cam_look = solarxy_host::cameras::camera_look_for(
            self.raster.scene().cameras(),
            self.look_through[i.min(3)],
        )
        .cloned();
        // The camera's look wins where it has one; what it falls back to is
        // the one thing the two shells still answer differently. The browser
        // falls back to the pane's own look, and this shell derives one from
        // the global tone and exposure, because it has no per-pane look to
        // fall back to yet. The two converge when per-pane look replaces the
        // global post state here and the Sidebar is retired with it.
        let look = solarxy_renderer::composite::resolve_look(
            cam_look.as_ref(),
            &solarxy_core::view_config::PaneLook::from_tone(
                self.renderer.post.tone_mode,
                self.renderer.post.exposure,
            ),
        );
        solarxy_host::cameras::bind_look_luts(
            &self.device,
            &self.queue,
            &mut self.renderer,
            cam_look.as_ref(),
        );
        let scene_present = self.scene_present();
        let outline = self.renderer.selection_style
            == solarxy_renderer::frame::SelectionStyle::Outline
            && self
                .selected_object
                .is_some_and(|id| self.raster.scene().draw_object(id).is_some());

        // Everything below borrows fields other than `raster`, so the backend
        // can be driven mutably: the backend assembles the draw list from the
        // scene it owns.
        let content = match cam_data {
            None => PaneContent::Empty,
            Some(_) if is_uv_map => PaneContent::Uv {
                source: UvSource::Scene {
                    preferred: self.selected_object,
                },
            },
            Some(cam_data) => PaneContent::Scene {
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

    /// Drain queued scene deltas into the multi-object scene.
    ///
    /// Called at the top of a frame, before any pane encodes, which is the
    /// engine's per-frame commit point. Also called at adoption, so a document
    /// that arrived already cooked has its bounds known before the panes are
    /// framed rather than one frame later.
    pub(super) fn apply_pending_scene_deltas(&mut self) {
        if self.pending_scene_deltas.is_empty() {
            return;
        }
        for delta in std::mem::take(&mut self.pending_scene_deltas) {
            self.raster.apply(&self.device, &self.queue, &delta);
            // The backend collects upload failures rather than logging them:
            // it has no logging facility and this shell is the layer that
            // knows where a message belongs.
            //
            // A toast rather than only a console line, because a mesh the
            // device cannot hold is refused through here and the user would
            // otherwise see a model simply fail to appear. The toast carries
            // its own `tracing` event, so nothing is logged twice.
            for e in self.raster.take_errors() {
                self.gui.set_toast(
                    &format!("Scene delta apply failed: {e}"),
                    crate::gui::ToastSeverity::Error,
                );
            }
            self.apply_scene_environment(&delta);
        }
        // The panels read summed counters and one merged validation report.
        // Both are derived from what just landed, so they are rebuilt here
        // rather than per frame: a delta is the only thing that can change
        // either. The same is true of the per-mesh overlay buffers, which are
        // baked geometry rather than a pass over the live scene.
        self.refresh_engine_scene_info();
        self.viz_dirty = true;
    }

    /// Whether this frame has scene content: at least
    /// one visible object in the multi-object scene.
    ///
    /// The composite pass folds in the bloom and ambient-occlusion textures
    /// only when this is true. It deliberately is not "does the pane have a
    /// camera": a pane with a camera and nothing in it renders the
    /// background, the grid and the floor, and blooming that would put a glow
    /// on a bare viewport nobody asked for.
    fn scene_present(&self) -> bool {
        self.raster.scene().draw_objects().next().is_some()
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
        use crate::gui::{HudInfo, PanelSettings};

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("UI Encoder"),
            });

        let divider = self
            .compute_divider_hit_rect()
            .map(|hit| crate::gui::DividerInfo {
                hit,
                layout: self.view.display.layout,
            });
        let pane_gaps = self.compute_gap_rects();

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
        // Everything the panels ask for this pass. Raised during the egui
        // pass, applied by `drain_intents` once it is over.
        let mut intents = crate::gui::Intents::default();

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
        // Borrowed rather than copied: the panels read the real settings and
        // ask for changes, so there is nothing to write back afterwards.
        let settings = PanelSettings {
            panes: &self.view.pane_settings,
            active: ap,
            display: &self.view.display,
            post: &self.renderer.post,
            ibl_mode: self.renderer.ibl_res.ibl_mode,
            cameras_linked: self.view.cameras_linked,
            is_split,
            projection_mode,
            cook: self.cook_readout,
            canvas: self.preferences.canvas,
        };
        let active_inspection = self.view.pane_settings[self.view.active_pane].inspection_mode;
        let active_pane_mode = self.view.pane_settings[self.view.active_pane].pane_mode;
        let hud = HudInfo {
            pane_label,
            cameras_linked: if is_split {
                Some(self.view.cameras_linked)
            } else {
                None
            },
            has_uvs: self.raster.scene().iter().any(|(_, o)| o.model.has_uvs),
            overdraw_active: active_inspection == InspectionMode::Overdraw
                && active_pane_mode == PaneMode::Scene3D,
        };
        let validation = match &self.engine_scene {
            Some(info) => crate::gui::ValidationView {
                report: Some(&info.validation.report),
                owners: &info.validation.labels,
            },
            None => crate::gui::ValidationView::default(),
        };
        let outliner_source = match &self.engine_scene {
            Some(info) => crate::gui::OutlinerSource::Scene {
                objects: self.raster.scene(),
                names: &info.object_names,
            },
            None => crate::gui::OutlinerSource::Empty,
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
            None => crate::gui::NodeTreeSource::Empty,
        };
        // The canvas seeds from the revision rather than from a delta,
        // because it draws things no delta carries: edges, positions and
        // selection. `Empty` for a closed tab, so a canvas nobody is
        // looking at costs nothing.
        //
        // The cook facts are gathered here rather than mirrored, for the
        // shown context alone: they come from four different places on
        // the engine, a panel never sees one, and a context holds a
        // handful of nodes.
        let canvas_cook = self.canvas_cook();
        let canvas_assets: BTreeMap<String, String> =
            match (&self.engine, self.gui.nodes_tab_present()) {
                (Some(engine), true) => engine.asset_manifest().into_iter().collect(),
                _ => BTreeMap::new(),
            };
        let canvas_scene = match &self.engine {
            Some(engine) if self.gui.nodes_tab_present() => Some(crate::gui::CanvasScene {
                doc: engine.document(),
                registry: engine.registry(),
                revision: engine.revision(),
                cook: &canvas_cook,
                assets: &canvas_assets,
                manual: engine.cook_mode() == solarxy_graph::engine::CookMode::Manual,
                playing: engine.clock().playing,
            }),
            _ => None,
        };
        // The parameter panel's subject and what its last cook said.
        // Gathered here for the same reason the canvas's cook facts are:
        // the answers come from three places on the engine and a panel
        // sees none of them.
        // The gui reads are taken as values first, so the source below
        // borrows the engine alone: a method on `&self` would hold all of
        // it for as long as the source lives, and the interface pass
        // needs the renderer mutably.
        let params_scene = params_source(
            self.engine.as_deref(),
            self.gui.params_tab_present(),
            self.gui.graph_ctx(),
            self.gui.params_pin(),
        );
        let params_source = params_scene.as_ref().map_or(
            crate::gui::ParamPanelSource::Empty,
            crate::gui::ParamPanelSource::Scene,
        );
        let canvas_source = canvas_scene.as_ref().map_or(
            crate::gui::CanvasSource::Empty,
            crate::gui::CanvasSource::Scene,
        );

        // The Actions section's subject: the node selected in the Node Tree's
        // context, read from the document each frame rather than mirrored,
        // because selection is engine state.
        let selected = self
            .engine
            .as_deref()
            .and_then(|engine| selected_node(engine.document(), self.gui.graph_ctx()));
        let selected_name = match (&self.engine, selected) {
            (Some(engine), Some((ctx, id))) => engine
                .document()
                .graph(ctx)
                .ok()
                .and_then(|g| g.node(id))
                .map(|n| solarxy_graph::naming::node_name(n, engine.registry()))
                .unwrap_or_default(),
            _ => String::new(),
        };
        let actions_source = match (&self.engine, selected) {
            (Some(engine), Some((ctx, id))) => {
                let data = engine.document().graph(ctx).ok().and_then(|g| g.node(id));
                let descriptor = data.and_then(|n| engine.registry().get(&n.type_id));
                crate::gui::NodeActionsView {
                    node: Some((ctx, id)),
                    name: &selected_name,
                    type_name: descriptor.map_or("", |d| d.display_name),
                    params: descriptor.map_or(&[], |d| d.params.as_slice()),
                    stored: data.map(|n| &n.params),
                }
            }
            _ => crate::gui::NodeActionsView::default(),
        };

        let recent_files = self.preferences.history.recent_files.clone();
        // `PaneToolbarData` is passed by value — `render_ui` consumes it,
        // releasing its `&mut self.view.pane_settings` borrow before
        // the drain re-borrows the same field below.
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
        let pane_toolbar = crate::gui::PaneToolbarData {
            rects: &pane_rects,
            active: ap,
            projections: pane_projections,
            hdri_available,
            customs: &self.preferences.view.custom_backgrounds,
            uv_overlap_pct,
            cameras: &scene_cameras,
            look_through: look_through_mirror,
            turntable_rpm: self.view.display.turntable_rpm,
        };
        self.gui.render_ui(
            crate::gui::FramePaint {
                device: &self.device,
                queue: &self.queue,
                encoder: &mut encoder,
                window: &self.window,
                surface_texture: &output.texture,
                screen,
                frame_ms,
            },
            crate::gui::ViewportChrome {
                divider,
                pane_gaps: &pane_gaps,
                active_pane_rect,
                review_panes: &review_panes,
                toolbars: pane_toolbar,
            },
            crate::gui::PanelSources {
                settings,
                hud: &hud,
                validation,
                outliner: outliner_source,
                node_tree: node_tree_source,
                canvas: canvas_source,
                params: params_source,
                actions: actions_source,
                recent_files: &recent_files,
            },
            &mut self.review,
            &mut intents,
            &mut self.viewport_context_menu,
            crate::gui::CaptureFrame {
                capturing: self.capture_requested,
                expand_review: self.capture_requested && self.screenshot_expand_review,
            },
        );

        // Everything the pass raised, in one ordered pass: the settings the
        // panels changed, then the actions the menus asked for, then what the
        // panels asked the shell to do.
        self.drain_intents(&mut intents);

        // The review panel's open flag mirrors dock membership, and a panel
        // toggle the drain just applied is what changes it. Synced here
        // rather than at the end of the egui pass, which runs before the
        // drain: a toggle raised this frame would otherwise not be visible
        // until the next one, and the pre-pass reconciliation would undo it.
        self.review.panel_open = self.gui.tab_present(crate::gui::SolarxyTab::ReviewPanel);

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

/// The node the Actions section is about: the selection in `prefer` (the
/// Node Tree's dived context), else the root's, else the first subflow's
/// that has one. The most recently selected id wins within a graph.
///
/// Read from the document rather than mirrored, because selection is engine
/// state and this shell's only writer of it is the Node Tree's drain arm.
impl State {
    /// What the canvas needs to know about the shown context's cooks.
    ///
    /// Built per frame rather than mirrored, because the four answers
    /// live in four different places on the engine and a panel sees none
    /// of them. Empty when the tab is closed or nothing is open, so the
    /// cost is paid only by a canvas somebody is looking at.
    fn canvas_cook(&self) -> BTreeMap<NodeId, crate::gui::NodeCook> {
        let mut out = BTreeMap::new();
        if !self.gui.nodes_tab_present() {
            return out;
        }
        let Some(engine) = self.engine.as_deref() else {
            return out;
        };
        let ctx = self.gui.graph_ctx();
        let Ok(graph) = engine.document().graph(ctx) else {
            return out;
        };
        for node in graph.nodes() {
            let report = engine.node_report(ctx, node.id);
            let validation = engine.validation(node.id);
            out.insert(
                node.id,
                crate::gui::NodeCook {
                    state: engine.cook_state(node.id),
                    last_us: report.map_or(0, |r| r.last_cook_us),
                    error: self.cook_health.failure(node.id).map(str::to_owned),
                    #[allow(clippy::cast_possible_truncation)]
                    errors: validation.map_or(0, |v| v.report.error_count() as u32),
                    #[allow(clippy::cast_possible_truncation)]
                    warnings: validation.map_or(0, |v| v.report.warning_count() as u32),
                },
            );
        }
        out
    }
}

fn selected_node(
    doc: &solarxy_graph::document::Document,
    prefer: solarxy_graph::document::GraphContext,
) -> Option<(
    solarxy_graph::document::GraphContext,
    solarxy_graph::document::NodeId,
)> {
    use solarxy_graph::document::GraphContext;
    std::iter::once(prefer)
        .chain(std::iter::once(GraphContext::Root))
        .chain(doc.subflow_owners().map(GraphContext::Subflow))
        .find_map(|ctx| {
            let id = doc.graph(ctx).ok()?.selection.last().copied()?;
            Some((ctx, id))
        })
}

/// What the parameter panel edits, and what its last cook said.
///
/// A free function rather than a method, because the source borrows the
/// document and a method on `&self` would hold the whole shell for as
/// long as the source lives, which is until after the interface pass has
/// taken the renderer mutably.
///
/// The subject is resolved here rather than in the panel because the
/// statistics and the validation report are engine reads and have to be
/// taken for the node the panel will actually draw. The pin belongs to
/// the panel, so it is passed in.
fn params_source(
    engine: Option<&solarxy_graph::Engine>,
    tab_present: bool,
    ctx: GraphContext,
    pin: Option<NodeId>,
) -> Option<crate::gui::ParamScene<'_>> {
    let engine = engine.filter(|_| tab_present)?;
    let subject = pin.or_else(|| {
        engine
            .document()
            .graph(ctx)
            .ok()
            .and_then(|g| g.selection.first().copied())
    });
    let validation = subject.and_then(|node| engine.validation(node));
    Some(crate::gui::ParamScene {
        doc: engine.document(),
        registry: engine.registry(),
        ctx,
        stats: subject.and_then(|node| engine.node_stats(node)),
        // Presence, not cleanliness: a clean validate cook still stores a
        // report, and the tab that says so is the point of it.
        has_report: validation.is_some(),
        #[allow(clippy::cast_possible_truncation)]
        counts: validation.map_or((0, 0), |v| {
            (
                v.report.error_count() as u32,
                v.report.warning_count() as u32,
            )
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::selected_node;
    use solarxy_graph::document::{GraphContext, NodeId};
    use solarxy_graph::{Command, Engine, EngineEvent};

    fn add(engine: &mut Engine, ctx: GraphContext, ty: &str) -> NodeId {
        let batch = engine
            .apply(Command::AddNode {
                ctx,
                node_type: ty.to_string(),
                position: [0.0, 0.0],
            })
            .expect("the node adds");
        batch
            .events
            .iter()
            .find_map(|ev| match ev {
                EngineEvent::NodeAdded { node, .. } => Some(node.id),
                _ => None,
            })
            .expect("a node was added")
    }

    /// Selection is read off the document, preferring the context the Node
    /// Tree is showing, so a dive changes what the section is about.
    #[test]
    fn the_selected_node_is_read_from_the_document_preferring_the_tree_context() {
        let mut engine = Engine::new().expect("engine");
        assert_eq!(selected_node(engine.document(), GraphContext::Root), None);

        let container = add(&mut engine, GraphContext::Root, "sopnet");
        let inner = GraphContext::Subflow(container);
        let inner_box = add(&mut engine, inner, "box");
        engine
            .apply(Command::SetSelection {
                ctx: inner,
                ids: vec![inner_box],
            })
            .expect("selects");
        engine
            .apply(Command::SetSelection {
                ctx: GraphContext::Root,
                ids: vec![container],
            })
            .expect("selects");

        assert_eq!(
            selected_node(engine.document(), inner),
            Some((inner, inner_box))
        );
        assert_eq!(
            selected_node(engine.document(), GraphContext::Root),
            Some((GraphContext::Root, container))
        );
    }
}
