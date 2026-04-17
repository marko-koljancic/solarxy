mod dialogs;
mod menu_actions;

use winit::event::MouseButton;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;

use solarxy_renderer::camera_state::CameraState;
use crate::gui::ToastSeverity;
use solarxy_renderer::ibl::IblState;
use solarxy_core::preferences::{
    self, IblMode, InspectionMode, MaterialOverride, NormalsMode, PaneMode, ProjectionMode, UvMode,
    ViewMode,
};

use super::{BackgroundModeExt, BoundsMode, State, ViewLayout};

impl State {
    fn for_each_target_cam(&mut self, mut f: impl FnMut(&mut CameraState)) {
        let (primary, secondary) =
            super::cam_routing(self.view.active_pane, self.view.cameras_linked);
        if primary
            && self.view.pane_settings[0].pane_mode == PaneMode::Scene3D
            && let Some(scene) = &mut self.scene
        {
            f(&mut scene.cam);
        }
        if secondary
            && self
                .view
                .pane_settings
                .get(1)
                .is_some_and(|p| p.pane_mode == PaneMode::Scene3D)
            && let Some(cam) = &mut self.view.secondary_cam
        {
            f(cam);
        }
    }

    pub fn set_modifiers(&mut self, modifiers: winit::keyboard::ModifiersState) {
        self.input.modifiers = modifiers;
    }

    pub fn toggle_hints(&mut self) {
        self.gui.toggle_hints();
    }

    pub fn handle_key(&mut self, _event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if !is_pressed {
            self.for_each_target_cam(|cam| {
                cam.handle_key(code, is_pressed);
            });
            return;
        }
        match code {
            KeyCode::KeyH => {
                let bounds = self.scene.as_ref().map(|s| s.model.bounds);
                if let Some(bounds) = bounds {
                    self.for_each_target_cam(|cam| cam.reset_to_bounds(&bounds));
                }
            }
            KeyCode::KeyT => {
                if self.input.modifiers.shift_key() {
                    self.toggle_tone_mode();
                } else {
                    let bounds = self.scene.as_ref().map(|s| s.model.bounds);
                    if let Some(bounds) = bounds {
                        self.for_each_target_cam(|cam| {
                            cam.reset_to_bounds_axis(
                                &bounds,
                                cgmath::Vector3::unit_y(),
                                -cgmath::Vector3::unit_z(),
                            );
                        });
                    }
                }
            }
            KeyCode::KeyF => {
                let bounds = self.scene.as_ref().map(|s| s.model.bounds);
                if let Some(bounds) = bounds {
                    self.for_each_target_cam(|cam| {
                        cam.reset_to_bounds_axis(
                            &bounds,
                            cgmath::Vector3::unit_z(),
                            cgmath::Vector3::unit_y(),
                        );
                    });
                }
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
                    let bounds = self.scene.as_ref().map(|s| s.model.bounds);
                    if let Some(bounds) = bounds {
                        self.for_each_target_cam(|cam| {
                            cam.reset_to_bounds_axis(
                                &bounds,
                                -cgmath::Vector3::unit_x(),
                                cgmath::Vector3::unit_y(),
                            );
                        });
                    }
                }
            }
            KeyCode::KeyR => {
                let bounds = self.scene.as_ref().map(|s| s.model.bounds);
                if let Some(bounds) = bounds {
                    self.for_each_target_cam(|cam| {
                        cam.reset_to_bounds_axis(
                            &bounds,
                            cgmath::Vector3::unit_x(),
                            cgmath::Vector3::unit_y(),
                        );
                    });
                }
            }
            KeyCode::KeyP => {
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
                if self.input.modifiers.shift_key() {
                    self.save_preferences();
                } else {
                    self.view.pane_settings[self.view.active_pane].view_mode = ViewMode::Shaded;
                }
            }
            KeyCode::KeyC => {
                if self.scene.is_some() {
                    self.capture_requested = true;
                }
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
            KeyCode::F1 => self.set_view_layout(ViewLayout::Single),
            KeyCode::F2 => self.set_view_layout(ViewLayout::SplitVertical),
            KeyCode::F3 => self.set_view_layout(ViewLayout::SplitHorizontal),
            _ => {
                self.for_each_target_cam(|cam| {
                    cam.handle_key(code, is_pressed);
                });
            }
        }
    }

    fn write_composite_params(&self) {
        self.renderer.post.composite.write_params(
            &self.queue,
            self.renderer.post.bloom_enabled,
            self.renderer.post.ssao_enabled,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
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

    pub(super) fn apply_background_change(&mut self) {
        if self.view.active_pane == 0 {
            let (top, bottom) = self.view.pane_settings[0].background_mode.sky_colors();
            self.renderer.ibl_res.ibl =
                IblState::from_sky_colors(&self.device, &self.queue, top, bottom);
            self.rebuild_light_bind_group();
        }
    }

    pub(super) fn apply_composite_params(&self) {
        self.write_composite_params();
    }

    pub(super) fn apply_ibl_change(&mut self) {
        self.rebuild_light_bind_group();
    }

    fn cycle_background(&mut self) {
        let pds = &mut self.view.pane_settings[self.view.active_pane];
        pds.background_mode = pds.background_mode.next();
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

    fn save_preferences(&mut self) {
        let pds = &self.view.pane_settings[0];
        self.preferences.display.background = pds.background_mode;
        self.preferences.display.view_mode = pds.view_mode;
        self.preferences.display.normals_mode = pds.normals_mode;
        self.preferences.display.grid_visible = pds.show_grid;
        self.preferences.display.axis_gizmo_visible = pds.show_axis_gizmo;
        self.preferences.display.local_axes_visible = pds.show_local_axes;
        self.preferences.display.bloom_enabled = self.renderer.post.bloom_enabled;
        self.preferences.display.ssao_enabled = self.renderer.post.ssao_enabled;
        self.preferences.display.uv_mode = pds.uv_mode;
        self.preferences.display.turntable_active = self.view.display.turntable_active;
        self.preferences.display.turntable_rpm = self.view.display.turntable_rpm;
        if let Some(scene) = &self.scene {
            self.preferences.display.projection_mode = scene.cam.camera.projection;
        }
        self.preferences.rendering.wireframe_line_weight = pds.line_weight;
        self.preferences.lighting.lock = self.view.display.lights_locked;
        self.preferences.display.ibl_mode = self.renderer.ibl_res.ibl_mode;
        self.preferences.display.tone_mode = self.renderer.post.tone_mode;
        self.preferences.display.exposure = self.renderer.post.exposure;
        self.preferences.display.inspection_mode = pds.inspection_mode;
        self.preferences.display.texel_density_target = pds.texel_density_target;

        match preferences::save(&self.preferences) {
            Ok(()) => {
                tracing::info!("Preferences saved");
                self.gui
                    .set_toast("Preferences saved", ToastSeverity::Success);
            }
            Err(e) => {
                tracing::error!("Failed to save preferences: {}", e);
                self.gui
                    .set_toast(&format!("Save failed: {}", e), ToastSeverity::Error);
            }
        }
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        let ap = self.view.active_pane;
        if self.view.pane_settings[ap].pane_mode == PaneMode::UvMap {
            match button {
                MouseButton::Left => {
                    self.input.uv_left_pressed = pressed;
                    if !pressed {
                        self.input.uv_last_mouse_pos = None;
                    }
                }
                MouseButton::Middle => {
                    self.input.uv_middle_pressed = pressed;
                    if !pressed {
                        self.input.uv_last_mouse_pos = None;
                    }
                }
                _ => {}
            }
        } else {
            self.for_each_target_cam(|cam| cam.handle_mouse_button(button, pressed));
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        let ap = self.view.active_pane;
        if self.view.pane_settings[ap].pane_mode == PaneMode::UvMap {
            if let Some((lx, ly)) = self.input.uv_last_mouse_pos {
                let dx = x - lx;
                let dy = y - ly;
                if self.input.uv_left_pressed || self.input.uv_middle_pressed {
                    let panes = self.compute_panes();
                    let pane_w = panes.get(ap).map_or(self.config.width as f32, |p| p.width);
                    let pds = &mut self.view.pane_settings[ap];
                    let scale = 1.2 / (pds.uv_zoom * pane_w);
                    pds.uv_offset[0] -= dx * scale;
                    pds.uv_offset[1] += dy * scale;
                }
            }
            if self.input.uv_left_pressed || self.input.uv_middle_pressed {
                self.input.uv_last_mouse_pos = Some((x, y));
            }
        } else {
            self.for_each_target_cam(|cam| cam.handle_mouse_move(x, y));
        }
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        let ap = self.view.active_pane;
        if self.view.pane_settings[ap].pane_mode == PaneMode::UvMap {
            let pds = &mut self.view.pane_settings[ap];
            pds.uv_zoom = (pds.uv_zoom * (1.0 + delta * 0.1)).clamp(0.1, 50.0);
        } else {
            self.for_each_target_cam(|cam| cam.handle_scroll(delta));
        }
    }

}
