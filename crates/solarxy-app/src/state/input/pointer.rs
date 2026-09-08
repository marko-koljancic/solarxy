//! Pointer handling: what a click, a drag and a wheel mean.
//!
//! A click in review mode used to walk a three-step ladder before it landed.
//! Review anchors against a file-loaded model's meshes and cannot arm while
//! there is no such root, so the ladder came out with it; what the repointed
//! review picks against is the engine's anchor model, not this one.

use winit::event::MouseButton;

use solarxy_renderer::camera_state::CameraState;
use crate::gui::ViewportContextMenu;
use solarxy_renderer::input::PointerButton;
use solarxy_core::preferences::PaneMode;

use cgmath::{SquareMatrix, Transform};
use solarxy_core::scene::SceneObjectId;

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
    /// The frontmost **visible** scene object under the cursor, via a CPU
    /// raycast through the active 3D pane's content rect. `None` if the cursor
    /// is not over a `Scene3D` pane or hits nothing.
    ///
    /// Objects rather than meshes, because an object is what a document can
    /// durably say something about: its visibility is a parameter on the node
    /// that owns it, where a cooked mesh is an artefact of the cook and is
    /// re-derived on the next one.
    ///
    /// The ray is transformed into each object's own space rather than the
    /// geometry into the world, which is the same trick the hierarchy
    /// traversal uses and costs nothing per triangle. The direction is
    /// deliberately **not** renormalized, so the hit distances stay comparable
    /// across objects at different scales.
    pub(in crate::state) fn hovered_object(&self) -> Option<SceneObjectId> {
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

        let mut best: Option<(f32, SceneObjectId)> = None;
        for (id, object) in self.raster.scene().iter() {
            // A hidden object you cannot see must not steal the pick from the
            // geometry behind it.
            if !object.visible {
                continue;
            }
            let Some(inverse) = object.transform.invert() else {
                continue;
            };
            let local = crate::state::raycast::Ray {
                origin: inverse.transform_point(ray.origin),
                direction: inverse.transform_vector(ray.direction),
            };

            let mut views: Vec<crate::state::raycast::MeshView<'_>> = Vec::new();
            for (i, mesh) in object.model.meshes.iter().enumerate() {
                if !mesh.visible {
                    continue;
                }
                if let (Some(cpu), Some(bounds)) = (
                    object.model.cpu_meshes.get(i),
                    object.model.mesh_bounds.get(i),
                ) {
                    views.push(crate::state::raycast::MeshView {
                        positions: &cpu.positions,
                        indices: &cpu.indices,
                        bounds: *bounds,
                    });
                }
            }
            if let Some(hit) = crate::state::raycast::raycast_meshes(&local, &views)
                && best.is_none_or(|(t, _)| hit.distance < t)
            {
                best = Some((hit.distance, *id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Open the viewport right-click context menu when the cursor is over an
    /// object; right-clicking empty space clears any open menu.
    pub fn open_viewport_context_menu(&mut self) {
        let hit = self
            .hovered_object()
            .and_then(|id| self.raster.scene().get(id).map(|o| (id, o.visible)));
        self.viewport_context_menu = hit.map(|(object, visible)| {
            let ppp = self.window.scale_factor() as f32;
            ViewportContextMenu {
                object,
                visible,
                screen_pos: egui::pos2(
                    self.input.cursor_pos.0 / ppp,
                    self.input.cursor_pos.1 / ppp,
                ),
                suppress_dismiss: true,
            }
        });
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
