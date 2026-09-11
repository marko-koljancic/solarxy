//! `App`: the winit `ApplicationHandler`. Owns the event loop, defers
//! window creation to the first `resumed` event (winit 0.30 contract), and
//! forwards events to [`crate::state::State`].

use std::sync::Arc;

use wgpu::SurfaceError;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, PhysicalKey},
    window::Window,
};

use crate::console::LogBuffer;
use solarxy_core::preferences::Preferences;
use crate::state::{ShellKey, State, shell_key};

/// Returns `true` when a pointer/mouse event should drive the 3D camera
/// (orbit / pan / zoom) — i.e., the cursor is inside the Viewport tab's
/// rect AND no blocking modal owns focus. Post-egui_dock replacement for
/// the older `!gui.wants_pointer_input()` guard, which became unusable
/// because `egui_dock`'s tab body registers as a hover-sensing area and
/// caused `wants_pointer_input` to return `true` over the Viewport tab.
fn route_pointer_to_camera(state: &State) -> bool {
    if state.gui.any_blocking_modal_open(&state.review)
        || state.gui.any_popup_open()
        || state.viewport_context_menu.is_some()
    {
        return false;
    }
    let ppp = state.window.scale_factor() as f32;
    let cursor_logical = egui::pos2(
        state.input.cursor_pos.0 / ppp,
        state.input.cursor_pos.1 / ppp,
    );
    // Inside the Viewport tab, and specifically inside a pane's 3D
    // content rect — a click on a per-pane toolbar strip or an open
    // toolbar dropdown must not orbit the scene.
    state.gui.cursor_in_viewport(cursor_logical) && state.pointer_in_pane_content()
}

pub struct App {
    state: Option<State>,
    model_path: Option<String>,
    preferences: Preferences,
    console_buffer: LogBuffer,
    frame_count: u64,
}

impl App {
    pub fn new(
        model_path: Option<String>,
        preferences: Preferences,
        console_buffer: LogBuffer,
    ) -> Self {
        Self {
            state: None,
            model_path,
            preferences,
            console_buffer,
            frame_count: 0,
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn load_window_icon() -> Option<winit::window::Icon> {
    let bytes = include_bytes!("../../../res/bundle/solarxy-256.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}
#[cfg(target_os = "macos")]
fn load_window_icon() -> Option<winit::window::Icon> {
    None
}

impl ApplicationHandler<State> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("Solarxy")
            .with_window_icon(load_window_icon())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.preferences.window.window_width,
                self.preferences.window.window_height,
            ));
        let window = match event_loop.create_window(window_attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create window: {}", e);
                event_loop.exit();
                return;
            }
        };
        match pollster::block_on(State::new(
            window,
            self.model_path.clone(),
            self.preferences.clone(),
            self.console_buffer.clone(),
        )) {
            Ok(state) => self.state = Some(state),
            Err(e) => {
                tracing::error!("Failed to initialize renderer: {}", e);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: State) {
        self.state = Some(event);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.state else {
            return;
        };

        // The window's own bindings, claimed before the interface sees the
        // key. A claimed press is consumed: the key map below never sees it,
        // which is what stops one press from running two handlers. The
        // interface still sees every event, because its key state is built
        // from press and release pairs and a release with no press is a
        // no-op there.
        let mut pre_handled = false;
        if let WindowEvent::KeyboardInput { ref event, .. } = event
            && event.state.is_pressed()
            && let PhysicalKey::Code(code) = event.physical_key
        {
            let mods = state.input.modifiers;
            let cmd_or_ctrl = if cfg!(target_os = "macos") {
                mods.super_key()
            } else {
                mods.control_key()
            };
            let wants_text = state.gui.wants_keyboard_input();
            let over_canvas = state.gui.pointer_over_canvas();
            if let Some(key) =
                shell_key(code, cmd_or_ctrl, mods.shift_key(), wants_text, over_canvas)
            {
                match key {
                    ShellKey::ToggleSidebar => {
                        state.gui.toggle_tab(crate::gui::SolarxyTab::Sidebar);
                    }
                    ShellKey::ToggleMenuBar => {
                        state.gui.menu_bar_visible = !state.gui.menu_bar_visible;
                    }
                    ShellKey::ToggleFullscreen => state.toggle_fullscreen(),
                    ShellKey::NewScene => state.new_scene(),
                    ShellKey::OpenModel => state.open_model_dialog(),
                    ShellKey::OpenHdri => state.open_hdri_dialog(),
                    ShellKey::Save => {
                        state.save_document();
                    }
                    ShellKey::SaveAs => {
                        state.save_document_as();
                    }
                    ShellKey::ToggleConsole => {
                        state.gui.toggle_tab(crate::gui::SolarxyTab::Console);
                    }
                    ShellKey::ToggleViewport => {
                        state.gui.toggle_tab(crate::gui::SolarxyTab::Viewport);
                    }
                    // Outside the interface pass, so it calls the state
                    // directly rather than raising an intent.
                    ShellKey::CookNow => state.cook_now(),
                }
                pre_handled = true;
            }
        }

        let egui_consumed = state.gui.on_window_event(&state.window, &event);

        match event {
            // Through the guard rather than straight to the exit: a dirty
            // document asks first, and `exiting` still runs its flushes on
            // every path that does leave.
            WindowEvent::CloseRequested => state.request_quit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            // One event per dropped item, with no event to say the gesture
            // is complete. Collected, and handled once per frame, so a
            // folder of models is one gesture rather than a document replaced
            // once per file.
            WindowEvent::DroppedFile(path) => state.drop_path(path),
            WindowEvent::RedrawRequested => {
                self.frame_count = self.frame_count.wrapping_add(1);
                state.update();
                if state.quit_requested {
                    event_loop.exit();
                    return;
                }
                match state.render() {
                    Ok(()) => {}
                    Err(e) => {
                        if let Some(surface_error) = e.downcast_ref::<SurfaceError>() {
                            match surface_error {
                                SurfaceError::Lost | SurfaceError::Outdated => {
                                    let size = state.window.inner_size();
                                    state.resize(size.width, size.height);
                                }
                                SurfaceError::OutOfMemory => {
                                    event_loop.exit();
                                }
                                SurfaceError::Timeout => {
                                    tracing::warn!(
                                        "Surface timeout when rendering: {:?}",
                                        surface_error
                                    );
                                }
                                SurfaceError::Other => {
                                    let size = state.window.inner_size();
                                    state.resize(size.width, size.height);
                                    if self.frame_count % 60 == 1 {
                                        tracing::warn!(
                                            "Surface error (recovering via reconfigure): {:?}",
                                            surface_error
                                        );
                                    }
                                }
                            }
                        } else {
                            tracing::error!("Unable to render: {:?}", e);
                        }
                    }
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } if route_pointer_to_camera(state) => {
                state.open_viewport_context_menu();
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } if route_pointer_to_camera(state) => {
                state.handle_mouse_button(button, btn_state.is_pressed());
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.input.cursor_pos = (position.x as f32, position.y as f32);
                if route_pointer_to_camera(state) {
                    state.handle_mouse_move(position.x as f32, position.y as f32);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if route_pointer_to_camera(state) => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32 * 0.01,
                };
                state.handle_scroll(scroll);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.set_modifiers(modifiers.state());
            }
            WindowEvent::KeyboardInput { ref event, .. }
                if !pre_handled && !egui_consumed && !state.gui.wants_keyboard_input() =>
            {
                if let PhysicalKey::Code(code) = event.physical_key {
                    state.handle_key(event_loop, code, event.state.is_pressed());
                }
                if event.state.is_pressed()
                    && let Key::Character(ref ch) = event.logical_key
                    && ch.as_str() == "?"
                {
                    state.gui.open_shortcuts_modal();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = state.window.inner_size();
                state.resize(size.width, size.height);
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &mut self.state {
            state.flush_review_on_exit();
            state.flush_dock_layout_on_exit();
        }
    }
}

pub fn run_viewer(
    model_path: Option<String>,
    preferences: Preferences,
    console_buffer: LogBuffer,
) -> anyhow::Result<()> {
    let event_loop = EventLoop::with_user_event().build()?;
    let mut app = App::new(model_path, preferences, console_buffer);
    event_loop.run_app(&mut app)?;
    Ok(())
}
