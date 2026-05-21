//! Keyboard + mouse input dispatch for the `State`-rooted application.
//!
//! Submodules:
//! - `dialogs` — native file pickers (model open, HDRI import,
//!   screenshot save) via the `rfd` crate. Returns to the event loop;
//!   results land in `State::pending_load`.
//! - `menu_actions` — menu-bar event flags ([`super::super::gui::MenuActions`])
//!   draining: file/HDRI dialogs, preferences modal, view layout,
//!   recent-file opens, etc.
//!
//! The keyboard map lives in this module's `handle_key_pressed`; see
//! `gui::keyboard_shortcuts_modal` for the user-facing reference. Adding
//! a new binding means a match arm here PLUS an entry in the shortcuts
//! modal — they should never disagree.

mod dialogs;
mod menu_actions;

use winit::event::MouseButton;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::KeyCode;

use solarxy_renderer::camera_state::CameraState;
use crate::gui::{OutlinerAction, ToastSeverity, ViewportContextMenu};
use solarxy_renderer::ibl::IblState;
use solarxy_core::preferences::{
    self, BackgroundMode, BuiltinBg, CustomBackground, IblMode, InspectionMode, MaterialOverride,
    NormalsMode, PaneMode, ProjectionMode, UvMode, ViewMode,
};
use solarxy_core::validation::IssueScope;

use super::{BackgroundModeExt, BoundsMode, State, ViewLayout};

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
    /// Apply `f` to each pane camera the current gesture targets: the
    /// active pane, or — when cameras are linked — every pane the layout
    /// uses. UV-map panes are skipped.
    fn for_each_target_cam(&mut self, mut f: impl FnMut(&mut CameraState)) {
        let count = self.view.display.layout.pane_count();
        let active = self.view.active_pane;
        let linked = self.view.cameras_linked;
        for i in 0..count {
            if (linked || i == active)
                && self.view.pane_settings[i].pane_mode == PaneMode::Scene3D
                && let Some(cam) = &mut self.view.cameras[i]
            {
                f(cam);
            }
        }
    }

    pub fn set_modifiers(&mut self, modifiers: winit::keyboard::ModifiersState) {
        self.input.modifiers = modifiers;
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
                if self.input.modifiers.shift_key() {
                    self.hide_hovered_mesh();
                } else if self.input.modifiers.alt_key() {
                    self.show_all_meshes();
                } else {
                    let bounds = self.scene.as_ref().map(|s| s.model.bounds);
                    if let Some(bounds) = bounds {
                        self.for_each_target_cam(|cam| cam.reset_to_bounds(&bounds));
                    }
                }
            }
            KeyCode::Slash => {
                self.isolate_hovered_mesh();
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
                if self.input.modifiers.shift_key() {
                    self.toggle_review_mode();
                } else {
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
            _ => {
                self.for_each_target_cam(|cam| {
                    cam.handle_key(code, is_pressed);
                });
            }
        }
    }

    fn write_composite_params(&self) {
        let active_inspection = self.view.pane_settings[self.view.active_pane].inspection_mode;
        self.renderer.post.composite.write_params(
            &self.queue,
            self.renderer.post.bloom_enabled,
            self.renderer.post.ssao_enabled,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
            active_inspection,
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
        if self.view.active_pane != 0 {
            return;
        }
        let bg = self.view.pane_settings[0].background_mode;
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
    pub(super) fn toggle_review_mode(&mut self) {
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
        self.rebuild_light_bind_group();
        self.gui.clear_hdri_info();
        self.gui.set_toast("HDRI cleared", ToastSeverity::Success);
    }

    /// Fly the active pane's camera to frame the mesh a validation issue
    /// lives on (Properties → Validation row click) and enable that
    /// pane's per-face validation overlay so the defect is visible.
    pub(super) fn fly_to_validation_issue(&mut self, idx: usize) {
        let aabb = self.scene.as_ref().and_then(|scene| {
            scene.validation.issues.get(idx).and_then(|issue| {
                resolve_issue_aabb(&issue.scope, &scene.model, &scene.validation_raw_to_gpu)
            })
        });
        let Some(aabb) = aabb else {
            return;
        };
        self.view.pane_settings[self.view.active_pane].show_validation = true;
        self.frame_active_pane(aabb);
    }

    /// Smoothly fly the active pane's camera to frame `bounds`.
    fn frame_active_pane(&mut self, bounds: solarxy_core::AABB) {
        if let Some(cam) = &mut self.view.cameras[self.view.active_pane] {
            cam.reset_to_bounds(&bounds);
        }
    }

    /// Apply an [`OutlinerAction`] (mesh / material visibility or camera
    /// framing) raised by the Outliner panel.
    pub(super) fn handle_outliner_action(&mut self, action: OutlinerAction) {
        match action {
            OutlinerAction::ToggleMesh(i) => {
                if let Some(scene) = &mut self.scene
                    && let Some(mesh) = scene.model.meshes.get_mut(i)
                {
                    mesh.visible = !mesh.visible;
                }
            }
            OutlinerAction::HideMesh(i) => {
                if let Some(scene) = &mut self.scene
                    && let Some(mesh) = scene.model.meshes.get_mut(i)
                {
                    mesh.visible = false;
                }
            }
            OutlinerAction::IsolateMesh(i) => {
                if let Some(scene) = &mut self.scene {
                    for (j, mesh) in scene.model.meshes.iter_mut().enumerate() {
                        mesh.visible = j == i;
                    }
                }
            }
            OutlinerAction::ShowAll => {
                if let Some(scene) = &mut self.scene {
                    for mesh in &mut scene.model.meshes {
                        mesh.visible = true;
                    }
                }
            }
            OutlinerAction::ToggleMaterial(mat) => {
                if let Some(scene) = &mut self.scene {
                    let all_visible = scene
                        .model
                        .meshes
                        .iter()
                        .filter(|m| m.material == mat)
                        .all(|m| m.visible);
                    for mesh in &mut scene.model.meshes {
                        if mesh.material == mat {
                            mesh.visible = !all_visible;
                        }
                    }
                }
            }
            OutlinerAction::FrameMesh(i) => {
                let aabb = self
                    .scene
                    .as_ref()
                    .and_then(|s| s.model.mesh_bounds.get(i).copied());
                if let Some(aabb) = aabb {
                    self.frame_active_pane(aabb);
                }
            }
            OutlinerAction::FrameMaterial(mat) => {
                let aabb = self
                    .scene
                    .as_ref()
                    .and_then(|s| material_meshes_aabb(&s.model, mat));
                if let Some(aabb) = aabb {
                    self.frame_active_pane(aabb);
                }
            }
        }
    }

    /// Model index of the frontmost **visible** mesh under the cursor, via
    /// a CPU raycast through the active 3D pane's content rect. `None` if
    /// the cursor is not over a `Scene3D` pane or hits no visible mesh.
    pub(super) fn hovered_mesh(&self) -> Option<usize> {
        let scene = self.scene.as_ref()?;
        let panes = self.compute_panes();
        let cursor = self.input.cursor_pos;
        let pane_idx = super::hit_test_pane(&panes, cursor);
        if self.view.pane_settings[pane_idx].pane_mode != PaneMode::Scene3D {
            return None;
        }
        let content = panes[pane_idx].content(self.pane_toolbar_height_px());
        let mut camera = self.view.cameras[pane_idx].as_ref().map(|c| c.camera)?;
        camera.aspect = content.width.max(1.0) / content.height.max(1.0);
        let ray = crate::state::raycast::screen_to_world_ray(
            (cursor.0 - content.x, cursor.1 - content.y),
            (content.width, content.height),
            camera.build_view_projection_matrix(),
            camera.eye,
        );

        // Raycast only visible meshes — a hidden mesh you cannot see must
        // not steal the pick from the geometry behind it.
        let mut model_index: Vec<usize> = Vec::new();
        let mut views: Vec<crate::state::raycast::MeshView<'_>> = Vec::new();
        for (i, mesh) in scene.model.meshes.iter().enumerate() {
            if !mesh.visible {
                continue;
            }
            if let (Some(cpu), Some(bounds)) = (
                scene.model.cpu_meshes.get(i),
                scene.model.mesh_bounds.get(i),
            ) {
                model_index.push(i);
                views.push(crate::state::raycast::MeshView {
                    positions: &cpu.positions,
                    indices: &cpu.indices,
                    bounds: *bounds,
                });
            }
        }
        crate::state::raycast::raycast_meshes(&ray, &views)
            .map(|hit| model_index[hit.mesh_index as usize])
    }

    /// Open the viewport right-click context menu when the cursor is over
    /// a mesh; right-clicking empty space clears any open menu.
    pub fn open_viewport_context_menu(&mut self) {
        self.viewport_context_menu = self.hovered_mesh().map(|mesh_index| {
            let ppp = self.window.scale_factor() as f32;
            ViewportContextMenu {
                mesh_index,
                screen_pos: egui::pos2(
                    self.input.cursor_pos.0 / ppp,
                    self.input.cursor_pos.1 / ppp,
                ),
                suppress_dismiss: true,
            }
        });
    }

    /// `Shift+H` — hide the mesh under the cursor.
    fn hide_hovered_mesh(&mut self) {
        if self.gui.any_popup_open() || self.viewport_context_menu.is_some() {
            return;
        }
        match self.hovered_mesh() {
            Some(mesh) => self.handle_outliner_action(OutlinerAction::HideMesh(mesh)),
            None => self
                .gui
                .set_toast("No mesh under cursor", ToastSeverity::Info),
        }
    }

    /// `Alt+H` — make every mesh visible again.
    fn show_all_meshes(&mut self) {
        if self.gui.any_popup_open() || self.viewport_context_menu.is_some() {
            return;
        }
        self.handle_outliner_action(OutlinerAction::ShowAll);
    }

    /// `/` — hide every mesh except the one under the cursor.
    fn isolate_hovered_mesh(&mut self) {
        if self.gui.any_popup_open() || self.viewport_context_menu.is_some() {
            return;
        }
        match self.hovered_mesh() {
            Some(mesh) => self.handle_outliner_action(OutlinerAction::IsolateMesh(mesh)),
            None => self
                .gui
                .set_toast("No mesh under cursor", ToastSeverity::Info),
        }
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
        if let Some(cam) = &self.view.cameras[0] {
            self.preferences.display.projection_mode = cam.camera.projection;
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
                self.gui
                    .set_toast("Preferences saved", ToastSeverity::Success);
            }
            Err(e) => {
                self.gui
                    .set_toast(&format!("Save failed: {}", e), ToastSeverity::Error);
            }
        }
    }

    /// Auto-save the current dock layout into `preferences.dock.last_layout_json`
    /// and flush preferences to disk. Called on app exit so the next launch
    /// restores the layout the user actually left behind. Silent on failure —
    /// the user is on their way out and a toast wouldn't be seen anyway.
    pub fn flush_dock_layout_on_exit(&mut self) {
        let Some(json) = self.gui.serialize_layout() else {
            return;
        };
        if self.preferences.dock.last_layout_json.as_ref() == Some(&json) {
            return;
        }
        self.preferences.dock.last_layout_json = Some(json);
        if let Err(e) = preferences::save(&self.preferences) {
            tracing::warn!("Failed to persist dock layout on exit: {e}");
        }
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if pressed
            && matches!(button, MouseButton::Left)
            && self.review.active
            && self.try_review_pick()
        {
            return;
        }

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

    /// Resolve a review-mode click. Returns `true` if the click was
    /// consumed (the caller should not pass it down to camera handling).
    ///
    /// Routing order:
    /// 1. **Re-anchor pending** — raycast geometry and route the hit
    ///    through `ReviewState::complete_reanchor`. Always consumes the
    ///    click; never falls through to the other paths.
    /// 2. **Marker hit-test** — project visible markers to screen space
    ///    and check distance to the cursor. Within ~20 px ⇒ select that
    ///    annotation (cyan ring + panel scroll). Consumes the click.
    /// 3. **New annotation** — raycast geometry and open a fresh
    ///    `EditDraft` popup at the cursor; or toast "Click on the model
    ///    surface" on a miss. Consumes the click either way.
    fn try_review_pick(&mut self) -> bool {
        if self.review.editing.is_some() {
            return true;
        }
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };

        let panes = self.compute_panes();
        let cursor = self.input.cursor_pos;
        let pane_idx = super::hit_test_pane(&panes, cursor);
        let pane = &panes[pane_idx];

        if self.view.pane_settings[pane_idx].pane_mode != PaneMode::Scene3D {
            return false;
        }

        let Some(camera) = self.view.cameras[pane_idx].as_ref().map(|c| c.camera) else {
            return false;
        };

        let view_proj = camera.build_view_projection_matrix();
        let local = (cursor.0 - pane.x, cursor.1 - pane.y);

        if let Some(target_id) = self.review.reanchor_target.clone() {
            let ray = crate::state::raycast::screen_to_world_ray(
                local,
                (pane.width, pane.height),
                view_proj,
                camera.eye,
            );
            let model = &scene.model;
            let views: Vec<crate::state::raycast::MeshView<'_>> = model
                .cpu_meshes
                .iter()
                .zip(model.mesh_bounds.iter())
                .map(|(m, b)| crate::state::raycast::MeshView {
                    positions: &m.positions,
                    indices: &m.indices,
                    bounds: *b,
                })
                .collect();
            let preview = self.review.find(&target_id).map_or_else(
                || "annotation".to_string(),
                |a| crate::state::review::short_text_preview(&a.text),
            );
            match crate::state::raycast::raycast_meshes(&ray, &views) {
                Some(hit) => {
                    if self.review.complete_reanchor(&hit) {
                        self.gui.set_toast(
                            &format!("Re-anchored \u{201C}{preview}\u{201D}"),
                            ToastSeverity::Success,
                        );
                    }
                }
                None => {
                    self.gui.set_toast(
                        "No surface under cursor \u{2014} try again",
                        ToastSeverity::Info,
                    );
                }
            }
            return true;
        }

        if let Some(id) =
            self.review
                .marker_at_screen_pos(local, (pane.width, pane.height), view_proj, 20.0)
        {
            self.review.selected = Some(id);
            self.review.scroll_to_selected = true;
            self.review.dirty = true;
            return true;
        }

        let ray = crate::state::raycast::screen_to_world_ray(
            local,
            (pane.width, pane.height),
            view_proj,
            camera.eye,
        );

        let model = &scene.model;
        let views: Vec<crate::state::raycast::MeshView<'_>> = model
            .cpu_meshes
            .iter()
            .zip(model.mesh_bounds.iter())
            .map(|(m, b)| crate::state::raycast::MeshView {
                positions: &m.positions,
                indices: &m.indices,
                bounds: *b,
            })
            .collect();

        match crate::state::raycast::raycast_meshes(&ray, &views) {
            Some(hit) => {
                let anchor = solarxy_core::review::AnchorPosition {
                    mesh_index: hit.mesh_index,
                    face_index: hit.face_index,
                    barycentric: hit.barycentric,
                    world_pos_fallback: [hit.world_pos.x, hit.world_pos.y, hit.world_pos.z],
                };
                let seq = self.review.alloc_draft_seq();
                self.review.editing =
                    Some(crate::state::review::EditDraft::new_at(seq, anchor, cursor));
            }
            None => {
                self.gui.set_toast(
                    "Click on the model surface to annotate",
                    ToastSeverity::Info,
                );
            }
        }
        true
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
            let ap = self.view.active_pane;
            let orbiting = self.view.cameras[ap]
                .as_ref()
                .is_some_and(CameraState::is_orbiting);
            if orbiting {
                // CL-5: an orbit drag stays local to the active pane so
                // linked orthographic panes keep their axis lock. Pan and
                // zoom still propagate via `for_each_target_cam`.
                if self.view.pane_settings[ap].pane_mode == PaneMode::Scene3D
                    && let Some(cam) = &mut self.view.cameras[ap]
                {
                    cam.handle_mouse_move(x, y);
                }
            } else {
                self.for_each_target_cam(|cam| cam.handle_mouse_move(x, y));
            }
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

/// Resolve a validation issue's scope to an AABB for camera fly-to.
/// Mesh-granular for every scope — the per-face / per-edge validation
/// overlay highlights the exact defect once its mesh is framed. Raw issue
/// mesh indices are remapped to GPU mesh indices via `raw_to_gpu`.
fn resolve_issue_aabb(
    scope: &IssueScope,
    model: &solarxy_renderer::model::Model,
    raw_to_gpu: &[Option<usize>],
) -> Option<solarxy_core::AABB> {
    let gpu_mesh = |raw: usize| raw_to_gpu.get(raw).copied().flatten();
    match scope {
        IssueScope::Model => Some(model.bounds),
        IssueScope::Mesh(raw) | IssueScope::Face(raw, _) => {
            gpu_mesh(*raw).and_then(|g| model.mesh_bounds.get(g).copied())
        }
        IssueScope::Edge { mesh_index, .. } => {
            gpu_mesh(*mesh_index).and_then(|g| model.mesh_bounds.get(g).copied())
        }
        IssueScope::Material(mat) => material_meshes_aabb(model, *mat).or(Some(model.bounds)),
    }
}

/// Union of the bounds of every mesh using `material`, or `None` when no
/// mesh references it. Shared by validation fly-to and the Outliner's
/// frame-material action.
fn material_meshes_aabb(
    model: &solarxy_renderer::model::Model,
    material: usize,
) -> Option<solarxy_core::AABB> {
    let mut acc: Option<solarxy_core::AABB> = None;
    for (i, mesh) in model.meshes.iter().enumerate() {
        if mesh.material == material
            && let Some(b) = model.mesh_bounds.get(i).copied()
        {
            acc = Some(acc.map_or(b, |a| union_aabb(a, b)));
        }
    }
    acc
}

/// Smallest AABB enclosing both inputs.
fn union_aabb(a: solarxy_core::AABB, b: solarxy_core::AABB) -> solarxy_core::AABB {
    solarxy_core::AABB {
        min: cgmath::Point3::new(
            a.min.x.min(b.min.x),
            a.min.y.min(b.min.y),
            a.min.z.min(b.min.z),
        ),
        max: cgmath::Point3::new(
            a.max.x.max(b.max.x),
            a.max.y.max(b.max.y),
            a.max.z.max(b.max.z),
        ),
    }
}
