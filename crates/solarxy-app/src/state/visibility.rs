//! What is drawn and what is hidden: the Outliner's actions, and the keyboard
//! shortcuts that raise the same ones.
//!
//! **Hiding is a parameter change, never a direct write.** An object's
//! visibility belongs to the engine and is re-emitted from its owning node on
//! every cook, so writing the renderer's copy would look right for one frame
//! and be undone by the user's next edit. The shell wrote directly until
//! 0.10.0, because a file model's meshes were the shell's own; with one root
//! there is nothing the shell owns to write.

use crate::gui::OutlinerAction;
use solarxy_core::scene::SceneObjectId;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};

use super::State;

impl State {
    /// Apply an [`OutlinerAction`] (mesh / material visibility or camera
    /// framing) raised by the Outliner panel.
    pub(super) fn handle_outliner_action(&mut self, action: OutlinerAction) {
        match action {
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
}
