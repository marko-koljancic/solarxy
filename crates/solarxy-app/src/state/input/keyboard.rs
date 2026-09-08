//! The keyboard map, and the toggles it drives.
//!
//! Adding a binding means a match arm here **and** an entry in the shortcuts
//! modal, which is the drift this shell has not closed yet: they are two
//! hand-maintained lists of the same thing and the modal already omits four
//! bindings this file has.

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
                if self.input.modifiers.shift_key() {
                    self.hide_hovered_mesh();
                } else if self.input.modifiers.alt_key() {
                    self.show_all_meshes();
                } else {
                    let bounds = self.scene_bounds();
                    self.release_look_through_for_gesture();
                    self.for_each_target_cam(|cam| cam.reset_to_bounds(&bounds));
                }
            }
            KeyCode::Slash => {
                self.isolate_hovered_mesh();
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
                // `Shift+S` (save preferences) was retired in RC2 — view
                // settings now persist via Edit → Save View Settings as
                // Default. `Cmd/Ctrl+S` still saves the review sidecar.
                let cmd_or_ctrl = if cfg!(target_os = "macos") {
                    self.input.modifiers.super_key()
                } else {
                    self.input.modifiers.control_key()
                };
                if cmd_or_ctrl && self.review.active {
                    self.save_review_sidecar();
                } else if !self.input.modifiers.shift_key() {
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
            // the real SetEnvironment op, which nothing else on the
            // desktop emits until the shell gains the node engine.
            #[cfg(debug_assertions)]
            KeyCode::F10 => self.toggle_dev_environment(),
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
        let is_multi = self
            .scene
            .as_ref()
            .is_some_and(|s| s.model.meshes.len() > 1);
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
