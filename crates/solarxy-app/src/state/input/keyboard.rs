//! What a binding does, and the toggles it drives.
//!
//! The bindings themselves are declared in [`super::keymap`]; this module is
//! the other half, one arm per [`Action`]. The match is exhaustive, so a
//! binding added to the table does not compile until something here runs it,
//! which is the drift these two files replaced: the shell used to declare the
//! same fact in the window's claim list, in a key match, in the shortcuts
//! reference and in the menus, and the four disagreed.
//!
//! A key *release* is not a binding. The arrow keys nudge the camera while
//! held, so they are handled on press and release by [`State::handle_key`]
//! directly and never reach the table.

use winit::keyboard::KeyCode;

use crate::gui::ToastSeverity;
use solarxy_host::cameras::StandardView;
use solarxy_renderer::input::CameraKey;
use solarxy_core::preferences::{InspectionMode, PaneMode, ProjectionMode};

use super::keymap::{self, Action, Binding, Chord, Claim, KeyScope};
use crate::state::{CompositeLook, State, ViewLayout};

/// winit-to-renderer input mapping: the renderer is windowing-agnostic and
/// consumes its own [`CameraKey`] / [`PointerButton`] enums.
fn to_camera_key(code: KeyCode) -> Option<CameraKey> {
    match code {
        KeyCode::ArrowUp => Some(CameraKey::ArrowUp),
        KeyCode::ArrowDown => Some(CameraKey::ArrowDown),
        KeyCode::ArrowLeft => Some(CameraKey::ArrowLeft),
        KeyCode::ArrowRight => Some(CameraKey::ArrowRight),
        _ => None,
    }
}

/// Which scope a press resolves in, decided by where the pointer is.
///
/// The viewport first, then the node canvas, then everything else, which is
/// the model the browser's keymap describes: it is what lets a letter mean
/// one thing over a graph and another over a 3D view without either meaning
/// being modal.
pub(crate) fn key_scope(gui: &crate::gui::EguiRenderer) -> KeyScope {
    if gui.pointer_over_viewport() {
        KeyScope::Viewport
    } else if gui.pointer_over_canvas() {
        KeyScope::Canvas
    } else {
        KeyScope::Global
    }
}

/// Whether the window takes this press out from under the interface.
///
/// The interface still sees every event, because its key state is built from
/// press and release pairs and a release with no press is a no-op there. What
/// a claim buys is that the press does not *also* run the map's arm, which is
/// the collision this replaced.
pub(crate) fn window_claims(binding: &Binding, wants_text: bool) -> bool {
    match binding.claim {
        Claim::Always => true,
        Claim::UnlessTyping => !wants_text,
        Claim::Never | Claim::Panel => false,
    }
}

/// Whether the map runs this binding: a press the window never takes and no
/// panel owns.
///
/// A function rather than a comparison written at the one call site, because
/// `exactly_one_dispatcher_runs_each_binding` has to ask the same question
/// the dispatcher does. A test that re-states the rule instead of calling it
/// passes while the rule is broken.
pub(crate) fn map_runs(binding: &Binding) -> bool {
    !window_claims(binding, false) && binding.claim != Claim::Panel
}

impl State {
    pub fn set_modifiers(&mut self, modifiers: winit::keyboard::ModifiersState) {
        self.input.modifiers = modifiers;
    }

    /// A key press or release from the window, after the interface has had
    /// its say.
    ///
    /// Only bindings the window did not claim reach here, so this runs
    /// [`Claim::Never`] and nothing else; a claimed press already ran in the
    /// pre-pass and running it again would be the double dispatch the two
    /// dispatchers used to produce.
    pub fn handle_key(&mut self, code: KeyCode, is_pressed: bool) {
        // A held arrow nudges the camera and needs both edges, which is why
        // it is not in the table.
        if let Some(key) = to_camera_key(code) {
            if is_pressed {
                self.release_look_through_for_gesture();
            }
            self.for_each_target_cam(|cam| {
                cam.handle_key(key, is_pressed);
            });
            return;
        }
        if !is_pressed {
            return;
        }
        let chord = Chord::new(
            code,
            self.cmd_or_ctrl(),
            self.input.modifiers.shift_key(),
            self.input.modifiers.alt_key(),
        );
        if let Some(binding) = keymap::lookup(chord, key_scope(&self.gui))
            && map_runs(binding)
        {
            self.run_action(binding.action);
        }
    }

    /// The platform's command modifier, resolved once rather than at every
    /// arm that used to ask.
    pub(crate) fn cmd_or_ctrl(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.input.modifiers.super_key()
        } else {
            self.input.modifiers.control_key()
        }
    }

    /// What a binding does.
    ///
    /// Exhaustive with no catch-all, so a binding added to the table fails
    /// the build until it is wired. Every arm is one action: the modifier
    /// branching that used to live inside these bodies is now the difference
    /// between two table entries, which is what makes the reference able to
    /// name both.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn run_action(&mut self, action: Action) {
        match action {
            // File and edit, which the window claims.
            Action::NewScene => self.new_scene(),
            Action::OpenScene => self.open_model_dialog(),
            Action::Save => {
                self.save_document();
            }
            Action::SaveAs => {
                self.save_document_as();
            }
            Action::ShowShortcuts => self.gui.open_shortcuts_modal(),
            Action::Undo => self.undo(),
            Action::Redo | Action::RedoAlt => self.redo(),
            Action::Copy => self.copy_selection(),
            Action::Paste => self.paste_clipboard(),
            Action::Duplicate => self.duplicate_selection(),
            Action::CookNow => self.cook_now(),

            // Consumed by a panel during the interface pass, so no dispatcher
            // reaches this. They are declared as actions because the
            // reference lists them and the menus read their keys from here,
            // and they are grouped rather than spelled out one empty arm at
            // a time because they share one reason.
            Action::OpenPreferences
            | Action::OpenNodePalette
            | Action::Bypass
            | Action::DisplayFlag
            | Action::Rename
            | Action::NodeInfo
            | Action::CanvasGrid
            | Action::CanvasMinimap
            | Action::CanvasControls
            | Action::AutoLayout
            | Action::EdgeStyle
            | Action::CanvasFit
            | Action::PanelMaximize
            | Action::ReviewCancel => {}

            // Inspection.
            Action::InspectShaded => self.set_inspection(InspectionMode::Shaded, "Shaded"),
            Action::InspectMaterialId => {
                self.set_inspection(InspectionMode::MaterialId, "Material ID");
            }
            Action::ToggleUvPane => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                if pds.pane_mode == PaneMode::UvMap {
                    pds.pane_mode = PaneMode::Scene3D;
                    self.gui.set_toast("3D View", ToastSeverity::Success);
                } else {
                    pds.pane_mode = PaneMode::UvMap;
                    pds.uv_offset = [0.0, 0.0];
                    pds.uv_zoom = 1.0;
                    self.gui.set_toast("UV Map", ToastSeverity::Success);
                }
            }
            Action::InspectTexelDensity => {
                self.set_inspection(InspectionMode::TexelDensity, "Texel Density");
            }
            Action::InspectDepth => self.set_inspection(InspectionMode::Depth, "Depth"),
            Action::InspectOverdraw => self.set_inspection(InspectionMode::Overdraw, "Overdraw"),
            Action::InspectAoPreview => {
                self.set_inspection(InspectionMode::AoPreview, "AO Preview");
            }

            // Viewport and layout.
            Action::LayoutSingle => self.set_view_layout(ViewLayout::Single),
            Action::LayoutSplitVertical => self.set_view_layout(ViewLayout::SplitVertical),
            Action::LayoutSplitHorizontal => self.set_view_layout(ViewLayout::SplitHorizontal),
            Action::LayoutQuad => self.set_view_layout(ViewLayout::Quad),
            Action::LayoutThreeLeftBig => self.set_view_layout(ViewLayout::ThreeLeftBig),
            Action::FitView => {
                let bounds = self.scene_bounds();
                self.release_look_through_for_gesture();
                self.for_each_target_cam(|cam| cam.reset_to_bounds(&bounds));
            }
            Action::Screenshot => {
                self.capture_requested = true;
                self.screenshot_expand_review = false;
            }
            Action::ViewTop => self.frame_standard_view(StandardView::Top),
            Action::ViewFront => self.frame_standard_view(StandardView::Front),
            Action::ViewLeft => self.frame_standard_view(StandardView::Left),
            Action::ViewBottom => self.frame_standard_view(StandardView::Bottom),
            Action::ProjectionPerspective => {
                self.release_look_through_for_gesture();
                self.for_each_target_cam(|cam| cam.set_projection(ProjectionMode::Perspective));
            }
            Action::ProjectionOrthographic => {
                if !self.toggle_uv_overlap_in_a_uv_pane() {
                    self.release_look_through_for_gesture();
                    self.for_each_target_cam(|cam| {
                        cam.set_projection(ProjectionMode::Orthographic);
                    });
                }
            }

            // Review.
            Action::ToggleReviewMode => self.toggle_review_mode(),
            Action::ToggleReviewPanel => {
                self.gui.toggle_tab(crate::gui::SolarxyTab::ReviewPanel);
            }

            #[cfg(debug_assertions)]
            Action::DevObjects => self.toggle_dev_objects(),
            #[cfg(debug_assertions)]
            Action::DevEnvironment => self.toggle_dev_environment(),
        }
    }

    fn frame_standard_view(&mut self, view: StandardView) {
        let bounds = self.scene_bounds();
        self.release_look_through_for_gesture();
        self.for_each_target_cam(|cam| {
            solarxy_host::cameras::reset_to_view(cam, &bounds, view);
        });
    }

    fn set_inspection(&mut self, mode: InspectionMode, label: &str) {
        let pds = &mut self.view.pane_settings[self.view.active_pane];
        pds.pane_mode = PaneMode::Scene3D;
        pds.inspection_mode = mode;
        self.gui
            .set_toast(&format!("Inspection: {label}"), ToastSeverity::Success);
    }

    /// The UV pane's overlap toggle, which sits on the projection keys.
    ///
    /// `O` and `Shift+O` both land here first and neither reaches its own
    /// meaning inside a UV pane, which is what the shell has always done.
    /// Section 4.4 moves the toggle onto the pane's Display menu, and that
    /// is where this goes.
    fn toggle_uv_overlap_in_a_uv_pane(&mut self) -> bool {
        let pds = &mut self.view.pane_settings[self.view.active_pane];
        if pds.pane_mode != PaneMode::UvMap {
            return false;
        }
        pds.show_uv_overlap = !pds.show_uv_overlap;
        let on = pds.show_uv_overlap;
        if on {
            self.renderer.uv_overlap.stats_dirty = true;
        }
        let msg = if on { "Overlap: On" } else { "Overlap: Off" };
        self.gui.set_toast(msg, ToastSeverity::Success);
        true
    }

    pub(in crate::state) fn write_composite_params(&self) {
        let active_inspection = self.view.pane_settings[self.view.active_pane].inspection_mode;
        self.renderer.post.composite.write_params(
            &self.queue,
            self.renderer.post.bloom_enabled,
            self.renderer.post.ssao_enabled,
            &CompositeLook::from_tone(self.renderer.post.tone_mode, self.renderer.post.exposure),
            &self.renderer.post.luts,
            active_inspection,
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::input::keymap::BINDINGS;

    /// Exactly one thing runs a binding.
    ///
    /// The window's pre-pass, the map after the interface pass, or a panel
    /// inside it. Two of them running one press is the collision the two
    /// dispatch sites produced, and a binding none of them runs is a key
    /// that silently does nothing. Asked through the real predicates, not
    /// through the enum: a test that re-states the rule passes while the
    /// rule is broken.
    #[test]
    fn exactly_one_dispatcher_runs_each_binding() {
        for binding in BINDINGS {
            let runs = u8::from(window_claims(binding, false))
                + u8::from(map_runs(binding))
                + u8::from(binding.claim == Claim::Panel);
            assert_eq!(
                runs,
                1,
                "{} is run by {runs} dispatchers",
                binding.action.id()
            );
        }
    }

    /// Every binding the table declares has an arm that runs it.
    ///
    /// The match in `run_action` is exhaustive, so this cannot fail at
    /// runtime; it is here because the *reverse* can, and did: a binding
    /// whose behaviour hid inside a helper reading a modifier appeared in no
    /// list of bindings at all. The count is what a reader checks against the
    /// reference.
    #[test]
    fn every_declared_binding_is_distinct_from_the_camera_keys() {
        for binding in BINDINGS {
            let chord = Chord::parse(binding.keys).expect("parses");
            assert!(
                to_camera_key(chord.code).is_none(),
                "{} is bound to an arrow, which is a held gesture rather than a command",
                binding.action.id()
            );
        }
    }

    #[test]
    fn a_claim_is_suppressed_while_typing_only_when_the_table_says_so() {
        let claim_of = |keys: &str| {
            BINDINGS
                .iter()
                .find(|b| b.keys == keys)
                .map(|b| (window_claims(b, false), window_claims(b, true)))
        };
        // A chord a focused field has no use for stays global.
        assert_eq!(claim_of("mod+s"), Some((true, true)));
        // The field keeps its own undo.
        assert_eq!(claim_of("mod+z"), Some((true, false)));
        // The interface sees a bare key first, whatever has focus.
        assert_eq!(claim_of("g"), Some((false, false)));
    }
}
