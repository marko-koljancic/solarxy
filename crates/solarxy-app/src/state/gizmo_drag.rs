//! The transform drag loop that stays in this shell.
//!
//! The solver is `solarxy_host::gizmo`, shared with the browser. What is
//! here turns its answers into engine commands, and so names a document;
//! the shared host has no engine by design and the engine never sees a
//! manipulator, which is why this loop exists once per shell rather than
//! once. It is the browser host's `gizmo_drag.rs` with the boundary
//! removed: the same hit test, the same transaction per drag, the same
//! preview lane during the drag, the same no-movement rollback.
//!
//! One gesture is one undo step. The transaction opens on the press,
//! before a transform node is minted on the append path, so the node and
//! the move undo together; the moves stream into the preview lane and
//! write nothing; the release commits the previewed value as ordinary
//! parameter writes inside the transaction and closes it.

use cgmath::Vector3;

use solarxy_core::gizmo::TransformParams;
use solarxy_core::preferences::GizmoPrefs;
use solarxy_core::raycast::Ray;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::{Command, EngineEvent, GizmoTarget};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_host::gizmo::{self, GizmoPose, GizmoSettings, GizmoState, Orientation, ToolMode};
use solarxy_renderer::manipulator::{self, ManipulatorState};

use super::State;

/// Where the drag in flight writes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GizmoAddr {
    pub ctx: GraphContext,
    pub node: NodeId,
}

/// The shared solver's settings, read off the preference the three
/// writers share.
pub(super) fn settings_from_prefs(prefs: &GizmoPrefs) -> GizmoSettings {
    GizmoSettings {
        orientation: Orientation::parse(prefs.orientation.as_str()),
        snap_translate: prefs.snap_translate,
        snap_rotate: prefs.snap_rotate,
        snap_scale: prefs.snap_scale,
    }
}

/// The solver's view of a target: the engine's answer minus its address,
/// which the drag keeps separately.
fn gizmo_pose(t: &GizmoTarget) -> GizmoPose {
    GizmoPose {
        translate: t.translate,
        rotate: t.rotate,
        rotate_order: t.rotate_order,
        scale: t.scale,
        uniform_scale: t.uniform_scale,
        extent: t.extent,
        aim: t.aim,
        params: t.params,
        anchor: t.anchor,
        aim_anchor: t.aim_anchor,
        basis: t.basis,
        parent_basis: t.parent_basis,
        parent: t.parent,
    }
}

/// The parameter writes one solved drag value makes on one target: each
/// key the target declares for that drag, paired with the value for it.
///
/// Pairing happens in one place so preview, commit and rollback cannot
/// disagree about which key gets which number; the two sides are
/// positional, and the solver's own test keeps them the same length.
fn drag_writes(
    value: gizmo::DragValue,
    params: &TransformParams,
) -> Vec<(&'static str, ParamSource)> {
    let Some(keys) = value.param().keys(params) else {
        return Vec::new();
    };
    keys.iter()
        .zip(value.values().into_iter().flatten())
        .map(|(key, v)| {
            let value = match v {
                gizmo::DragScalarOrVec3::Vec3(v) => {
                    ParamValue::Vec3([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
                }
                gizmo::DragScalarOrVec3::Scalar(f) => ParamValue::Float(f64::from(f)),
            };
            (key, ParamSource::Literal(value))
        })
        .collect()
}

impl State {
    /// Whether a transform drag is in flight.
    pub(super) fn gizmo_dragging(&self) -> bool {
        self.gizmo.drag.is_some()
    }

    /// What the panels show about the tools this frame, for a caller that
    /// can borrow the whole state.
    pub(super) fn tool_readout(&self) -> crate::gui::ToolReadout<'_> {
        tool_readout(
            &self.gizmo,
            self.tools_available.as_deref(),
            &self.preferences.viewport,
            self.gizmo_readout.as_deref(),
        )
    }

    /// The manipulator as it stands under the cursor: the engine's target
    /// for the graph the user is looking at, scaled for that pane.
    /// `None` when no gizmo is showing there.
    fn manipulator_at(&self) -> Option<(GizmoTarget, ManipulatorState, Ray, f32)> {
        let engine = self.engine.as_deref()?;
        let target = engine.gizmo_target(self.gui.graph_ctx())?;
        let view = self.pane_view()?;
        let mut state = self
            .gizmo
            .manipulator(&gizmo_pose(&target), view.camera.forward(), 1.0)?;
        // Logical pixels, not physical: `GIZMO_PX` and `HIT_PX` are the
        // sizes the user sees, and the pane rect is physical. Divide by
        // the scale factor or the gizmo comes out half-size on a
        // high-density display, which is the trap the browser hit.
        let ppp = self.window.scale_factor() as f32;
        let world_per_px = view
            .camera
            .world_per_pixel(state.origin(), view.rect.height / ppp);
        state.scale = manipulator::GIZMO_PX * world_per_px;
        Some((target, state, view.ray, world_per_px))
    }

    /// Which handle the cursor is over, for the highlight, while no button
    /// is held.
    pub(super) fn update_gizmo_hover(&mut self) {
        self.gizmo.hovered = self
            .manipulator_at()
            .and_then(|(_, state, ray, wpp)| gizmo::hit_test(&ray, &state, wpp));
    }

    /// A primary press with a tool armed: grab a handle, if one is under
    /// the cursor. `true` when the press was taken, so the caller neither
    /// navigates nor tracks a click for it.
    ///
    /// On the append path this mints a transform node before the drag can
    /// preview anything, which is why it happens inside the drag's
    /// transaction: the node and the move then undo together, in one
    /// step.
    pub(super) fn begin_gizmo_drag(&mut self) -> bool {
        let Some((target, state, ray, wpp)) = self.manipulator_at() else {
            return false;
        };
        let Some(handle) = gizmo::hit_test(&ray, &state, wpp) else {
            return false; // a miss falls through to the camera
        };
        let Some(engine) = self.engine.as_deref_mut() else {
            return false;
        };
        let label = self.gizmo.tool.undo_label().to_string();
        if let Err(e) = engine.apply(Command::BeginTransaction { label }) {
            tracing::warn!("Could not begin a transform drag: {e}");
            return false;
        }

        // Resolve the real target. On the reuse path this is a no-op that
        // simply reports the tail transform; on the append path it creates
        // one, and the paired event is the only channel carrying the id.
        let mut target = target;
        if target.append_pending {
            let GraphContext::Subflow(sop) = target.ctx else {
                let _ = engine.apply(Command::CancelTransaction);
                return false;
            };
            let minted = engine
                .apply(Command::EnsureTransformTarget { sop })
                .ok()
                .and_then(|batch| {
                    batch.events.iter().find_map(|ev| match ev {
                        EngineEvent::TransformTargetReady { node, .. } => Some(*node),
                        _ => None,
                    })
                });
            // Re-resolve against the node the engine just minted: a fresh
            // transform is at identity, and reading its real params beats
            // patching the struct field by field.
            let fresh = minted.and_then(|_| engine.gizmo_target(target.ctx));
            let Some(fresh) = fresh else {
                let _ = engine.apply(Command::CancelTransaction);
                return false;
            };
            target = fresh;
        }

        let Some(drag) =
            gizmo::begin_drag(&ray, &state, gizmo_pose(&target), handle, self.gizmo.tool)
        else {
            let _ = engine.apply(Command::CancelTransaction);
            return false;
        };
        self.gizmo.drag = Some(drag);
        self.gizmo_addr = Some(GizmoAddr {
            ctx: target.ctx,
            node: target.node,
        });
        self.gizmo.hovered = Some(handle);
        true
    }

    /// End the in-flight drag, yielding both halves at once. Taking the
    /// pose and the address together is what keeps them from drifting
    /// into a drag that has a solved value and nowhere to write it.
    fn take_gizmo_drag(&mut self) -> Option<(gizmo::Drag, GizmoAddr)> {
        let drag = self.gizmo.drag.take()?;
        let addr = self.gizmo_addr.take()?;
        Some((drag, addr))
    }

    /// The manipulator as the live drag sees it: rebuilt at the drag's
    /// stored anchor, not at a freshly resolved target. Re-resolving
    /// mid-drag would move the gizmo's own origin under the maths, the
    /// object being what moves, and it would accelerate away from the
    /// cursor.
    fn drag_state(&self, drag: &gizmo::Drag) -> Option<(ManipulatorState, Ray)> {
        let view = self.pane_view()?;
        let tool = self.gizmo.tool.manipulator_tool()?;
        let mut state = self
            .gizmo
            .manipulator(&drag.target, view.camera.forward(), 1.0)?;
        state.tool = tool;
        state.active = Some(drag.handle);
        let ppp = self.window.scale_factor() as f32;
        state.scale = manipulator::GIZMO_PX
            * view
                .camera
                .world_per_pixel(state.origin(), view.rect.height / ppp);
        Some((state, view.ray))
    }

    /// A pointer move during a drag: solve, and stream into the preview
    /// lane. No document write, no undo entry.
    pub(super) fn update_gizmo_drag(&mut self, mods: u8) {
        let Some(mut drag) = self.gizmo.drag else {
            return;
        };
        let Some((state, ray)) = self.drag_state(&drag) else {
            return;
        };
        let settings = self.gizmo.settings;
        let Some((value, wrap)) = gizmo::solve_drag(&ray, &state, &drag, &settings, mods) else {
            return; // a degenerate view angle: hold still rather than jump
        };

        // The rotate solve accumulates across the half-turn seam, so its
        // wrap state has to ride back onto the drag or a sweep past 180
        // degrees would snap back the other way.
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

        if let (Some(addr), Some(engine)) = (self.gizmo_addr, self.engine.as_deref_mut()) {
            for (key, source) in drag_writes(value, &drag.target.params) {
                engine.preview_param(addr.ctx, addr.node, key, source);
            }
        }
    }

    /// Release: commit the dragged value as authoritative parameter writes
    /// inside the open transaction, then close it. The transaction is what
    /// makes the drag one undo step, and each write also clears its own
    /// preview.
    pub(super) fn commit_gizmo_drag(&mut self) {
        let Some((drag, addr)) = self.take_gizmo_drag() else {
            return;
        };
        self.gizmo_readout = None;
        let Some(engine) = self.engine.as_deref_mut() else {
            return;
        };

        // Whatever the preview lane last resolved to is the final value.
        // Asked through the drag's own parameter, so the commit cannot
        // read a different one than the drag wrote.
        let final_value = engine
            .gizmo_target(addr.ctx)
            .map_or(drag.start, |t| drag.param.read(&gizmo_pose(&t)));

        // A click on a handle that never moved is not an edit. Committing
        // it would push an undo step that visibly does nothing and, on the
        // append path, leave a transform node behind for a click.
        if !final_value.differs_from(drag.start) {
            self.rollback_gizmo_drag(&drag, addr);
            return;
        }

        for (key, value) in drag_writes(final_value, &drag.target.params) {
            if let Err(e) = engine.apply(Command::SetParam {
                ctx: addr.ctx,
                node: addr.node,
                key: key.to_string(),
                value,
            }) {
                tracing::warn!("Could not write the dragged value: {e}");
            }
        }
        if let Err(e) = engine.apply(Command::EndTransaction) {
            tracing::warn!("Could not close the transform drag: {e}");
        }
    }

    /// Unwind the drag in flight without committing, so the document
    /// returns to where the drag started and the object snaps back. The
    /// transaction rollback undoes the document, an appended transform
    /// node included; clearing the preview releases the transient value
    /// the drag was streaming, without which the viewport would keep
    /// asserting the dragged pose against the parameter panel.
    pub(super) fn cancel_gizmo_drag(&mut self) {
        if let Some((drag, addr)) = self.take_gizmo_drag() {
            self.rollback_gizmo_drag(&drag, addr);
        }
    }

    fn rollback_gizmo_drag(&mut self, drag: &gizmo::Drag, addr: GizmoAddr) {
        self.gizmo_readout = None;
        let Some(engine) = self.engine.as_deref_mut() else {
            return;
        };
        if let Some(keys) = drag.param.keys(&drag.target.params) {
            for key in keys.iter() {
                engine.clear_preview(addr.ctx, addr.node, key);
            }
        }
        if let Err(e) = engine.apply(Command::CancelTransaction) {
            tracing::warn!("Could not cancel the transform drag: {e}");
        }
    }

    /// Arm a tool. Switching mid-drag abandons the drag by rolling it back
    /// rather than dropping it, which is what keeps the preview lane from
    /// stranding a value the document never agreed to.
    pub(super) fn set_tool(&mut self, tool: ToolMode) {
        if tool == self.gizmo.tool {
            return;
        }
        self.cancel_gizmo_drag();
        self.gizmo.tool = tool;
        self.gizmo.hovered = None;
        self.gizmo_readout = None;
    }

    /// Re-read the solver's settings from the preference after any of its
    /// three writers changed it.
    pub(super) fn apply_gizmo_prefs(&mut self) {
        self.gizmo.settings = settings_from_prefs(&self.preferences.viewport);
    }

    /// Per frame: hand the renderer the manipulator for the current
    /// selection, and record which tools that selection can take.
    ///
    /// Pull-based, recomputed every frame from the engine's own view of
    /// the world, so a selection change or an undo moves or removes the
    /// gizmo with no extra plumbing. The view direction and the scale are
    /// per pane, so they are placeholders here; `write_manipulator`
    /// overwrites both before each pane's pass.
    pub(super) fn sync_gizmo(&mut self) {
        let target = self
            .engine
            .as_deref()
            .and_then(|engine| engine.gizmo_target(self.gui.graph_ctx()));
        let manip = target.as_ref().and_then(|t| {
            self.gizmo
                .manipulator(&gizmo_pose(t), Vector3::unit_z(), 1.0)
        });
        self.renderer.set_manipulator(manip);
        self.tools_available = target.map(|t| gizmo::tools_for(&t.params));
    }
}

/// Whether a tool applies to the selection. With nothing selected there is
/// no target and nothing is narrowed, which is what keeps an empty scene's
/// tool column looking the way it always has.
fn tool_applies(available: Option<&[ToolMode]>, tool: ToolMode) -> bool {
    available.is_none_or(|available| available.contains(&tool))
}

/// What the panels show about the tools this frame. Built from the fields
/// rather than from `State`, so the interface pass can hold it beside the
/// mutable borrows it takes of everything else.
pub(super) fn tool_readout<'a>(
    gizmo: &GizmoState,
    available: Option<&[ToolMode]>,
    prefs: &GizmoPrefs,
    readout: Option<&'a str>,
) -> crate::gui::ToolReadout<'a> {
    crate::gui::ToolReadout {
        tool: gizmo.tool,
        applies: gizmo::ALL_TOOLS.map(|tool| tool_applies(available, tool)),
        orientation: prefs.orientation,
        readout,
        dragging: gizmo.drag.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The preference and the solver state the same three snaps and the
    /// same frame, so a fresh installation drags exactly as the browser
    /// does and exactly as the solver's own tests expect.
    #[test]
    fn the_gizmo_preference_defaults_are_the_solvers() {
        let from_prefs = settings_from_prefs(&GizmoPrefs::default());
        let solver = GizmoSettings::default();
        assert_eq!(from_prefs.orientation, solver.orientation);
        assert!((from_prefs.snap_translate - solver.snap_translate).abs() < f32::EPSILON);
        assert!((from_prefs.snap_rotate - solver.snap_rotate).abs() < f32::EPSILON);
        assert!((from_prefs.snap_scale - solver.snap_scale).abs() < f32::EPSILON);
    }

    /// The preference's wire words are the ones the solver parses, so a
    /// stored `local` arms local handles rather than falling back to world.
    #[test]
    fn the_orientation_words_are_the_solvers() {
        use solarxy_core::preferences::GizmoOrientation;
        assert_eq!(
            Orientation::parse(GizmoOrientation::Local.as_str()),
            Orientation::Local
        );
        assert_eq!(
            Orientation::parse(GizmoOrientation::World.as_str()),
            Orientation::World
        );
        assert_eq!(GizmoOrientation::World.toggled(), GizmoOrientation::Local);
        assert_eq!(GizmoOrientation::Local.toggled(), GizmoOrientation::World);
    }

    /// A vector value writes one key with three components and a scalar
    /// writes one key with one, in the target's own names.
    #[test]
    fn drag_writes_pair_each_declared_key_with_its_value() {
        let params = TransformParams {
            translate: Some("translate"),
            rotate: Some("rotate"),
            rotate_order: Some("rotate_order"),
            scale: solarxy_core::gizmo::ScaleParams::Vec3 {
                scale: "scale",
                uniform: "uniform_scale",
            },
            pivot: None,
            aim: None,
        };
        let writes = drag_writes(gizmo::DragValue::Translate([1.0, 2.0, 3.0]), &params);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "translate");
        let ParamSource::Literal(ParamValue::Vec3(v)) = writes[0].1 else {
            panic!("a translate writes a vector literal");
        };
        assert!(
            v.iter()
                .zip([1.0, 2.0, 3.0])
                .all(|(a, b)| (a - b).abs() < 1e-9)
        );
        let writes = drag_writes(gizmo::DragValue::UniformScale(2.5), &params);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "uniform_scale");
        let ParamSource::Literal(ParamValue::Float(f)) = writes[0].1 else {
            panic!("a uniform scale writes a float literal");
        };
        assert!((f - 2.5).abs() < 1e-9);
    }

    /// A drag on a parameter the target does not declare writes nothing,
    /// rather than a key the resolver would refuse.
    #[test]
    fn a_value_for_an_undeclared_parameter_writes_nothing() {
        let params = TransformParams::default();
        assert!(drag_writes(gizmo::DragValue::Rotate([0.0, 90.0, 0.0]), &params).is_empty());
    }
}
