use winit::event::MouseButton;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;

use crate::cgi::camera_state::CameraState;
use crate::cgi::ibl::IblState;
use crate::cgi::resources;
use crate::preferences::{
    self, IblMode, InspectionMode, NormalsMode, PaneMode, ProjectionMode, UvMode, ViewMode,
};

use super::{BoundsMode, State, ViewLayout};

impl State {
    fn for_each_target_cam(&mut self, mut f: impl FnMut(&mut CameraState)) {
        let (primary, secondary) = super::cam_routing(self.active_pane, self.cameras_linked);
        if primary
            && self.pane_settings[0].pane_mode == PaneMode::Scene3D
            && let Some(scene) = &mut self.scene
        {
            f(&mut scene.cam);
        }
        if secondary
            && self
                .pane_settings
                .get(1)
                .is_some_and(|p| p.pane_mode == PaneMode::Scene3D)
            && let Some(cam) = &mut self.secondary_cam
        {
            f(cam);
        }
    }
    pub fn handle_dropped_file(&mut self, path: std::path::PathBuf) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("hdr") || ext.eq_ignore_ascii_case("exr") {
                match IblState::from_hdri(&self.device, &self.queue, &path) {
                    Ok(new_ibl) => {
                        self.ibl_res.ibl = new_ibl;
                        self.ibl_res.ibl_mode = IblMode::Full;
                        self.ibl_res.last_active_ibl_mode = IblMode::Full;
                        self.rebuild_light_bind_group();
                        self.gui.set_toast("HDRI loaded", [0.0, 0.4, 0.0, 1.0]);
                    }
                    Err(e) => {
                        self.gui
                            .set_toast(&format!("HDRI error: {}", e), [0.6, 0.0, 0.0, 1.0]);
                    }
                }
                return;
            }
        }

        if !resources::is_supported_model_extension(&path) {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("none");
            self.gui.set_toast(
                &format!("Unsupported format: .{}", ext),
                [0.6, 0.0, 0.0, 1.0],
            );
            return;
        }

        let model_path = match path.canonicalize() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(e) => {
                self.gui
                    .set_toast(&format!("Invalid path: {}", e), [0.6, 0.0, 0.0, 1.0]);
                return;
            }
        };

        self.spawn_load(model_path);
    }

    pub fn set_modifiers(&mut self, modifiers: winit::keyboard::ModifiersState) {
        self.modifiers = modifiers;
    }

    pub fn toggle_hints(&mut self) {
        self.gui.toggle_hints();
    }

    pub fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if !is_pressed {
            self.for_each_target_cam(|cam| {
                cam.handle_key(code, is_pressed);
            });
            return;
        }
        match code {
            KeyCode::Escape => event_loop.exit(),
            KeyCode::KeyH => {
                let bounds = self.scene.as_ref().map(|s| s.model.bounds);
                if let Some(bounds) = bounds {
                    self.for_each_target_cam(|cam| cam.reset_to_bounds(&bounds));
                }
            }
            KeyCode::KeyT => {
                if self.modifiers.shift_key() {
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
                if self.modifiers.shift_key() {
                    self.display.lights_locked = !self.display.lights_locked;
                    let msg = if self.display.lights_locked {
                        "Lights locked"
                    } else {
                        "Lights unlocked"
                    };
                    self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
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
                if self.pane_settings[self.active_pane].pane_mode == PaneMode::UvMap {
                    let pds = &mut self.pane_settings[self.active_pane];
                    pds.show_uv_overlap = !pds.show_uv_overlap;
                    if pds.show_uv_overlap {
                        self.uv_overlap.stats_dirty = true;
                    }
                    let msg = if pds.show_uv_overlap {
                        "Overlap: On"
                    } else {
                        "Overlap: Off"
                    };
                    self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
                } else if self.modifiers.shift_key() {
                    self.toggle_ssao();
                } else {
                    self.for_each_target_cam(|cam| {
                        cam.set_projection(ProjectionMode::Orthographic);
                    });
                }
            }
            KeyCode::KeyW => {
                let pds = &mut self.pane_settings[self.active_pane];
                if self.modifiers.shift_key() {
                    pds.line_weight = pds.line_weight.next();
                    self.gui.set_toast(
                        &format!(
                            "Line Weight: {}",
                            self.pane_settings[self.active_pane].line_weight
                        ),
                        [0.0, 0.4, 0.0, 1.0],
                    );
                } else if pds.view_mode == ViewMode::Ghosted {
                    pds.ghosted_wireframe = !pds.ghosted_wireframe;
                } else {
                    pds.view_mode = match pds.view_mode {
                        ViewMode::Shaded => ViewMode::ShadedWireframe,
                        ViewMode::ShadedWireframe => ViewMode::WireframeOnly,
                        ViewMode::WireframeOnly => ViewMode::Shaded,
                        ViewMode::Ghosted => unreachable!(),
                    };
                }
            }
            KeyCode::KeyX => {
                let pds = &mut self.pane_settings[self.active_pane];
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
                if self.modifiers.shift_key() {
                    self.save_preferences();
                } else {
                    self.pane_settings[self.active_pane].view_mode = ViewMode::Shaded;
                }
            }
            KeyCode::KeyC => {
                if self.scene.is_some() {
                    self.capture_requested = true;
                }
            }
            KeyCode::KeyA => {
                let pds = &mut self.pane_settings[self.active_pane];
                if self.modifiers.shift_key() {
                    pds.show_local_axes = !pds.show_local_axes;
                    let msg = if pds.show_local_axes {
                        "Local Axes: On"
                    } else {
                        "Local Axes: Off"
                    };
                    self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
                } else {
                    pds.show_axis_gizmo = !pds.show_axis_gizmo;
                }
            }
            KeyCode::KeyG => {
                let pds = &mut self.pane_settings[self.active_pane];
                pds.show_grid = !pds.show_grid;
            }
            KeyCode::KeyI => self.toggle_ibl(),
            KeyCode::KeyB => {
                if self.modifiers.shift_key() {
                    self.cycle_bounds_mode();
                } else {
                    self.cycle_background();
                }
            }
            KeyCode::KeyM => {
                if self.modifiers.shift_key() {
                    self.toggle_bloom();
                }
            }
            KeyCode::KeyE => {
                if self.modifiers.shift_key() {
                    self.adjust_exposure(false);
                } else {
                    self.adjust_exposure(true);
                }
            }
            KeyCode::KeyN => {
                let pds = &mut self.pane_settings[self.active_pane];
                pds.normals_mode = match pds.normals_mode {
                    NormalsMode::Off => NormalsMode::Face,
                    NormalsMode::Face => NormalsMode::Vertex,
                    NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                    NormalsMode::FaceAndVertex => NormalsMode::Off,
                };
            }
            KeyCode::KeyV => self.display.turntable_active = !self.display.turntable_active,
            KeyCode::KeyU => {
                let pds = &mut self.pane_settings[self.active_pane];
                if pds.pane_mode == PaneMode::UvMap {
                    pds.uv_bg = pds.uv_bg.next();
                    self.gui.set_toast(
                        &format!("UV Background: {}", pds.uv_bg),
                        [0.0, 0.4, 0.0, 1.0],
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
                let pds = &mut self.pane_settings[self.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::Shaded;
                self.gui
                    .set_toast("Inspection: Shaded", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::Digit2 => {
                let pds = &mut self.pane_settings[self.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::MaterialId;
                self.gui
                    .set_toast("Inspection: Material ID", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::Digit3 => {
                let pds = &mut self.pane_settings[self.active_pane];
                if pds.pane_mode == PaneMode::UvMap {
                    pds.pane_mode = PaneMode::Scene3D;
                    self.gui.set_toast("3D View", [0.0, 0.4, 0.0, 1.0]);
                } else {
                    pds.pane_mode = PaneMode::UvMap;
                    pds.uv_offset = [0.0, 0.0];
                    pds.uv_zoom = 1.0;
                    self.gui.set_toast("UV Map", [0.0, 0.4, 0.0, 1.0]);
                }
            }
            KeyCode::Digit4 => {
                let pds = &mut self.pane_settings[self.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::TexelDensity;
                self.gui
                    .set_toast("Inspection: Texel Density", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::Digit5 => {
                let pds = &mut self.pane_settings[self.active_pane];
                pds.pane_mode = PaneMode::Scene3D;
                pds.inspection_mode = InspectionMode::Depth;
                self.gui
                    .set_toast("Inspection: Depth", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::F1 => {
                if self.display.layout != ViewLayout::Single {
                    if self.active_pane == 1 {
                        if let Some(sec) = self.secondary_cam.take()
                            && let Some(scene) = &mut self.scene
                        {
                            scene.cam = sec;
                            self.post.ssao.rebuild_bind_groups(
                                &self.device,
                                &self.layouts,
                                &scene.cam.buffer,
                            );
                            scene.vis.rebuild_camera_bind_groups(
                                &self.device,
                                &self.layouts,
                                &scene.cam.buffer,
                            );
                        }
                    } else {
                        self.secondary_cam = None;
                    }
                }
                if self.active_pane == 1 {
                    self.pane_settings[0] = self.pane_settings[1].clone();
                }
                self.active_pane = 0;
                self.display.layout = ViewLayout::Single;
                let (tw, th) = self.target_dimensions();
                self.resize_render_targets(tw, th);
                self.gui.set_toast("Single Viewport", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::F2 => {
                if self.display.layout == ViewLayout::Single {
                    self.pane_settings[1] = self.pane_settings[0].clone();
                    self.pane_settings[0].pane_mode = PaneMode::UvMap;
                    self.pane_settings[0].uv_offset = [0.0, 0.0];
                    self.pane_settings[0].uv_zoom = 1.0;
                    self.pane_settings[1].pane_mode = PaneMode::Scene3D;
                    if let Some(scene) = &self.scene {
                        self.secondary_cam = Some(
                            scene
                                .cam
                                .clone_with_new_resources(&self.device, &self.layouts.camera),
                        );
                    }
                }
                self.display.layout = ViewLayout::SplitVertical;
                let (tw, th) = self.target_dimensions();
                self.resize_render_targets(tw, th);
                self.gui.set_toast("Split Vertical", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::F3 => {
                if self.display.layout == ViewLayout::Single {
                    self.pane_settings[1] = self.pane_settings[0].clone();
                    self.pane_settings[0].pane_mode = PaneMode::UvMap;
                    self.pane_settings[0].uv_offset = [0.0, 0.0];
                    self.pane_settings[0].uv_zoom = 1.0;
                    self.pane_settings[1].pane_mode = PaneMode::Scene3D;
                    if let Some(scene) = &self.scene {
                        self.secondary_cam = Some(
                            scene
                                .cam
                                .clone_with_new_resources(&self.device, &self.layouts.camera),
                        );
                    }
                }
                self.display.layout = ViewLayout::SplitHorizontal;
                let (tw, th) = self.target_dimensions();
                self.resize_render_targets(tw, th);
                self.gui.set_toast("Split Horizontal", [0.0, 0.4, 0.0, 1.0]);
            }
            _ => {
                self.for_each_target_cam(|cam| {
                    cam.handle_key(code, is_pressed);
                });
            }
        }
    }

    fn write_composite_params(&self) {
        self.post.composite.write_params(
            &self.queue,
            self.post.bloom_enabled,
            self.post.ssao_enabled,
            self.post.tone_mode,
            self.post.exposure,
        );
    }

    fn toggle_tone_mode(&mut self) {
        self.post.tone_mode = self.post.tone_mode.next();
        self.write_composite_params();
        self.gui.set_toast(
            &format!("Tone: {}", self.post.tone_mode),
            [0.0, 0.4, 0.0, 1.0],
        );
    }

    fn toggle_ssao(&mut self) {
        self.post.ssao_enabled = !self.post.ssao_enabled;
        self.write_composite_params();
        let msg = if self.post.ssao_enabled {
            "SSAO: On"
        } else {
            "SSAO: Off"
        };
        self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
    }

    fn toggle_bloom(&mut self) {
        self.post.bloom_enabled = !self.post.bloom_enabled;
        self.write_composite_params();
        let msg = if self.post.bloom_enabled {
            "Bloom: On"
        } else {
            "Bloom: Off"
        };
        self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
    }

    fn adjust_exposure(&mut self, increase: bool) {
        let step = if increase { 0.5 } else { -0.5 };
        self.post.exposure = (self.post.exposure + step).clamp(0.1, 10.0);
        self.write_composite_params();
        self.gui.set_toast(
            &format!("Exposure: {:.1}", self.post.exposure),
            [0.0, 0.4, 0.0, 1.0],
        );
    }

    fn toggle_ibl(&mut self) {
        if self.modifiers.shift_key() {
            if self.ibl_res.ibl_mode != IblMode::Off {
                self.ibl_res.ibl_mode = match self.ibl_res.ibl_mode {
                    IblMode::Diffuse => IblMode::Full,
                    IblMode::Full => IblMode::Diffuse,
                    IblMode::Off => unreachable!(),
                };
                self.ibl_res.last_active_ibl_mode = self.ibl_res.ibl_mode;
            }
        } else if self.ibl_res.ibl_mode == IblMode::Off {
            self.ibl_res.ibl_mode = self.ibl_res.last_active_ibl_mode;
        } else {
            self.ibl_res.last_active_ibl_mode = self.ibl_res.ibl_mode;
            self.ibl_res.ibl_mode = IblMode::Off;
        }
        self.rebuild_light_bind_group();
        let msg = match self.ibl_res.ibl_mode {
            IblMode::Off => "IBL: Off",
            IblMode::Diffuse => "IBL: Diffuse",
            IblMode::Full => "IBL: Full",
        };
        self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
    }

    pub(super) fn apply_background_change(&mut self) {
        if self.active_pane == 0 {
            let (top, bottom) = self.pane_settings[0].background_mode.sky_colors();
            self.ibl_res.ibl = IblState::from_sky_colors(&self.device, &self.queue, top, bottom);
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
        let pds = &mut self.pane_settings[self.active_pane];
        pds.background_mode = pds.background_mode.next();
        self.apply_background_change();
    }

    fn cycle_bounds_mode(&mut self) {
        let is_multi = self
            .scene
            .as_ref()
            .is_some_and(|s| s.model.meshes.len() > 1);
        let pds = &mut self.pane_settings[self.active_pane];
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
        self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
    }

    fn save_preferences(&mut self) {
        let pds = &self.pane_settings[0];
        self.preferences.display.background = pds.background_mode;
        self.preferences.display.view_mode = pds.view_mode;
        self.preferences.display.normals_mode = pds.normals_mode;
        self.preferences.display.grid_visible = pds.show_grid;
        self.preferences.display.axis_gizmo_visible = pds.show_axis_gizmo;
        self.preferences.display.local_axes_visible = pds.show_local_axes;
        self.preferences.display.bloom_enabled = self.post.bloom_enabled;
        self.preferences.display.ssao_enabled = self.post.ssao_enabled;
        self.preferences.display.uv_mode = pds.uv_mode;
        self.preferences.display.turntable_active = self.display.turntable_active;
        self.preferences.display.turntable_rpm = self.display.turntable_rpm;
        if let Some(scene) = &self.scene {
            self.preferences.display.projection_mode = scene.cam.camera.projection;
        }
        self.preferences.rendering.wireframe_line_weight = pds.line_weight;
        self.preferences.lighting.lock = self.display.lights_locked;
        self.preferences.display.ibl_mode = self.ibl_res.ibl_mode;
        self.preferences.display.tone_mode = self.post.tone_mode;
        self.preferences.display.exposure = self.post.exposure;
        self.preferences.display.inspection_mode = pds.inspection_mode;
        self.preferences.display.texel_density_target = pds.texel_density_target;

        match preferences::save(&self.preferences) {
            Ok(()) => self
                .gui
                .set_toast("Preferences saved", [0.0, 0.4, 0.0, 1.0]),
            Err(e) => self
                .gui
                .set_toast(&format!("Save failed: {}", e), [0.6, 0.0, 0.0, 1.0]),
        }
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        let ap = self.active_pane;
        if self.pane_settings[ap].pane_mode == PaneMode::UvMap {
            match button {
                MouseButton::Left => {
                    self.uv_left_pressed = pressed;
                    if !pressed {
                        self.uv_last_mouse_pos = None;
                    }
                }
                MouseButton::Middle => {
                    self.uv_middle_pressed = pressed;
                    if !pressed {
                        self.uv_last_mouse_pos = None;
                    }
                }
                _ => {}
            }
        } else {
            self.for_each_target_cam(|cam| cam.handle_mouse_button(button, pressed));
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        let ap = self.active_pane;
        if self.pane_settings[ap].pane_mode == PaneMode::UvMap {
            if let Some((lx, ly)) = self.uv_last_mouse_pos {
                let dx = x - lx;
                let dy = y - ly;
                if self.uv_left_pressed || self.uv_middle_pressed {
                    let panes = self.compute_panes();
                    let pane_w = panes.get(ap).map_or(self.config.width as f32, |p| p.width);
                    let pds = &mut self.pane_settings[ap];
                    let scale = 1.2 / (pds.uv_zoom * pane_w);
                    pds.uv_offset[0] -= dx * scale;
                    pds.uv_offset[1] += dy * scale;
                }
            }
            if self.uv_left_pressed || self.uv_middle_pressed {
                self.uv_last_mouse_pos = Some((x, y));
            }
        } else {
            self.for_each_target_cam(|cam| cam.handle_mouse_move(x, y));
        }
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        let ap = self.active_pane;
        if self.pane_settings[ap].pane_mode == PaneMode::UvMap {
            let pds = &mut self.pane_settings[ap];
            pds.uv_zoom = (pds.uv_zoom * (1.0 + delta * 0.1)).clamp(0.1, 50.0);
        } else {
            self.for_each_target_cam(|cam| cam.handle_scroll(delta));
        }
    }
}
