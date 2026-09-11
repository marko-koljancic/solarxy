//! Per-frame state updates. Owns `rebuild_light_bind_group` — the **single
//! IBL chokepoint** triggered on HDRI drop, [`IblMode`] toggle (`I` /
//! `Shift+I`), and background change. Adding any IBL-derived uniform means
//! routing it through this function so Clay modes etc. update instantly
//! without waiting for the next camera-driven frame.

use std::sync::mpsc;

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::naming::node_name;

use super::*;
use super::engine_scene;

/// Wall-clock ceiling on one frame's cook.
///
/// Half a 60 Hz frame, so a cook that cannot finish leaves the rest of the
/// budget for rendering and input rather than dropping the window to the
/// engine's pace. Cooks are resumable, so the only cost of a low ceiling is
/// that a heavy scene converges over more frames while staying interactive.
const COOK_BUDGET: std::time::Duration = std::time::Duration::from_millis(8);

impl State {
    pub(super) fn rebuild_light_bind_group(&mut self) {
        solarxy_host::rebuild_light_bind_group(
            &self.device,
            &self.queue,
            &mut self.renderer,
            &mut self.env,
            self.view.display.hdri_intensity,
        );
    }

    /// The bounds every camera frames against and every bounds-derived
    /// uniform fits to: the loaded file model unioned with the visible
    /// multi-object scene, falling back to the box the environment is
    /// currently fitted to when the viewport holds neither.
    ///
    /// The two halves are independent. A file model can be open with nothing
    /// cooked, cooked objects can exist with no file open, and closing a
    /// model leaves the pane cameras alive framing whatever is left. Framing
    /// a union is also what keeps a cooked object inside the shadow frustum
    /// and inside the Depth mode's fitted near/far range.
    pub(super) fn scene_bounds(&self) -> solarxy_core::AABB {
        // Deliberately the fixed placeholder rather than `env_bounds` when
        // there is nothing to fit to. `env_bounds` records what the
        // environment is currently fitted to, which on an emptied scene is
        // whatever was there last; using it here would let each round of "add
        // something, frame it, remove it" answer from the previous round's
        // box, so repeated cycles would shrink toward nothing.
        self.raster
            .scene()
            .visible_bounds()
            .unwrap_or_else(solarxy_renderer::environment::placeholder_bounds)
    }

    /// Replace the scene environment with one fitted to `bounds`.
    ///
    /// The fresh environment arrives with a three-point rig synthesized from
    /// `bounds`, which is right when the lights are free to follow the view
    /// and wrong when they are locked: a locked rig is the one thing the user
    /// asked not to move, and no later frame would put it back, because every
    /// path that recomputes lighting is skipped under the lock. So the rig is
    /// carried across the swap while locked, shadow frustum included.
    ///
    /// The IBL chokepoint at the end restores the environment intensity and
    /// the ambient average, and writes the whole uniform, so the carried rig
    /// reaches the GPU without a second write.
    fn rebuild_env(&mut self, bounds: solarxy_core::AABB) {
        // The fresh environment's visualization half is bounds-only, so
        // whatever per-mesh overlays were baked into the old one are gone.
        self.viz_dirty = true;
        let grid_color = self
            .resolve_background(&self.view.pane_settings[0])
            .grid_color();
        let carried = self
            .view
            .display
            .lights_locked
            .then_some(self.env.lights_uniform);
        self.env = solarxy_host::build_bounds_env(
            &self.device,
            &self.queue,
            &self.renderer,
            &bounds,
            grid_color,
            self.preferences.rendering.shadow_map_size,
        );
        self.env_bounds = bounds;
        if let Some(previous) = carried {
            self.env.lights_uniform = previous;
            let key = previous.lights[0].position;
            self.env.shadow.update_light_vp(
                &self.queue,
                cgmath::Point3::new(key[0], key[1], key[2]),
                bounds.center(),
                bounds.diagonal() / 2.0,
            );
        }
        self.rebuild_light_bind_group();
    }

    /// Swap the file model's environment for a bounds-only one, keeping
    /// whatever the multi-object scene still holds in frame.
    ///
    /// Called when the model closes. That environment's visualization half
    /// was built from the model - per-mesh bounds, per-mesh normal arrows -
    /// and the normal-arrow segments are drawn zipped against the frame's
    /// draw list, so leaving it installed would paint the closed model's
    /// arrows over whatever geometry remains.
    ///
    /// The ground keeps the box it already had when nothing is left to fit
    /// to. The pane cameras deliberately survive a close, so they are still
    /// framing what was just closed; snapping the floor and the grid to a
    /// fixed placeholder underneath them would read as the ground jumping.
    pub(super) fn reset_env_for_empty_scene(&mut self) {
        let bounds = self
            .raster
            .scene()
            .visible_bounds()
            .unwrap_or(self.env_bounds);
        self.rebuild_env(bounds);
    }

    /// Refit the environment, meaning grid, floor and shadow frustum, when
    /// the scene's bounds move.
    ///
    /// This used to be frozen while a file model was loaded, because that
    /// environment came off the loader thread carrying the model's own
    /// normal-arrow and per-mesh-bounds buffers and refitting from bounds
    /// alone would have dropped them. With one root there is no such
    /// environment to protect, and the overlay buffers are rebuilt on their
    /// own schedule instead.
    pub(super) fn sync_env_bounds(&mut self) {
        // Nothing to fit to leaves the ground where it is, for the reason
        // stated on `reset_env_for_empty_scene`: refitting an emptied scene
        // to a placeholder moves the floor and the grid under a camera that
        // has not moved. Framing and sizing do not read the environment for
        // this, they ask `scene_bounds`, which answers with the placeholder.
        let Some(bounds) = self.raster.scene().visible_bounds() else {
            return;
        };
        let eps = (self.env_bounds.diagonal() * 1e-3).max(1e-6);
        let close = |a: cgmath::Point3<f32>, b: cgmath::Point3<f32>| {
            (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps && (a.z - b.z).abs() < eps
        };
        if close(bounds.min, self.env_bounds.min) && close(bounds.max, self.env_bounds.max) {
            return;
        }
        self.rebuild_env(bounds);
    }

    /// Rebuild the per-mesh overlay buffers when they are stale and a pane
    /// actually shows one.
    ///
    /// The normal arrows and the per-mesh bounds boxes are baked geometry, not
    /// a pass over the live scene, so something has to rebuild them when the
    /// scene moves. The shell used to get them free: a file model's
    /// environment came off the loader thread already carrying them, and a
    /// document's never did, so the overlays worked for one root and silently
    /// drew nothing for the other. With one root they are built here for
    /// every document, from the aggregate both shells share.
    pub(super) fn sync_visualization(&mut self) {
        if !self.viz_dirty
            || !solarxy_host::visualization::overlays_wanted(
                &self.view.pane_settings,
                self.view.display.layout.pane_count(),
            )
        {
            return;
        }
        let Some(bounds) = self.raster.scene().visible_bounds() else {
            return;
        };
        self.viz_dirty = false;
        let (mesh_bounds, normals) =
            solarxy_host::visualization::build_aggregate(self.raster.scene());
        let grid_color = self
            .resolve_background(&self.view.pane_settings[0])
            .grid_color();
        self.env.vis = solarxy_renderer::visualization::VisualizationState::new_from_parts(
            &self.device,
            &self.renderer.layouts,
            &bounds,
            &mesh_bounds,
            Some(&normals),
            grid_color,
        );
    }

    /// Apply any `SceneOp::SetEnvironment` in a drained delta.
    ///
    /// Separate from `SceneObjects::apply` because the environment is the
    /// IBL and the skybox, which that type cannot reach. The web host runs
    /// the same tracker over the same op; this is the desktop half.
    ///
    /// Nothing emits this op on the desktop yet: `solarxy-app` has no
    /// engine until the desktop gains one, so the only producer today is
    /// the `F9` developer harness. That is deliberate rather than dead
    /// code, and the harness is what makes it verifiable now.
    /// Apply a scene delta's environment. The body is
    /// [`solarxy_host::apply_scene_environment`], shared with the web shell
    /// since 0.10.0; what stays here is the one reaction that is this
    /// shell's, which is marking a traced backend's environment copy stale.
    pub(super) fn apply_scene_environment(&mut self, delta: &solarxy_core::scene::SceneDelta) {
        let applied = solarxy_host::apply_scene_environment(
            &self.device,
            &self.queue,
            &mut self.renderer,
            &mut self.env,
            &mut self.environment,
            &mut self.view,
            &self.preferences.view.custom_backgrounds,
            delta,
        );
        if applied.tracer_dirty {
            self.traced_env_dirty = true;
        }
    }

    /// Snap every bound pane's camera to its camera node's current pose.
    ///
    /// The body is [`solarxy_host::cameras::follow_camera_bindings`], shared
    /// with the web shell. This shell suppresses no pane: it has neither of
    /// the two states the browser holds the follow off for, so the mask is
    /// all false and every bound pane follows every frame.
    fn follow_look_through_cameras(&mut self) {
        let Some(defs) = self.raster.scene().cameras() else {
            return;
        };
        solarxy_host::cameras::follow_camera_bindings(
            defs,
            &self.look_through,
            &[false; 4],
            &mut self.view.cameras,
        );
    }

    /// Push one pane's wireframe uniform.
    ///
    /// **This is very likely redundant and is deliberately not deleted here.**
    /// The shared buffer is rewritten before every pane's passes, by the 3D
    /// path and by the image path, so nothing this writes can survive to a
    /// draw. It is kept, now reading the right pane rather than pane 0, so the
    /// removal is a considered change rather than a side effect of a defect
    /// fix.
    pub(super) fn update_wireframe_params(&self, pane: usize) {
        let pds = &self.view.pane_settings[pane.min(3)];
        solarxy_host::write_wireframe_params(
            &self.queue,
            &self.renderer,
            self.resolve_background(pds),
            pds,
            &self.view.display,
        );
    }

    /// Advance the node engine one frame: clock, cook, then hand the
    /// resulting geometry to the renderer.
    ///
    /// Runs immediately before the environment refit and immediately before
    /// the frame's delta drain, so geometry cooked this frame is on screen
    /// this frame rather than one behind.
    ///
    /// Cooking is bounded by wall clock rather than run to completion. A cook
    /// is resumable, so a scene too heavy to finish inside the budget makes
    /// progress every frame and stays interactive throughout, instead of
    /// freezing the window until it converges.
    ///
    /// Jobs resolve inline. Asynchronous offloading exists for the browser,
    /// which has an import worker; here the engine parses during the cook and
    /// the drained queue is normally empty, so this loop is a safety net for
    /// anything the cook chose to defer rather than the main path.
    fn drive_engine(&mut self) {
        let Some(engine) = self.engine.as_mut() else {
            self.cook_readout = crate::gui::CookReadout::default();
            return;
        };

        let batch = engine.tick();
        let dirty = !batch.events.is_empty();

        let deadline = Instant::now() + COOK_BUDGET;
        let mut within_budget = || Instant::now() < deadline;
        let cooked = engine.cook(&mut within_budget);

        for (ctx, id, request) in engine.take_jobs() {
            let result = engine.resolve_job(&request);
            engine.submit_job_result(ctx, id, result);
        }

        // The header's readout. The mode is read rather than mirrored, so a
        // scene opened in manual mode says so with no event plumbing. The
        // stale count walks every node, so it is taken only while the strip
        // can show it: in manual mode, while a cook is still working in
        // either mode, and on the frame it settles so the count reads zero
        // rather than what it was.
        let mode = engine.cook_mode();
        let cooking = engine.has_pending_cook();
        let stale = if mode == solarxy_graph::engine::CookMode::Manual
            || cooking
            || self.cook_readout.cooking
        {
            engine.dirty_nodes().len()
        } else {
            0
        };
        self.cook_readout = crate::gui::CookReadout {
            open: true,
            mode,
            stale,
            cooking,
        };

        // Surface fresh cook failures. The engine emits a status event
        // only on a transition, so this toasts once per failed cook, not
        // once per frame; warnings never ride this stream (they are
        // pull-based on the driver), so bake announcements and migration
        // notices stay out of the toast queue by construction. Names are
        // resolved while the engine borrow is live, toasted after it ends.
        let mut fresh = self.cook_health.absorb(&batch.events);
        fresh.extend(self.cook_health.absorb(&cooked));
        let toasts: Vec<String> = fresh
            .into_iter()
            .map(|(id, message)| {
                let name = find_node_name(engine, id);
                format!("{name} failed to cook: {message}")
            })
            .collect();

        if dirty || !cooked.is_empty() {
            let delta = engine.take_scene_delta();
            if !delta.ops.is_empty() {
                self.pending_scene_deltas.push(delta);
            }
        }

        for message in toasts {
            self.gui
                .set_toast(&message, crate::gui::ToastSeverity::Error);
        }
    }

    /// Re-derive everything the inspection panels read from the cooked
    /// scene, after a delta has been applied.
    ///
    /// Object names come from the document rather than from the renderer:
    /// the engine mints each object's id from its owning node, so an object
    /// resolves back to the name the user gave that node instead of being
    /// listed as an opaque number.
    pub(super) fn refresh_engine_scene_info(&mut self) {
        let (Some(engine), Some(info)) = (self.engine.as_ref(), self.engine_scene.as_mut()) else {
            return;
        };

        let registry = engine.registry();
        let graph = engine.document().graph(GraphContext::Root).ok();
        info.object_names = self
            .raster
            .scene()
            .iter()
            .map(|(id, _)| {
                let name = graph
                    .and_then(|g| g.node(NodeId(id.0)))
                    .map_or_else(|| format!("Object {}", id.0), |n| node_name(n, registry));
                (*id, name)
            })
            .collect();

        info.counts = engine_scene::count_geometry(self.raster.scene());

        // Zipped rather than looked up per object: `object_names` was built
        // from this same iterator a moment ago, so the orders agree by
        // construction and the merged issue order is the iteration order.
        let merged = engine_scene::merge_validation(
            self.raster
                .scene()
                .iter()
                .zip(&info.object_names)
                .filter_map(|((id, _), (_, name))| {
                    let result = self.raster.scene().validation(*id)?;
                    Some((*id, name.as_str(), &result.report))
                }),
        );
        info.validation = merged;

        let bounds = self
            .raster
            .scene()
            .visible_bounds()
            .unwrap_or(self.env_bounds)
            .size();
        self.gui.update_scene_info(
            &info.filename,
            &info.path,
            info.file_size,
            info.counts,
            [bounds.x, bounds.y, bounds.z],
        );
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

    /// Lazily create a `CameraState` for every pane slot the current layout
    /// uses, framing `bounds`. Idempotent — a slot that already holds a
    /// camera is skipped, so layout toggles preserve per-slot cameras within
    /// a session. Slot 0 is the perspective Single-layout camera; slots 1-3
    /// seed to orthographic Top / Front / Left — a one-time convenience that
    /// the user re-orients with T / F / L.
    ///
    /// Takes its subject explicitly because the two callers disagree about
    /// what to frame: a freshly opened file frames itself, while the
    /// per-frame path frames the whole scene.
    pub(super) fn ensure_pane_cameras_with(&mut self, bounds: &solarxy_core::AABB) {
        let (tw, th) = self.target_dimensions();
        let aspect = tw as f32 / th.max(1) as f32;
        solarxy_host::ensure_pane_cameras(
            &self.device,
            &self.renderer.layouts.camera,
            &mut self.view.cameras,
            bounds,
            aspect,
            self.view.display.layout.pane_count(),
            Some(self.preferences.display.projection_mode),
        );
    }

    /// Seed any missing pane camera against the whole scene.
    ///
    /// The per-frame entry point. With nothing loaded it frames the
    /// placeholder box rather than doing nothing, so every pane has a camera
    /// from the first frame and renders through the full pass chain — grid,
    /// floor, axes and background — instead of the empty pass.
    pub(super) fn ensure_pane_cameras(&mut self) {
        let bounds = self.scene_bounds();
        self.ensure_pane_cameras_with(&bounds);
    }

    pub(super) fn resize_render_targets(&mut self, width: u32, height: u32) {
        if !self.renderer.resize_targets(&self.device, width, height) {
            return;
        }
        // Shell policy, which is why it stays here rather than moving into the
        // renderer with the body above: the overlap statistic is measured over
        // a texture that just changed size, so a pane showing it has to
        // remeasure. The web shell has no equivalent because it recomputes the
        // statistic from its own view state.
        if self.view.pane_settings.iter().any(|p| p.show_uv_overlap) {
            self.renderer.uv_overlap.stats_dirty = true;
        }
    }

    pub fn update(&mut self) {
        self.refresh_title();
        if let Some(choice) = self.gui.take_unsaved_choice() {
            self.resolve_discard(choice);
        }
        if let Some(choice) = self.gui.take_recovery_choice() {
            self.resolve_recovery(choice);
        }
        self.poll_autosave();
        // Uncaptured GPU faults recorded since the last frame. The hook
        // already logged each full message on the `solarxy::gpu` target,
        // which the console captures; the toast is the short pointer, and
        // its wording does not promise the picture is right afterwards.
        for fault in self.gpu_faults.drain() {
            use solarxy_renderer::faults::GpuFaultKind;
            let what = match fault.kind {
                GpuFaultKind::Validation => {
                    "Graphics validation error; the view may be wrong. Details in the Console."
                }
                GpuFaultKind::OutOfMemory => {
                    "The GPU ran out of memory; the view may be incomplete. Details in the Console."
                }
                GpuFaultKind::Internal => {
                    "Graphics driver error; the view may be wrong. Details in the Console."
                }
            };
            if fault.count > 1 {
                self.gui.set_toast(
                    &format!("{what} (repeated {} times)", fault.count),
                    ToastSeverity::Error,
                );
            } else {
                self.gui.set_toast(what, ToastSeverity::Error);
            }
        }
        self.follow_look_through_cameras();
        let hdri_poll = self.pending_hdri.as_ref().map(|p| p.receiver.try_recv());
        match hdri_poll {
            Some(Ok(Ok(new_ibl))) => {
                // A new file replaces whatever hash a save had staged.
                self.hdri_hash = None;
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
                // The sidebar picker replaced the IBL outside the scene
                // contract; see `clear_hdri` for why the tracker has to
                // forget what it thinks is installed.
                self.environment.invalidate();
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

        self.poll_pending_open();
        self.handle_pending_drop();

        self.drive_engine();
        self.gui.set_scene_open(self.engine.is_some());
        self.sync_env_bounds();
        self.sync_visualization();

        let now = Instant::now();
        self.dt = (now - self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        self.view.active_pane = self.active_pane_index();
        self.ensure_pane_cameras();
        self.apply_pending_frame();

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

        if !self.install_authored_lights()
            && !self.view.display.lights_locked
            && let Some(cam0) = self.view.cameras[0].as_ref().map(|c| c.camera)
        {
            let ibl_avg = solarxy_host::active_ibl(&self.renderer).irradiance_average;
            let bounds = self.scene_bounds();
            solarxy_host::setup_pane_lighting(&self.queue, &mut self.env, &cam0, &bounds, ibl_avg);
        }
    }

    /// Install the scene's own lights, if it has any. Returns whether it did.
    ///
    /// Authored lights outrank both the synthesized viewer rig and Lock
    /// Lights. A scene carrying lights is describing how it wants to look,
    /// and a rig that followed the camera on top of that would light it
    /// twice; Lock Lights is a control over the synthesized rig, so with
    /// nothing synthesized there is nothing for it to freeze.
    ///
    /// Both rig-writing paths call this first - the per-frame one for the
    /// primary pane, and the per-pane one for the rest - because either
    /// alone would let the other overwrite the authored rig on the next
    /// frame.
    pub(super) fn install_authored_lights(&mut self) -> bool {
        let Some(defs) = self.raster.scene().authored_lights() else {
            return false;
        };
        let ibl_avg = solarxy_host::active_ibl(&self.renderer).irradiance_average;
        let sphere_scale = self.scene_bounds().diagonal() * 0.04;
        self.env.lights_uniform =
            solarxy_renderer::light::LightsUniform::from_defs(defs, sphere_scale, ibl_avg);
        self.queue.write_buffer(
            &self.env.light_buffer,
            0,
            bytemuck::cast_slice(&[self.env.lights_uniform]),
        );
        true
    }
}

/// A node's display name looked up across every context, root first,
/// because a cook status event carries no context. Falls back to the
/// bare id for a node that vanished between the event and this frame.
pub(super) fn find_node_name(engine: &solarxy_graph::Engine, id: NodeId) -> String {
    let doc = engine.document();
    let registry = engine.registry();
    std::iter::once(GraphContext::Root)
        .chain(doc.subflow_owners().map(GraphContext::Subflow))
        .find_map(|ctx| doc.graph(ctx).ok().and_then(|g| g.node(id)))
        .map_or_else(|| format!("Node {}", id.0), |n| node_name(n, registry))
}
