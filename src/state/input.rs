use winit::event::MouseButton;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;

use crate::cgi::camera_state::CameraState;
use crate::cgi::ibl::IblState;
use crate::cgi::resources;
use crate::preferences::{self, IblMode, NormalsMode, ProjectionMode, UvMode, ViewMode};

use super::{BoundsMode, State, ViewLayout};

impl State {
    fn for_each_target_cam(&mut self, mut f: impl FnMut(&mut CameraState)) {
        let (primary, secondary) = super::cam_routing(self.active_pane, self.cameras_linked);
        if primary {
            if let Some(scene) = &mut self.scene {
                f(&mut scene.cam);
            }
        }
        if secondary {
            if let Some(cam) = &mut self.secondary_cam {
                f(cam);
            }
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
                if self.modifiers.shift_key() {
                    self.toggle_ssao();
                } else {
                    self.for_each_target_cam(|cam| {
                        cam.set_projection(ProjectionMode::Orthographic);
                    });
                }
            }
            KeyCode::KeyW => {
                if self.modifiers.shift_key() {
                    self.display.line_weight = self.display.line_weight.next();
                    self.update_wireframe_params();
                    self.gui.set_toast(
                        &format!("Line Weight: {}", self.display.line_weight),
                        [0.0, 0.4, 0.0, 1.0],
                    );
                } else if self.display.view_mode == ViewMode::Ghosted {
                    self.display.ghosted_wireframe = !self.display.ghosted_wireframe;
                } else {
                    self.display.view_mode = match self.display.view_mode {
                        ViewMode::Shaded => ViewMode::ShadedWireframe,
                        ViewMode::ShadedWireframe => ViewMode::WireframeOnly,
                        ViewMode::WireframeOnly => ViewMode::Shaded,
                        ViewMode::Ghosted => unreachable!(),
                    };
                }
            }
            KeyCode::KeyX => {
                if self.display.view_mode == ViewMode::Ghosted {
                    self.display.view_mode = self.display.prev_non_ghosted_mode;
                } else {
                    self.display.prev_non_ghosted_mode = self.display.view_mode;
                    self.display.ghosted_wireframe = matches!(
                        self.display.view_mode,
                        ViewMode::ShadedWireframe | ViewMode::WireframeOnly
                    );
                    self.display.view_mode = ViewMode::Ghosted;
                }
            }
            KeyCode::KeyS => {
                if self.modifiers.shift_key() {
                    self.save_preferences();
                } else {
                    self.display.view_mode = ViewMode::Shaded;
                }
            }
            KeyCode::KeyC => {
                if self.scene.is_some() {
                    self.capture_requested = true;
                }
            }
            KeyCode::KeyA => {
                if self.modifiers.shift_key() {
                    self.display.show_local_axes = !self.display.show_local_axes;
                    let msg = if self.display.show_local_axes {
                        "Local Axes: On"
                    } else {
                        "Local Axes: Off"
                    };
                    self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
                } else {
                    self.display.show_axis_gizmo = !self.display.show_axis_gizmo;
                }
            }
            KeyCode::KeyG => self.display.show_grid = !self.display.show_grid,
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
                self.display.normals_mode = match self.display.normals_mode {
                    NormalsMode::Off => NormalsMode::Face,
                    NormalsMode::Face => NormalsMode::Vertex,
                    NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                    NormalsMode::FaceAndVertex => NormalsMode::Off,
                };
            }
            KeyCode::KeyV => self.display.turntable_active = !self.display.turntable_active,
            KeyCode::KeyU => {
                self.display.uv_mode = match self.display.uv_mode {
                    UvMode::Off => UvMode::Gradient,
                    UvMode::Gradient => UvMode::Checker,
                    UvMode::Checker => UvMode::Off,
                };
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
                self.active_pane = 0;
                self.display.layout = ViewLayout::Single;
                self.gui.set_toast("Single Viewport", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::F2 => {
                if self.display.layout == ViewLayout::Single
                    && let Some(scene) = &self.scene
                {
                    self.secondary_cam = Some(
                        scene
                            .cam
                            .clone_with_new_resources(&self.device, &self.layouts.camera),
                    );
                }
                self.display.layout = ViewLayout::SplitVertical;
                self.gui.set_toast("Split Vertical", [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::F3 => {
                if self.display.layout == ViewLayout::Single
                    && let Some(scene) = &self.scene
                {
                    self.secondary_cam = Some(
                        scene
                            .cam
                            .clone_with_new_resources(&self.device, &self.layouts.camera),
                    );
                }
                self.display.layout = ViewLayout::SplitHorizontal;
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
        self.update_wireframe_params();
        self.update_grid_color();
        let (top, bottom) = self.display.background_mode.sky_colors();
        self.ibl_res.ibl = IblState::from_sky_colors(&self.device, &self.queue, top, bottom);
        self.rebuild_light_bind_group();
    }

    pub(super) fn apply_composite_params(&self) {
        self.write_composite_params();
    }

    pub(super) fn apply_ibl_change(&mut self) {
        self.rebuild_light_bind_group();
    }

    fn cycle_background(&mut self) {
        self.display.background_mode = self.display.background_mode.next();
        self.apply_background_change();
    }

    fn cycle_bounds_mode(&mut self) {
        let is_multi = self
            .scene
            .as_ref()
            .is_some_and(|s| s.model.meshes.len() > 1);
        self.display.bounds_mode = match self.display.bounds_mode {
            BoundsMode::Off => BoundsMode::WholeModel,
            BoundsMode::WholeModel if is_multi => BoundsMode::PerMesh,
            BoundsMode::WholeModel | BoundsMode::PerMesh => BoundsMode::Off,
        };
        let msg = match self.display.bounds_mode {
            BoundsMode::Off => "Bounds: Off",
            BoundsMode::WholeModel => "Bounds: Whole Model",
            BoundsMode::PerMesh => "Bounds: Per Mesh",
        };
        self.gui.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
    }

    fn save_preferences(&mut self) {
        self.preferences.display.background = self.display.background_mode;
        self.preferences.display.view_mode = self.display.view_mode;
        self.preferences.display.normals_mode = self.display.normals_mode;
        self.preferences.display.grid_visible = self.display.show_grid;
        self.preferences.display.axis_gizmo_visible = self.display.show_axis_gizmo;
        self.preferences.display.local_axes_visible = self.display.show_local_axes;
        self.preferences.display.bloom_enabled = self.post.bloom_enabled;
        self.preferences.display.ssao_enabled = self.post.ssao_enabled;
        self.preferences.display.uv_mode = self.display.uv_mode;
        self.preferences.display.turntable_active = self.display.turntable_active;
        self.preferences.display.turntable_rpm = self.display.turntable_rpm;
        if let Some(scene) = &self.scene {
            self.preferences.display.projection_mode = scene.cam.camera.projection;
        }
        self.preferences.rendering.wireframe_line_weight = self.display.line_weight;
        self.preferences.lighting.lock = self.display.lights_locked;
        self.preferences.display.ibl_mode = self.ibl_res.ibl_mode;
        self.preferences.display.tone_mode = self.post.tone_mode;
        self.preferences.display.exposure = self.post.exposure;

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
        self.for_each_target_cam(|cam| cam.handle_mouse_button(button, pressed));
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        self.for_each_target_cam(|cam| cam.handle_mouse_move(x, y));
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        self.for_each_target_cam(|cam| cam.handle_scroll(delta));
    }
}
