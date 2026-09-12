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
use solarxy_core::preferences::{
    BackgroundMode, BuiltinBg, CustomBackground, IblMode, InspectionMode, MaterialOverride,
    NormalsMode, PaneMode, ProjectionMode, UvMode, ViewMode,
};

use super::keymap::{self, Action, Binding, Chord, Claim, KeyScope};
use crate::state::{BoundsMode, CompositeLook, State, ViewLayout};

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

/// The ordered background list the `B` key cycles through: every builtin
/// (skipping `HDRI Sky` until an HDRI is loaded) followed by every user
/// custom background.
fn background_cycle_options(customs: &[CustomBackground], has_hdri: bool) -> Vec<BackgroundMode> {
    let mut options: Vec<BackgroundMode> = BuiltinBg::ALL
        .iter()
        .filter(|b| has_hdri || **b != BuiltinBg::HdriSky)
        .map(|b| BackgroundMode::Builtin(*b))
        .collect();
    options.extend(customs.iter().map(|c| BackgroundMode::Custom(c.id)));
    options
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
            Action::Undo => self.undo(),
            Action::Redo | Action::RedoAlt => self.redo(),
            Action::Copy => self.copy_selection(),
            Action::Paste => self.paste_clipboard(),
            Action::Duplicate => self.duplicate_selection(),
            Action::CookNow => self.cook_now(),

            // Chrome.
            Action::ToggleSidebar => self.gui.toggle_tab(crate::gui::SolarxyTab::Sidebar),
            // Claimed by no dispatcher: the node panel consumes Tab during
            // the interface pass, where the pointer position inside the
            // canvas is known. Declared so the sidebar's global Tab does not
            // shadow it, and so the Add menu can show the key.
            Action::OpenNodePalette => {}
            Action::ToggleMenuBar => self.gui.menu_bar_visible = !self.gui.menu_bar_visible,
            Action::ToggleFullscreen => self.toggle_fullscreen(),
            Action::ToggleConsole => self.gui.toggle_tab(crate::gui::SolarxyTab::Console),
            Action::ToggleViewportPanel => self.gui.toggle_tab(crate::gui::SolarxyTab::Viewport),

            // Framing and views.
            Action::FitView => {
                let bounds = self.scene_bounds();
                self.release_look_through_for_gesture();
                self.for_each_target_cam(|cam| cam.reset_to_bounds(&bounds));
            }
            Action::ViewTop => self.frame_standard_view(StandardView::Top),
            Action::ViewFront => self.frame_standard_view(StandardView::Front),
            Action::ViewLeft => self.frame_standard_view(StandardView::Left),
            Action::ViewRight => self.frame_standard_view(StandardView::Right),
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
            Action::LinkCameras => {
                if self.view.display.layout != ViewLayout::Single {
                    self.view.cameras_linked = !self.view.cameras_linked;
                    let msg = if self.view.cameras_linked {
                        "Cameras linked"
                    } else {
                        "Cameras independent"
                    };
                    self.gui.set_toast(msg, ToastSeverity::Success);
                }
            }

            // Pane layouts.
            Action::LayoutSingle => self.set_view_layout(ViewLayout::Single),
            Action::LayoutSplitVertical => self.set_view_layout(ViewLayout::SplitVertical),
            Action::LayoutSplitHorizontal => self.set_view_layout(ViewLayout::SplitHorizontal),
            Action::LayoutQuad => self.set_view_layout(ViewLayout::Quad),
            Action::LayoutThreeLeftBig => self.set_view_layout(ViewLayout::ThreeLeftBig),

            // Display and overlays.
            Action::ToggleGrid => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.show_grid = !pds.show_grid;
            }
            Action::ToggleAxisGizmo => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.show_axis_gizmo = !pds.show_axis_gizmo;
            }
            Action::ToggleLocalAxes => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.show_local_axes = !pds.show_local_axes;
                let msg = if pds.show_local_axes {
                    "Local Axes: On"
                } else {
                    "Local Axes: Off"
                };
                self.gui.set_toast(msg, ToastSeverity::Success);
            }
            Action::CycleBackground => self.cycle_background(),
            Action::CycleBounds => self.cycle_bounds_mode(),
            Action::CycleNormals => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.normals_mode = match pds.normals_mode {
                    NormalsMode::Off => NormalsMode::Face,
                    NormalsMode::Face => NormalsMode::Vertex,
                    NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                    NormalsMode::FaceAndVertex => NormalsMode::Off,
                };
            }
            Action::CycleUvMode => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                if pds.pane_mode == PaneMode::UvMap {
                    pds.uv_bg = pds.uv_bg.next();
                    self.gui.set_toast(
                        &format!("UV Background: {}", pds.uv_bg),
                        ToastSeverity::Success,
                    );
                } else {
                    pds.uv_mode = match pds.uv_mode {
                        UvMode::Off => UvMode::Gradient,
                        UvMode::Gradient => UvMode::Checker,
                        UvMode::Checker => UvMode::Off,
                    };
                }
            }
            Action::ToggleTurntable => {
                self.view.display.turntable_active = !self.view.display.turntable_active;
            }
            Action::ToggleValidationOverlay => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.show_validation = !pds.show_validation;
                let msg = if pds.show_validation {
                    "Validation on"
                } else {
                    "Validation off"
                };
                self.gui.set_toast(msg, ToastSeverity::Success);
            }

            // Shading and post.
            Action::CycleViewMode => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                if pds.view_mode == ViewMode::Ghosted {
                    pds.ghosted_wireframe = !pds.ghosted_wireframe;
                } else {
                    pds.view_mode = match pds.view_mode {
                        ViewMode::Shaded => ViewMode::ShadedWireframe,
                        ViewMode::ShadedWireframe => ViewMode::WireframeOnly,
                        ViewMode::WireframeOnly | ViewMode::Ghosted => ViewMode::Shaded,
                    };
                }
            }
            Action::CycleLineWeight => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.line_weight = pds.line_weight.next();
                let weight = pds.line_weight;
                self.gui
                    .set_toast(&format!("Line Weight: {weight}"), ToastSeverity::Success);
            }
            Action::ToggleGhosted => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                if pds.view_mode == ViewMode::Ghosted {
                    pds.view_mode = pds.prev_non_ghosted_mode;
                } else {
                    pds.prev_non_ghosted_mode = pds.view_mode;
                    pds.ghosted_wireframe = matches!(
                        pds.view_mode,
                        ViewMode::ShadedWireframe | ViewMode::WireframeOnly
                    );
                    pds.view_mode = ViewMode::Ghosted;
                }
            }
            Action::SetShaded => {
                self.view.pane_settings[self.view.active_pane].view_mode = ViewMode::Shaded;
            }
            Action::ToggleMaterialOverride => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.material_override = if pds.material_override == MaterialOverride::None {
                    MaterialOverride::Clay
                } else {
                    MaterialOverride::None
                };
                let msg = format!("Material: {}", pds.material_override);
                self.gui.set_toast(&msg, ToastSeverity::Success);
            }
            Action::NextMaterialOverride => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.material_override = pds.material_override.next();
                let msg = format!("Material: {}", pds.material_override);
                self.gui.set_toast(&msg, ToastSeverity::Success);
            }
            Action::ToggleIbl => self.set_ibl(false),
            Action::CycleIblMode => self.set_ibl(true),
            Action::LockLights => {
                self.view.display.lights_locked = !self.view.display.lights_locked;
                let msg = if self.view.display.lights_locked {
                    "Lights locked"
                } else {
                    "Lights unlocked"
                };
                self.gui.set_toast(msg, ToastSeverity::Success);
            }
            Action::ToggleToneMode => self.toggle_tone_mode(),
            Action::ToggleBloom => self.toggle_bloom(),
            Action::ToggleSsao => {
                if !self.toggle_uv_overlap_in_a_uv_pane() {
                    self.toggle_ssao();
                }
            }
            Action::ExposureUp => self.adjust_exposure(true),
            Action::ExposureDown => self.adjust_exposure(false),

            // Inspection modes.
            Action::InspectShaded => self.set_inspection(InspectionMode::Shaded, "Shaded"),
            Action::InspectMaterialId => {
                self.set_inspection(InspectionMode::MaterialId, "Material ID");
            }
            Action::InspectUvMap => {
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

            // Capture and review.
            Action::Screenshot => {
                self.capture_requested = true;
                self.screenshot_expand_review = false;
            }
            Action::ToggleReviewMode => self.toggle_review_mode(),

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

    fn toggle_tone_mode(&mut self) {
        self.renderer.post.tone_mode = self.renderer.post.tone_mode.next();
        self.write_composite_params();
        self.gui.set_toast(
            &format!("Tone: {}", self.renderer.post.tone_mode),
            ToastSeverity::Success,
        );
    }

    fn toggle_ssao(&mut self) {
        self.renderer.post.ssao_enabled = !self.renderer.post.ssao_enabled;
        self.write_composite_params();
        let msg = if self.renderer.post.ssao_enabled {
            "SSAO: On"
        } else {
            "SSAO: Off"
        };
        self.gui.set_toast(msg, ToastSeverity::Success);
    }

    fn toggle_bloom(&mut self) {
        self.renderer.post.bloom_enabled = !self.renderer.post.bloom_enabled;
        self.write_composite_params();
        let msg = if self.renderer.post.bloom_enabled {
            "Bloom: On"
        } else {
            "Bloom: Off"
        };
        self.gui.set_toast(msg, ToastSeverity::Success);
    }

    fn adjust_exposure(&mut self, increase: bool) {
        let step = if increase { 0.5 } else { -0.5 };
        self.renderer.post.exposure = (self.renderer.post.exposure + step).clamp(0.1, 10.0);
        self.write_composite_params();
        self.gui.set_toast(
            &format!("Exposure: {:.1}", self.renderer.post.exposure),
            ToastSeverity::Success,
        );
    }

    /// Image-based lighting: `cycle` switches between diffuse and full,
    /// where the plain form turns it off and on.
    ///
    /// This used to read the shift modifier itself, which is how `Shift+I`
    /// stayed a real binding while appearing in no list of them.
    fn set_ibl(&mut self, cycle: bool) {
        if cycle {
            if self.renderer.ibl_res.ibl_mode != IblMode::Off {
                self.renderer.ibl_res.ibl_mode = match self.renderer.ibl_res.ibl_mode {
                    IblMode::Diffuse => IblMode::Full,
                    IblMode::Full | IblMode::Off => IblMode::Diffuse,
                };
                self.renderer.ibl_res.last_active_ibl_mode = self.renderer.ibl_res.ibl_mode;
            }
        } else if self.renderer.ibl_res.ibl_mode == IblMode::Off {
            self.renderer.ibl_res.ibl_mode = self.renderer.ibl_res.last_active_ibl_mode;
        } else {
            self.renderer.ibl_res.last_active_ibl_mode = self.renderer.ibl_res.ibl_mode;
            self.renderer.ibl_res.ibl_mode = IblMode::Off;
        }
        self.rebuild_light_bind_group();
        let msg = match self.renderer.ibl_res.ibl_mode {
            IblMode::Off => "IBL: Off",
            IblMode::Diffuse => "IBL: Diffuse",
            IblMode::Full => "IBL: Full",
        };
        self.gui.set_toast(msg, ToastSeverity::Success);
    }

    fn cycle_background(&mut self) {
        // `B` walks every builtin (skipping `HDRI Sky` until an HDRI is
        // loaded) then every user custom background.
        let has_hdri = self.renderer.ibl_res.ibl.equirect.is_some();
        let options = background_cycle_options(&self.preferences.view.custom_backgrounds, has_hdri);
        let pds = &mut self.view.pane_settings[self.view.active_pane];
        let i = options
            .iter()
            .position(|m| *m == pds.background_mode)
            .unwrap_or(0);
        pds.background_mode = options[(i + 1) % options.len()];
        self.apply_background_change();
    }

    fn cycle_bounds_mode(&mut self) {
        // Per-mesh bounds are only a distinct picture when there is more than
        // one mesh to tell apart, so the mode is skipped over otherwise.
        let is_multi = self
            .raster
            .scene()
            .iter()
            .map(|(_, o)| o.model.meshes.len())
            .sum::<usize>()
            > 1;
        let pds = &mut self.view.pane_settings[self.view.active_pane];
        pds.bounds_mode = match pds.bounds_mode {
            BoundsMode::Off => BoundsMode::WholeModel,
            BoundsMode::WholeModel if is_multi => BoundsMode::PerMesh,
            BoundsMode::WholeModel | BoundsMode::PerMesh => BoundsMode::Off,
        };
        let msg = match pds.bounds_mode {
            BoundsMode::Off => "Bounds: Off",
            BoundsMode::WholeModel => "Bounds: Whole Model",
            BoundsMode::PerMesh => "Bounds: Per Mesh",
        };
        self.gui.set_toast(msg, ToastSeverity::Success);
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
