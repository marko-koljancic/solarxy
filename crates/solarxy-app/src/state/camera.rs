//! Where the pane cameras point, and which of them a gesture moves.
//!
//! `for_each_target_cam` is the answer to "which cameras does this act on",
//! and it is one answer rather than one per caller: the active pane, or every
//! pane when the cameras are linked. Every framing action below routes through
//! it or names a pane explicitly.

use solarxy_core::preferences::PaneMode;
use solarxy_graph::Command;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::validation::resolve_issue_aabb;

use super::State;

/// One pose parameter written as a literal on a root node: the shape both
/// the camera authored from a view and the pose written back to a locked
/// camera use, so the two cannot spell a vector differently.
fn pose_write(node: NodeId, key: &str, value: [f32; 3]) -> Command {
    Command::SetParam {
        ctx: GraphContext::Root,
        node,
        key: key.to_string(),
        value: ParamSource::Literal(ParamValue::Vec3([
            f64::from(value[0]),
            f64::from(value[1]),
            f64::from(value[2]),
        ])),
    }
}

impl State {
    /// Apply `f` to each pane camera the current gesture targets: the
    /// active pane, or — when cameras are linked — every pane the layout
    /// uses. UV-map panes are skipped.
    pub(in crate::state) fn for_each_target_cam(&mut self, mut f: impl FnMut(&mut CameraState)) {
        let count = self.view.display.layout.pane_count();
        let active = self.view.active_pane;
        let linked = self.view.cameras_linked;
        for i in 0..count {
            if (linked || i == active)
                && self.view.pane_settings[i].pane_mode == PaneMode::Scene3D
                && let Some(cam) = &mut self.view.cameras[i]
            {
                f(cam);
            }
        }
    }

    /// A navigation gesture began on every pane it targets (the same set
    /// `for_each_target_cam` mutates). A locked pane keeps its binding and
    /// holds the follow off for the gesture, so the release commits the
    /// navigated pose to the camera node; an unlocked pane is released to a
    /// free view, keeping its followed pose as the free view's starting
    /// point.
    ///
    /// The release is this shell's own rule. The browser never releases: its
    /// follow snaps an unlocked pane straight back every frame, so navigating
    /// one there does nothing until it is locked. Taking the view over is the
    /// more useful answer, and the divergence is recorded rather than copied.
    pub(super) fn release_look_through_for_gesture(&mut self) {
        let count = self.view.display.layout.pane_count().min(4);
        let active = self.view.active_pane;
        let linked = self.view.cameras_linked;
        let mut released = false;
        for i in 0..count {
            if !(linked || i == active)
                || self.view.pane_settings[i].pane_mode != PaneMode::Scene3D
                || self.look_through[i].is_none()
            {
                continue;
            }
            if self.is_locked_look_through(i) {
                self.camera_editing[i] = true;
            } else {
                self.unbind_pane(i);
                released = true;
            }
        }
        if released {
            self.gui
                .set_toast("Released to free view", crate::gui::ToastSeverity::Info);
        }
    }

    /// Release one pane's binding, for the actions that reframe a single
    /// camera: framing, fly-to-issue, fly-to-annotation.
    pub(super) fn release_look_through_pane(&mut self, pane: usize) {
        if pane < 4 && self.look_through[pane].is_some() {
            self.unbind_pane(pane);
            self.gui
                .set_toast("Released to free view", crate::gui::ToastSeverity::Info);
        }
    }

    /// Take a pane's binding away, and with it its lock and its gesture:
    /// the one place the three are cleared together, so a released pane
    /// cannot keep a lock that a rebinding would inherit.
    pub(super) fn unbind_pane(&mut self, pane: usize) {
        if pane < 4 {
            self.look_through[pane] = None;
            self.view.camera_locked.release(pane);
            self.camera_editing[pane] = false;
        }
    }

    /// Whether `pane` is bound and locked, so navigating it reframes the
    /// camera node.
    pub(super) fn is_locked_look_through(&self, pane: usize) -> bool {
        self.view
            .camera_locked
            .is_locked(pane, pane < 4 && self.look_through[pane].is_some())
    }

    /// Lock or unlock a bound pane. A free view cannot be locked, which the
    /// shared rule enforces rather than this call.
    pub(super) fn set_pane_camera_lock(&mut self, pane: usize, locked: bool) {
        if pane < 4 {
            self.view
                .camera_locked
                .set(pane, locked, self.look_through[pane].is_some());
            self.camera_editing[pane] = false;
        }
    }

    /// The navigation gesture on the active pane ended: if it was reframing
    /// a locked camera, commit the pose to the node and let the follow
    /// resume.
    pub(super) fn end_camera_gesture(&mut self) {
        let active = self.view.active_pane;
        if active < 4 && self.camera_editing[active] {
            self.camera_editing[active] = false;
            self.commit_pane_camera_to_node(active);
        }
    }

    /// Write a locked pane's current camera pose back to its bound `camera`
    /// node as one undo step: the two pose parameters inside one
    /// transaction, the browser's `Frame Camera`.
    ///
    /// A press and release with no movement commits nothing, compared
    /// against what is on the node now rather than a pose cached at press
    /// time, so a follow that ran mid-gesture cannot make an unchanged pose
    /// look changed; without the guard, locking a pane would dirty the
    /// document and push an undo step that changes nothing.
    pub(super) fn commit_pane_camera_to_node(&mut self, pane: usize) {
        let Some(bound) = self.look_through.get(pane).copied().flatten() else {
            return;
        };
        let Some((eye, target)) = self.view.cameras[pane]
            .as_ref()
            .map(|cam| (cam.camera.eye, cam.camera.target))
        else {
            return;
        };
        let Some(engine) = self.engine.as_deref_mut() else {
            return;
        };
        let node = NodeId(bound.0);
        let eye = [eye.x, eye.y, eye.z];
        let target = [target.x, target.y, target.z];
        if engine
            .document()
            .graph(GraphContext::Root)
            .ok()
            .and_then(|graph| graph.node(node))
            .is_some_and(|data| {
                solarxy_graph::camera_commit::pose_unchanged(&data.params, eye, target)
            })
        {
            return;
        }
        let commands = [
            Command::BeginTransaction {
                label: "Frame Camera".to_string(),
            },
            pose_write(node, "position", eye),
            pose_write(node, "target", target),
            Command::EndTransaction,
        ];
        for command in commands {
            if let Err(e) = engine.apply(command) {
                tracing::warn!("Could not write the camera pose back: {e}");
                let _ = engine.apply(Command::CancelTransaction);
                return;
            }
        }
    }

    /// Author a `camera` node at a pane's current pose, and bind that pane to
    /// it.
    ///
    /// How a framing decision made by flying around becomes part of the
    /// document. One transaction, so the node and its two pose parameters
    /// undo together: a user who regrets it wants one undo, not three.
    ///
    /// The binding is marked **unresolved** rather than settled, which is the
    /// difference from picking an existing camera. The node does not reach
    /// the scene until the next cook, so the resolver is what waits for it,
    /// and it already knows both how to wait and when to give up.
    pub(super) fn create_camera_from_view(&mut self, pane: usize) {
        if pane >= 4 {
            return;
        }
        // Read the pose before the engine borrow: it answers with owned
        // arrays, so the two borrows never overlap.
        let (position, target) =
            solarxy_host::cameras::pane_pose(self.view.cameras.get(pane).and_then(Option::as_ref));
        let Some(engine) = self.engine.as_mut() else {
            self.gui.set_toast(
                "No document to add a camera to",
                crate::gui::ToastSeverity::Warning,
            );
            return;
        };
        let Some(node) = author_camera(engine, position, target) else {
            self.gui.set_toast(
                "The camera could not be created",
                crate::gui::ToastSeverity::Error,
            );
            return;
        };
        self.look_through[pane] = Some(solarxy_core::scene::SceneObjectId(node.0));
        self.unresolved_binding[pane] = true;
        self.gui
            .set_toast("Camera created from view", crate::gui::ToastSeverity::Info);
    }

    /// Fly the active pane's camera to frame the mesh a validation issue
    /// lives on (Properties → Validation row click) and enable that
    /// pane's per-face validation overlay so the defect is visible.
    pub(super) fn fly_to_node_issue(
        &mut self,
        ctx: solarxy_graph::document::GraphContext,
        node: solarxy_graph::document::NodeId,
        index: usize,
    ) {
        let Some(aabb) = self.node_issue_aabb(ctx, node, index) else {
            return;
        };
        self.view.pane_settings[self.view.active_pane].show_validation = true;
        self.frame_active_pane(aabb);
    }

    /// World-space bounds of the geometry behind one row of a node's own
    /// validation report.
    ///
    /// The object the camera frames is the one the node's network belongs
    /// to: a node inside a container names that container's scene object,
    /// and a container at the root names its own. The issue's scope is
    /// resolved against that object's uploaded model, as the browser does,
    /// so a node that is not the network's display node frames the object
    /// the network shows rather than geometry nobody can see.
    fn node_issue_aabb(
        &self,
        ctx: solarxy_graph::document::GraphContext,
        node: solarxy_graph::document::NodeId,
        index: usize,
    ) -> Option<solarxy_core::AABB> {
        let owner = match ctx {
            solarxy_graph::document::GraphContext::Subflow(owner) => owner,
            solarxy_graph::document::GraphContext::Root => node,
        };
        let id = solarxy_core::scene::SceneObjectId(owner.0);
        let engine = self.engine.as_ref()?;
        let result = engine.validation(node)?;
        let issue = result.report.issues.get(index)?;
        let object = self.raster.scene().get(id)?;
        let raw_to_gpu = self.raster.scene().raw_to_gpu(id)?;
        resolve_issue_aabb(&issue.scope, &object.model, raw_to_gpu)
            .map(|b| b.transformed(&object.transform))
    }

    /// Smoothly fly the active pane's camera to frame `bounds`.
    pub(in crate::state) fn frame_active_pane(&mut self, bounds: solarxy_core::AABB) {
        self.release_look_through_pane(self.view.active_pane);
        if let Some(cam) = &mut self.view.cameras[self.view.active_pane] {
            cam.reset_to_bounds(&bounds);
        }
    }

    /// Fly the active pane's camera to a review annotation's marker (Review
    /// panel row click). The point is the engine's live resolution of the
    /// anchor, or the stored fallback for a stale note and for a reply,
    /// which draws no marker but sits where its parent does. Frames a small
    /// box around it, sized to a fraction of the scene, so the marker lands
    /// centered at a consistent, useful zoom.
    pub(super) fn focus_review_annotation(&mut self, id: solarxy_graph::review::AnnotationId) {
        let Some(engine) = self.engine.as_deref() else {
            return;
        };
        let world = engine
            .review_markers_world()
            .into_iter()
            .find(|m| m.id == id)
            .and_then(|m| m.world)
            .or_else(|| {
                engine
                    .document()
                    .review()
                    .get(id)
                    .and_then(|a| a.anchor.world_fallback)
            });
        let Some([x, y, z]) = world else {
            return;
        };
        // A fraction of the scene rather than a fixed size, so the marker
        // lands at the same apparent zoom whatever scale the document is
        // authored at.
        let half = (self.scene_bounds().diagonal() * 0.12).max(0.05);
        let center = cgmath::Point3::new(x, y, z);
        let offset = cgmath::Vector3::new(half, half, half);
        self.frame_active_pane(solarxy_core::AABB {
            min: center - offset,
            max: center + offset,
        });
    }
}

/// Run the add-and-pose sequence as one undo step, answering with the node.
///
/// A free function rather than a method because it borrows only the engine,
/// which is what lets the caller above toast on failure without holding a
/// borrow of the whole shell across the transaction.
///
/// Any failure cancels rather than leaving the transaction open: an abandoned
/// one swallows every later edit into itself.
fn author_camera(
    engine: &mut solarxy_graph::engine::Engine,
    position: [f32; 3],
    target: [f32; 3],
) -> Option<NodeId> {
    engine
        .apply(Command::BeginTransaction {
            label: "Add Camera".to_string(),
        })
        .ok()?;
    let added = engine
        .apply(Command::AddNode {
            ctx: GraphContext::Root,
            node_type: "camera".to_string(),
            position: [0.0, 0.0],
        })
        .ok()
        .as_ref()
        .and_then(solarxy_graph::model_document::added_node);
    let Some(node) = added else {
        let _ = engine.apply(Command::CancelTransaction);
        return None;
    };
    for (key, value) in [("position", position), ("target", target)] {
        if engine.apply(pose_write(node, key, value)).is_err() {
            let _ = engine.apply(Command::CancelTransaction);
            return None;
        }
    }
    engine.apply(Command::EndTransaction).ok()?;
    Some(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera written back to its node from a locked pane and a camera
    /// authored from a view spell a pose the same way: one literal vector
    /// on the root node under the key the camera node declares. The guard
    /// that decides whether a pose moved reads exactly this shape back, so
    /// a write that matched it would compare changed on every gesture.
    #[test]
    fn a_pose_write_is_the_literal_the_no_movement_guard_reads() {
        let node = NodeId(7);
        let Command::SetParam {
            ctx,
            node: written,
            key,
            value,
        } = pose_write(node, "position", [1.0, 2.5, -3.0])
        else {
            panic!("a pose write is a parameter write");
        };
        assert_eq!(ctx, GraphContext::Root);
        assert_eq!(written, node);
        assert_eq!(key, "position");
        let params = std::collections::BTreeMap::from([(key, value)]);
        assert!(!solarxy_graph::camera_commit::pose_unchanged(
            &params,
            [1.0, 2.5, -3.0],
            [0.0; 3]
        ));
        let params = std::collections::BTreeMap::from([
            ("position".to_string(), pose_source([1.0, 2.5, -3.0])),
            ("target".to_string(), pose_source([0.0, 0.0, 0.0])),
        ]);
        assert!(solarxy_graph::camera_commit::pose_unchanged(
            &params,
            [1.0, 2.5, -3.0],
            [0.0; 3]
        ));
    }

    fn pose_source(value: [f32; 3]) -> ParamSource {
        let Command::SetParam { value, .. } = pose_write(NodeId(1), "x", value) else {
            panic!("a pose write is a parameter write");
        };
        value
    }
}
