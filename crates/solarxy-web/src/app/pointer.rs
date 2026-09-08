//! Pointer routing and picking: CSS pixels in, pane aware.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    // ---- pointer routing (CSS px in; pane-aware) ----

    /// Selects the active viewport tool ("select" | "move" | "rotate" | "scale").
    /// The only JS surface the gizmo needs: the drag itself never crosses the
    /// boundary.
    pub fn set_tool(&mut self, tool: &str) -> Result<JsValue, JsError> {
        let next = ToolMode::parse(tool);
        if next == self.gizmo.tool {
            return Ok(JsValue::NULL);
        }
        // Switching tools mid-drag abandons the drag. Rolling it back rather
        // than dropping it is what keeps the preview lane from stranding a value
        // the document never agreed to.
        let rollback = match self.take_gizmo_drag() {
            Some((drag, addr)) => self.rollback_gizmo_drag(&drag, addr)?,
            None => JsValue::NULL,
        };
        self.gizmo.tool = next;
        self.gizmo.hovered = None;
        self.gizmo_readout = None;
        Ok(rollback)
    }

    /// The gizmo's drag ergonomics, pushed from the TS prefs store.
    ///
    /// Pushed rather than polled because the drag loop never crosses back into
    /// JS: it would have to ask once per pointer move, which is exactly the
    /// traffic this design exists to avoid.
    pub fn set_gizmo_settings(
        &mut self,
        orientation: &str,
        snap_translate: f32,
        snap_rotate: f32,
        snap_scale: f32,
    ) {
        self.gizmo.settings = gizmo::GizmoSettings {
            orientation: gizmo::Orientation::parse(orientation),
            snap_translate: snap_translate.max(0.0),
            snap_rotate: snap_rotate.max(0.0),
            snap_scale: snap_scale.max(0.0),
        };
    }

    /// Enters (or leaves) player mode.
    ///
    /// Locks the layout to a single pane and clears any lingering editing
    /// affordance. The clock is NOT started here: whether a published scene
    /// autoplays is the document's `autoplay` setting, read by the player
    /// shell, so the same flag means the same thing whether the scene was
    /// exported or opened in the editor.
    pub fn set_player_mode(&mut self, on: bool) {
        self.player_mode = on;
        if on {
            self.view.display.layout = ViewLayout::Single;
            self.view.active_pane = 0;
            self.selected_object = None;
            self.gizmo_readout = None;
            self.renderer.set_manipulator(None);
            self.host_events.push(HostEvent::ViewChanged);
        }
    }

    /// The clock's current frame, polled by the player's transport readout.
    ///
    /// Polled rather than pushed for the same reason the gizmo readout is:
    /// under a playing clock a pushed value would cross the wasm boundary
    /// once per frame to update a number nobody is reading that closely.
    #[must_use]
    pub fn clock_frame(&self) -> f64 {
        self.engine.clock().frame as f64
    }

    /// Whether the clock is running right now.
    ///
    /// Polled beside [`SolarxyApp::clock_frame`] rather than tracked by the
    /// caller: a `once` range CLEARS `playing` when it reaches the end, so a
    /// shell holding its own boolean shows "Pause" over a clock that stopped
    /// by itself.
    #[must_use]
    pub fn clock_playing(&self) -> bool {
        self.engine.clock().playing
    }

    /// Whether a published scene should start playing, from the document's
    /// own runtime settings. The player shell reads this after load rather
    /// than the editor acting on it: autoplay in an authoring tool is a
    /// surprise, and in a viewer it is the point.
    #[must_use]
    pub fn autoplay(&self) -> bool {
        self.engine.clock().autoplay
    }

    /// The display defaults, pushed from the TS prefs store (the
    /// gizmo-settings pattern). The turntable rpm and the point size apply
    /// immediately: both are live session state that never serializes into
    /// `.slxy`. Wireframe weight and background are stored as the pane seed
    /// and, per `apply_*` flag, written into every pane: the boot push sets
    /// both flags; a mid-session preference save sets only the flags for
    /// fields that actually changed, so per-pane Display-menu overrides
    /// survive unrelated preference edits.
    ///
    /// The boundary mirrors the TypeScript preference bundle field for
    /// field, which is why the argument list is long and bool-heavy: a
    /// struct here would drag serde through the wasm boundary for a plain
    /// setter.
    #[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
    pub fn set_display_defaults(
        &mut self,
        wireframe_weight: &str,
        background: &str,
        turntable_rpm: f32,
        point_size: f32,
        ssao_enabled: bool,
        bloom_enabled: bool,
        bloom_strength: f32,
        bloom_threshold: f32,
        ssao_strength: f32,
        preview_denoise: bool,
        apply_wireframe: bool,
        apply_background: bool,
    ) {
        self.display_defaults = DisplayDefaults {
            line_weight: display_defaults::parse_line_weight(wireframe_weight),
            background: display_defaults::parse_background(background),
        };
        // Renderer-global post effects, applied on every push: the write is
        // idempotent and the next frame picks it up, so a preference toggle
        // needs no reload and no host event.
        self.renderer.post.ssao_enabled = ssao_enabled;
        self.renderer.post.bloom_enabled = bloom_enabled;
        // Clamped by the setter, so a hand-edited stored preference cannot
        // push the composite somewhere the sliders cannot reach.
        self.renderer
            .post
            .set_strengths(solarxy_core::view_config::PostStrengths {
                bloom_strength,
                bloom_threshold,
                ssao_strength,
            });
        // The preview's filter. Held here rather than baked into
        // `preview_trace_settings`, which asserts it on every encode.
        self.preview_denoise = preview_denoise;
        self.view.display.turntable_rpm = if turntable_rpm.is_finite() {
            turntable_rpm.clamp(1.0, 60.0)
        } else {
            6.0
        };
        self.view.display.point_size = if point_size.is_finite() {
            point_size.clamp(
                solarxy_core::view_config::MIN_POINT_SIZE,
                solarxy_core::view_config::MAX_POINT_SIZE,
            )
        } else {
            solarxy_core::view_config::DEFAULT_POINT_SIZE
        };
        let mut changed = false;
        for pds in &mut self.view.pane_settings {
            if apply_wireframe && pds.line_weight != self.display_defaults.line_weight {
                pds.line_weight = self.display_defaults.line_weight;
                changed = true;
            }
            if apply_background && pds.background_mode != self.display_defaults.background {
                pds.background_mode = self.display_defaults.background;
                changed = true;
            }
        }
        if changed {
            self.host_events.push(HostEvent::ViewChanged);
        }
    }

    /// The live drag readout ("X +1.250 m"), or `null` when nothing is dragging.
    ///
    /// POLLED once per frame, not pushed: `pointer_move` stays void so the hot
    /// path keeps costing zero boundary crossings, and the frame loop is already
    /// crossing anyway for the cook.
    #[must_use]
    pub fn gizmo_readout(&self) -> Option<String> {
        self.gizmo_readout.clone()
    }

    /// Pointer button down. `button`: 0 left, 1 middle, 2 right.
    ///
    /// Returns an `EventBatch` when the press STARTED a gizmo drag that mutated
    /// the document (the append path mints a transform node), else `null`.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: u32) -> Result<JsValue, JsError> {
        let p = (x * self.dpr, y * self.dpr);
        if self.pointer_buttons_down == 0 {
            self.set_hovered_pane(panes::hit_test_pane(&self.compute_panes(), p));
        }
        self.pointer_buttons_down |= 1 << button;

        // The gizmo gets first refusal on a LEFT press, and only on a left press:
        // middle and right always reach the camera, so orbit and pan can never be
        // stolen by a tool. In Select mode this whole branch is skipped and the
        // behaviour is bit-for-bit what it was.
        if button == 0
            && self.gizmo.tool.is_transform_tool()
            && let Some(batch) = self.begin_gizmo_drag(p)?
        {
            return Ok(batch);
        }

        let active = self.view.active_pane;
        if let Some(cam) = self.view.cameras[active].as_mut() {
            cam.handle_mouse_move(p.0, p.1);
            if let Some(btn) = map_button(button) {
                cam.handle_mouse_button(btn, true);
            }
        }
        // On a locked look-through pane, this drag reframes the bound camera, so
        // suppress the node-to-pane follow for the duration of the gesture.
        if self.is_locked_look_through(active) {
            self.camera_editing[active] = true;
        }
        Ok(JsValue::NULL)
    }

    /// Pointer move; updates the hovered (active) pane while no drag is in
    /// flight, and feeds the active pane's camera controller.
    ///
    /// Deliberately returns nothing: this is the hot path, and a live gizmo drag
    /// streams straight into the engine's preview lane without crossing into JS
    /// at all.
    pub fn pointer_move(&mut self, x: f32, y: f32, mods: u8) {
        let p = (x * self.dpr, y * self.dpr);
        let last = std::mem::replace(&mut self.last_pointer, p);
        if self.pointer_buttons_down == 0 {
            self.set_hovered_pane(panes::hit_test_pane(&self.compute_panes(), p));
        }

        // A live drag owns the pointer entirely.
        if self.gizmo.drag.is_some() {
            self.update_gizmo_drag(p, mods);
            return;
        }
        // Otherwise, with a tool armed, keep the hover highlight fresh.
        if self.gizmo.tool.is_transform_tool() && self.pointer_buttons_down == 0 {
            self.update_gizmo_hover(p);
        }

        let active = self.view.active_pane;
        if self.view.pane_settings[active].pane_mode == PaneMode::UvMap {
            // Drag pans the UV view (screen px scaled by the visible UV
            // span; the camera half-height is 0.6 / zoom).
            if self.pointer_buttons_down != 0 {
                let rects = self.compute_panes();
                let height = rects.get(active).map_or(1.0, |r| r.height.max(1.0));
                let pds = &mut self.view.pane_settings[active];
                let uv_per_px = (1.2 / pds.uv_zoom) / height;
                pds.uv_offset[0] -= (p.0 - last.0) * uv_per_px;
                pds.uv_offset[1] -= (p.1 - last.1) * uv_per_px;
                self.host_events.push(HostEvent::ViewChanged);
            }
            return;
        }
        if let Some(cam) = self.view.cameras[active].as_mut() {
            cam.handle_mouse_move(p.0, p.1);
        }
    }

    /// Updates the pointer-hovered active pane, mirroring the change to
    /// the frontend as a host event (the keyboard context reads the
    /// mirror, so it must track pointer routing).
    pub(super) fn set_hovered_pane(&mut self, pane: usize) {
        if pane != self.view.active_pane {
            self.view.active_pane = pane;
            self.host_events.push(HostEvent::ActivePane { pane });
        }
    }

    /// Pointer button up. Returns the commit `EventBatch` when it ended a gizmo
    /// drag, else `null`.
    pub fn pointer_up(&mut self, button: u32) -> Result<JsValue, JsError> {
        self.pointer_buttons_down &= !(1 << button);

        if button == 0 && self.gizmo.drag.is_some() {
            return self.commit_gizmo_drag();
        }

        let active = self.view.active_pane;
        if let Some(cam) = self.view.cameras[active].as_mut()
            && let Some(btn) = map_button(button)
        {
            cam.handle_mouse_button(btn, false);
        }
        // A locked look-through reframe ends when the last button lifts: commit
        // the new camera pose to the node (one undo step) and let the follow
        // resume. The batch flows back so the parameter panel reflects the pose.
        if self.pointer_buttons_down == 0 && self.camera_editing[active] {
            self.camera_editing[active] = false;
            if let Some(batch) = self.commit_pane_camera_to_node(active) {
                return to_js(&batch);
            }
        }
        Ok(JsValue::NULL)
    }

    /// Escape during a drag: the document returns to where the drag started and
    /// the object snaps back. Returns the rollback `EventBatch`, or `null` when
    /// no drag was in flight.
    pub fn cancel_gizmo_drag(&mut self) -> Result<JsValue, JsError> {
        let Some((drag, addr)) = self.take_gizmo_drag() else {
            return Ok(JsValue::NULL);
        };
        self.rollback_gizmo_drag(&drag, addr)
    }

    /// Wheel zoom on the active pane; positive zooms in.
    pub fn wheel(&mut self, delta: f32) {
        let active = self.view.active_pane;
        if self.view.pane_settings[active].pane_mode == PaneMode::UvMap {
            let pds = &mut self.view.pane_settings[active];
            pds.uv_zoom = (pds.uv_zoom * (1.0 + delta * 0.1)).clamp(0.1, 50.0);
            self.host_events.push(HostEvent::ViewChanged);
            return;
        }
        if let Some(cam) = self.view.cameras[active].as_mut() {
            cam.handle_scroll(delta);
        }
        // A dolly on a locked look-through pane reframes the bound camera. Wheel
        // has no return channel; committing bumps the revision, and the frame
        // loop's next batch carries it so the mirror self-heals (resnapshot on
        // the gap), keeping the node params in step with the pose.
        if self.is_locked_look_through(active) {
            let _ = self.commit_pane_camera_to_node(active);
        }
    }

    /// Picks the node under a canvas CSS pixel, pane-aware: the ray is
    /// built from the pane under the cursor with that pane's camera.
    /// Returns the node id as a number, or `undefined` on a miss.
    ///
    /// A light's marker is tested before the geometry, and only where the pane
    /// actually draws markers, so a pane with them turned off picks exactly
    /// what it did before.
    pub fn pick(&self, x: f32, y: f32) -> Option<f64> {
        if self.player_mode {
            return None;
        }
        let p = (x * self.dpr, y * self.dpr);
        let rects = self.compute_panes();
        let pane_idx = panes::hit_test_pane(&rects, p);
        let pane = rects.get(pane_idx)?;
        let mut cam = self.view.cameras[pane_idx].as_ref()?.camera;
        cam.aspect = pane.width / pane.height.max(1.0);
        let view_proj = cam.build_view_projection_matrix();
        let ray = screen_to_world_ray(
            (p.0 - pane.x, p.1 - pane.y),
            (pane.width, pane.height),
            view_proj,
        );
        let origin = [ray.origin.x, ray.origin.y, ray.origin.z];
        let dir = [ray.direction.x, ray.direction.y, ray.direction.z];
        let markers = self.view.pane_settings[pane_idx]
            .show_light_markers
            .then(|| solarxy_graph::engine::MarkerPick {
                view_proj: view_proj.into(),
                // Physical pixels on both, matching the ray above. The radius
                // is a CSS size, so it scales with the device ratio or a
                // marker would be half as clickable on a retina display as it
                // looks -- the same trap the gizmo hit.
                viewport_px: [pane.width, pane.height],
                cursor_px: [p.0 - pane.x, p.1 - pane.y],
                radius_px: manipulator::MARKER_PX * self.dpr,
            });
        self.engine.pick(origin, dir, markers).map(|n| n.0 as f64)
    }

    /// [`SolarxyApp::pick`] with the full hit detail (mesh, face,
    /// barycentric, world point, pane): the anchor source for creating and
    /// re-placing review annotations. `undefined` on a miss.
    pub fn pick_detailed(&self, x: f32, y: f32) -> Result<JsValue, JsError> {
        let p = (x * self.dpr, y * self.dpr);
        let rects = self.compute_panes();
        let pane_idx = panes::hit_test_pane(&rects, p);
        let detail = rects.get(pane_idx).and_then(|pane| {
            let mut cam = self.view.cameras[pane_idx].as_ref()?.camera;
            cam.aspect = pane.width / pane.height.max(1.0);
            let ray = screen_to_world_ray(
                (p.0 - pane.x, p.1 - pane.y),
                (pane.width, pane.height),
                cam.build_view_projection_matrix(),
            );
            let origin = [ray.origin.x, ray.origin.y, ray.origin.z];
            let dir = [ray.direction.x, ray.direction.y, ray.direction.z];
            self.engine
                .pick_detailed(origin, dir)
                .map(|d| PickDetailDto {
                    node: d.node.0 as f64,
                    mesh: d.mesh,
                    face: d.face,
                    barycentric: d.barycentric,
                    world_pos: d.world_pos,
                    distance: d.distance,
                    pane: pane_idx,
                })
        });
        to_js(&detail)
    }
}
