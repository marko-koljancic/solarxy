//! The keyboard map, and the toggles it drives.
//!
//! Adding a binding means a match arm here **and** an entry in the shortcuts
//! modal, which is the drift this shell has not closed yet: they are two
//! hand-maintained lists of the same thing and the modal already omits four
//! bindings this file has.
//!
//! Two dispatchers see a press, in order. The window claims a handful of
//! bindings first, through [`shell_key`], before the interface pass sees the
//! key; the map in `handle_key` runs only for a press the window did not
//! claim. That order is what keeps one press from running twice: the window
//! used to handle its keys and fall through, so a chord such as the open
//! dialog's also ran the bare key's arm here.

use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;

use crate::gui::ToastSeverity;
use solarxy_renderer::input::CameraKey;
use solarxy_core::preferences::{
    BackgroundMode, BuiltinBg, CustomBackground, IblMode, InspectionMode, MaterialOverride,
    NormalsMode, PaneMode, ProjectionMode, UvMode, ViewMode,
};

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

/// A binding the window handles before the interface sees the key.
///
/// These are the bindings that have to work whatever has focus: the ones
/// that show and hide chrome, and the file dialogs. A press one of these
/// claims never reaches the key map, which is the rule the collision this
/// replaced had broken: every window arm fell through, so a claimed key ran
/// its window action and then the map's arm for the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellKey {
    ToggleSidebar,
    ToggleMenuBar,
    ToggleFullscreen,
    /// Replace the document with an empty one. A chord, so a text field
    /// keeps its bare `N`.
    NewScene,
    OpenModel,
    /// Write the document to its own path, or ask for one.
    Save,
    /// Ask for a path, then write there.
    SaveAs,
    /// Take back the last step. Not claimed while a text field has focus,
    /// whose own undo the field keeps.
    Undo,
    Redo,
    /// The clipboard, likewise left to a focused text field.
    Copy,
    Paste,
    Duplicate,
    ToggleConsole,
    ToggleViewport,
    /// Cook what is stale now, in manual cook mode. A chord, so it stays
    /// global even with a text field focused.
    CookNow,
}

/// What the window claims for a pressed key, if anything.
///
/// `cmd_or_ctrl` is resolved by the caller, because which key it is depends
/// on the platform. `wants_text` says a text field has focus, which keeps
/// the bare keys typeable there while the function keys and the chords stay
/// global.
///
/// `over_canvas` is the one place a claim is decided by where the pointer
/// is rather than by what is focused, and it exists for one key. Tab is
/// the sidebar's everywhere and the node palette's over the canvas, which
/// is the arrangement the browser's own keymap describes even though its
/// implementation does not honour it. Without this the window claims Tab
/// first and the palette never opens; with it the sidebar keeps the key
/// everywhere a user is not looking at a graph.
pub(crate) fn shell_key(
    code: KeyCode,
    cmd_or_ctrl: bool,
    shift: bool,
    wants_text: bool,
    over_canvas: bool,
) -> Option<ShellKey> {
    match code {
        KeyCode::Tab if !wants_text && !over_canvas => Some(ShellKey::ToggleSidebar),
        KeyCode::F10 => Some(ShellKey::ToggleMenuBar),
        KeyCode::F11 => Some(ShellKey::ToggleFullscreen),
        KeyCode::KeyN if cmd_or_ctrl => Some(ShellKey::NewScene),
        KeyCode::KeyO if cmd_or_ctrl && !shift => Some(ShellKey::OpenModel),
        KeyCode::KeyS if cmd_or_ctrl && shift => Some(ShellKey::SaveAs),
        KeyCode::KeyS if cmd_or_ctrl => Some(ShellKey::Save),
        KeyCode::KeyZ if cmd_or_ctrl && shift && !wants_text => Some(ShellKey::Redo),
        KeyCode::KeyZ if cmd_or_ctrl && !wants_text => Some(ShellKey::Undo),
        KeyCode::KeyY if cmd_or_ctrl && !wants_text => Some(ShellKey::Redo),
        KeyCode::KeyC if cmd_or_ctrl && !wants_text => Some(ShellKey::Copy),
        KeyCode::KeyV if cmd_or_ctrl && !wants_text => Some(ShellKey::Paste),
        KeyCode::KeyD if cmd_or_ctrl && !wants_text => Some(ShellKey::Duplicate),
        KeyCode::Backquote if !wants_text => Some(ShellKey::ToggleConsole),
        KeyCode::Digit1 if cmd_or_ctrl && !wants_text => Some(ShellKey::ToggleViewport),
        KeyCode::Enter | KeyCode::NumpadEnter if cmd_or_ctrl => Some(ShellKey::CookNow),
        _ => None,
    }
}

impl State {
    pub fn set_modifiers(&mut self, modifiers: winit::keyboard::ModifiersState) {
        self.input.modifiers = modifiers;
    }

    pub fn handle_key(&mut self, _event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if !is_pressed {
            if let Some(key) = to_camera_key(code) {
                self.for_each_target_cam(|cam| {
                    cam.handle_key(key, is_pressed);
                });
            }
            return;
        }
        match code {
            KeyCode::KeyH => {
                let bounds = self.scene_bounds();
                self.release_look_through_for_gesture();
                self.for_each_target_cam(|cam| cam.reset_to_bounds(&bounds));
            }
            KeyCode::KeyT => {
                if self.input.modifiers.shift_key() {
                    self.toggle_tone_mode();
                } else {
                    let bounds = self.scene_bounds();
                    self.release_look_through_for_gesture();
                    self.for_each_target_cam(|cam| {
                        solarxy_host::cameras::reset_to_view(
                            cam,
                            &bounds,
                            solarxy_host::cameras::StandardView::Top,
                        );
                    });
                }
            }
            KeyCode::KeyF => {
                let bounds = self.scene_bounds();
                self.release_look_through_for_gesture();
                self.for_each_target_cam(|cam| {
                    solarxy_host::cameras::reset_to_view(
                        cam,
                        &bounds,
                        solarxy_host::cameras::StandardView::Front,
                    );
                });
            }
            KeyCode::KeyL => {
                let cmd_or_ctrl = if cfg!(target_os = "macos") {
                    self.input.modifiers.super_key()
                } else {
                    self.input.modifiers.control_key()
                };
                if cmd_or_ctrl {
                    if self.view.display.layout != ViewLayout::Single {
                        self.view.cameras_linked = !self.view.cameras_linked;
                        let msg = if self.view.cameras_linked {
                            "Cameras linked"
                        } else {
                            "Cameras independent"
                        };
                        self.gui.set_toast(msg, ToastSeverity::Success);
                    }
                } else if self.input.modifiers.shift_key() {
                    self.view.display.lights_locked = !self.view.display.lights_locked;
                    let msg = if self.view.display.lights_locked {
                        "Lights locked"
                    } else {
                        "Lights unlocked"
                    };
                    self.gui.set_toast(msg, ToastSeverity::Success);
                } else {
                    let bounds = self.scene_bounds();
                    self.release_look_through_for_gesture();
                    self.for_each_target_cam(|cam| {
                        solarxy_host::cameras::reset_to_view(
                            cam,
                            &bounds,
                            solarxy_host::cameras::StandardView::Left,
                        );
                    });
                }
            }
            KeyCode::KeyR => {
                if self.input.modifiers.shift_key() {
                    self.toggle_review_mode();
                } else {
                    let bounds = self.scene_bounds();
                    self.release_look_through_for_gesture();
                    self.for_each_target_cam(|cam| {
                        solarxy_host::cameras::reset_to_view(
                            cam,
                            &bounds,
                            solarxy_host::cameras::StandardView::Right,
                        );
                    });
                }
            }
            KeyCode::KeyP => {
                self.release_look_through_for_gesture();
                self.for_each_target_cam(|cam| {
                    cam.set_projection(ProjectionMode::Perspective);
                });
            }
            KeyCode::KeyO => {
                if self.view.pane_settings[self.view.active_pane].pane_mode == PaneMode::UvMap {
                    let pds = &mut self.view.pane_settings[self.view.active_pane];
                    pds.show_uv_overlap = !pds.show_uv_overlap;
                    if pds.show_uv_overlap {
                        self.renderer.uv_overlap.stats_dirty = true;
                    }
                    let msg = if pds.show_uv_overlap {
                        "Overlap: On"
                    } else {
                        "Overlap: Off"
                    };
                    self.gui.set_toast(msg, ToastSeverity::Success);
                } else if self.input.modifiers.shift_key() {
                    self.toggle_ssao();
                } else {
                    self.release_look_through_for_gesture();
                    self.for_each_target_cam(|cam| {
                        cam.set_projection(ProjectionMode::Orthographic);
                    });
                }
            }
            KeyCode::KeyW => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                if self.input.modifiers.shift_key() {
                    pds.line_weight = pds.line_weight.next();
                    self.gui.set_toast(
                        &format!(
                            "Line Weight: {}",
                            self.view.pane_settings[self.view.active_pane].line_weight
                        ),
                        ToastSeverity::Success,
                    );
                } else if pds.view_mode == ViewMode::Ghosted {
                    pds.ghosted_wireframe = !pds.ghosted_wireframe;
                } else {
                    pds.view_mode = match pds.view_mode {
                        ViewMode::Shaded => ViewMode::ShadedWireframe,
                        ViewMode::ShadedWireframe => ViewMode::WireframeOnly,
                        ViewMode::WireframeOnly | ViewMode::Ghosted => ViewMode::Shaded,
                    };
                }
            }
            KeyCode::KeyX => {
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
            KeyCode::KeyS => {
                // The bare key only: the save chords are the window's, claimed
                // in `shell_key` before this map sees the press. `Shift+S`
                // (save preferences) was retired in the 0.5.0 release
                // candidates; view settings persist via Edit, Save View
                // Settings as Default.
                if !self.input.modifiers.shift_key() {
                    self.view.pane_settings[self.view.active_pane].view_mode = ViewMode::Shaded;
                }
            }
            KeyCode::KeyC => {
                self.capture_requested = true;
                self.screenshot_expand_review = false;
            }
            KeyCode::KeyA => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                if self.input.modifiers.shift_key() {
                    pds.show_local_axes = !pds.show_local_axes;
                    let msg = if pds.show_local_axes {
                        "Local Axes: On"
                    } else {
                        "Local Axes: Off"
                    };
                    self.gui.set_toast(msg, ToastSeverity::Success);
                } else {
                    pds.show_axis_gizmo = !pds.show_axis_gizmo;
                }
            }
            KeyCode::KeyG => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.show_grid = !pds.show_grid;
            }
            KeyCode::KeyI => self.toggle_ibl(),
            KeyCode::KeyB => {
                if self.input.modifiers.shift_key() {
                    self.cycle_bounds_mode();
                } else {
                    self.cycle_background();
                }
            }
            KeyCode::KeyM => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                if self.input.modifiers.shift_key() {
                    pds.material_override = pds.material_override.next();
                } else {
                    pds.material_override = if pds.material_override == MaterialOverride::None {
                        MaterialOverride::Clay
                    } else {
                        MaterialOverride::None
                    };
                }
                let msg = format!("Material: {}", pds.material_override);
                self.gui.set_toast(&msg, ToastSeverity::Success);
            }
            KeyCode::KeyD => {
                if self.input.modifiers.shift_key() {
                    self.toggle_bloom();
                }
            }
            KeyCode::KeyE => {
                if self.input.modifiers.shift_key() {
                    self.adjust_exposure(false);
                } else {
                    self.adjust_exposure(true);
                }
            }
            KeyCode::KeyN => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.normals_mode = match pds.normals_mode {
                    NormalsMode::Off => NormalsMode::Face,
                    NormalsMode::Face => NormalsMode::Vertex,
                    NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                    NormalsMode::FaceAndVertex => NormalsMode::Off,
                };
            }
            KeyCode::KeyV => {
                if self.input.modifiers.shift_key() {
                    let pds = &mut self.view.pane_settings[self.view.active_pane];
                    pds.show_validation = !pds.show_validation;
                    let msg = if pds.show_validation {
                        "Validation on"
                    } else {
                        "Validation off"
                    };
                    self.gui.set_toast(msg, ToastSeverity::Success);
                } else {
                    self.view.display.turntable_active = !self.view.display.turntable_active;
                }
            }
            KeyCode::KeyU => {
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
            KeyCode::Digit1 => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::Shaded;
                self.gui
                    .set_toast("Inspection: Shaded", ToastSeverity::Success);
            }
            KeyCode::Digit2 => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::MaterialId;
                self.gui
                    .set_toast("Inspection: Material ID", ToastSeverity::Success);
            }
            KeyCode::Digit3 => {
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
            KeyCode::Digit4 => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::TexelDensity;
                self.gui
                    .set_toast("Inspection: Texel Density", ToastSeverity::Success);
            }
            KeyCode::Digit5 => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::Depth;
                self.gui
                    .set_toast("Inspection: Depth", ToastSeverity::Success);
            }
            KeyCode::Digit6 => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::Overdraw;
                self.gui
                    .set_toast("Inspection: Overdraw", ToastSeverity::Success);
            }
            KeyCode::Digit7 => {
                let pds = &mut self.view.pane_settings[self.view.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::AoPreview;
                self.gui
                    .set_toast("Inspection: AO Preview", ToastSeverity::Success);
            }
            KeyCode::F1 => self.set_view_layout(ViewLayout::Single),
            KeyCode::F2 => self.set_view_layout(ViewLayout::SplitVertical),
            KeyCode::F3 => self.set_view_layout(ViewLayout::SplitHorizontal),
            KeyCode::F4 => self.set_view_layout(ViewLayout::Quad),
            KeyCode::F5 => self.set_view_layout(ViewLayout::ThreeLeftBig),
            // Debug-build-only: toggle the two multi-object dev cubes
            // (the multi-object render harness; see state/dev.rs).
            #[cfg(debug_assertions)]
            KeyCode::F9 => self.toggle_dev_objects(),
            // Debug-build-only: toggle a synthesized environment through
            // the real SetEnvironment op with no document open. F8 rather
            // than F10, which the window claims for the menu bar.
            #[cfg(debug_assertions)]
            KeyCode::F8 => self.toggle_dev_environment(),
            _ => {
                if let Some(key) = to_camera_key(code) {
                    self.release_look_through_for_gesture();
                    self.for_each_target_cam(|cam| {
                        cam.handle_key(key, is_pressed);
                    });
                }
            }
        }
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

    fn toggle_ibl(&mut self) {
        if self.input.modifiers.shift_key() {
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

    /// The window's claims, in full, beside the bare keys the map keeps.
    /// Cmd+O and Cmd+1 used to run both dispatchers in every build, and
    /// F10 both in a debug build, because the window never said it had
    /// handled a press.
    #[test]
    fn the_window_claims_these_keys_and_leaves_the_rest_to_the_map() {
        let cases = [
            (KeyCode::F10, false, false, Some(ShellKey::ToggleMenuBar)),
            (KeyCode::F11, false, false, Some(ShellKey::ToggleFullscreen)),
            (KeyCode::Tab, false, false, Some(ShellKey::ToggleSidebar)),
            (
                KeyCode::Backquote,
                false,
                false,
                Some(ShellKey::ToggleConsole),
            ),
            (KeyCode::KeyO, true, false, Some(ShellKey::OpenModel)),
            // Import HDRI's chord went with the Environment dialog.
            (KeyCode::KeyO, true, true, None),
            (KeyCode::KeyN, true, false, Some(ShellKey::NewScene)),
            (KeyCode::KeyS, true, false, Some(ShellKey::Save)),
            (KeyCode::KeyS, true, true, Some(ShellKey::SaveAs)),
            (KeyCode::KeyZ, true, false, Some(ShellKey::Undo)),
            (KeyCode::KeyZ, true, true, Some(ShellKey::Redo)),
            (KeyCode::KeyY, true, false, Some(ShellKey::Redo)),
            (KeyCode::KeyZ, false, false, None),
            (KeyCode::KeyY, false, false, None),
            (KeyCode::KeyC, true, false, Some(ShellKey::Copy)),
            (KeyCode::KeyV, true, false, Some(ShellKey::Paste)),
            (KeyCode::KeyD, true, false, Some(ShellKey::Duplicate)),
            // Bare C screenshots, bare V spins the turntable, Shift+D is
            // bloom: all the map's.
            (KeyCode::KeyC, false, false, None),
            (KeyCode::KeyV, false, false, None),
            (KeyCode::KeyD, false, true, None),
            // Bare N cycles normals and bare S sets shaded, both the map's.
            (KeyCode::KeyN, false, false, None),
            (KeyCode::KeyS, false, false, None),
            (KeyCode::KeyS, false, true, None),
            (KeyCode::Digit1, true, false, Some(ShellKey::ToggleViewport)),
            // The bare keys belong to the map: projection, overlap, shaded.
            (KeyCode::KeyO, false, false, None),
            (KeyCode::KeyO, false, true, None),
            (KeyCode::Digit1, false, false, None),
            // The developer harness keys are the map's, debug builds only.
            (KeyCode::F8, false, false, None),
            (KeyCode::F9, false, false, None),
            // The explicit cook is a chord; bare Enter belongs to whatever
            // has focus.
            (KeyCode::Enter, true, false, Some(ShellKey::CookNow)),
            (KeyCode::NumpadEnter, true, false, Some(ShellKey::CookNow)),
            (KeyCode::Enter, false, false, None),
        ];
        for (code, cmd, shift, expected) in cases {
            assert_eq!(
                shell_key(code, cmd, shift, false, false),
                expected,
                "{code:?} cmd={cmd} shift={shift}"
            );
        }
    }

    /// Tab belongs to the sidebar everywhere except over the node
    /// canvas, where it belongs to the palette.
    ///
    /// The one claim decided by where the pointer is rather than by what
    /// is focused. Without it the window takes Tab before egui sees the
    /// press and the palette never opens; with it the sidebar keeps the
    /// key everywhere a user is not looking at a graph.
    #[test]
    fn tab_is_the_sidebars_until_the_pointer_is_over_the_canvas() {
        assert_eq!(
            shell_key(KeyCode::Tab, false, false, false, false),
            Some(ShellKey::ToggleSidebar)
        );
        assert_eq!(
            shell_key(KeyCode::Tab, false, false, false, true),
            None,
            "over the canvas the window must not claim it"
        );
        // And a focused field still keeps it, wherever the pointer is.
        assert_eq!(shell_key(KeyCode::Tab, false, false, true, false), None);
        // A focused field also keeps its own undo and redo.
        assert_eq!(shell_key(KeyCode::KeyZ, true, false, true, false), None);
        assert_eq!(shell_key(KeyCode::KeyZ, true, true, true, false), None);
        assert_eq!(shell_key(KeyCode::KeyY, true, false, true, false), None);
        assert_eq!(shell_key(KeyCode::KeyC, true, false, true, false), None);
        assert_eq!(shell_key(KeyCode::KeyV, true, false, true, false), None);
        assert_eq!(shell_key(KeyCode::KeyD, true, false, true, false), None);
        assert_eq!(shell_key(KeyCode::Tab, false, false, true, true), None);

        // Nothing else changes with the pointer.
        assert_eq!(
            shell_key(KeyCode::F10, false, false, false, true),
            Some(ShellKey::ToggleMenuBar)
        );
        assert_eq!(
            shell_key(KeyCode::Backquote, false, false, false, true),
            Some(ShellKey::ToggleConsole)
        );
    }

    /// A focused text field keeps the keys it could be typing into, and the
    /// function keys and chords stay global.
    #[test]
    fn a_focused_text_field_keeps_its_typeable_keys() {
        assert_eq!(shell_key(KeyCode::Tab, false, false, true, false), None);
        assert_eq!(
            shell_key(KeyCode::Backquote, false, false, true, false),
            None
        );
        assert_eq!(shell_key(KeyCode::Digit1, true, false, true, false), None);
        assert_eq!(
            shell_key(KeyCode::F10, false, false, true, false),
            Some(ShellKey::ToggleMenuBar)
        );
        assert_eq!(
            shell_key(KeyCode::F11, false, false, true, false),
            Some(ShellKey::ToggleFullscreen)
        );
        assert_eq!(
            shell_key(KeyCode::KeyO, true, false, true, false),
            Some(ShellKey::OpenModel)
        );
    }
}
