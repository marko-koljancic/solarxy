//! Where the pane cameras point, and which of them a gesture moves.
//!
//! `for_each_target_cam` is the answer to "which cameras does this act on",
//! and it is one answer rather than one per caller: the active pane, or every
//! pane when the cameras are linked. Every framing action below routes through
//! it or names a pane explicitly.

use solarxy_renderer::camera_state::CameraState;
use solarxy_core::preferences::PaneMode;
use solarxy_renderer::validation::resolve_issue_aabb;

use super::State;

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

    /// Release the look-through binding on every pane the current gesture
    /// targets (the same set `for_each_target_cam` mutates), keeping each
    /// pane's followed pose as its free view's starting point. Navigating a
    /// bound pane means taking the view over, not reframing the camera
    /// node: the write-back the web's locked mode performs is authoring
    /// machinery, and it waits for the desktop node canvas.
    pub(super) fn release_look_through_for_gesture(&mut self) {
        let count = self.view.display.layout.pane_count().min(4);
        let active = self.view.active_pane;
        let linked = self.view.cameras_linked;
        let mut released = false;
        for i in 0..count {
            if (linked || i == active)
                && self.view.pane_settings[i].pane_mode == PaneMode::Scene3D
                && self.look_through[i].take().is_some()
            {
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
        if pane < 4 && self.look_through[pane].take().is_some() {
            self.gui
                .set_toast("Released to free view", crate::gui::ToastSeverity::Info);
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
        self.gui.set_toast(
            "Camera created from view",
            crate::gui::ToastSeverity::Success,
        );
    }

    /// Fly the active pane's camera to frame the mesh a validation issue
    /// lives on (Properties → Validation row click) and enable that
    /// pane's per-face validation overlay so the defect is visible.
    pub(super) fn fly_to_validation_issue(&mut self, idx: usize) {
        let Some(aabb) = self.scene_issue_aabb(idx) else {
            return;
        };
        self.view.pane_settings[self.view.active_pane].show_validation = true;
        self.frame_active_pane(aabb);
    }

    /// World-space bounds of the geometry behind one row of the **merged**
    /// validation list.
    ///
    /// The list is N objects' reports concatenated, so the row index alone
    /// is ambiguous and has to be resolved through the owner recorded when
    /// they were merged. Re-deriving that owner here would create a second
    /// ordering that must agree with the first forever, and a disagreement
    /// would fly the camera to a different object's mesh, which looks
    /// entirely plausible on screen and so would not be caught by looking.
    fn scene_issue_aabb(&self, idx: usize) -> Option<solarxy_core::AABB> {
        let info = self.engine_scene.as_ref()?;
        let (id, local) = info.validation.owners.get(idx).copied()?;
        let object = self.raster.scene().get(id)?;
        let issue = self
            .raster
            .scene()
            .validation(id)?
            .report
            .issues
            .get(local)?;
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

    /// Fly the active pane's camera to a review annotation's anchor
    /// (Review panel row click). Frames a small box around the anchor
    /// point — sized to a fraction of the model — so the marker lands
    /// centered at a consistent, useful zoom.
    pub(super) fn focus_review_annotation(&mut self, id: &str) {
        let Some(ann) = self.review.find(id) else {
            return;
        };
        let [x, y, z] = ann.anchor.world_pos_fallback;
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
) -> Option<solarxy_graph::document::NodeId> {
    use solarxy_graph::Command;
    use solarxy_graph::document::GraphContext;
    use solarxy_graph::params::{ParamSource, ParamValue};

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
        let value = ParamValue::Vec3([
            f64::from(value[0]),
            f64::from(value[1]),
            f64::from(value[2]),
        ]);
        if engine
            .apply(Command::SetParam {
                ctx: GraphContext::Root,
                node,
                key: key.to_string(),
                value: ParamSource::Literal(value),
            })
            .is_err()
        {
            let _ = engine.apply(Command::CancelTransaction);
            return None;
        }
    }
    engine.apply(Command::EndTransaction).ok()?;
    Some(node)
}
