//! The drain: one ordered application of everything an interface pass raised.
//!
//! The queue itself, and the two rules that govern raising into it, are in
//! `gui::intent`. What lives here is the other half of the contract: every
//! variant has an arm, the match is exhaustive with no catch-all, and so an
//! intent nothing applies is a build failure rather than a click that quietly
//! does nothing.

use solarxy_core::scene::SceneObjectId;

use super::State;
use crate::gui::{Intent, Intents, LookThroughChange, PanelIntent};

impl State {
    /// Apply everything the interface pass raised, in `Intent::order`.
    ///
    /// Called after the pass rather than during it, which is the whole point,
    /// and after the settings write-back rather than before it, which is not
    /// arbitrary: two of these arms write per-pane display settings that the
    /// write-back would overwrite from a snapshot taken at the top of the
    /// frame. `clear_hdri` returns every HDRI-sky pane to the gradient, and
    /// flying to a validation issue switches the active pane's validation
    /// overlay on. Run either first and the write-back silently undoes half
    /// of it.
    pub(in crate::state) fn drain_intents(&mut self, intents: &mut Intents) {
        for intent in intents.take_ordered() {
            match intent {
                Intent::Projection { pane, mode } => {
                    // A projection picked on a bound pane takes the view over;
                    // without the release, the per-frame follow would silently
                    // revert the choice on the next frame.
                    self.release_look_through_pane(pane);
                    if let Some(Some(cam)) = self.view.cameras.get_mut(pane) {
                        cam.set_projection(mode);
                    }
                }
                Intent::LookThrough { pane, change } => {
                    // The pose lands on the next frame's follow, one frame
                    // after the click.
                    if let Some(slot) = self.look_through.get_mut(pane) {
                        *slot = match change {
                            LookThroughChange::Bind(id) => Some(SceneObjectId(id)),
                            LookThroughChange::Free => None,
                        };
                    }
                }
                Intent::Panel(PanelIntent::FlyToIssue(idx)) => {
                    self.fly_to_validation_issue(idx);
                }
                Intent::Panel(PanelIntent::ClearHdri) => self.clear_hdri(),
                Intent::Panel(PanelIntent::LoadHdri) => self.open_hdri_dialog(),
                Intent::Panel(PanelIntent::Outliner(action)) => {
                    self.handle_outliner_action(action);
                }
                Intent::Panel(PanelIntent::NodeTree(action)) => {
                    self.handle_node_tree_action(action);
                }
            }
        }
    }
}
