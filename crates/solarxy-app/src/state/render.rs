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

use solarxy_core::preferences::{InspectionMode, PaneMode, ResolvedBackground};
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_host::EncodedPane;
use solarxy_renderer::backend::{FrameCtx, PaneContent, RenderBackend, UvSource};
use solarxy_renderer::camera::Camera;

use super::view_state::PaneDisplaySettings;
use super::{Pane, State};

impl State {
    /// Resolve a pane's background choice into concrete colours for the
    /// renderer and IBL. There is no list of user backgrounds to resolve
    /// against: this shell offers none, and a stored default that named one
    /// was read as the builtin at launch.
    pub(super) fn resolve_background(pds: &PaneDisplaySettings) -> ResolvedBackground {
        pds.background_mode.resolve(&[])
    }

    /// Per-frame render entry point. Computes pane rectangles, dispatches
    /// per-pane scene/UV passes, paints the egui overlay (sidebar, menu,
    /// HUD, modals, toasts), and presents the swapchain frame.
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

        self.gui.clear_expired_toasts();
        // A running still or turntable owns the shared render targets;
        // resizing them back to the panes every frame would fight the
        // per-tile sizing of whichever is running.
        if self.still.is_none() && self.turntable.is_none() {
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
            self.render_gui_overlay(&output, &[], false);
            output.present();
            return Ok(());
        }

        // A turntable export owns the frame for the same reason, once per
        // frame of the turn rather than once per tile of one picture.
        if self.turntable.is_some() {
            self.pump_turntable_export();
            self.clear_surface(&surface_view);
            self.render_gui_overlay(&output, &[], false);
            output.present();
            return Ok(());
        }

        let viewport_present = self.gui.viewport_tab_present();
        if !viewport_present {
            self.clear_surface(&surface_view);
            self.render_gui_overlay(&output, &[], false);
            output.present();
            return Ok(());
        }

        let panes = self.compute_panes();
        let is_split = panes.len() > 1;

        // A screenshot is the frame it is taken on, so the furniture is
        // cleared before the panes render rather than in an offscreen pass
        // as the browser does: a delivered image carries no gizmo, no
        // helper and no marker on either shell. The frame the user sees is
        // the same one, bare for that one frame.
        if self.capture_requested {
            self.renderer.clear_viewport_furniture();
        }

        for (i, pane) in panes.iter().enumerate() {
            self.render_pane(i, pane, &surface_view, is_split);
        }

        self.render_gui_overlay(&output, &panes, is_split);
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
        if !is_uv_map && let Some(cam_data) = cam_data {
            // The gizmo's world size is per pane, because a pane's camera
            // and height decide how many world units a pixel is, so it is
            // re-written before each pane's pass rather than once per frame.
            // Logical pixels, so a high-density display does not halve it.
            let ppp = self.window.scale_factor() as f32;
            self.renderer
                .write_manipulator(&self.queue, &cam_data, pane.height / ppp);
            // Markers are per pane for the same reason the manipulator is:
            // screen-constant means a pane's own camera and height decide the
            // world size. Written only when the pane draws them, and the
            // draw is gated on the same flag.
            if pds.show_light_markers {
                let selected = self.selected_object;
                let lights = self.raster.scene().lights().map(<[_]>::to_vec);
                self.renderer.write_light_markers(
                    &self.queue,
                    lights.as_deref().unwrap_or(&[]),
                    &cam_data,
                    pane.height / ppp,
                    selected,
                );
            }
        }

        let background = Self::resolve_background(&pds);
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
        // The per-mesh overlay buffers are baked geometry rather than a pass
        // over the live scene, so they are rebuilt from what just landed
        // rather than per frame: a delta is the only thing that changes them.
        // The attribute channels are baked the same way.
        self.viz_dirty = true;
        self.attr_dirty = true;
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

        // Borrowed rather than copied: the panels read the real settings and
        // ask for changes, so there is nothing to write back afterwards.
        let settings = PanelSettings {
            panes: &self.view.pane_settings,
            active: ap,
            display: &self.view.display,
            post: &self.renderer.post,
            ibl_mode: self.renderer.ibl_res.ibl_mode,
            cook: self.cook_readout,
            history: self.history_readout(),
            clipboard: self.clipboard_readout(),
            canvas: self.preferences.canvas,
            tools: super::gizmo_drag::tool_readout(
                &self.gizmo,
                self.tools_available.as_deref(),
                &self.preferences.viewport,
                self.gizmo_readout.as_deref(),
            ),
            transport: self.transport_readout(),
            transport_bar: self.preferences.ui.transport_bar,
        };
        let active_inspection = self.view.pane_settings[self.view.active_pane].inspection_mode;
        let active_pane_mode = self.view.pane_settings[self.view.active_pane].pane_mode;
        let hud = HudInfo {
            has_uvs: self.raster.scene().iter().any(|(_, o)| o.model.has_uvs),
            overdraw_active: active_inspection == InspectionMode::Overdraw
                && active_pane_mode == PaneMode::Scene3D,
        };
        // Folded fresh each frame rather than cached on a delta, because
        // selection is part of what the tree draws and selection changes
        // without a delta. Skipped outright when the tab is closed, which
        // is the only case where the cost would be paid for nothing.
        let tree_source = match &self.engine {
            _ if !self.gui.tree_tab_present() => crate::gui::TreeSource::Empty,
            Some(engine) => crate::gui::TreeSource::Scene {
                doc: engine.document(),
                registry: engine.registry(),
            },
            None => crate::gui::TreeSource::Empty,
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
        // The info card's node, gathered only while the card is up: one
        // report, one warnings list, one stats read and one validation read.
        let node_info: Option<crate::gui::NodeInfoView> = self
            .engine
            .as_deref()
            .zip(self.gui.canvas_info())
            .map(|(engine, node)| {
                let ctx = self.gui.graph_ctx();
                #[allow(clippy::cast_precision_loss)]
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_millis() as f64);
                let report = engine.node_report(ctx, node).and_then(|report| {
                    let graph = engine.document().graph(ctx).ok()?;
                    Some(solarxy_studio::node::node_report_text(
                        &report,
                        graph,
                        node,
                        engine.registry(),
                        now_ms,
                    ))
                });
                crate::gui::NodeInfoView {
                    report,
                    stats: engine.node_stats(node),
                    warnings: engine.cook_warnings(node),
                    validation: engine
                        .validation(node)
                        .map(|v| (v.report.error_count(), v.report.warning_count())),
                }
            });
        let canvas_scene = match &self.engine {
            Some(engine) if self.gui.nodes_tab_present() => Some(crate::gui::CanvasScene {
                doc: engine.document(),
                registry: engine.registry(),
                revision: engine.revision(),
                cook: &canvas_cook,
                assets: &canvas_assets,
                manual: engine.cook_mode() == solarxy_graph::engine::CookMode::Manual,
                playing: engine.clock().playing,
                info: node_info.as_ref(),
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
        // Owned here rather than inside the source, because the source
        // holds borrows and the engine builds this list fresh. Gathered
        // only when the tab is up: it walks every staged asset.
        let docked_up = self.gui.properties_tab_present();
        let floating_up = self.gui.floating_props_open();
        let staged: Vec<(String, String)> = self
            .engine
            .as_deref()
            .filter(|_| docked_up || floating_up)
            .map(solarxy_graph::Engine::asset_manifest)
            .unwrap_or_default();
        // One subject per host. The docked panel and the floating one each
        // have a pin, so each may be showing a different node, and what a
        // node's panel needs owned for it is assembled once per host that
        // is up. A closed host costs nothing.
        let docked = self.param_subject_facts(docked_up, self.gui.params_pin());
        let floating = self.param_subject_facts(floating_up, self.gui.floating_params_pin());
        let docked_error = docked
            .subject
            .and_then(|node| self.cook_health.failure(node));
        let floating_error = floating
            .subject
            .and_then(|node| self.cook_health.failure(node));
        let params_scene = params_source(
            self.engine.as_deref(),
            docked_up,
            self.gui.graph_ctx(),
            self.gui.params_pin(),
            &staged,
            &docked.lanes,
            docked_error,
            &docked.resolved,
        );
        let params_floating_scene = params_source(
            self.engine.as_deref(),
            floating_up,
            self.gui.graph_ctx(),
            self.gui.floating_params_pin(),
            &staged,
            &floating.lanes,
            floating_error,
            &floating.resolved,
        );
        // The image network's published output, asked for only while the
        // tab is up: an `Arc` clone and a hash compare per frame.
        let texture_owner = self
            .engine
            .as_deref()
            .filter(|_| self.gui.texture_tab_present())
            .and_then(|e| {
                crate::gui::texture_owner(e.document(), e.registry(), self.gui.graph_ctx())
            });
        let texture_label: Option<String> = texture_owner.and_then(|id| {
            let engine = self.engine.as_deref()?;
            let node = engine.document().graph(GraphContext::Root).ok()?.node(id)?;
            Some(solarxy_graph::naming::node_name(node, engine.registry()))
        });
        let texture_image = texture_owner.and_then(|id| self.engine.as_deref()?.display_image(id));
        let texture_source = match (&self.engine, self.gui.texture_tab_present()) {
            (Some(_), true) => crate::gui::TextureSource::Scene {
                owner: texture_label.as_deref().map(|label| crate::gui::OwnerView {
                    label,
                    image: texture_image.as_ref(),
                }),
            },
            _ => crate::gui::TextureSource::Empty,
        };
        let params_source = params_scene.as_ref().map_or(
            crate::gui::ParamPanelSource::Empty,
            crate::gui::ParamPanelSource::Scene,
        );
        let params_floating_source = params_floating_scene.as_ref().map_or(
            crate::gui::ParamPanelSource::Empty,
            crate::gui::ParamPanelSource::Scene,
        );
        let canvas_source = canvas_scene.as_ref().map_or(
            crate::gui::CanvasSource::Empty,
            crate::gui::CanvasSource::Scene,
        );

        let recent_files = self.preferences.history.recent_files.clone();
        // The lanes the attribute strip can offer, only while the viewport
        // is up to draw it: a walk over the displayed geometries' summaries.
        let scene_lanes = if self.gui.viewport_tab_present() {
            self.scene_lanes()
        } else {
            Vec::new()
        };
        let arrangements: Vec<String> = self
            .preferences
            .dock
            .arrangements
            .iter()
            .map(|arrangement| arrangement.name.clone())
            .collect();
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
            uv_overlap_pct,
            cameras: &scene_cameras,
            look_through: look_through_mirror,
            camera_locked: std::array::from_fn(|i| self.is_locked_look_through(i)),
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
            },
            crate::gui::ViewportChrome {
                divider,
                pane_gaps: &pane_gaps,
                active_pane_rect,
                review_panes: &review_panes,
                toolbars: pane_toolbar,
            },
            &crate::gui::PanelSources {
                settings,
                hud: &hud,
                tree: tree_source,
                assets: match &self.engine {
                    Some(engine)
                        if self.gui.assets_tab_present()
                            || self.gui.asset_preview_tab_present() =>
                    {
                        crate::gui::AssetsSource::Scene {
                            table: engine.asset_table(),
                        }
                    }
                    _ => crate::gui::AssetsSource::Empty,
                },
                texture: texture_source,
                attributes: match &self.engine {
                    Some(engine) if self.gui.attributes_tab_present() => {
                        crate::gui::AttributesSource::Scene {
                            engine,
                            ctx: self.gui.graph_ctx(),
                        }
                    }
                    _ => crate::gui::AttributesSource::Empty,
                },
                text: match &self.engine {
                    Some(engine) if self.gui.text_tab_present() => crate::gui::TextSource::Scene {
                        doc: engine.document(),
                        registry: engine.registry(),
                        current: self.gui.graph_ctx(),
                    },
                    _ => crate::gui::TextSource::Empty,
                },
                preview: crate::gui::PreviewView {
                    texture: self.preview.texture(),
                    loading: self.preview.loading(),
                    error: self.preview.error(),
                },
                canvas: canvas_source,
                params: params_source,
                params_floating: params_floating_source,
                recent_files: &recent_files,
                arrangements: &arrangements,
                attr: crate::gui::AttrColumnSource {
                    viz: &self.attr_viz,
                    lanes: &scene_lanes,
                    capacity: self.attr_pin_stats.0,
                    total: self.attr_pin_stats.1,
                },
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
            // The handle frame and the snaps reach the solver through the
            // same re-read the key and the viewport menu use.
            self.apply_gizmo_prefs();
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
        self.handle_turntable_modal();
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
fn params_source<'a>(
    engine: Option<&'a solarxy_graph::Engine>,
    tab_present: bool,
    ctx: GraphContext,
    pin: Option<NodeId>,
    assets: &'a [(String, String)],
    lanes: &'a [(String, String)],
    error: Option<&'a str>,
    resolved: &'a crate::gui::ResolvedParams,
) -> Option<crate::gui::ParamScene<'a>> {
    let engine = engine.filter(|_| tab_present)?;
    let subject = params_subject(Some(engine), tab_present, ctx, pin);
    let validation = subject.and_then(|node| engine.validation(node));
    Some(crate::gui::ParamScene {
        doc: engine.document(),
        registry: engine.registry(),
        ctx,
        stats: subject.and_then(|node| engine.node_stats(node)),
        // Presence, not cleanliness: a clean validate cook still stores a
        // report, and the tab that says so is the point of it.
        has_report: validation.is_some(),
        report: validation.map(|v| &v.report),
        #[allow(clippy::cast_possible_truncation)]
        counts: validation.map_or((0, 0), |v| {
            (
                v.report.error_count() as u32,
                v.report.warning_count() as u32,
            )
        }),
        assets,
        lanes,
        error,
        resolved,
    })
}

/// What each expression-driven parameter on a node currently resolves to.
///
/// **The driven rows only.** Resolving every parameter would run the
/// evaluator over a node's whole schema once a frame to answer a question
/// no row is asking; the literal rows already know their own value.
fn resolved_expressions(
    engine: &solarxy_graph::Engine,
    ctx: GraphContext,
    node: NodeId,
) -> Option<crate::gui::ResolvedParams> {
    let data = engine.document().graph(ctx).ok()?.node(node)?;
    let driven: Vec<&String> = data
        .params
        .iter()
        .filter(|(_, source)| {
            matches!(
                source,
                solarxy_graph::params::ParamSource::Expression { .. }
            )
        })
        .map(|(key, _)| key)
        .collect();
    if driven.is_empty() {
        return None;
    }
    Some(
        driven
            .into_iter()
            .map(|key| (key.clone(), engine.resolved_param(ctx, node, key)))
            .collect(),
    )
}

/// Which node the parameter panel will draw.
///
/// The pin, else the selection's first. Resolved once and read by three
/// callers, because the statistics, the lanes and the cook failure all
/// have to describe the node the panel actually shows: taking one of them
/// for a different node is the kind of mistake that looks like a stale
/// readout rather than like a bug.
/// What one host of the parameter panel needs owned on its behalf.
///
/// Owned here rather than inside the source, because the source holds
/// borrows: the lanes come from a cooked geometry the engine assembles on
/// request, and the resolved values are pulled once a frame for the driven
/// rows only, since an expression's value moves with every applied command
/// and pushing it would be one event per expression per frame.
#[derive(Default)]
struct SubjectFacts {
    subject: Option<NodeId>,
    lanes: Vec<(String, String)>,
    resolved: crate::gui::ResolvedParams,
}

impl State {
    /// Assemble a host's subject, or nothing at all while the host is down.
    fn param_subject_facts(&self, up: bool, pin: Option<NodeId>) -> SubjectFacts {
        let ctx = self.gui.graph_ctx();
        let Some(engine) = self.engine.as_deref().filter(|_| up) else {
            return SubjectFacts::default();
        };
        let subject = params_subject(Some(engine), up, ctx, pin);
        SubjectFacts {
            subject,
            lanes: subject
                .and_then(|node| upstream_lanes(engine, ctx, node))
                .unwrap_or_default(),
            resolved: subject
                .and_then(|node| resolved_expressions(engine, ctx, node))
                .unwrap_or_default(),
        }
    }
}

fn params_subject(
    engine: Option<&solarxy_graph::Engine>,
    tab_present: bool,
    ctx: GraphContext,
    pin: Option<NodeId>,
) -> Option<NodeId> {
    let engine = engine.filter(|_| tab_present)?;
    pin.or_else(|| {
        engine
            .document()
            .graph(ctx)
            .ok()
            .and_then(|g| g.selection.first().copied())
    })
}

/// The attribute lanes on the geometry feeding a node.
///
/// **The node's default geometry input, else its first**, which is the
/// browser's rule. Nothing when the input is unwired or the upstream node
/// has not cooked: an attribute name is free text and a node with no
/// completions is ordinary rather than broken.
fn upstream_lanes(
    engine: &solarxy_graph::Engine,
    ctx: GraphContext,
    node: NodeId,
) -> Option<Vec<(String, String)>> {
    let graph = engine.document().graph(ctx).ok()?;
    let desc = engine.registry().get(&graph.node(node)?.type_id)?;
    let geometry = |port: &&solarxy_graph::registry::PortSpec| {
        port.data_type == solarxy_graph::registry::coerce::DataType::Geometry
    };
    let input = desc
        .inputs
        .iter()
        .find(|port| port.is_default && geometry(port))
        .or_else(|| desc.inputs.iter().find(geometry))?;
    let edge = graph
        .edges()
        .find(|edge| edge.to == node && edge.to_port == input.key)?;
    let summary = engine.attribute_summary(edge.from)?;
    Some(
        summary
            .point
            .iter()
            .chain(summary.primitive.iter())
            .map(|lane| (lane.name.clone(), lane.ty.to_string()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::resolved_expressions;
    use solarxy_graph::document::{GraphContext, NodeId};
    use solarxy_graph::{Command, Engine, EngineEvent};

    /// The readout is pulled for the driven rows and for nothing else.
    ///
    /// Resolving a node's whole schema once a frame would run the
    /// evaluator over parameters no row is asking about, and the literal
    /// rows already know their own value.
    #[test]
    fn only_the_expression_driven_rows_are_resolved() {
        let mut engine = Engine::new().expect("registry builds");
        let geo = add(&mut engine, GraphContext::Root, "sopnet");
        let ctx = GraphContext::Subflow(geo);
        let node = add(&mut engine, ctx, "box");

        // Nothing is driven yet, so there is nothing to pull.
        assert!(
            resolved_expressions(&engine, ctx, node).is_none(),
            "a node with no expression must cost no evaluation"
        );

        // A literal write is still not a driven row.
        engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: "height".to_string(),
                value: solarxy_graph::params::ParamSource::Literal(
                    solarxy_graph::params::ParamValue::Float(3.0),
                ),
            })
            .expect("a literal write");
        assert!(
            resolved_expressions(&engine, ctx, node).is_none(),
            "a literal row knows its own value and must not be resolved"
        );

        engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: "width".to_string(),
                value: solarxy_graph::params::ParamSource::Expression {
                    expr: "2 + 3".to_string(),
                },
            })
            .expect("a float takes an expression");
        let resolved = resolved_expressions(&engine, ctx, node).expect("one driven row");
        assert_eq!(
            resolved.keys().collect::<Vec<_>>(),
            vec!["width"],
            "only the driven row is resolved"
        );
        assert_eq!(
            resolved.get("width"),
            Some(&Ok(solarxy_graph::params::ParamValue::Float(5.0)))
        );

        // A broken one resolves to the parser's complaint rather than to
        // nothing, which is what the row prints under the field.
        engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: "width".to_string(),
                value: solarxy_graph::params::ParamSource::Expression {
                    expr: "2 +".to_string(),
                },
            })
            .expect("the engine stores a broken expression rather than refusing it");
        let resolved = resolved_expressions(&engine, ctx, node).expect("one driven row");
        assert!(
            resolved.get("width").is_some_and(Result::is_err),
            "a broken expression must resolve to a message, not to a value"
        );
    }

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
}
