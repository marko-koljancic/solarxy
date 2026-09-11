//! What a scene object's actions do: the Outliner's, the viewport right-click
//! menu's, and the keyboard shortcuts that raise the same ones.
//!
//! **Hiding is a parameter change, never a direct write.** An object's
//! visibility belongs to the engine and is re-emitted from its owning node on
//! every cook, so writing the renderer's copy would look right for one frame
//! and be undone by the user's next edit. The shell wrote directly until
//! 0.10.0, because a file model's meshes were the shell's own; with one root
//! there is nothing the shell owns to write.

use crate::gui::ViewportAction;
use solarxy_core::scene::SceneObjectId;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};

use super::State;

/// One thing the viewport context menu can ask of a scene object. These
/// were the Outliner's actions until that panel was replaced by the Tree;
/// the menu raised them through the Outliner's handler and still does
/// through this one.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ObjectAction {
    /// Frame the active pane on the object's world bounds.
    FrameObject(solarxy_core::scene::SceneObjectId),
    /// Flip the object's visibility through its node's parameter.
    ToggleObject(solarxy_core::scene::SceneObjectId),
}

impl State {
    /// Apply a [`ViewportAction`] raised by the right-click menu.
    ///
    /// Every arm acts on the object the pointer landed on, which the menu
    /// carried through rather than looking up again: a pick is a raycast, and
    /// re-running it against a cursor that has since moved would act on
    /// something else.
    pub(super) fn handle_viewport_action(&mut self, action: ViewportAction) {
        match action {
            ViewportAction::FrameView => {
                let bounds = self.scene_bounds();
                let pane = self.view.active_pane;
                self.release_look_through_pane(pane);
                if let Some(Some(cam)) = self.view.cameras.get_mut(pane) {
                    cam.reset_to_bounds(&bounds);
                }
            }
            ViewportAction::FrameObject(id) => {
                self.handle_object_action(ObjectAction::FrameObject(id));
            }
            ViewportAction::ToggleVisible(id) => {
                self.handle_object_action(ObjectAction::ToggleObject(id));
            }
            ViewportAction::Duplicate(id) => {
                self.apply_node_command(solarxy_graph::Command::DuplicateNodes {
                    ctx: GraphContext::Root,
                    ids: vec![NodeId(id.0)],
                });
            }
            ViewportAction::Delete(id) => {
                self.apply_node_command(solarxy_graph::Command::RemoveNodes {
                    ctx: GraphContext::Root,
                    ids: vec![NodeId(id.0)],
                });
            }
            ViewportAction::ResetTransform(id) => self.reset_transform(NodeId(id.0)),
        }
    }

    /// Put every transform parameter a node declares back to its default.
    ///
    /// One command rather than one per parameter, so a user who regrets it
    /// gets one undo: the reset writes several values and wanting them back
    /// individually is not a thing anyone wants.
    ///
    /// Which parameters those are is the registry's answer through the
    /// engine, so a node type that renames its position, or a light that has
    /// one and no rotation, is handled without a list here.
    fn reset_transform(&mut self, node: NodeId) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let Some(params) = engine.transform_params(GraphContext::Root, node) else {
            return;
        };
        let keys: Vec<String> = params.names().into_iter().map(str::to_owned).collect();
        if keys.is_empty() {
            return;
        }
        self.apply_node_command(solarxy_graph::Command::ResetParams {
            ctx: GraphContext::Root,
            node,
            keys: Some(keys),
        });
    }

    /// Dispatch one document command and report a failure rather than
    /// swallowing it.
    ///
    /// The delta is left to the frame loop, unlike the visibility toggle
    /// below: these change what the cook produces rather than only a render
    /// flag, so there is nothing useful to push before the cook runs.
    pub(super) fn apply_node_command(&mut self, command: solarxy_graph::Command) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if let Err(err) = engine.apply(command) {
            self.gui
                .set_toast(&format!("{err}"), crate::gui::ToastSeverity::Error);
        }
    }

    /// Apply an [`ObjectAction`] raised by the viewport context menu.
    pub(super) fn handle_object_action(&mut self, action: ObjectAction) {
        match action {
            ObjectAction::FrameObject(id) => {
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
            ObjectAction::ToggleObject(id) => self.toggle_scene_object(id),
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
