#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::fn_params_excessive_bools,
    clippy::many_single_char_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::pub_underscore_fields,
    clippy::return_self_not_must_use,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unreadable_literal,
    clippy::used_underscore_binding,
    clippy::wildcard_imports
)]

pub mod aabb;
#[cfg(any(feature = "viewer", feature = "analyzer"))]
pub mod cgi;
pub mod preferences;
#[cfg(feature = "viewer")]
mod state;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["obj", "stl", "ply", "gltf", "glb"];

#[cfg(feature = "viewer")]
use preferences::Preferences;
#[cfg(feature = "viewer")]
use state::State;
#[cfg(feature = "viewer")]
use std::sync::Arc;
#[cfg(feature = "viewer")]
use wgpu::SurfaceError;
#[cfg(feature = "viewer")]
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, PhysicalKey},
    window::Window,
};

#[cfg(feature = "viewer")]
pub struct App {
    state: Option<State>,
    model_path: Option<String>,
    preferences: Preferences,
}

#[cfg(feature = "viewer")]
impl App {
    pub fn new(model_path: Option<String>, preferences: Preferences) -> Self {
        Self {
            state: None,
            model_path,
            preferences,
        }
    }
}

#[cfg(feature = "viewer")]
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
                state.cursor_pos = (position.x as f32, position.y as f32);
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

pub fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }

    result
}

#[cfg(feature = "viewer")]
pub fn run_viewer(model_path: Option<String>, preferences: Preferences) -> anyhow::Result<()> {
    let event_loop = EventLoop::with_user_event().build()?;
    let mut app = App::new(model_path, preferences);
    event_loop.run_app(&mut app)?;
    Ok(())
}
