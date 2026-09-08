//! What is drawn and what is hidden: the Outliner's actions, and the keyboard
//! shortcuts that raise the same ones.
//!
//! The two roots differ here and the difference is load-bearing. A file model's
//! meshes are the shell's own, so hiding one is a direct write. A scene
//! object's visibility belongs to the engine and is re-emitted on every cook,
//! so it travels as a parameter change instead; a direct write would be undone
//! by the user's next edit.

use crate::gui::{OutlinerAction, ToastSeverity};
use solarxy_renderer::validation::material_meshes_aabb;
use solarxy_core::scene::SceneObjectId;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};

use super::State;

impl State {
    /// Apply an [`OutlinerAction`] (mesh / material visibility or camera
    /// framing) raised by the Outliner panel.
    pub(super) fn handle_outliner_action(&mut self, action: OutlinerAction) {
        match action {
            OutlinerAction::ToggleMesh(i) => {
                if let Some(scene) = &mut self.scene
                    && let Some(mesh) = scene.model.meshes.get_mut(i)
                {
                    mesh.visible = !mesh.visible;
                }
            }
            OutlinerAction::HideMesh(i) => {
                if let Some(scene) = &mut self.scene
                    && let Some(mesh) = scene.model.meshes.get_mut(i)
                {
                    mesh.visible = false;
                }
            }
            OutlinerAction::IsolateMesh(i) => {
                if let Some(scene) = &mut self.scene {
                    for (j, mesh) in scene.model.meshes.iter_mut().enumerate() {
                        mesh.visible = j == i;
                    }
                }
            }
            OutlinerAction::ShowAll => {
                if let Some(scene) = &mut self.scene {
                    for mesh in &mut scene.model.meshes {
                        mesh.visible = true;
                    }
                }
            }
            OutlinerAction::ToggleMaterial(mat) => {
                if let Some(scene) = &mut self.scene {
                    let all_visible = scene
                        .model
                        .meshes
                        .iter()
                        .filter(|m| m.material == mat)
                        .all(|m| m.visible);
                    for mesh in &mut scene.model.meshes {
                        if mesh.material == mat {
                            mesh.visible = !all_visible;
                        }
                    }
                }
            }
            OutlinerAction::FrameMesh(i) => {
                let aabb = self
                    .scene
                    .as_ref()
                    .and_then(|s| s.model.mesh_bounds.get(i).copied());
                if let Some(aabb) = aabb {
                    self.frame_active_pane(aabb);
                }
            }
            OutlinerAction::FrameMaterial(mat) => {
                let aabb = self
                    .scene
                    .as_ref()
                    .and_then(|s| material_meshes_aabb(&s.model, mat));
                if let Some(aabb) = aabb {
                    self.frame_active_pane(aabb);
                }
            }
            OutlinerAction::FrameObject(id) => {
                // The object's own bounds are in its local space; the
                // transform is what places it in the world, and framing the
                // untransformed box would send the camera to the origin for
                // anything the scene has moved.
                let aabb = self
                    .raster
                    .scene()
                    .get(id)
                    .map(|o| o.model.bounds.transformed(&o.transform));
                if let Some(aabb) = aabb {
                    self.frame_active_pane(aabb);
                }
            }
            OutlinerAction::FrameObjectMesh(id, mesh) => {
                let aabb = self.raster.scene().get(id).and_then(|o| {
                    o.model
                        .mesh_bounds
                        .get(mesh)
                        .map(|b| b.transformed(&o.transform))
                });
                if let Some(aabb) = aabb {
                    self.frame_active_pane(aabb);
                }
            }
            OutlinerAction::ToggleObject(id) => self.toggle_scene_object(id),
        }
    }

    /// Flip a scene object's visibility through the engine.
    ///
    /// Writing the renderer's copy directly would look identical for one
    /// frame and then be undone: the scene delta re-emits every object's
    /// render flags from its owning node on each cook, so the parameter is
    /// the only durable place to put this. Routing it through the engine
    /// also puts the toggle in the undo stack for free.
    fn toggle_scene_object(&mut self, id: SceneObjectId) {
        let Some(visible) = self.raster.scene().get(id).map(|o| o.visible) else {
            return;
        };
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let command = solarxy_graph::Command::SetParam {
            ctx: GraphContext::Root,
            node: NodeId(id.0),
            key: "visible".to_string(),
            value: ParamSource::Literal(ParamValue::Bool(!visible)),
        };
        match engine.apply(command) {
            Ok(_) => {
                // Take the delta here rather than leaving it to the frame
                // loop. `visible` is a render flag, not a cook input, so the
                // next cook can legitimately produce nothing, and the frame
                // loop only drains a delta when something ticked or cooked.
                // A delta is a full rebuild from the document, so taking one
                // now is what carries the flag to the renderer; the upserts
                // it repeats are deduped by attribute identity.
                let delta = engine.take_scene_delta();
                if !delta.ops.is_empty() {
                    self.pending_scene_deltas.push(delta);
                }
            }
            Err(e) => tracing::warn!("Could not toggle object visibility: {e}"),
        }
    }

    /// `Shift+H` — hide the mesh under the cursor.
    pub(in crate::state) fn hide_hovered_mesh(&mut self) {
        if self.gui.any_popup_open() || self.viewport_context_menu.is_some() {
            return;
        }
        match self.hovered_mesh() {
            Some(mesh) => self.handle_outliner_action(OutlinerAction::HideMesh(mesh)),
            None => self
                .gui
                .set_toast("No mesh under cursor", ToastSeverity::Info),
        }
    }

    /// `Alt+H` — make every mesh visible again.
    pub(in crate::state) fn show_all_meshes(&mut self) {
        if self.gui.any_popup_open() || self.viewport_context_menu.is_some() {
            return;
        }
        self.handle_outliner_action(OutlinerAction::ShowAll);
    }

    /// `/` — hide every mesh except the one under the cursor.
    pub(in crate::state) fn isolate_hovered_mesh(&mut self) {
        if self.gui.any_popup_open() || self.viewport_context_menu.is_some() {
            return;
        }
        match self.hovered_mesh() {
            Some(mesh) => self.handle_outliner_action(OutlinerAction::IsolateMesh(mesh)),
            None => self
                .gui
                .set_toast("No mesh under cursor", ToastSeverity::Info),
        }
    }
}
