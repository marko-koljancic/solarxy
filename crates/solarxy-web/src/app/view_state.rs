//! The host-owned view state the frontend mirrors: layout, panes, cameras
//! and their bindings.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    // ---- view-state boundary (host-owned; React mirrors) ----

    /// The full view state for the mirror.
    pub fn view_state(&self) -> Result<JsValue, JsError> {
        to_js(&self.view_state_dto())
    }

    /// Sets the pane layout (F1-F5). `layout` is the camelCase name
    /// (`"single"`, `"splitVertical"`, ...). Returns the new view state.
    pub fn set_view_layout(&mut self, layout: JsValue) -> Result<JsValue, JsError> {
        let layout: ViewLayout = serde_wasm_bindgen::from_value(layout)
            .map_err(|e| JsError::new(&format!("bad layout: {e}")))?;
        self.view.display.layout = layout;
        if self.view.active_pane >= layout.pane_count() {
            self.view.active_pane = 0;
        }
        self.ensure_pane_cameras();
        self.view_state()
    }

    /// Sets the two-pane divider ratio (clamped 0.05-0.95).
    pub fn set_split_ratio(&mut self, ratio: f32) -> Result<JsValue, JsError> {
        self.view.display.split_ratio = DisplaySettings::clamp_split_ratio(ratio);
        self.view_state()
    }

    pub fn set_active_pane(&mut self, pane: usize) -> Result<JsValue, JsError> {
        if pane < self.view.display.layout.pane_count() {
            self.view.active_pane = pane;
        }
        self.view_state()
    }

    /// Binds pane `pane` to look through the `camera` node id, or clears to a
    /// free view when `camera` is negative / non-finite. Returns the view state.
    pub fn set_pane_camera(&mut self, pane: usize, camera: f64) -> Result<JsValue, JsError> {
        if pane < 4 {
            self.look_through[pane] = if camera.is_finite() && camera >= 0.0 {
                Some(NodeId(camera as u64))
            } else {
                None
            };
            if self.look_through[pane].is_none() {
                self.camera_locked.release(pane);
            }
            self.camera_editing[pane] = false;
        }
        self.view_state()
    }

    /// Toggles lock-camera-to-view for a look-through pane (Blender semantics:
    /// navigation reframes the bound camera). No effect on a free view.
    pub fn set_pane_camera_lock(&mut self, pane: usize, locked: bool) -> Result<JsValue, JsError> {
        if pane < 4 {
            self.camera_locked
                .set(pane, locked, self.look_through[pane].is_some());
            self.camera_editing[pane] = false;
        }
        self.view_state()
    }

    /// Jumps a pane's (free) view to a camera node's saved pose without binding
    /// or locking it (the bookmark action). Returns the view state.
    pub fn jump_to_camera(&mut self, pane: usize, camera: f64) -> Result<JsValue, JsError> {
        if pane < 4 && camera.is_finite() && camera >= 0.0 {
            let id = SceneObjectId(camera as u64);
            let scene = self.raster.scene();
            if let Some(cam) = self.view.cameras[pane].as_mut() {
                solarxy_host::cameras::jump_to_camera(&mut cam.camera, scene, id);
            }
        }
        self.view_state()
    }

    /// The current pose (eye + target) of a pane's camera, so the frontend can
    /// author a new `camera` node framed on the current view (create-from-view
    /// is a frontend-orchestrated `AddNode` + `SetParam`, keeping the
    /// mirror-and-command model intact).
    pub fn pane_camera_pose(&self, pane: usize) -> Result<JsValue, JsError> {
        let (position, target) =
            solarxy_host::cameras::pane_pose(self.view.cameras.get(pane).and_then(|c| c.as_ref()));
        to_js(&CameraPoseDto { position, target })
    }

    /// Flies the active pane's camera to frame the mesh a validation issue
    /// lives on (report-panel row click; the desktop Properties fly-to) and
    /// enables that pane's validation overlay so the defect is visible.
    /// `object` is the owning geo node's id (= scene object id); `source`
    /// the node whose report the panel is showing (its engine-cached
    /// result is authoritative, which may differ from the object's
    /// effective overlay validation); `issue` the row index. Returns the
    /// view state.
    pub fn fly_to_issue(
        &mut self,
        object: f64,
        source: f64,
        issue: usize,
    ) -> Result<JsValue, JsError> {
        let id = SceneObjectId(object as u64);
        let source = solarxy_graph::document::NodeId(source as u64);
        let aabb = self.engine.validation(source).and_then(|v| {
            let issue = v.report.issues.get(issue)?;
            let obj = self.raster.scene().get(id)?;
            let raw_to_gpu = self.raster.scene().raw_to_gpu(id)?;
            solarxy_renderer::validation::resolve_issue_aabb(&issue.scope, &obj.model, raw_to_gpu)
        });
        if let Some(aabb) = aabb {
            let pane = self.view.active_pane;
            if let Some(settings) = self.view.pane_settings.get_mut(pane) {
                settings.show_validation = true;
            }
            if let Some(cam) = self.view.cameras.get_mut(pane).and_then(|c| c.as_mut()) {
                cam.reset_to_bounds(&aabb);
            }
        }
        self.view_state()
    }

    /// Replaces one pane's display settings with the full settings object.
    pub fn set_pane_settings(
        &mut self,
        pane: usize,
        settings: JsValue,
    ) -> Result<JsValue, JsError> {
        let settings: PaneDisplaySettings = serde_wasm_bindgen::from_value(settings)
            .map_err(|e| JsError::new(&format!("bad pane settings: {e}")))?;
        let mut engine_flip = None;
        if let Some(slot) = self.view.pane_settings.get_mut(pane) {
            // Turning the overlap display on arms a fresh statistic run
            // (the desktop `O`-toggle behavior).
            if settings.show_uv_overlap && !slot.show_uv_overlap {
                self.renderer.uv_overlap.overlap_pct = None;
                self.renderer.uv_overlap.stats_dirty = true;
            }
            // A newly enabled normals/bounds overlay may need the (lazily
            // built) visualization aggregate.
            if settings.normals_mode != slot.normals_mode
                || settings.bounds_mode != slot.bounds_mode
            {
                self.viz_dirty = true;
            }
            if settings.pane_engine != slot.pane_engine {
                engine_flip = Some(settings.pane_engine);
            }
            *slot = settings;
        }
        if let Some(engine) = engine_flip {
            // Flipping to the tracer builds it on first use and hands it
            // the scene it has never seen: the per-frame delta feed only
            // reaches it while a pane is watching, so a scene edited with
            // every pane raster has moved on without it. The snapshot
            // reconciles, so an unchanged scene is a hierarchy-cache hit.
            if engine == PaneEngine::Traced {
                if self.tracer.is_none() {
                    self.tracer = Some(PathBackend::new(&self.device, &self.queue));
                    self.traced_env_dirty = true;
                }
                let delta = self.engine.scene_snapshot();
                if let Some(t) = self.tracer.as_mut() {
                    t.apply_snapshot(&self.device, &self.queue, &delta);
                }
            }
            // Either direction resets the pane: the first traced frame is
            // sample one rather than a stale mean, and a return to raster
            // leaves nothing parked.
            if let Some(t) = self.tracer.as_mut() {
                t.invalidate_pane(pane);
            }
            if let Some(slot) = self.traced_cam_keys.get_mut(pane) {
                *slot = None;
            }
            if let Some(slot) = self.last_pane_samples.get_mut(pane) {
                *slot = None;
            }
        }
        self.view_state()
    }

    /// What each render backend can do, for the frontend's menu gating.
    ///
    /// The capability fields are constants, so the answer needs no device: a
    /// menu deciding what a backend produces should not have to build one to
    /// ask. **Availability is not a constant**, and used to be treated as one.
    /// The tracer spends core WebGPU's per-stage budget exactly, so a device at
    /// downlevel limits cannot build its layouts, and answering from a constant
    /// offered the mode on a device where it would have failed at pipeline
    /// creation. `available` asks the real device's limits.
    #[wasm_bindgen(js_name = backendCaps)]
    pub fn backend_caps(&self) -> Result<JsValue, JsError> {
        // Four independent yes-or-no facts about a backend, which is what the
        // wire shape is; grouping them to satisfy the lint would invent a
        // structure the TypeScript mirror would then have to invent too.
        #[allow(clippy::struct_excessive_bools)]
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CapsDto {
            progressive: bool,
            supports_instancing: bool,
            writes_aovs: bool,
            /// Whether this device can run the backend at all.
            available: bool,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct BackendCapsDto {
            raster: CapsDto,
            traced: CapsDto,
        }
        fn dto(c: solarxy_renderer::backend::BackendCaps, available: bool) -> CapsDto {
            CapsDto {
                progressive: c.progressive,
                supports_instancing: c.supports_instancing,
                writes_aovs: c.writes_aovs,
                available,
            }
        }
        // The rasterizer is what the surface is already drawing with, so its
        // availability is not in question by the time anything can ask.
        to_js(&BackendCapsDto {
            raster: dto(solarxy_host::RasterBackend::CAPS, true),
            traced: dto(
                PathBackend::CAPS,
                solarxy_renderer::pathtrace::device_supports_tracing(&self.device.limits()),
            ),
        })
    }

    /// Replaces the global display settings (layout, split, turntable,
    /// lights lock, material scales, HDRI rotation).
    /// Replace one pane's own look: exposure, tone mapper, and the
    /// lift/gamma/gain grade.
    ///
    /// Only reaches a pane that is a free view. A pane looking through a
    /// camera composites with that camera's look, which is a document
    /// value and is edited by setting the node's parameters like any
    /// other. Carries no lookup-table slots for the same reason: a table
    /// is a staged document asset, and a pane is not a document object.
    pub fn set_pane_look(&mut self, pane: usize, look: JsValue) -> Result<JsValue, JsError> {
        let look: PaneLook = serde_wasm_bindgen::from_value(look)
            .map_err(|e| JsError::new(&format!("bad pane look: {e}")))?;
        if let Some(slot) = self.pane_looks.get_mut(pane) {
            *slot = look;
        }
        self.view_state()
    }

    pub fn set_display_settings(&mut self, settings: JsValue) -> Result<JsValue, JsError> {
        let settings: DisplaySettings = serde_wasm_bindgen::from_value(settings)
            .map_err(|e| JsError::new(&format!("bad display settings: {e}")))?;
        self.view.display = settings;
        self.ensure_pane_cameras();
        self.view_state()
    }

    /// A camera command on a pane: `{kind:"fit"}`, `{kind:"view",
    /// axis:"top"|"bottom"|"front"|"back"|"left"|"right"}`, or
    /// `{kind:"projection", mode:"perspective"|"orthographic"}`. Returns the
    /// refreshed [`ViewStateDto`] like every other view mutator -- a view
    /// preset flips the pane to orthographic, and without the mirror update
    /// the toolbar's Persp/Ortho label kept showing the stale mode.
    pub fn camera_command(&mut self, pane: usize, cmd: JsValue) -> Result<JsValue, JsError> {
        let cmd: CameraCommandDto = serde_wasm_bindgen::from_value(cmd)
            .map_err(|e| JsError::new(&format!("bad camera command: {e}")))?;
        let bounds = self.scene_bounds();
        // Resolved before the camera is borrowed mutably, and unconditionally
        // rather than inside the arm, because both reads want `&self`.
        let selection = self.selection_bounds().unwrap_or(bounds);
        let Some(cam) = self.view.cameras.get_mut(pane).and_then(|c| c.as_mut()) else {
            return self.view_state();
        };
        match cmd.kind.as_str() {
            "fit" => cam.reset_to_bounds(&bounds),
            // Falls back to the whole scene when nothing is selected or the
            // selection has no place in the world, which is the same thing
            // `fit` does and never a camera that goes nowhere.
            "fitSelection" => cam.reset_to_bounds(&selection),
            "view" => {
                let Some(view) = solarxy_host::cameras::StandardView::from_name(&cmd.axis) else {
                    return Err(JsError::new(&format!("bad view axis: {}", cmd.axis)));
                };
                solarxy_host::cameras::reset_to_view(cam, &bounds, view);
            }
            "projection" => {
                let mode = if cmd.mode == "orthographic" {
                    ProjectionMode::Orthographic
                } else {
                    ProjectionMode::Perspective
                };
                cam.set_projection(mode);
            }
            other => return Err(JsError::new(&format!("bad camera command: {other}"))),
        }
        self.view_state()
    }

    /// The current pane rectangles in CSS pixels (DOM toolbar positioning).
    pub fn pane_rects(&self) -> Result<JsValue, JsError> {
        to_js(&self.pane_rects_css())
    }

    /// Drains queued host events (pane-rect changes, async results).
    pub fn take_host_events(&mut self) -> Result<JsValue, JsError> {
        // Uncaptured GPU faults ride the same per-frame drain as every
        // other async happening.
        for fault in self.gpu_faults.drain() {
            use solarxy_renderer::faults::GpuFaultKind;
            self.host_events.push(HostEvent::GpuFault {
                kind: match fault.kind {
                    GpuFaultKind::Validation => "validation",
                    GpuFaultKind::OutOfMemory => "outOfMemory",
                    GpuFaultKind::Internal => "internal",
                },
                message: fault.message,
                count: fault.count,
            });
        }
        let events = std::mem::take(&mut self.host_events);
        to_js(&events)
    }

    // ---- mirror / persistence boundary (unchanged surfaces) ----

    /// The full document mirror (recovery after desync / structural undo).
    pub fn snapshot(&self) -> Result<JsValue, JsError> {
        to_js(&self.engine.snapshot())
    }

    /// The static registry snapshot (fetched once; drives palette + panel).
    pub fn registry_snapshot(&self) -> Result<JsValue, JsError> {
        to_js(&self.engine.registry_snapshot())
    }

    /// Captures a clipboard fragment of the given nodes (the frontend
    /// serializes it to `application/x-solarxy-nodes`). `ctx` is a
    /// `GraphContext`; `ids` a number array.
    pub fn copy_nodes(&self, ctx: JsValue, ids: Vec<f64>) -> Result<JsValue, JsError> {
        let ctx = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let ids: Vec<solarxy_graph::document::NodeId> = ids
            .into_iter()
            .map(|n| solarxy_graph::document::NodeId(n as u64))
            .collect();
        to_js(&self.engine.copy_nodes(ctx, &ids))
    }

    /// The ids of currently stale (dirty) nodes, for manual-mode badges and
    /// the header stale count.
    pub fn stale_nodes(&self) -> Vec<f64> {
        self.engine
            .dirty_nodes()
            .into_iter()
            .map(|n| n.0 as f64)
            .collect()
    }

    /// The number of registered node types (a boot smoke check).
    pub fn node_type_count(&self) -> usize {
        self.engine.registry().len()
    }

    /// The number of rendered objects (a smoke check that cooked geometry
    /// reached the GPU).
    pub fn object_count(&self) -> usize {
        self.raster.scene().draw_objects().count()
    }
}
