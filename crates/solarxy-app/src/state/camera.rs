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

    /// Fly the active pane's camera to frame the mesh a validation issue
    /// lives on (Properties → Validation row click) and enable that
    /// pane's per-face validation overlay so the defect is visible.
    pub(super) fn fly_to_validation_issue(&mut self, idx: usize) {
        let aabb = match &self.scene {
            Some(scene) => scene.validation.issues.get(idx).and_then(|issue| {
                resolve_issue_aabb(&issue.scope, &scene.model, &scene.validation_raw_to_gpu)
            }),
            None => self.scene_issue_aabb(idx),
        };
        let Some(aabb) = aabb else {
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
        let half = self
            .scene
            .as_ref()
            .map_or(1.0, |s| (s.model.bounds.diagonal() * 0.12).max(0.05));
        let center = cgmath::Point3::new(x, y, z);
        let offset = cgmath::Vector3::new(half, half, half);
        self.frame_active_pane(solarxy_core::AABB {
            min: center - offset,
            max: center + offset,
        });
    }
}
