//! Pointer handling: what a click, a drag and a wheel mean, and the three-step
//! ladder a click in review mode walks before it lands.

use winit::event::MouseButton;

use solarxy_renderer::camera_state::CameraState;
use crate::gui::{ToastSeverity, ViewportContextMenu};
use solarxy_renderer::input::PointerButton;
use solarxy_core::preferences::PaneMode;

use crate::state::State;

fn to_pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Left,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Right => PointerButton::Right,
        _ => PointerButton::Other,
    }
}

impl State {
    /// Model index of the frontmost **visible** mesh under the cursor, via
    /// a CPU raycast through the active 3D pane's content rect. `None` if
    /// the cursor is not over a `Scene3D` pane or hits no visible mesh.
    pub(in crate::state) fn hovered_mesh(&self) -> Option<usize> {
        let scene = self.scene.as_ref()?;
        let panes = self.compute_panes();
        let cursor = self.input.cursor_pos;
        let pane_idx = crate::state::hit_test_pane(&panes, cursor);
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
            let mapped = to_pointer_button(button);
            // Only the buttons the camera navigates with count: a right or
            // side button is ignored by the controller, so a drag with one
            // held must not read as navigation and release a binding.
            if matches!(mapped, PointerButton::Left | PointerButton::Middle) {
                self.input.nav_button_down = pressed;
            }
            self.for_each_target_cam(|cam| cam.handle_mouse_button(mapped, pressed));
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
        let pane_idx = crate::state::hit_test_pane(&panes, cursor);
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
            return true;
        }

        // The click missed every marker — it lands on geometry or empty
        // space. Either way, collapse any open card (B4): selection is
        // cleared before the new-annotation draft (if any) opens.
        self.review.selected = None;

        let ray =
            crate::state::raycast::screen_to_world_ray(local, (pane.width, pane.height), view_proj);

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
            // A move with a camera button held is a navigation drag, and a
            // drag on a bound pane takes the view over. A plain move or a
            // click-release never releases anything.
            if self.input.nav_button_down {
                self.release_look_through_for_gesture();
            }
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
            self.release_look_through_for_gesture();
            self.for_each_target_cam(|cam| cam.handle_scroll(delta));
        }
    }
}
