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
    CaptureIntent, DisplayChange, EditIntent, FileIntent, HelpIntent, Intent, Intents,
    LayoutIntent, LookThroughChange, PaneChange, PanelIntent, PostChange, ReviewIntent,
    ToastSeverity,
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
        let mut recompute = Recompute::default();
        for intent in intents.take_ordered() {
            recompute.mark(&intent);
            match intent {
                Intent::Pane { pane, change } => {
                    if let Some(pds) = self.view.pane_settings.get_mut(pane) {
                        apply_pane_change(pds, change);
                    }
                }
                Intent::Display(change) => apply_display_change(&mut self.view.display, change),
                Intent::Post(change) => apply_post_change(&mut self.renderer.post, change),
                Intent::Ibl(mode) => self.renderer.ibl_res.ibl_mode = mode,
                Intent::LinkCameras(linked) => self.view.cameras_linked = linked,
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
        self.apply_recompute(recompute);
    }

    /// Push whatever the settings above made stale to the GPU, once, in the
    /// order the mirror's diff used.
    ///
    /// Applied after the loop rather than inside it, which is safe for a
    /// reason worth stating: every intent here is raised by a click, so two of
    /// them in one frame needs two clicks in one frame. The mirror this
    /// replaces wrote every field every frame whether anything moved or not,
    /// which is what made its ordering against the other arms load-bearing.
    fn apply_recompute(&mut self, recompute: Recompute) {
        if recompute.background {
            self.apply_background_change();
        }
        if recompute.wireframe_only() {
            self.update_wireframe_params();
        }
        if recompute.composite {
            self.apply_composite_params();
        }
        if recompute.ibl {
            self.apply_ibl_change();
        }
    }
}

fn apply_display_change(
    display: &mut crate::state::view_state::DisplaySettings,
    change: DisplayChange,
) {
    match change {
        DisplayChange::TurntableActive(v) => display.turntable_active = v,
        DisplayChange::TurntableRpm(v) => display.turntable_rpm = v,
        DisplayChange::LightsLocked(v) => display.lights_locked = v,
        DisplayChange::RoughnessScale(v) => display.roughness_scale = v,
        DisplayChange::MetallicScale(v) => display.metallic_scale = v,
        DisplayChange::HdriRotation(v) => display.hdri_rotation = v,
        DisplayChange::HdriIntensity(v) => display.hdri_intensity = v,
    }
}

/// What the settings intents made stale, gathered across one drain and applied
/// once at the end of it.
///
/// The rule lives here rather than in the arms so that it is one decision a
/// test can make without a device, which is what the retired mirror's diff
/// tests were testing.
#[derive(Debug, Default, Clone, Copy)]
struct Recompute {
    background: bool,
    wireframe: bool,
    composite: bool,
    ibl: bool,
}

impl Recompute {
    /// Record what one intent makes stale.
    ///
    /// Exhaustive rather than wildcarded, so a setting added later has to say
    /// whether anything has to be pushed for it.
    ///
    /// The mirror this replaces asked the same question by comparing floats,
    /// and carried a comment about why the comparison had to be exact: a
    /// tolerance would have swallowed the smallest drag a slider can produce.
    /// That question is gone rather than answered, because a widget reports
    /// whether it moved and no longer has to be asked whether its value
    /// differs.
    fn mark(&mut self, intent: &Intent) {
        match intent {
            Intent::Pane { change, .. } => match change {
                PaneChange::BackgroundMode(_) => self.background = true,
                PaneChange::LineWeight(_) => self.wireframe = true,
                // The rest are read by the per-pane draw every frame, so
                // writing them is the whole of the work.
                PaneChange::PaneMode(_)
                | PaneChange::ViewMode(_)
                | PaneChange::InspectionMode(_)
                | PaneChange::MaterialOverride(_)
                | PaneChange::NormalsMode(_)
                | PaneChange::UvMode(_)
                | PaneChange::BoundsMode(_)
                | PaneChange::ShowGrid(_)
                | PaneChange::ShowAxisGizmo(_)
                | PaneChange::ShowLocalAxes(_)
                | PaneChange::ShowValidation(_)
                | PaneChange::UvBackground(_)
                | PaneChange::ShowUvOverlap(_) => {}
            },
            // Intensity joins the mode below because both are IBL-derived
            // uniforms that reach the GPU only through the lighting
            // chokepoint. Rotation does not: it rides the per-pane camera
            // uniform, which is written every frame anyway.
            Intent::Display(DisplayChange::HdriIntensity(_)) | Intent::Ibl(_) => self.ibl = true,
            Intent::Post(_) => self.composite = true,
            // The remaining scene-global settings are read where they are used
            // and need nothing pushed, and everything below them is an action
            // rather than a setting, pushing whatever it needs itself.
            Intent::Display(
                DisplayChange::TurntableActive(_)
                | DisplayChange::TurntableRpm(_)
                | DisplayChange::LightsLocked(_)
                | DisplayChange::RoughnessScale(_)
                | DisplayChange::MetallicScale(_)
                | DisplayChange::HdriRotation(_),
            )
            | Intent::LinkCameras(_)
            | Intent::PaneProjection { .. }
            | Intent::LookThrough { .. }
            | Intent::Projection(_)
            | Intent::File(_)
            | Intent::Edit(_)
            | Intent::Capture(_)
            | Intent::Review(_)
            | Intent::Layout(_)
            | Intent::Help(_)
            | Intent::Panel(_) => {}
        }
    }

    /// Whether the wireframe parameters need their own upload.
    ///
    /// A background change rebuilds them on its way through, so asking for
    /// both would do the second piece of work twice.
    fn wireframe_only(self) -> bool {
        self.wireframe && !self.background
    }
}

fn apply_pane_change(pds: &mut crate::state::view_state::PaneDisplaySettings, change: PaneChange) {
    match change {
        PaneChange::PaneMode(v) => pds.pane_mode = v,
        PaneChange::ViewMode(v) => pds.view_mode = v,
        PaneChange::InspectionMode(v) => pds.inspection_mode = v,
        PaneChange::MaterialOverride(v) => pds.material_override = v,
        PaneChange::BackgroundMode(v) => pds.background_mode = v,
        PaneChange::NormalsMode(v) => pds.normals_mode = v,
        PaneChange::UvMode(v) => pds.uv_mode = v,
        PaneChange::BoundsMode(v) => pds.bounds_mode = v,
        PaneChange::LineWeight(v) => pds.line_weight = v,
        PaneChange::ShowGrid(v) => pds.show_grid = v,
        PaneChange::ShowAxisGizmo(v) => pds.show_axis_gizmo = v,
        PaneChange::ShowLocalAxes(v) => pds.show_local_axes = v,
        PaneChange::ShowValidation(v) => pds.show_validation = v,
        PaneChange::UvBackground(v) => pds.uv_bg = v,
        PaneChange::ShowUvOverlap(v) => pds.show_uv_overlap = v,
    }
}

/// The post settings reach two passes through one setter that clamps, so
/// every arm here writes through the renderer's own accessors rather than its
/// fields.
fn apply_post_change(post: &mut solarxy_renderer::frame::PostProcessing, change: PostChange) {
    match change {
        PostChange::Bloom(v) => post.bloom_enabled = v,
        PostChange::Ssao(v) => post.ssao_enabled = v,
        PostChange::Strengths(v) => post.set_strengths(v),
        PostChange::ToneMode(v) => post.tone_mode = v,
        PostChange::Exposure(v) => post.exposure = v,
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

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::preferences::{BackgroundMode, IblMode, LineWeight, ToneMode};
    use solarxy_core::view_config::PostStrengths;

    /// The five tests below are the retired mirror's diff tests, re-expressed
    /// against the rule that replaced it. They ask the same question: which
    /// expensive rebuild does this change make necessary.
    fn marked(intents: &[Intent]) -> Recompute {
        let mut recompute = Recompute::default();
        for intent in intents {
            recompute.mark(intent);
        }
        recompute
    }

    #[test]
    fn nothing_raised_makes_nothing_stale() {
        let r = marked(&[]);
        assert!(!r.background);
        assert!(!r.wireframe);
        assert!(!r.composite);
        assert!(!r.ibl);
    }

    #[test]
    fn a_bloom_toggle_marks_the_composite() {
        let r = marked(&[Intent::Post(PostChange::Bloom(true))]);
        assert!(r.composite);
        assert!(!r.background);
        assert!(!r.ibl);
    }

    #[test]
    fn every_post_setting_marks_the_composite() {
        // One case each: they reach the uniform by different routes, the
        // composite writing two of them and the bloom pass the third, so a
        // rule that noticed only one would still look like it worked.
        for change in [
            PostChange::Bloom(true),
            PostChange::Ssao(true),
            PostChange::Strengths(PostStrengths::default()),
            PostChange::ToneMode(ToneMode::Reinhard),
            PostChange::Exposure(1.4),
        ] {
            let r = marked(&[Intent::Post(change)]);
            assert!(r.composite, "{change:?} must mark the composite");
            assert!(!r.background);
            assert!(!r.ibl);
        }
    }

    #[test]
    fn the_lighting_is_marked_by_the_mode_and_by_the_intensity() {
        for intent in [
            Intent::Ibl(IblMode::Off),
            Intent::Display(DisplayChange::HdriIntensity(2.0)),
        ] {
            let r = marked(std::slice::from_ref(&intent));
            assert!(r.ibl, "{intent:?} must mark the lighting");
            assert!(!r.composite);
        }
    }

    /// Rotation is the case that must **not** mark it: it rides the per-pane
    /// camera uniform, which is written every frame regardless.
    #[test]
    fn an_hdri_rotation_marks_nothing() {
        let r = marked(&[Intent::Display(DisplayChange::HdriRotation(0.5))]);
        assert!(!r.ibl);
        assert!(!r.composite);
    }

    #[test]
    fn a_background_change_suppresses_the_wireframe_rebuild() {
        let r = marked(&[
            Intent::Pane {
                pane: 0,
                change: PaneChange::BackgroundMode(BackgroundMode::WHITE),
            },
            Intent::Pane {
                pane: 0,
                change: PaneChange::LineWeight(LineWeight::Bold),
            },
        ]);
        assert!(r.background);
        assert!(r.wireframe, "the raise is still recorded");
        assert!(
            !r.wireframe_only(),
            "a background rebuild covers the wireframe parameters, so asking \
             for both would do the second piece of work twice"
        );
    }

    #[test]
    fn a_line_weight_change_alone_rebuilds_the_wireframe() {
        let r = marked(&[Intent::Pane {
            pane: 0,
            change: PaneChange::LineWeight(LineWeight::Light),
        }]);
        assert!(r.wireframe_only());
        assert!(!r.background);
    }
}
