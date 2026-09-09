//! The drain: one ordered application of everything an interface pass raised.
//!
//! The queue itself, and the two rules that govern raising into it, are in
//! `gui::intent`. What lives here is the other half of the contract: every
//! variant has an arm, the match is exhaustive with no catch-all, and so an
//! intent nothing applies is a build failure rather than a click that quietly
//! does nothing.

use solarxy_core::preferences::BackgroundMode;
use solarxy_graph::document::GraphContext;
use solarxy_core::scene::SceneObjectId;
use solarxy_renderer::ibl::IblState;

use super::BackgroundModeExt;

use super::State;
use crate::gui::{
    CaptureIntent, CookIntent, DisplayChange, EditIntent, FileIntent, HelpIntent, Intent, Intents,
    LayoutIntent, LookThroughChange, NodeTreeAction, PaneChange, PanelIntent, PaneView, PostChange,
    ReviewIntent, ToastSeverity,
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
                Intent::PaneView { pane, view } => {
                    // Bounds first: the borrow it takes is immutable and the
                    // camera write below is not, so reading it inside the
                    // write would not compile.
                    let bounds = self.scene_bounds();
                    // Framing a bound pane takes the view over, for the same
                    // reason a projection pick does: the per-frame follow
                    // would otherwise revert it on the next frame.
                    self.release_look_through_pane(pane);
                    if let Some(Some(cam)) = self.view.cameras.get_mut(pane) {
                        match view {
                            PaneView::Fit => cam.reset_to_bounds(&bounds),
                            PaneView::Axis(axis) => {
                                solarxy_host::cameras::reset_to_view(cam, &bounds, axis);
                            }
                        }
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
                        // Picked from the cameras that exist, so there is
                        // nothing left to check, whatever an open left flagged.
                        self.unresolved_binding[pane] = false;
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
                Intent::Capture(CaptureIntent::Still) => {
                    // From the menu, the still renders the document's one
                    // render node; a target left by a node's own action
                    // must not outlive that press.
                    self.still_target = None;
                    self.open_still_dialog();
                }
                Intent::Cook(CookIntent::SetMode(mode)) => self.set_cook_mode(mode),
                Intent::Cook(CookIntent::CookNow) => self.cook_now(),
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
                Intent::Panel(PanelIntent::InvokeAction { ctx, node, key }) => {
                    self.invoke_action(ctx, node, &key);
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
                | PaneChange::ShowUvOverlap(_)
                | PaneChange::TurntableActive(_) => {}
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
            | Intent::PaneView { .. }
            | Intent::LookThrough { .. }
            | Intent::Projection(_)
            | Intent::File(_)
            | Intent::Edit(_)
            | Intent::Capture(_)
            | Intent::Cook(_)
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
        PaneChange::TurntableActive(v) => pds.turntable_active = v,
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

impl State {
    /// Regenerate the scene-global gradient IBL from the **active pane's**
    /// background. The viewer keeps one IBL but a background per pane, so
    /// the active pane — the one being worked in — drives the lighting.
    /// Switching the active pane does *not* relight (regenerating the IBL
    /// as the cursor crossed panes would flicker); only changing the
    /// active pane's background does. Changing a non-active pane's
    /// background via its toolbar updates that pane's backdrop but leaves
    /// the IBL until that pane is made active and edited.
    pub(super) fn apply_background_change(&mut self) {
        let bg = self.view.pane_settings[self.view.active_pane].background_mode;
        // Once an HDRI is loaded it is the scene's light source — a
        // background change never regenerates IBL from sky colours while
        // an HDRI is active (that would discard the equirect the skybox
        // pass needs). The background mode then only drives the backdrop.
        if bg.is_hdri_sky() || self.renderer.ibl_res.ibl.equirect.is_some() {
            return;
        }
        let (top, bottom) = bg
            .resolve(&self.preferences.view.custom_backgrounds)
            .sky_colors();
        self.renderer.ibl_res.ibl =
            IblState::from_sky_colors(&self.device, &self.queue, top, bottom);
        self.environment.invalidate();
        self.rebuild_light_bind_group();
    }

    pub(super) fn apply_composite_params(&self) {
        self.write_composite_params();
    }

    pub(super) fn apply_ibl_change(&mut self) {
        self.rebuild_light_bind_group();
    }

    /// Toggle review mode (`Shift+R` or the Review menu) — flips the bit,
    /// opens the panel on entry, and emits the matching toast.
    ///
    /// **Refused outright for this release.** Review anchors against a
    /// file-loaded model's meshes, and the second root that held one went away
    /// with the one-document-root change; repointing it at the engine's own
    /// review store is its own piece of work. Arming a mode that would report
    /// itself active and then discard every click in silence is the one thing
    /// worse than not offering it. Turning an already-active mode off stays
    /// allowed, so a stale bit can never wedge the shell.
    pub(super) fn toggle_review_mode(&mut self) {
        if !self.review.active {
            self.gui.set_toast(
                "Review is unavailable until it reads the document's own annotations",
                ToastSeverity::Warning,
            );
            return;
        }
        let now_active = self.review.toggle_active();
        if now_active {
            self.review.panel_open = true;
        }
        let msg = if now_active {
            "Review mode: On (click a face to annotate)"
        } else {
            "Review mode: Off"
        };
        self.gui.set_toast(msg, ToastSeverity::Success);
    }

    /// Drop the loaded HDRI (Properties → HDRI → Clear). Full revert:
    /// every pane still on the `HdriSky` background falls back to
    /// `Gradient`, the IBL returns to the procedural sky-colour gradient,
    /// and the skybox is released (`rebuild_light_bind_group` rebuilds it
    /// as `None`).
    pub(super) fn clear_hdri(&mut self) {
        for pds in &mut self.view.pane_settings {
            if pds.background_mode.is_hdri_sky() {
                pds.background_mode = BackgroundMode::GRADIENT;
            }
        }
        let (top, bottom) = self
            .resolve_background(&self.view.pane_settings[0])
            .sky_colors();
        self.renderer.ibl_res.ibl =
            IblState::from_sky_colors(&self.device, &self.queue, top, bottom);
        // The IBL just moved without the scene contract knowing, so forget
        // what the tracker thinks is installed. Otherwise re-selecting the
        // same HDRI through an environment node would match the stale hash
        // and be skipped, leaving the procedural sky in place.
        self.environment.invalidate();
        self.rebuild_light_bind_group();
        self.gui.clear_hdri_info();
        self.gui.set_toast("HDRI cleared", ToastSeverity::Success);
    }

    /// Apply a Node Tree row click: select the node engine-side, and
    /// outline its object if it has one.
    ///
    /// **Only a root-context selection can outline anything.** The scene
    /// delta names a geo container's object `SceneObjectId(geo.0)`, so a
    /// root node id maps straight onto one; a node inside a container owns
    /// no object of its own. Selecting one still selects it in the engine
    /// and still highlights the row, it just leaves the viewport alone —
    /// the same behaviour the web shell has for the same gesture.
    ///
    /// Unlike [`Self::toggle_scene_object`], no delta is taken: selection
    /// is neither a render flag nor a cook input, and the engine emits no
    /// scene ops for it.
    pub(super) fn handle_node_tree_action(&mut self, action: NodeTreeAction) {
        let NodeTreeAction::Select(ctx, node) = action;
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if let Err(e) = engine.apply(solarxy_graph::Command::SetSelection {
            ctx,
            ids: vec![node],
        }) {
            tracing::warn!("Could not select node: {e}");
            return;
        }
        self.selected_object = match ctx {
            GraphContext::Root => {
                let id = SceneObjectId(node.0);
                // Absent or hidden objects are filtered out of the draw
                // list entirely, so pointing at one would outline nothing
                // while claiming a selection is showing.
                self.raster
                    .scene()
                    .get(id)
                    .filter(|o| o.visible)
                    .map(|_| id)
            }
            GraphContext::Subflow(_) => None,
        };
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

    /// Every [`PaneChange`] writes the one field it names, and no other.
    ///
    /// The failure this catches is a transposed arm, which nothing else can
    /// see: every arm has the same shape and several share a type, so a
    /// `TurntableActive` writing `show_validation` compiles, passes the
    /// staleness tests above, and shows up only as a menu entry doing
    /// somebody else's job.
    ///
    /// Each case both asserts the change moved something and, after putting
    /// the named field back, that nothing else moved. An arm that writes
    /// nothing at all fails the first half rather than passing vacuously.
    #[test]
    fn every_pane_change_writes_only_the_field_it_names() {
        use solarxy_core::preferences::{
            InspectionMode, MaterialOverride, NormalsMode, PaneMode, UvMapBackground, UvMode,
            ViewMode,
        };
        use solarxy_core::view_config::PaneDisplaySettings;

        type Restore = fn(&mut PaneDisplaySettings, &PaneDisplaySettings);
        let base = PaneDisplaySettings::for_still(BackgroundMode::GRADIENT);
        let cases: &[(PaneChange, Restore)] = &[
            (PaneChange::PaneMode(PaneMode::UvMap), |p, b| {
                p.pane_mode = b.pane_mode;
            }),
            (PaneChange::ViewMode(ViewMode::WireframeOnly), |p, b| {
                p.view_mode = b.view_mode;
            }),
            (PaneChange::InspectionMode(InspectionMode::Depth), |p, b| {
                p.inspection_mode = b.inspection_mode;
            }),
            (
                PaneChange::MaterialOverride(MaterialOverride::Chrome),
                |p, b| {
                    p.material_override = b.material_override;
                },
            ),
            (
                PaneChange::BackgroundMode(BackgroundMode::Builtin(
                    solarxy_core::preferences::BuiltinBg::Black,
                )),
                |p, b| {
                    p.background_mode = b.background_mode;
                },
            ),
            (PaneChange::NormalsMode(NormalsMode::Face), |p, b| {
                p.normals_mode = b.normals_mode;
            }),
            (PaneChange::UvMode(UvMode::Checker), |p, b| {
                p.uv_mode = b.uv_mode;
            }),
            (
                PaneChange::BoundsMode(solarxy_core::view_config::BoundsMode::WholeModel),
                |p, b| {
                    p.bounds_mode = b.bounds_mode;
                },
            ),
            (PaneChange::LineWeight(LineWeight::Bold), |p, b| {
                p.line_weight = b.line_weight;
            }),
            (PaneChange::ShowGrid(true), |p, b| {
                p.show_grid = b.show_grid;
            }),
            (PaneChange::ShowAxisGizmo(true), |p, b| {
                p.show_axis_gizmo = b.show_axis_gizmo;
            }),
            (PaneChange::ShowLocalAxes(true), |p, b| {
                p.show_local_axes = b.show_local_axes;
            }),
            (PaneChange::ShowValidation(true), |p, b| {
                p.show_validation = b.show_validation;
            }),
            (
                PaneChange::UvBackground(UvMapBackground::Checker),
                |p, b| {
                    p.uv_bg = b.uv_bg;
                },
            ),
            (PaneChange::ShowUvOverlap(true), |p, b| {
                p.show_uv_overlap = b.show_uv_overlap;
            }),
            (PaneChange::TurntableActive(true), |p, b| {
                p.turntable_active = b.turntable_active;
            }),
        ];

        for (change, restore) in cases {
            let mut pds = base;
            apply_pane_change(&mut pds, *change);
            assert_ne!(pds, base, "{change:?} wrote nothing");
            restore(&mut pds, &base);
            assert_eq!(pds, base, "{change:?} wrote a field it does not name");
        }
    }

    #[test]
    fn nothing_raised_makes_nothing_stale() {
        let r = marked(&[]);
        assert!(!r.background);
        assert!(!r.wireframe);
        assert!(!r.composite);
        assert!(!r.ibl);
    }

    /// A cook is an action the engine performs, and whatever it changes
    /// arrives through the scene delta rather than through a rebuild here.
    #[test]
    fn a_cook_intent_makes_nothing_stale() {
        for intent in [
            Intent::Cook(CookIntent::CookNow),
            Intent::Cook(CookIntent::SetMode(solarxy_graph::engine::CookMode::Manual)),
        ] {
            let r = marked(std::slice::from_ref(&intent));
            assert!(!r.background, "{intent:?}");
            assert!(!r.wireframe, "{intent:?}");
            assert!(!r.composite, "{intent:?}");
            assert!(!r.ibl, "{intent:?}");
        }
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
