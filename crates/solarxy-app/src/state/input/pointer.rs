//! Pointer handling: what a click, a drag and a wheel mean.
//!
//! A click that did not travel walks a ladder before it lands, in the
//! browser's order: a transform drag in flight owns the release, then a
//! pending re-anchor, then review mode, then an ordinary pick. The drag and
//! the pick are built; the two review rungs arrive with review, and the
//! ladder is written so each slots in above the pick rather than around it.
//!
//! Picking asks the engine, never the shell. The engine answers with the
//! node that produced what is under the cursor, which is what a selection
//! is, and a marker pick rides the same call so a light with no geometry is
//! as clickable as a mesh.

use std::time::Instant;

use winit::event::MouseButton;

use crate::gui::{ContextTarget, ViewportContextMenu};
use solarxy_core::preferences::PaneMode;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::input::PointerButton;

use solarxy_core::scene::SceneObjectId;

use super::click::{CLICK_SLOP_PX, Click, DOUBLE_CLICK_INTERVAL, DOUBLE_CLICK_PX};
use crate::state::State;

fn to_pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Left,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Right => PointerButton::Right,
        _ => PointerButton::Other,
    }
}

/// The 3D pane under the cursor as a pick or a drag sees it: its rect in
/// physical pixels, its camera with the pane's aspect, and the world ray
/// through the cursor. One recipe for both, so the gizmo grabs what the
/// pick would hit.
pub(in crate::state) struct PaneView {
    pub index: usize,
    pub rect: crate::state::Pane,
    pub camera: solarxy_renderer::camera::Camera,
    pub ray: crate::state::raycast::Ray,
}

/// The ray under the cursor through one 3D pane, and what the engine's
/// marker pick needs to judge a click against the pane's light markers.
pub(in crate::state) struct PaneRay {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub markers: Option<solarxy_graph::engine::MarkerPick>,
}

impl State {
    /// The 3D pane under the cursor as a pick or a drag sees it: its rect,
    /// its camera with the pane's aspect, and the world ray through the
    /// cursor. `None` when the cursor is not over a 3D pane with a camera.
    ///
    /// The whole pane rect, not the content rect under the toolbar strip:
    /// the pane renders with the aspect of its full rect and the strip
    /// floats over the picture, so a ray built over the shorter rect would
    /// land above what the cursor is on by the strip's share. The old
    /// raycast did exactly that; the browser builds over the full rect.
    pub(in crate::state) fn pane_view(&self) -> Option<PaneView> {
        let panes = self.compute_panes();
        let cursor = self.input.cursor_pos;
        let pane = crate::state::hit_test_pane(&panes, cursor);
        if self.view.pane_settings[pane].pane_mode != PaneMode::Scene3D {
            return None;
        }
        let rect = panes[pane];
        let mut camera = self.view.cameras[pane].as_ref().map(|c| c.camera)?;
        camera.aspect = rect.width.max(1.0) / rect.height.max(1.0);
        let ray = crate::state::raycast::screen_to_world_ray(
            (cursor.0 - rect.x, cursor.1 - rect.y),
            (rect.width, rect.height),
            camera.build_view_projection_matrix(),
        );
        Some(PaneView {
            index: pane,
            rect,
            camera,
            ray,
        })
    }

    /// The ray under the cursor, or `None` when the cursor is not over a
    /// 3D pane with a camera.
    ///
    /// Physical pixels throughout, matching the cursor, so the marker
    /// radius scales with the window's pixel ratio: a marker is drawn at a
    /// logical size and a click radius in physical pixels would make it half
    /// as clickable as it looks on a high-density display. Markers are
    /// offered only when the pane draws them, so a pane with them off picks
    /// exactly the geometry it shows.
    pub(in crate::state) fn pane_ray(&self) -> Option<PaneRay> {
        let view = self.pane_view()?;
        let cursor = self.input.cursor_pos;
        let rect = view.rect;
        let cursor_px = (cursor.0 - rect.x, cursor.1 - rect.y);
        let ppp = self.window.scale_factor() as f32;
        let markers = self.view.pane_settings[view.index]
            .show_light_markers
            .then(|| solarxy_graph::engine::MarkerPick {
                view_proj: view.camera.build_view_projection_matrix().into(),
                viewport_px: [rect.width, rect.height],
                cursor_px: [cursor_px.0, cursor_px.1],
                radius_px: solarxy_renderer::manipulator::MARKER_PX * ppp,
            });
        Some(PaneRay {
            origin: view.ray.origin.into(),
            direction: view.ray.direction.into(),
            markers,
        })
    }

    /// The root node that produced what is under the cursor, or `None`
    /// over empty space. A light marker wins over geometry when the pane
    /// draws markers, which the engine decides.
    pub(in crate::state) fn pick_under_cursor(&self) -> Option<NodeId> {
        let engine = self.engine.as_deref()?;
        let ray = self.pane_ray()?;
        engine.pick(ray.origin, ray.direction, ray.markers)
    }

    /// Open the viewport right-click context menu.
    ///
    /// It opens on empty space too, with only the entries that make sense
    /// there enabled. A menu that sometimes fails to appear reads as a broken
    /// gesture, and framing the view is worth reaching for wherever the
    /// pointer happens to be.
    ///
    /// The menu acts on what is under the pointer, which the browser's does
    /// not: its right-click only positions the menu, which then reads the
    /// selection. Picking here is the ruled exception rather than an
    /// oversight, because a menu that acts on something other than what was
    /// clicked is the confusing one.
    pub fn open_viewport_context_menu(&mut self) {
        let target = self.pick_under_cursor().and_then(|node| {
            let object = SceneObjectId(node.0);
            let visible = self.raster.scene().get(object)?.visible;
            // Whether the node has a transform at all is the registry's
            // answer, not a list kept here: the reset writes exactly the
            // parameters that node declares, and a type declaring none has
            // nothing to reset rather than a reset that does nothing.
            let resettable = self
                .engine
                .as_ref()
                .is_some_and(|engine| engine.transform_params(GraphContext::Root, node).is_some());
            Some(ContextTarget {
                object,
                visible,
                resettable,
            })
        });
        let ppp = self.window.scale_factor() as f32;
        let tools = self.tool_readout();
        self.viewport_context_menu = Some(ViewportContextMenu {
            target,
            tools: crate::gui::ToolRows {
                armed: tools.tool,
                applies: tools.applies,
            },
            screen_pos: egui::pos2(self.input.cursor_pos.0 / ppp, self.input.cursor_pos.1 / ppp),
            suppress_dismiss: true,
        });
    }

    /// A primary-button click that did not travel, in a 3D pane.
    ///
    /// A hit selects the producing node at the root and brings the graph
    /// surfaces back to the root to show it, since a selection made inside
    /// a container would be invisible from the viewport. A miss leaves the
    /// selection as it was, which is the browser's rule: its ladder
    /// dispatches only on a hit.
    ///
    /// A double-click additionally dives into the node when its type opens
    /// a network. The first click of the pair already selected, as the
    /// browser's click and dblclick both fire.
    fn viewport_click(&mut self, click: Click) {
        let Some(hit) = self.pick_under_cursor() else {
            return;
        };
        self.gui.set_graph_ctx(GraphContext::Root);
        self.handle_selection(GraphContext::Root, vec![hit]);
        if click == Click::Double && self.opens_network(hit) {
            self.gui.set_graph_ctx(GraphContext::Subflow(hit));
        }
    }

    /// Whether a root node's type opens a child network. Asked of the
    /// registry rather than compared against a type id: picking returns
    /// lights as well as containers, and diving into a light would leave
    /// the canvas showing a context that does not exist.
    fn opens_network(&self, node: NodeId) -> bool {
        self.engine.as_deref().is_some_and(|engine| {
            engine
                .document()
                .graph(GraphContext::Root)
                .ok()
                .and_then(|graph| graph.node(node))
                .is_some_and(|data| engine.registry().opens(&data.type_id).is_some())
        })
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
            if mapped == PointerButton::Left {
                // A press with a transform tool armed grabs a handle if one
                // is under the cursor, and the whole gesture is then the
                // gizmo's: no navigation, no click. A miss falls through to
                // the camera and the pick.
                if pressed && self.gizmo.tool.is_transform_tool() && self.begin_gizmo_drag() {
                    return;
                }
                if !pressed && self.gizmo_dragging() {
                    self.commit_gizmo_drag();
                    return;
                }
            }
            // Only the buttons the camera navigates with count: a right or
            // side button is ignored by the controller, so a drag with one
            // held must not read as navigation and release a binding.
            if matches!(mapped, PointerButton::Left | PointerButton::Middle) {
                self.input.nav_button_down = pressed;
            }
            self.for_each_target_cam(|cam| cam.handle_mouse_button(mapped, pressed));

            if mapped == PointerButton::Left {
                let cursor = self.input.cursor_pos;
                if pressed {
                    self.input.clicks.press(cursor);
                } else {
                    let ppp = self.window.scale_factor() as f32;
                    let click = self.input.clicks.release(
                        cursor,
                        Instant::now(),
                        DOUBLE_CLICK_INTERVAL,
                        DOUBLE_CLICK_PX * ppp,
                    );
                    if let Some(click) = click {
                        self.viewport_click(click);
                    }
                }
            }
        }
    }

    /// The snap modifier as the solver reads it: control, or the command
    /// key on a Mac, where control-click is the secondary click and command
    /// is what the hand reaches for.
    fn gizmo_mods(&self) -> u8 {
        if self.input.modifiers.control_key() || self.input.modifiers.super_key() {
            solarxy_host::gizmo::MOD_SNAP
        } else {
            0
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
            // A drag in flight owns every move; with a tool armed and no
            // button held, a move only decides which handle lights up.
            if self.gizmo_dragging() {
                let mods = self.gizmo_mods();
                self.update_gizmo_drag(mods);
                return;
            }
            if self.gizmo.tool.is_transform_tool() && !self.input.nav_button_down {
                self.update_gizmo_hover();
            }
            let ppp = self.window.scale_factor() as f32;
            self.input.clicks.moved_to((x, y), CLICK_SLOP_PX * ppp);
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
                // An orbit drag stays local to the active pane so linked
                // orthographic panes keep their axis lock. Pan and zoom
                // still propagate via `for_each_target_cam`.
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
