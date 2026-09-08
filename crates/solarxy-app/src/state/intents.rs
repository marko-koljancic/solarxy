//! The drain: one ordered application of everything an interface pass raised.
//!
//! The queue itself, and the two rules that govern raising into it, are in
//! `gui::intent`. What lives here is the other half of the contract: every
//! variant has an arm, the match is exhaustive with no catch-all, and so an
//! intent nothing applies is a build failure rather than a click that quietly
//! does nothing.

use solarxy_core::scene::SceneObjectId;

use super::State;
use crate::gui::{
    CaptureIntent, EditIntent, FileIntent, HelpIntent, Intent, Intents, LayoutIntent,
    LookThroughChange, PanelIntent, ReviewIntent, ToastSeverity,
};

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
                Intent::PaneProjection { pane, mode } => {
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
                Intent::Projection(mode) => {
                    // The View menu's projection follows the camera link
                    // rather than naming a pane, which is why it is not the
                    // toolbar's arm with a different argument.
                    self.for_each_target_cam(|cam| cam.set_projection(mode));
                }
                Intent::File(intent) => self.apply_file_intent(intent),
                Intent::Edit(EditIntent::OpenPreferences) => {
                    self.gui.open_preferences(self.preferences.clone());
                }
                Intent::Edit(EditIntent::SaveViewDefaults) => self.save_preferences(),
                Intent::Capture(CaptureIntent::Screenshot) => {
                    self.capture_requested = true;
                    self.screenshot_expand_review = false;
                }
                Intent::Capture(CaptureIntent::Still) => self.open_still_dialog(),
                Intent::Review(intent) => self.apply_review_intent(intent),
                Intent::Layout(intent) => self.apply_layout_intent(intent),
                Intent::Help(intent) => self.apply_help_intent(intent),
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

impl State {
    fn apply_file_intent(&mut self, intent: FileIntent) {
        match intent {
            FileIntent::OpenModel => self.open_model_dialog(),
            FileIntent::OpenHdri => self.open_hdri_dialog(),
            // Through the router rather than the model loader: the one list
            // holds scenes and models, and the routing on extension exists
            // once.
            FileIntent::OpenRecent(path) => self.open_file(std::path::PathBuf::from(path)),
            FileIntent::Close => self.close_document(),
            FileIntent::Quit => self.quit_requested = true,
        }
    }

    fn apply_review_intent(&mut self, intent: ReviewIntent) {
        match intent {
            ReviewIntent::ToggleMode => self.toggle_review_mode(),
            ReviewIntent::ToggleMarkers => {
                self.review.markers_hidden = !self.review.markers_hidden;
            }
            ReviewIntent::SaveNotes => self.save_review_sidecar(),
            // The state was already written inside the pass, by the widget
            // that owns it. What is left is the toast, which is the shell's
            // to give rather than a panel's.
            ReviewIntent::Exited => {
                self.gui
                    .set_toast("Review mode: Off", ToastSeverity::Success);
            }
            ReviewIntent::ReanchorCancelled => {
                self.gui
                    .set_toast("Re-anchor cancelled", ToastSeverity::Info);
            }
        }
    }

    fn apply_layout_intent(&mut self, intent: LayoutIntent) {
        match intent {
            LayoutIntent::ToggleTab(tab) => self.gui.toggle_tab(tab),
            LayoutIntent::ToggleMenuBar => {
                self.gui.menu_bar_visible = !self.gui.menu_bar_visible;
            }
            LayoutIntent::ToggleStatusBar => {
                self.gui.status_bar_visible = !self.gui.status_bar_visible;
            }
            LayoutIntent::SetLayout(layout) => self.set_view_layout(layout),
            LayoutIntent::SetSplitRatio(ratio) => {
                self.view.display.split_ratio =
                    solarxy_core::view_config::DisplaySettings::clamp_split_ratio(ratio);
            }
            LayoutIntent::SaveDock => self.save_dock_layout(),
            LayoutIntent::RestoreDock => {
                if let Some(json) = self.preferences.dock.saved_layout_json.clone()
                    && self.gui.apply_layout_json(&json)
                {
                    self.gui.set_toast("Layout restored.", ToastSeverity::Info);
                } else {
                    self.gui
                        .set_toast("No valid saved layout to restore.", ToastSeverity::Warning);
                }
            }
            LayoutIntent::ResetDock => {
                self.gui.reset_dock_layout();
                self.gui
                    .set_toast("Layout reset to default.", ToastSeverity::Info);
            }
        }
    }

    /// Serialize the arrangement into preferences and persist it silently, so
    /// the click yields one layout-specific toast rather than a generic
    /// "Preferences saved" stacked on top of it.
    fn save_dock_layout(&mut self) {
        let Some(json) = self.gui.serialize_layout() else {
            self.gui
                .set_toast("Failed to save layout.", ToastSeverity::Warning);
            return;
        };
        self.preferences.dock.saved_layout_json = Some(json);
        self.gui.set_has_saved_layout(true);
        match self.persist_preferences() {
            Ok(()) => self.gui.set_toast("Layout saved.", ToastSeverity::Success),
            Err(e) => self
                .gui
                .set_toast(&format!("Save failed: {e}"), ToastSeverity::Error),
        }
    }

    fn apply_help_intent(&mut self, intent: HelpIntent) {
        match intent {
            HelpIntent::OpenWiki => {
                if let Err(e) = open::that(solarxy_core::WIKI_URL) {
                    tracing::warn!("Failed to open wiki URL: {e}");
                }
            }
            HelpIntent::OpenShortcuts => self.gui.open_shortcuts_modal(),
            HelpIntent::CheckForUpdates => self.gui.check_for_updates(),
            HelpIntent::OpenAbout => self.gui.open_about(),
        }
    }
}
