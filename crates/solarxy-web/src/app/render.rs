//! The per-pane render orchestration this host drives.

use super::*;

// Internal orchestration: the web port of the desktop per-pane render loop.
impl SolarxyApp {
    /// The still the render node's settings describe, as this shell renders it.
    pub(super) fn still_spec(
        &self,
        opts: &solarxy_graph::nodes::RenderSettings,
        engine: solarxy_host::still::StillEngine,
        readback: solarxy_host::still::StillReadback,
    ) -> solarxy_host::still::StillSpec {
        solarxy_host::still::StillSpec {
            width: opts.width,
            height: opts.height,
            engine,
            samples: opts.samples,
            // Bloom is the only screen-space pass a still keeps; ambient
            // occlusion is off for traced output and a raster still inherits
            // whatever the viewport had.
            screen_space_post: self.renderer.post.bloom_enabled,
            tile_budget: solarxy_host::still::TILE_BUDGET_PIXELS,
            // Chosen in the dialog before the render starts, because the
            // readback decides what the tiles are and cannot be changed once
            // they are arriving.
            readback,
            // Albedo and normal come out of one store, so either of them asks
            // for the same copy. Derived exactly the way the headless command
            // derives them, so a scene renders the same passes wherever it is
            // opened.
            aux: opts.aov_albedo || opts.aov_normal,
            depth: opts.aov_depth,
            // The dialog is watching. At the production tile budget a 1920 by
            // 1080 render is a single tile, so without this nothing reaches the
            // canvas until the whole image is finished.
            preview_interval_ms: solarxy_host::still::PREVIEW_INTERVAL_MS,
            transparent: opts.transparent_background,
        }
    }

    pub(super) fn compute_panes(&self) -> Vec<PaneRect> {
        panes::compute_panes(
            self.view.display.layout,
            self.view.display.split_ratio,
            (0.0, 0.0),
            (self.config.width as f32, self.config.height as f32),
        )
    }

    pub(super) fn pane_rects_css(&self) -> Vec<RectDto> {
        self.compute_panes()
            .iter()
            .map(|p| RectDto {
                x: p.x / self.dpr,
                y: p.y / self.dpr,
                width: p.width / self.dpr,
                height: p.height / self.dpr,
            })
            .collect()
    }

    pub(super) fn push_pane_rects_if_changed(&mut self, _physical: &[PaneRect]) {
        let css = self.pane_rects_css();
        if css != self.last_pane_rects {
            self.last_pane_rects.clone_from(&css);
            self.host_events.push(HostEvent::PaneRects { rects: css });
        }
    }

    /// The desktop `rebuild_light_bind_group` chokepoint, ported: retargets
    /// the skybox at the active IBL's equirect, rebuilds the light bind
    /// group per the IBL mode (full / diffuse-only / fallback), and pushes
    /// the IBL-derived ambient average so clay modes update instantly.
    pub(super) fn rebuild_light_bind_group(&mut self) {
        solarxy_host::rebuild_light_bind_group(
            &self.device,
            &self.queue,
            &mut self.renderer,
            &mut self.env,
            self.view.display.hdri_intensity,
        );
    }

    /// Keep a watched tracer in step with the frame: the scene delta, the
    /// environment scalars, and any host-side view mutation each reset the
    /// accumulation, which is the preview's whole reset contract (camera
    /// moves are per pane and handled at encode). Does nothing while no
    /// pane is traced, so a session that never traces pays one boolean.
    pub(super) fn feed_traced_preview(&mut self, delta: &solarxy_core::scene::SceneDelta) {
        let any_traced = self
            .view
            .pane_settings
            .iter()
            .any(|p| p.pane_mode == PaneMode::Scene3D && p.pane_engine == PaneEngine::Traced);
        if !any_traced || self.tracer.is_none() {
            return;
        }
        if !delta.ops.is_empty()
            && let Some(t) = self.tracer.as_mut()
        {
            // The same feed the raster gets: the delta lands and every
            // accumulation resets, since the mean was of another scene.
            // The camera keys reset with it so each pane re-anchors its
            // pose on its next encode.
            t.apply(&self.device, &self.queue, delta);
            t.invalidate();
            self.traced_cam_keys = [None; 4];
        }
        let env_params = (
            self.view.display.hdri_intensity,
            self.view.display.hdri_rotation,
        );
        // Not while a still is running: the job owns the shared backend,
        // including its environment, and a pane install here would swap a
        // still's sky out from under it and reset the mean it has been
        // accumulating. Leaving the flag set is what records that the install
        // is owed once the job lets go.
        if self.still.is_none() && (self.traced_env_dirty || env_params != self.traced_env_params) {
            self.sync_traced_environment();
            self.traced_env_params = env_params;
            if let Some(t) = self.tracer.as_mut() {
                t.invalidate();
                self.traced_cam_keys = [None; 4];
            }
        }
        if self
            .host_events
            .iter()
            .any(|e| matches!(e, HostEvent::ViewChanged))
            && let Some(t) = self.tracer.as_mut()
        {
            t.invalidate();
            self.traced_cam_keys = [None; 4];
        }
    }

    /// One traced pane's pre-encode housekeeping: assert the preview's
    /// settings, reset the accumulation when the pane's camera moved, and
    /// re-apply the viewer rig.
    pub(super) fn prepare_traced_pane(&mut self, i: usize) {
        // Asserted per encode rather than held, because the still job
        // authors its own settings on the same backend and whichever ran
        // last would otherwise win.
        if let Some(t) = self.tracer.as_mut() {
            t.set_settings(preview_trace_settings(self.preview_denoise));
            // The filter's steering, for the same reason and it is newly load
            // bearing: a still authored from a render node now writes these
            // four, so without this a preview would inherit whatever the last
            // still asked for. The preview keeps the measured defaults, which
            // is what it ran at when nothing called this setter at all. The
            // two denoise settings are deliberately separate: one is a
            // delivered frame's, the other a preview's, and they want
            // different answers.
            t.set_denoise_settings(DenoiseSettings::default());
        }
        // A pane bound to a camera previews through that camera's lens; a
        // free view is a pinhole. Asserted per encode like the settings above
        // and for the same reason: the still job authors its own on the same
        // backend, so whichever ran last would otherwise win. `set_lens` is a
        // no-op when nothing moved, which is what lets a pane keep converging.
        let pane_lens = self
            .pane_camera_def(i)
            .map(solarxy_host::cameras::lens_for)
            .unwrap_or_default();
        if let Some(t) = self.tracer.as_mut() {
            t.set_lens(pane_lens);
        }
        let Some(cam) = self.view.cameras.get(i).and_then(Option::as_ref) else {
            return;
        };
        let key = camera_key(&cam.camera);
        if self.traced_cam_keys.get(i).copied().flatten() != Some(key) {
            if let Some(t) = self.tracer.as_mut() {
                t.invalidate_pane(i);
            }
            if let Some(slot) = self.traced_cam_keys.get_mut(i) {
                *slot = Some(key);
            }
        }
        // The viewer rig is re-applied every traced frame, not only on
        // reset: it is scene data shared by every pane, and with two
        // traced panes the last writer would otherwise win across frames.
        // A no-op under authored lights, and a constant write while the
        // camera rests, so it never disturbs a converging mean.
        let camera = cam.camera;
        if let Some(t) = self.tracer.as_mut() {
            solarxy_host::apply_viewer_rig(
                &self.device,
                &self.queue,
                t,
                self.raster.scene(),
                &camera,
            );
        }
    }

    /// Push a traced pane's sample counter, on change rather than per
    /// frame. A `Complete` outcome is a converged pane: the count parks
    /// at the target instead of vanishing.
    pub(super) fn push_pane_samples(&mut self, i: usize, outcome: FrameOutcome) {
        let counts = match outcome {
            FrameOutcome::Converging {
                samples,
                target_samples,
            } => Some((samples, target_samples)),
            FrameOutcome::Complete => self
                .last_pane_samples
                .get(i)
                .copied()
                .flatten()
                .map(|(_, target)| (target, target)),
        };
        if counts.is_some()
            && let Some(slot) = self.last_pane_samples.get_mut(i)
            && *slot != counts
        {
            *slot = counts;
            if let Some((samples, target)) = counts {
                self.host_events.push(HostEvent::PaneSamples {
                    pane: i,
                    samples,
                    target,
                });
            }
        }
    }

    /// Brings a traced pane's environment up to date with the scene's, which
    /// is what makes a traced pane light the way the raster pane beside it
    /// does.
    ///
    /// The traced scene cache deliberately drops the environment op, on the
    /// reasoning that a host already holds the decoded and convolved image and
    /// should build the traced environment from that rather than keep a second
    /// copy of the largest asset in a scene. This is the host half of that
    /// decision. Without it the kernel integrates against its own constant sky
    /// and an image lit by a sunset renders as though lit by a dim room.
    ///
    /// A still does not come through here. See `install_still_environment`
    /// for what it installs instead, and why a pane and a still are allowed
    /// to disagree about this one thing.
    pub(super) fn sync_traced_environment(&mut self) {
        if self.tracer.is_none() {
            return;
        }
        if !std::mem::take(&mut self.traced_env_dirty) {
            let intensity = self.view.display.hdri_intensity;
            let rotation = self.view.display.hdri_rotation;
            if let Some(tracer) = self.tracer.as_mut() {
                tracer.set_environment_params(intensity, rotation);
            }
            return;
        }
        // Resolved before the tracer is borrowed, because resolving reads the
        // whole host and the tracer is a field of it.
        //
        // Where the scene authors no image, a pane falls back to the same
        // background the raster path resolves, so a traced pane and the raster
        // pane beside it agree by construction rather than by coincidence.
        let sky = self
            .resolve_background(&self.view.pane_settings[0])
            .sky_colors();
        self.install_traced_environment(sky);
    }

    /// The environment a still integrates against, installed at the moment the
    /// render starts.
    ///
    /// Where the scene authors an image this is the pane path's answer. Where
    /// it authors none, a still gets no environment at all rather than the
    /// background a pane happens to be showing: a render is a property of the
    /// scene and a viewport background is a viewing preference, so a document
    /// declaring itself lit by nothing but its own lights renders that way. It
    /// is also the only way this shell and the terminal can agree, the
    /// terminal having no panes to borrow a background from.
    ///
    /// Black rather than merely dim: the kernel excludes an all-zero sky from
    /// the direct-lighting estimator's choice entirely, so the draws that were
    /// going to a sky occluded from nearly every point inside a closed room go
    /// to the lights that are really there instead.
    ///
    /// Installed unconditionally, because the dirty flag is the pane path's
    /// cache over an image rebuild and a still has to overwrite whatever sky
    /// the panes last left on the shared backend even when nothing is dirty.
    pub(super) fn install_still_environment(&mut self) {
        self.install_traced_environment(([0.0; 3], [0.0; 3]));
    }

    /// Builds the traced environment from the scene's image and hands it to
    /// the tracer, falling back to the given constant sky where the scene
    /// authors no image.
    ///
    /// Nothing here uploads the image. The equirect the sky pass retains and
    /// the equirect the kernel walks are the same texture in the same format,
    /// so the view is shared and only the two distribution tables are built,
    /// from the distribution both HDRI routes already compute.
    pub(super) fn install_traced_environment(&mut self, sky: ([f32; 3], [f32; 3])) {
        let intensity = self.view.display.hdri_intensity;
        let rotation = self.view.display.hdri_rotation;
        let ibl = &self.renderer.ibl_res.ibl;
        let built = match (ibl.equirect.as_ref(), ibl.distribution.as_ref()) {
            (Some(equirect), Some(distribution)) => Some(
                solarxy_renderer::pathtrace::environment::TraceEnvironment::from_shared_equirect(
                    &self.device,
                    &self.queue,
                    &equirect.view,
                    distribution,
                ),
            ),
            _ => None,
        };
        let Some(tracer) = self.tracer.as_mut() else {
            return;
        };
        match built {
            Some(environment) => {
                tracer.set_environment(&self.device, environment, intensity, rotation);
            }
            None => tracer.set_sky(sky.0, sky.1),
        }
    }

    /// Apply any `SceneOp::SetEnvironment` in the frame's delta.
    ///
    /// Separate from `SceneObjects::apply` because the environment is the
    /// IBL and the skybox, which that type cannot reach. The desktop shell
    /// runs the same tracker over the same op.
    /// Apply a scene delta's environment. The body is
    /// [`solarxy_host::apply_scene_environment`], shared with the desktop
    /// shell since 0.10.0; what stays here are the two reactions that are
    /// this shell's, which are marking a traced backend's environment copy
    /// stale and telling the frontend the view moved.
    ///
    /// The empty custom-background slice is not an oversight: this shell has
    /// no user-defined background registry.
    pub(super) fn apply_scene_environment(&mut self, delta: &solarxy_core::scene::SceneDelta) {
        let applied = solarxy_host::apply_scene_environment(
            &self.device,
            &self.queue,
            &mut self.renderer,
            &mut self.env,
            &mut self.environment,
            &mut self.view,
            &[],
            delta,
        );
        if applied.tracer_dirty {
            self.traced_env_dirty = true;
        }
        if applied.applied {
            self.host_events.push(HostEvent::ViewChanged);
        }
    }

    /// The `.slxy` environment section from the host state. The scene-wide
    /// HDRI rotation rides the free-form `background` object (the global
    /// `DisplaySettings` is otherwise host-session state).
    pub(super) fn environment_json(&self) -> solarxy_scenefile::EnvironmentJson {
        let mut background = std::collections::BTreeMap::new();
        background.insert(
            "hdriRotation".to_string(),
            serde_json::json!(self.view.display.hdri_rotation),
        );
        solarxy_scenefile::EnvironmentJson {
            ibl_mode: match self.renderer.ibl_res.ibl_mode {
                IblMode::Off => "off",
                IblMode::Diffuse => "diffuse",
                IblMode::Full => "full",
            }
            .to_string(),
            hdri_asset: self.hdri.as_ref().map(|h| h.hash.clone()),
            background,
        }
    }

    /// The scene's visible bounds, or the placeholder before anything cooks.
    pub(super) fn scene_bounds(&self) -> AABB {
        self.raster
            .scene()
            .visible_bounds()
            .unwrap_or(self.env_bounds)
    }

    /// What "frame the selection" should put the camera around, or `None` when
    /// the selection has no place in the world.
    ///
    /// Two kinds of answer, because there are two kinds of thing to frame. An
    /// object has real bounds. A light has a position and no size at all, so
    /// it gets a box scaled to the scene: framing a point would put the camera
    /// arbitrarily close to it, and a fraction of the scene is the only
    /// measure available that means anything.
    pub(super) fn selection_bounds(&self) -> Option<AABB> {
        let id = self.selected_object?;
        if let Some(b) = self.raster.scene().object_world_bounds(id) {
            return Some(b);
        }
        // Not an object, so it may be a light. Its marker anchor rather than
        // its raw position, so framing an ambient or hemisphere light goes
        // where its marker is drawn rather than where it is not.
        let light = self
            .raster
            .scene()
            .lights()?
            .iter()
            .find(|l| l.id == id && l.visible)?;
        let at = solarxy_renderer::helpers::marker_anchor(light);
        let half = (self.scene_bounds().diagonal() * 0.05).max(0.25);
        Some(AABB {
            min: cgmath::Point3::new(at.x - half, at.y - half, at.z - half),
            max: cgmath::Point3::new(at.x + half, at.y + half, at.z + half),
        })
    }

    /// Keeps the UV pane's source current: the selected node's committed
    /// geometry (uploaded into the one-object preview scene, deduped by
    /// `Arc` identity), else the selected / first scene object. A source
    /// change invalidates the overlap statistic.
    pub(super) fn sync_uv_preview(&mut self) {
        if !self
            .view
            .pane_settings
            .iter()
            .any(|p| p.pane_mode == PaneMode::UvMap)
        {
            return;
        }
        let source = self
            .engine
            .selected_geometry(self.current_ctx)
            .map(|(node, set)| {
                (
                    node.0,
                    std::sync::Arc::as_ptr(set).cast::<()>() as usize,
                    std::sync::Arc::clone(set),
                )
            });
        self.uv_use_preview = source.is_some();
        let identity = match &source {
            Some((node, addr, _)) => Some((*node, *addr)),
            None => self
                .selected_object
                .or_else(|| self.raster.scene().iter().next().map(|(id, _)| *id))
                .map(|id| (id.0, 0)),
        };
        if identity != self.last_uv_source {
            self.last_uv_source = identity;
            if self.view.pane_settings.iter().any(|p| p.show_uv_overlap) {
                self.renderer.uv_overlap.overlap_pct = None;
                self.renderer.uv_overlap.stats_dirty = true;
            }
        }
        if let Some((_, _, set)) = source {
            let mut delta = SceneDelta::default();
            delta.push(SceneOp::UpsertGeometry {
                id: UV_PREVIEW_ID,
                geometry: std::sync::Arc::new(set.to_cooked()),
            });
            if let Err(e) =
                self.uv_scene
                    .apply(&self.device, &self.queue, &self.renderer.layouts, &delta)
            {
                error(&format!("uv preview upload failed: {e}"));
            }
        }
    }

    pub(super) fn resolve_background(&self, pds: &PaneDisplaySettings) -> ResolvedBackground {
        // The web has no user custom-background registry yet.
        pds.background_mode.resolve(&[])
    }

    /// Whether any active-layout 3D pane wants the per-mesh visualization
    /// overlays: the normal arrows and the per-mesh bounds boxes.
    pub(super) fn viz_overlays_wanted(&self) -> bool {
        solarxy_host::visualization::overlays_wanted(
            &self.view.pane_settings,
            self.view.display.layout.pane_count(),
        )
    }

    /// Rebuilds `env.vis` from every displayed geometry when the aggregate
    /// is stale and a pane actually shows it: world-baked normal lines
    /// (positions via the object matrix, directions via its
    /// inverse-transpose) and per-mesh world AABBs, flattened in draw order
    /// (the renderer zips segments against the flattened scene meshes).
    /// Lights/shadow are untouched -- only the visualization member swaps.
    pub(super) fn sync_visualization(&mut self) {
        if !self.viz_dirty || !self.viz_overlays_wanted() {
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
        self.env.vis = VisualizationState::new_from_parts(
            &self.device,
            &self.renderer.layouts,
            &bounds,
            &mesh_bounds,
            Some(&normals),
            grid_color,
        );
        // The rebuilt state's attr channel is empty; refill it.
        self.attr_dirty = true;
    }

    /// Rebuilds (or clears) BOTH attribute channels (vector lines and GPU
    /// labels) when they are stale, then reports the sampling facts. One
    /// consumer of `attr_dirty` by construction: splitting the channels
    /// over two consumers would starve whichever ran second. Independent of
    /// `sync_visualization`: the overlays draw whenever the strip enables
    /// them, with or without the normals/bounds overlays.
    pub(super) fn sync_attr_channels(&mut self) {
        if !self.attr_dirty {
            return;
        }
        self.attr_dirty = false;

        if self.attr_viz.vectors && self.attr_viz.name.is_some() {
            let lines = self.build_attr_vector_lines();
            self.env.vis.set_attr_lines(&self.device, &lines);
        } else if self.env.vis.attr_lines_count > 0 {
            self.env.vis.set_attr_lines(&self.device, &[]);
        }

        let (capacity, total) = self.rebuild_attr_labels();
        self.host_events.push(HostEvent::AttrPinStats {
            capacity,
            total: total as f64,
        });
    }

    /// Rebuilds the GPU label set from a deterministic stride sample of
    /// every displayed geometry's points (all of them up to the budget):
    /// world-space anchors plus per-label glyph words, uploaded once here
    /// and projected in the vertex shader thereafter. Returns
    /// `(capacity, total displayed points)` for the sampling notice.
    pub(super) fn rebuild_attr_labels(&mut self) -> (u32, usize) {
        use cgmath::{Matrix4, Transform};
        if !self.attr_viz.pins_wanted() {
            self.renderer
                .set_attr_labels(&self.device, &self.queue, &[], &[]);
            return (0, 0);
        }
        let lane = self
            .attr_viz
            .name
            .as_deref()
            .filter(|_| self.attr_viz.labels);
        let geos = self.engine.display_geometries();
        let total: usize = geos
            .iter()
            .flat_map(|(_, set, _)| set.meshes.iter())
            .map(solarxy_kernel::KernelMesh::vertex_count)
            .sum();
        if total == 0 {
            self.renderer
                .set_attr_labels(&self.device, &self.queue, &[], &[]);
            return (0, 0);
        }
        let cap = self.attr_viz.effective_cap(total);
        let stride = total.div_ceil(cap).max(1);

        let mut candidates: Vec<solarxy_host::attr_labels::LabelCandidate> =
            Vec::with_capacity(cap);
        let mut global = 0usize;
        for (_node, set, m) in &geos {
            let matrix = Matrix4::from(*m);
            let mut ptnum: u64 = 0;
            for mesh in &set.meshes {
                let len = mesh.vertex_count();
                let values =
                    lane.and_then(|n| solarxy_graph::engine::attr_table::resolve_lane(mesh, n));
                let first = global.next_multiple_of(stride);
                let mut g = first;
                while g < global + len {
                    let i = g - global;
                    let tp = matrix.transform_point(Point3::from(mesh.positions[i]));
                    candidates.push(solarxy_host::attr_labels::LabelCandidate {
                        world: [tp.x, tp.y, tp.z],
                        ptnum: ptnum + i as u64,
                        value: values.map(|l| l.components(i).unwrap_or_default()),
                    });
                    g += stride;
                }
                ptnum += len as u64;
                global += len;
            }
        }
        let (instances, words) = solarxy_host::attr_labels::build_labels(
            &candidates,
            self.attr_viz.labels,
            self.attr_viz.points,
            self.attr_viz.label_decimals,
        );
        self.renderer
            .set_attr_labels(&self.device, &self.queue, &instances, &words);
        (cap as u32, total)
    }

    /// World-space arrow segments for the picked point lane (vec3, or the
    /// xyz of vec4; map lane or the fixed `N` buffer), over
    /// every displayed geometry: positions through the object matrix,
    /// directions through the normal matrix for the reserved `N` lane
    /// (bivector semantics under nonuniform scale) and the plain linear
    /// part for everything else. Length is the bounds-derived factor
    /// times the strip's scale multiplier, over the value (or its unit
    /// direction under normalize); color is the uniform pick, or the
    /// cold-to-warm ramp over this frame's magnitude range.
    pub(super) fn build_attr_vector_lines(&self) -> Vec<GizmoVertex> {
        use cgmath::{InnerSpace, Matrix3, Matrix4, SquareMatrix, Transform};
        let Some(name) = self.attr_viz.name.as_deref() else {
            return Vec::new();
        };
        let is_normal_lane = name == solarxy_kernel::reserved::NORMAL;
        let multiplier = self.attr_viz.scale_multiplier();
        let normalize = self.attr_viz.normalize;

        // First pass: world-space segments plus each arrow's magnitude
        // (pre-normalization), so the ramp can span the real range.
        let mut segments: Vec<([f32; 3], [f32; 3], f32)> = Vec::new();
        for (_node, set, m) in self.engine.display_geometries() {
            let matrix = Matrix4::from(m);
            let linear = Matrix3::from_cols(
                matrix.x.truncate(),
                matrix.y.truncate(),
                matrix.z.truncate(),
            );
            let dir_matrix = if is_normal_lane {
                linear
                    .invert()
                    .map_or(linear, |inv| cgmath::Matrix::transpose(&inv))
            } else {
                linear
            };
            let scale = {
                let d = set.bounds.diagonal();
                if d > 1e-10 { d * 0.05 } else { 0.1 }
            } * multiplier;
            for mesh in &set.meshes {
                // Vec3 and vec4 (xyz) lanes draw, map or fixed-buffer N;
                // float/vec2 lanes have no spatial reading and skip.
                let Some(lane) = solarxy_graph::engine::attr_table::resolve_lane(mesh, name) else {
                    continue;
                };
                for (i, p) in mesh.positions.iter().enumerate() {
                    let Some(v) = lane.direction(i) else { continue };
                    let tp = matrix.transform_point(Point3::from(*p));
                    let mut dir = dir_matrix * Vector3::from(v);
                    let magnitude = dir.magnitude();
                    if normalize {
                        if magnitude <= 1e-10 {
                            continue;
                        }
                        dir /= magnitude;
                    }
                    segments.push((
                        [tp.x, tp.y, tp.z],
                        [
                            tp.x + dir.x * scale,
                            tp.y + dir.y * scale,
                            tp.z + dir.z * scale,
                        ],
                        magnitude,
                    ));
                }
            }
        }

        // Second pass: colors. Flat per arrow (both vertices alike) so
        // direction stays readable under the ramp.
        let color_for: Box<dyn Fn(f32) -> [f32; 3]> = match self.attr_viz.color_mode {
            AttrColorMode::Uniform => {
                let c = self.attr_viz.color;
                Box::new(move |_| c)
            }
            AttrColorMode::Ramp => {
                let (min, max) = segments
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), (_, _, m)| {
                        (lo.min(*m), hi.max(*m))
                    });
                if max - min <= 1e-10 {
                    // A degenerate range has nothing to rank; fall back
                    // to the uniform color.
                    let c = self.attr_viz.color;
                    Box::new(move |_| c)
                } else {
                    let preset = self.attr_viz.ramp_preset;
                    Box::new(move |m: f32| {
                        let t = ((m - min) / (max - min)).clamp(0.0, 1.0);
                        ramp_color(preset, t)
                    })
                }
            }
        };
        segments
            .into_iter()
            .flat_map(|(a, b, magnitude)| {
                let color = color_for(magnitude);
                [
                    GizmoVertex { position: a, color },
                    GizmoVertex { position: b, color },
                ]
            })
            .collect()
    }

    pub(super) fn sync_env_bounds(&mut self) {
        // Keep the ground environment (grid, floor, shadow frustum) world-fixed
        // during interactive edits. A subflow gizmo drag writes a `transform`
        // node that BAKES into the cooked points, so the visible bounds churn
        // every frame; refitting here would rescale/slide the grid and floor
        // under the gizmo. Any interaction streams through the preview lane, so
        // we skip the refit while a preview is in flight and let it settle once
        // when the edit commits (the preview clears on the authoritative write).
        if self.engine.has_active_previews() {
            return;
        }
        // The same reasoning one guard up, for the scene clock. An animated
        // scene bakes new point positions every frame, so the visible bounds
        // churn continuously and refitting here would make the grid, floor
        // and shadow frustum breathe in time with the animation. The world is
        // not something playback is allowed to rescale. Playback stopping
        // needs no bookkeeping: `frame()` calls this unconditionally, so the
        // first non-playing tick settles the environment once.
        if self.engine.clock().playing {
            return;
        }
        let Some(bounds) = self.raster.scene().visible_bounds() else {
            return;
        };
        let eps = (self.env_bounds.diagonal() * 1e-3).max(1e-6);
        let close = |a: Point3<f32>, b: Point3<f32>| {
            (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps && (a.z - b.z).abs() < eps
        };
        if close(bounds.min, self.env_bounds.min) && close(bounds.max, self.env_bounds.max) {
            return;
        }
        let grid_color = self
            .resolve_background(&self.view.pane_settings[0])
            .grid_color();
        self.env = solarxy_host::build_bounds_env(
            &self.device,
            &self.queue,
            &self.renderer,
            &bounds,
            grid_color,
            SHADOW_MAP_SIZE,
        );
        self.env_bounds = bounds;
        // The rebuilt environment starts with empty per-mesh viz data; the
        // aggregate refills it when a pane wants overlays (and the attr
        // channel refills on its own dirty pass).
        self.viz_dirty = true;
        self.attr_dirty = true;
    }

    /// Drives each look-through pane's camera from its bound `camera` node, so
    /// param-panel edits (and non-navigating panes) always show the node's
    /// saved pose.
    ///
    /// The body is [`solarxy_host::cameras::follow_camera_bindings`], shared
    /// with the desktop shell. What stays here is this shell's suppression:
    /// a pane mid-navigation, so the follow never fights a live orbit or pan
    /// on a locked pane, and a pane whose turntable is spinning its scratch
    /// camera.
    pub(super) fn follow_look_through_cameras(&mut self) {
        let bindings: [Option<SceneObjectId>; 4] =
            std::array::from_fn(|i| self.look_through[i].map(|node| SceneObjectId(node.0)));
        let suppressed: [bool; 4] = std::array::from_fn(|i| {
            self.camera_editing[i] || self.view.pane_settings[i].turntable_active
        });
        let Some(defs) = self.raster.scene().cameras() else {
            return;
        };
        solarxy_host::cameras::follow_camera_bindings(
            defs,
            &bindings,
            &suppressed,
            &mut self.view.cameras,
        );
    }

    /// Whether pane `pane` is a locked look-through pane (navigation reframes
    /// its bound camera node).
    pub(super) fn is_locked_look_through(&self, pane: usize) -> bool {
        pane < 4 && self.look_through[pane].is_some() && self.camera_locked[pane]
    }

    /// Writes a locked look-through pane's current camera pose back to its bound
    /// `camera` node as one undo step, returning the merged event batch so the
    /// frontend mirror reflects the new params (position + target).
    pub(super) fn commit_pane_camera_to_node(&mut self, pane: usize) -> Option<EventBatch> {
        let node = self.look_through.get(pane).copied().flatten()?;
        let (eye, target) = {
            let cam = self.view.cameras[pane].as_ref()?;
            (cam.camera.eye, cam.camera.target)
        };
        // A press and release with no movement commits nothing. The frontend
        // treats a returned batch as proof the press belonged to a camera
        // gesture and skips the click ladder, so an unconditional commit made
        // every click on a locked look-through pane unpickable and pushed an
        // undo step that changed nothing. Compared against what is on the
        // node now, not a pose cached at press time, so a follow that ran
        // mid-gesture cannot make an unchanged pose look changed; the
        // comparison space is the guard's to explain.
        if let Ok(graph) = self.engine.document().graph(GraphContext::Root)
            && let Some(data) = graph.node(node)
            && crate::camera_commit::pose_unchanged(
                &data.params,
                [eye.x, eye.y, eye.z],
                [target.x, target.y, target.z],
            )
        {
            return None;
        }
        let cmds = [
            Command::BeginTransaction {
                label: "Frame Camera".to_string(),
            },
            Command::SetParam {
                ctx: GraphContext::Root,
                node,
                key: "position".to_string(),
                value: ParamSource::Literal(ParamValue::Vec3([
                    f64::from(eye.x),
                    f64::from(eye.y),
                    f64::from(eye.z),
                ])),
            },
            Command::SetParam {
                ctx: GraphContext::Root,
                node,
                key: "target".to_string(),
                value: ParamSource::Literal(ParamValue::Vec3([
                    f64::from(target.x),
                    f64::from(target.y),
                    f64::from(target.z),
                ])),
            },
            Command::EndTransaction,
        ];
        let mut events = Vec::new();
        let mut revision = self.engine.revision();
        for cmd in cmds {
            if let Ok(batch) = self.engine.apply(cmd) {
                revision = batch.revision;
                events.extend(batch.events);
            }
        }
        Some(EventBatch { revision, events })
    }

    /// Uploads the camera gizmos for pane `i`, hiding the camera the pane is
    /// looking through. Cloned first so the `scene_objects` borrow ends before
    /// the mutable renderer write.
    pub(super) fn write_pane_camera_helpers(&mut self, i: usize) {
        let skip = self.look_through[i].map(|n| SceneObjectId(n.0));
        let cams: Vec<solarxy_core::scene::CameraDef> = self
            .raster
            .scene()
            .cameras()
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        self.renderer.write_camera_helpers(&self.queue, &cams, skip);
    }

    /// Lazily creates a `CameraState` for every pane slot the layout uses
    /// (the desktop recipe: slot 0 primary perspective; slots 1-3 cloned
    /// then reset to Top / Front / Left).
    pub(super) fn ensure_pane_cameras(&mut self) {
        let bounds = self.scene_bounds();
        let aspect = self.renderer.target_width as f32 / self.renderer.target_height.max(1) as f32;
        solarxy_host::ensure_pane_cameras(
            &self.device,
            &self.renderer.layouts.camera,
            &mut self.view.cameras,
            &bounds,
            aspect,
            self.view.display.layout.pane_count(),
            // This shell has no startup projection preference; slot 0 keeps the
            // camera's own default.
            None,
        );
    }

    /// Per-frame lighting: engine light nodes (root additive lights) drive
    /// the 8-light array when present; otherwise the synthesized viewer rig
    /// follows the primary camera (desktop parity).
    pub(super) fn update_lights(&mut self) {
        let bounds = self.scene_bounds();
        let ibl_avg = solarxy_host::active_ibl(&self.renderer).irradiance_average;

        // The helpers ride the same light list the shading does, so a helper can
        // never describe a light the renderer is not actually using. Sized in
        // world units, so unlike the manipulator this is once per frame, not
        // once per pane.
        match self.raster.scene().lights() {
            Some(defs) => self.renderer.write_light_helpers(&self.queue, defs),
            None => self.renderer.write_light_helpers(&self.queue, &[]),
        }
        // Immediately after the write that repopulates them, because that is
        // the only thing that could undo it. The lighting below still runs: a
        // still needs the lights, it just must not photograph the furniture.
        self.suppress_furniture_during_still();

        if let Some(defs) = self.raster.scene().authored_lights() {
            self.env.lights_uniform =
                LightsUniform::from_defs(defs, bounds.diagonal() * 0.04, ibl_avg);
        } else if !self.view.display.lights_locked {
            let Some(cam0) = self.view.cameras[0].as_ref().map(|c| c.camera) else {
                return;
            };
            self.env.lights_uniform = lights_from_camera(&cam0, &bounds, ibl_avg);
        } else {
            return;
        }
        self.queue.write_buffer(
            &self.env.light_buffer,
            0,
            bytemuck::cast_slice(&[self.env.lights_uniform]),
        );
        // The shadow map follows THE flagged caster (the engine's
        // exclusive-caster rule guarantees at most one), not blindly the
        // first entry; the synthesized viewer rig keeps its key at entry 0
        // flagged, so its behavior is unchanged.
        let count =
            (self.env.lights_uniform.count as usize).min(self.env.lights_uniform.lights.len());
        let caster = self.env.lights_uniform.lights[..count]
            .iter()
            .position(|l| l.shadowed > 0.5)
            .unwrap_or(0);
        let key = self.env.lights_uniform.lights[caster].position;
        let key_pos = if key.iter().all(|c| c.abs() < f32::EPSILON) {
            // A positionless (directional) key: synthesize a shadow eye
            // along its direction outside the bounds.
            let d = self.env.lights_uniform.lights[caster].direction;
            bounds.center() - Vector3::new(d[0], d[1], d[2]) * bounds.diagonal()
        } else {
            Point3::new(key[0], key[1], key[2])
        };
        self.env.shadow.update_light_vp(
            &self.queue,
            key_pos,
            bounds.center(),
            bounds.diagonal() / 2.0,
        );
    }

    /// Resizes the shared HDR target (and derived buffers) to the largest
    /// pane of the current layout. Port of the desktop
    /// `resize_render_targets` + `sync_render_target_dims`.
    pub(super) fn sync_render_target_dims(&mut self) {
        let (width, height) = panes::compute_target_dimensions(
            self.view.display.layout,
            self.config.width,
            self.config.height,
        );
        if width == 0 || height == 0 {
            return;
        }
        self.set_target_dims(width, height);
    }

    /// Resizes the shared render targets to exact dimensions (the layout
    /// sync above, the screenshot path's capture-resolution render, and the
    /// still job's per-tile size; restoration after a capture is the next
    /// frame's sync call).
    pub(super) fn set_target_dims(&mut self, width: u32, height: u32) {
        self.renderer.resize_targets(&self.device, width, height);
    }

    /// Recomputes the manipulator and, when it changed, tells the frontend
    /// what the selection can be manipulated with.
    ///
    /// The manipulator is pull-based: recomputed every frame from the engine's
    /// own view of the world, so a selection change or an undo moves or
    /// removes it with no extra plumbing. `view_dir` and `scale` are per-pane,
    /// so they are placeholders here; `Renderer::write_manipulator` overwrites
    /// both before each pane's pass.
    pub(super) fn sync_gizmo(&mut self) {
        // A published scene has nothing to manipulate.
        let target = if self.player_mode {
            None
        } else {
            self.engine.gizmo_target(self.current_ctx)
        };
        let manip = target.and_then(|t| {
            self.gizmo
                .manipulator(&gizmo_pose(&t), cgmath::Vector3::unit_z(), 1.0)
        });
        self.renderer.set_manipulator(manip);

        // With nothing selected there is no target and therefore no answer.
        // The frontend reads that as "do not narrow anything", which is what
        // keeps an empty scene's tool column looking the way it always has.
        let capability = target.map(|t| {
            (
                gizmo::tools_for(&t.params)
                    .into_iter()
                    .map(gizmo::ToolMode::id)
                    .collect::<Vec<_>>(),
                t.params.names(),
            )
        });
        if capability != self.last_capability {
            self.last_capability.clone_from(&capability);
            let (tools, transform_params) = capability.unwrap_or_default();
            self.host_events.push(HostEvent::SelectionCapability {
                tools,
                transform_params,
            });
        }
    }

    /// Drops the viewport's furniture while a still render owns the frame.
    ///
    /// A still is a photograph of the scene rather than a screenshot of the
    /// viewport. `PaneDisplaySettings::for_still` states that for everything a
    /// pane flag reaches; the manipulator, the two helper channels and the
    /// light markers are host-fed and reach none of them, so they held
    /// whatever the last ordinary frame left in them and a rasterized still
    /// photographed the gizmo. Cleared rather than gated, and repopulated by
    /// the next ordinary frame, so there is nothing to restore.
    pub(super) fn suppress_furniture_during_still(&mut self) {
        if self.still.is_some() {
            self.renderer.clear_viewport_furniture();
        }
    }

    /// Assemble this pane's parameters and hand them to the shared body.
    ///
    /// What is left here is policy and assembly: the grading tables, the
    /// manipulator and the camera helpers, all of which this shell has and the
    /// desktop does not; the light-rig guard, which writes through `&mut self`;
    /// and the draw list and the UV source, which this shell resolves against
    /// the document rather than a loaded file.
    /// One pane's per-frame host writes (LUTs, manipulator, camera
    /// helpers, split lighting), and the derived [`PaneInputs`] its encode
    /// and composite read.
    pub(super) fn prepare_pane(&mut self, i: usize, pane: PaneRect, is_split: bool) -> PaneInputs {
        let pds = self.view.pane_settings[i];
        let cam_data = self.view.cameras[i].as_ref().map(|c| c.camera);
        let is_uv_map = pds.pane_mode == PaneMode::UvMap;

        // Before the pane renders, whichever of the three arms it takes: all
        // of them composite, and the composite is what reads the tables.
        self.bind_pane_luts(i);

        if let Some(cam_data) = cam_data
            && !is_uv_map
        {
            // The gizmo's world size is per-pane (a pane's camera and height
            // decide how many world units a pixel is), so it is re-written
            // before each pane's pass rather than once per frame.
            self.renderer
                .write_manipulator(&self.queue, &cam_data, pane.height / self.dpr);
            self.write_pane_camera_helpers(i);
            // Markers are per pane for the same reason the manipulator is:
            // screen-constant means a pane's own camera and height decide the
            // world size. CSS pixels, so a retina display does not halve them.
            if pds.show_light_markers {
                let selected = self.selected_object;
                let lights = self.raster.scene().lights().map(<[_]>::to_vec);
                self.renderer.write_light_markers(
                    &self.queue,
                    lights.as_deref().unwrap_or(&[]),
                    &cam_data,
                    pane.height / self.dpr,
                    selected,
                );
            }
            if is_split && i >= 1 {
                self.setup_pane_lighting(&cam_data);
            }
        }

        // A traced 3D pane goes to the tracer instead of the rasterizer;
        // everything around the encode (the pane context, the composite,
        // the look) is shared, which is the backend contract's point.
        let traced = pds.pane_engine == PaneEngine::Traced
            && !is_uv_map
            && cam_data.is_some()
            && self.tracer.is_some();
        if traced {
            self.prepare_traced_pane(i);
        }

        PaneInputs {
            traced,
            background: self.resolve_background(&pds),
            bounds: self.scene_bounds(),
            look: self.pane_look(i),
            // The composite folds bloom and ambient occlusion in only when
            // there is something to fold them around. This shell used to pass
            // a constant here, which put a glow on an empty viewport.
            scene_present: self.raster.scene().draw_objects().next().is_some(),
            outline: self.renderer.selection_style
                == solarxy_renderer::frame::SelectionStyle::Outline
                && self
                    .selected_object
                    .is_some_and(|id| self.raster.scene().draw_object(id).is_some()),
            // The grid plane follows the pane camera: perspective keeps the
            // XZ ground; an orthographic axis elevation (front/side) gets a
            // view-plane grid so it is not seen edge-on. Keyed off the
            // transition destination so a view-preset animation switches
            // plane once, at click time, not partway through the lerp.
            grid_plane: self.view.cameras[i]
                .as_ref()
                .map(|c| grid_plane_for(&c.destination_camera())),
        }
    }

    // One pane's frame, start to finish: the context, the backend that draws
    // it, the composite. Most of the body is a single struct literal, so the
    // only extraction available is one taking twenty parameters.
    #[allow(clippy::too_many_lines)]
    pub(super) fn render_pane(
        &mut self,
        i: usize,
        pane: PaneRect,
        surface_view: &wgpu::TextureView,
        is_split: bool,
    ) {
        let pds = self.view.pane_settings[i];
        let cam_data = self.view.cameras[i].as_ref().map(|c| c.camera);
        let is_uv_map = pds.pane_mode == PaneMode::UvMap;
        let PaneInputs {
            background,
            bounds,
            look,
            scene_present,
            outline,
            grid_plane,
            traced,
        } = self.prepare_pane(i, pane, is_split);

        // Field-level borrows from here on, so the shared body can take the
        // renderer mutably while the draw list borrows the scene.
        // Everything below borrows fields other than `raster`, so the backend
        // can be driven mutably. The preview scene is this host's own; the
        // fallback is an object the backend owns, so it resolves that itself.
        let content = match cam_data {
            None => PaneContent::Empty,
            Some(_) if is_uv_map => PaneContent::Uv {
                source: if self.uv_use_preview {
                    self.uv_scene
                        .draw_object(UV_PREVIEW_ID)
                        .map_or(UvSource::None, UvSource::External)
                } else {
                    UvSource::Scene {
                        preferred: self.selected_object,
                    }
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
        let hdr_target = self.renderer.targets.hdr_resolve_view.clone();
        let outcome = {
            let mut ctx = FrameCtx {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.renderer,
                encoder: &mut encoder,
                index: i,
                rect: pane,
                is_split,
                pds: &pds,
                display: &self.view.display,
                background,
                camera: self.view.cameras[i].as_mut(),
                env: &self.env,
                bounds: Some(&bounds),
                grid_plane,
                look,
                scene_present,
                outline,
                // An ordinary frame is a view in its own right, not a window on
                // a larger picture. Only the still render sets this.
                window: None,
                content,
            };
            if traced {
                self.tracer
                    .as_mut()
                    .map_or(FrameOutcome::Complete, |t| t.encode(&mut ctx, &hdr_target))
            } else {
                self.raster.encode(&mut ctx, &hdr_target)
            }
        };
        if traced {
            self.push_pane_samples(i, outcome);
        }
        let caps = if traced {
            PathBackend::CAPS
        } else {
            RasterBackend::CAPS
        };
        let pass = if traced {
            solarxy_host::EncodedPane {
                is_uv_map: false,
                scene_present,
            }
        } else {
            self.raster.encoded(i).unwrap_or(solarxy_host::EncodedPane {
                is_uv_map: false,
                scene_present: false,
            })
        };

        solarxy_host::composite_and_submit(
            &self.queue,
            &self.renderer,
            encoder,
            surface_view,
            &solarxy_host::PaneComposite {
                index: i,
                rect: pane,
                look,
                inspection: pds.inspection_mode,
                is_uv_map: pass.is_uv_map,
                scene_present: pass.scene_present,
                outline,
                writes_occlusion: caps.writes_occlusion,
            },
        );
    }

    /// The look of the `camera` node a pane is looking through, if any.
    ///
    /// Reads the lowered `CameraDef` rather than the document, so it sees
    /// exactly what the last delta carried, including the tables the
    /// camera's cook decoded.
    pub(super) fn pane_camera_look(&self, pane: usize) -> Option<&solarxy_core::scene::CameraLook> {
        let binding = (*self.look_through.get(pane)?).map(|node| SceneObjectId(node.0));
        solarxy_host::cameras::camera_look_for(self.raster.scene().cameras(), binding)
    }

    /// The camera definition a pane is bound to, if it is looking through one.
    pub(super) fn pane_camera_def(&self, pane: usize) -> Option<&solarxy_core::scene::CameraDef> {
        let node = (*self.look_through.get(pane)?)?;
        self.raster
            .scene()
            .cameras()?
            .iter()
            .find(|c| c.id == solarxy_core::scene::SceneObjectId(node.0))
    }

    /// The lens a shot takes: the named camera's, or the active pane's
    /// camera's, or a pinhole. The same precedence the look resolves by,
    /// because both describe the shot rather than the viewport.
    pub(super) fn still_lens(&self, camera: Option<NodeId>) -> solarxy_core::scene::CameraLens {
        camera
            .and_then(|node| {
                let id = SceneObjectId(node.0);
                self.raster
                    .scene()
                    .cameras()
                    .and_then(|cams| cams.iter().find(|c| c.id == id))
            })
            .or_else(|| self.pane_camera_def(self.view.active_pane))
            .map(solarxy_host::cameras::lens_for)
            .unwrap_or_default()
    }

    /// The resolved look a pane composites with.
    pub(super) fn pane_look(&self, pane: usize) -> CompositeLook {
        let fallback = self.pane_looks.get(pane).copied().unwrap_or_default();
        solarxy_renderer::composite::resolve_look(self.pane_camera_look(pane), &fallback)
    }

    /// Bind the grading tables a pane's camera carries, before that pane
    /// composites.
    ///
    /// There is one pair of table textures for the whole renderer while a
    /// look is per pane, so the pair has to follow whichever pane is about
    /// to composite. `set_lut` dedupes on content hash, so the common case
    /// (no tables, or every pane through the same camera) costs two
    /// comparisons and rebuilds nothing. The case that does cost something
    /// is several panes through several cameras with *different* tables,
    /// which rebuilds the composite bind group once per pane per frame;
    /// that is a known cost of one shared pair, and caching a bind group
    /// per distinct pair is the fix if it ever shows up in a profile.
    /// Resolve the look a still composites with, and bind the tables it asks
    /// for.
    ///
    /// The look belongs to the camera being shot through. Falling back to the
    /// active pane's camera is right only when the render node names none, and
    /// that is exactly when the shot *is* the active pane's view, grade and
    /// all.
    ///
    /// Binding here is what [`Self::bind_pane_luts`] cannot do for a still.
    /// There is one table pair for the whole renderer, the pane path is its
    /// only other binder, and a running job replaces that path wholesale: with
    /// nothing binding for the job's duration the composite reads whatever the
    /// last drawn pane left behind. Once at the start is enough, because the
    /// tables cannot change while a job runs, and it is self-healing, because
    /// the pane path rebinds its own on the first ordinary frame afterwards.
    /// The bind dedupes on content hash, so an ungraded shot costs two
    /// comparisons.
    pub(super) fn prepare_still_look(&mut self, camera: Option<NodeId>) {
        let camera_look = camera
            .and_then(|node| {
                let id = SceneObjectId(node.0);
                self.raster
                    .scene()
                    .cameras()
                    .and_then(|cams| cams.iter().find(|c| c.id == id).map(|c| c.look.clone()))
            })
            .or_else(|| self.pane_camera_look(self.view.active_pane).cloned());
        let pane_fallback = self
            .pane_looks
            .get(self.view.active_pane)
            .copied()
            .unwrap_or_default();
        self.still_look =
            solarxy_renderer::composite::resolve_look(camera_look.as_ref(), &pane_fallback);

        solarxy_host::cameras::bind_look_luts(
            &self.device,
            &self.queue,
            &mut self.renderer,
            camera_look.as_ref(),
        );
    }

    pub(super) fn bind_pane_luts(&mut self, pane: usize) {
        let look = self.pane_camera_look(pane).cloned();
        solarxy_host::cameras::bind_look_luts(
            &self.device,
            &self.queue,
            &mut self.renderer,
            look.as_ref(),
        );
    }

    /// The tracer's half of the viewer rig, before a still starts.
    ///
    /// A scene with no light nodes is lit in the viewport by the rig the panes
    /// write into the lights uniform. The tracer binds no such uniform, so it
    /// takes the same three definitions as scene data, from the camera this
    /// shot is taken through rather than from whichever pane happens to be
    /// active.
    pub(super) fn light_traced_still(&mut self, camera: &Camera) {
        if let Some(t) = self.tracer.as_mut() {
            solarxy_host::apply_viewer_rig(
                &self.device,
                &self.queue,
                t,
                self.raster.scene(),
                camera,
            );
        }
    }

    /// Recomputes the camera-relative light rig for a non-primary pane
    /// (only meaningful for the synthesized viewer rig; engine light nodes
    /// are world-fixed).
    pub(super) fn setup_pane_lighting(&mut self, cam_data: &Camera) {
        // Engine light nodes are world-fixed and owe nothing to a camera, so
        // the synthesized viewer rig is the only thing this applies to.
        if self.view.display.lights_locked || self.raster.scene().authored_lights().is_some() {
            return;
        }
        let bounds = self.scene_bounds();
        let ibl_avg = solarxy_host::active_ibl(&self.renderer).irradiance_average;
        solarxy_host::setup_pane_lighting(&self.queue, &mut self.env, cam_data, &bounds, ibl_avg);
    }

    pub(super) fn view_state_dto(&self) -> ViewStateDto {
        let default_projection = ProjectionMode::Perspective;
        let projections = std::array::from_fn(|i| {
            projection_name(
                self.view.cameras[i]
                    .as_ref()
                    .map_or(default_projection, |c| c.camera.projection),
            )
            .to_string()
        });
        let cams = self.raster.scene().cameras();
        let pane_look_through = std::array::from_fn(|i| self.look_through[i].map(|n| n.0 as f64));
        let pane_gate_aspect = std::array::from_fn(|i| {
            let node = self.look_through[i]?;
            cams?
                .iter()
                .find(|c| c.id == SceneObjectId(node.0))
                .map(|c| c.aspect)
        });
        ViewStateDto {
            layout: self.view.display.layout,
            split_ratio: self.view.display.split_ratio,
            active_pane: self.view.active_pane,
            cameras_linked: self.view.cameras_linked,
            pane_settings: self.view.pane_settings,
            display: self.view.display,
            pane_projections: projections,
            pane_rects: self.pane_rects_css(),
            pane_looks: self.pane_looks,
            pane_look_through,
            pane_camera_locked: self.camera_locked,
            pane_gate_aspect,
            attr_viz: self.attr_viz.clone(),
        }
    }
}
