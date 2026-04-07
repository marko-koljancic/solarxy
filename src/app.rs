use std::sync::Arc;

use wgpu::SurfaceError;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, PhysicalKey},
    window::Window,
};

use crate::preferences::Preferences;
use crate::state::State;

pub struct App {
    state: Option<State>,
    model_path: Option<String>,
    preferences: Preferences,
}

impl App {
    pub fn new(model_path: Option<String>, preferences: Preferences) -> Self {
        Self {
            state: None,
            model_path,
            preferences,
        }
    }
}

impl ApplicationHandler<State> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("Solarxy")
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
        )) {
            Ok(state) => self.state = Some(state),
            Err(e) => {
                tracing::error!("Failed to initialize renderer: {}", e);
                event_loop.exit();
            }
        }
    }

    #[allow(unused_mut)]
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, mut event: State) {
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

        if let WindowEvent::KeyboardInput { ref event, .. } = event
            && event.state.is_pressed()
            && event.physical_key == PhysicalKey::Code(winit::keyboard::KeyCode::Tab)
        {
            state.gui.sidebar_visible = !state.gui.sidebar_visible;
        }

        let egui_consumed = state.gui.on_window_event(&state.window, &event);

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::DroppedFile(path) => {
                state.handle_dropped_file(path);
            }
            WindowEvent::RedrawRequested => {
                state.update();
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
                                    tracing::error!(
                                        "Unhandled surface error when rendering: {:?}",
                                        surface_error
                                    );
                                }
                            }
                        } else {
                            tracing::error!("Unable to render: {:?}", e);
                        }
                    }
                }
            }

            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } if !egui_consumed && !state.gui.wants_pointer_input() => {
                state.handle_mouse_button(button, btn_state.is_pressed());
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.input.cursor_pos = (position.x as f32, position.y as f32);
                if !egui_consumed && !state.gui.wants_pointer_input() {
                    state.handle_mouse_move(position.x as f32, position.y as f32);
                }
            }
            WindowEvent::MouseWheel { delta, .. }
                if !egui_consumed && !state.gui.wants_pointer_input() =>
            {
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
                if !egui_consumed && !state.gui.wants_keyboard_input() =>
            {
                if let PhysicalKey::Code(code) = event.physical_key {
                    state.handle_key(event_loop, code, event.state.is_pressed());
                }
                if event.state.is_pressed()
                    && let Key::Character(ref ch) = event.logical_key
                    && ch.as_str() == "?"
                {
                    state.toggle_hints();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = state.window.inner_size();
                state.resize(size.width, size.height);
            }
            _ => {}
        }
    }
}

pub fn run_viewer(model_path: Option<String>, preferences: Preferences) -> anyhow::Result<()> {
    let event_loop = EventLoop::with_user_event().build()?;
    let mut app = App::new(model_path, preferences);
    event_loop.run_app(&mut app)?;
    Ok(())
}
