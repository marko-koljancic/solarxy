//! The transform drag loop that stays in this shell.
//!
//! The solver is `solarxy_host::gizmo`, shared since 0.9.0. What is here
//! dispatches engine commands and returns event batches, so it names a
//! document and belongs above the shared layer rather than in it.

use super::*;

// The gizmo drag loop. Lives entirely in Rust: `pointer_move` runs at pointer
// rate, and a boundary crossing per move would be a waste.
impl SolarxyApp {
    /// The pane under a point, its camera, and a world ray through it. Exactly
    /// the recipe `pick` uses, so the gizmo grabs what the user sees.
    pub(super) fn pane_ray(&self, p: (f32, f32)) -> Option<(usize, PaneRect, Camera, Ray)> {
        let rects = self.compute_panes();
        let idx = panes::hit_test_pane(&rects, p);
        let pane = *rects.get(idx)?;
        // A UV pane has no 3D scene to manipulate.
        if self.view.pane_settings[idx].pane_mode == PaneMode::UvMap {
            return None;
        }
        let mut cam = self.view.cameras[idx].as_ref()?.camera;
        cam.aspect = pane.width / pane.height.max(1.0);
        let ray = screen_to_world_ray(
            (p.0 - pane.x, p.1 - pane.y),
            (pane.width, pane.height),
            cam.build_view_projection_matrix(),
        );
        Some((idx, pane, cam, ray))
    }

    /// The manipulator as it stands under a given pointer position: the engine's
    /// target, scaled for that pane. `None` when no gizmo is showing there.
    pub(super) fn manipulator_at(
        &self,
        p: (f32, f32),
    ) -> Option<(GizmoTarget, ManipulatorState, Camera, Ray, f32)> {
        let target = self.engine.gizmo_target(self.current_ctx)?;
        let (_, pane, cam, ray) = self.pane_ray(p)?;
        let mut state = self
            .gizmo
            .manipulator(&gizmo_pose(&target), cam.forward(), 1.0)?;
        // CSS px, not physical: `GIZMO_PX` and `HIT_PX` are the sizes the USER
        // sees, and pane rects are physical. Divide by the dpr or the gizmo comes
        // out half-size on a retina display (it did).
        let world_per_px = cam.world_per_pixel(state.origin(), pane.height / self.dpr);
        state.scale = manipulator::GIZMO_PX * world_per_px;
        Some((target, state, cam, ray, world_per_px))
    }

    pub(super) fn update_gizmo_hover(&mut self, p: (f32, f32)) {
        self.gizmo.hovered = self
            .manipulator_at(p)
            .and_then(|(_, state, _, ray, wpp)| gizmo::hit_test(&ray, &state, wpp));
    }

    /// Left press with a tool armed: grab a handle, if one is under the cursor.
    ///
    /// On the APPEND path this mints a transform node before the drag can preview
    /// anything, which is why it happens inside the drag's transaction: the node
    /// and the move then undo together, in one step.
    pub(super) fn begin_gizmo_drag(&mut self, p: (f32, f32)) -> Result<Option<JsValue>, JsError> {
        let Some((target, state, _, ray, wpp)) = self.manipulator_at(p) else {
            return Ok(None);
        };
        let Some(handle) = gizmo::hit_test(&ray, &state, wpp) else {
            return Ok(None); // a miss falls through to the camera, as before
        };

        let mut events = Vec::new();
        let begin = self
            .engine
            .apply(Command::BeginTransaction {
                label: self.gizmo.tool.undo_label().to_string(),
            })
            .map_err(|e| JsError::new(&format!("{e}")))?;
        events.extend(begin.events);

        // Resolve the real target. On the reuse path this is a no-op that simply
        // reports the tail transform; on the append path it creates one.
        let mut target = target;
        if target.append_pending {
            let GraphContext::Subflow(sop) = target.ctx else {
                return Ok(None);
            };
            let batch = self
                .engine
                .apply(Command::EnsureTransformTarget { sop })
                .map_err(|e| JsError::new(&format!("{e}")))?;
            // The paired event is the ONLY channel carrying the id (the reuse
            // path emits no NodeAdded).
            let node = batch.events.iter().find_map(|ev| match ev {
                EngineEvent::TransformTargetReady { node, .. } => Some(*node),
                _ => None,
            });
            let Some(node) = node else {
                return Ok(None);
            };
            events.extend(batch.events);

            // Re-resolve against the node the engine just minted: a fresh
            // transform is at identity, and reading its real params beats
            // hand-patching the struct field by field (which is how the old code
            // did it, and how it would have quietly kept a stale rotate).
            let Some(fresh) = self.engine.gizmo_target(target.ctx) else {
                return Ok(None);
            };
            debug_assert_eq!(fresh.node, node, "the engine minted a different node");
            target = fresh;
        }

        let Some(drag) =
            gizmo::begin_drag(&ray, &state, gizmo_pose(&target), handle, self.gizmo.tool)
        else {
            return Ok(None);
        };
        self.gizmo.drag = Some(drag);
        self.gizmo_addr = Some(GizmoAddr {
            ctx: target.ctx,
            node: target.node,
        });
        self.gizmo.hovered = Some(handle);

        let revision = self.engine.revision();
        Ok(Some(to_js(&EventBatch { revision, events })?))
    }

    /// End the in-flight drag, yielding both halves at once.
    ///
    /// The only way a drag ends. Taking the pose and the address together is
    /// what keeps them from drifting into a drag that has a solved value and
    /// nowhere to write it.
    pub(super) fn take_gizmo_drag(&mut self) -> Option<(gizmo::Drag, GizmoAddr)> {
        let drag = self.gizmo.drag.take()?;
        let addr = self.gizmo_addr.take()?;
        Some((drag, addr))
    }

    /// The manipulator as the LIVE drag sees it: rebuilt at the drag's stored
    /// anchor, not at a freshly resolved target.
    ///
    /// That distinction is load-bearing. Re-resolving mid-drag would move the
    /// gizmo's own origin under the maths (the object is moving, after all), and
    /// the object would accelerate away from the cursor.
    pub(super) fn drag_state(
        &self,
        drag: &gizmo::Drag,
        p: (f32, f32),
    ) -> Option<(ManipulatorState, Ray)> {
        let (_, pane, cam, ray) = self.pane_ray(p)?;
        let tool = self.gizmo.tool.manipulator_tool()?;
        let mut state = self.gizmo.manipulator(&drag.target, cam.forward(), 1.0)?;
        state.tool = tool;
        state.active = Some(drag.handle);
        state.scale =
            manipulator::GIZMO_PX * cam.world_per_pixel(state.origin(), pane.height / self.dpr);
        Some((state, ray))
    }

    /// Pointer move during a drag: solve, and stream into the preview lane. No
    /// document write, no undo entry, no event, no JS traffic.
    pub(super) fn update_gizmo_drag(&mut self, p: (f32, f32), mods: u8) {
        let Some(mut drag) = self.gizmo.drag else {
            return;
        };
        let Some((state, ray)) = self.drag_state(&drag, p) else {
            return;
        };

        let settings = self.gizmo.settings;
        let Some((value, wrap)) = gizmo::solve_drag(&ray, &state, &drag, &settings, mods) else {
            return; // degenerate view angle: hold still rather than jump
        };

        // The rotate solve accumulates across the +/- pi seam, so its wrap state
        // has to ride back onto the drag or a sweep past 180 degrees would snap
        // back the other way.
        if let Some((last_raw, turns)) = wrap
            && let gizmo::DragGrab::Rotate {
                axis, start_vec, ..
            } = drag.grab
        {
            drag.grab = gizmo::DragGrab::Rotate {
                axis,
                start_vec,
                last_raw,
                turns,
            };
        }
        self.gizmo.drag = Some(drag);
        self.gizmo_readout = value.readout(drag.start);

        if let Some(addr) = self.gizmo_addr {
            for (key, source) in drag_writes(value, &drag.target.params) {
                self.engine.preview_param(addr.ctx, addr.node, key, source);
            }
        }
    }

    /// Release: commit the dragged value as authoritative `SetParam`s inside
    /// the open transaction, then close it. That is the whole "one undo step
    /// per drag" contract -- the transaction is what makes it one step, and
    /// each `SetParam` also clears its own preview.
    ///
    /// Plural because a target sized by two edge lengths writes both when its
    /// size is dragged; everything else writes one.
    pub(super) fn commit_gizmo_drag(&mut self) -> Result<JsValue, JsError> {
        let Some((drag, addr)) = self.take_gizmo_drag() else {
            return Ok(JsValue::NULL);
        };
        self.gizmo_readout = None;
        let mut events = Vec::new();

        // Whatever the preview lane last resolved to IS the final value. Asked
        // through the drag's own `DragParam`, so the commit cannot read a
        // different param than the drag wrote.
        let final_value = self
            .engine
            .gizmo_target(addr.ctx)
            .map_or(drag.start, |t| drag.param.read(&gizmo_pose(&t)));

        // A click on a handle that never moved is not an edit. Committing it
        // would push an undo step that visibly does nothing (and, on the append
        // path, would leave a transform node behind for a click). Roll it back
        // instead, which is exactly what Escape does.
        if !final_value.differs_from(drag.start) {
            return self.rollback_gizmo_drag(&drag, addr);
        }

        for (key, value) in drag_writes(final_value, &drag.target.params) {
            let set = self
                .engine
                .apply(Command::SetParam {
                    ctx: addr.ctx,
                    node: addr.node,
                    key: key.to_string(),
                    value,
                })
                .map_err(|e| JsError::new(&format!("{e}")))?;
            events.extend(set.events);
        }

        let end = self
            .engine
            .apply(Command::EndTransaction)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        events.extend(end.events);

        let revision = self.engine.revision();
        to_js(&EventBatch { revision, events })
    }

    /// Unwinds a drag without committing: the document returns to where the drag
    /// started and the object snaps back.
    ///
    /// Two halves, and BOTH are needed. The transaction rollback undoes the
    /// document (an appended transform node); clearing the preview releases the
    /// transient value the drag was streaming. Skip the second and the viewport
    /// would keep asserting the dragged pose forever, disagreeing with the
    /// parameter panel. The keys come from the drag's own `DragParam` resolved
    /// against its own target, so a rotate cancel can never clear a translate
    /// and a two-edge cancel cannot leave one of them stranded.
    pub(super) fn rollback_gizmo_drag(
        &mut self,
        drag: &gizmo::Drag,
        addr: GizmoAddr,
    ) -> Result<JsValue, JsError> {
        self.gizmo_readout = None;
        if let Some(keys) = drag.param.keys(&drag.target.params) {
            for key in keys.iter() {
                self.engine.clear_preview(addr.ctx, addr.node, key);
            }
        }
        let batch = self
            .engine
            .apply(Command::CancelTransaction)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        to_js(&batch)
    }
}
