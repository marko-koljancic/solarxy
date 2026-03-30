mod cgi;
mod state;

use state::State;
use std::sync::Arc;
use wgpu::SurfaceError;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, PhysicalKey},
    window::Window,
};

pub struct App {
    state: Option<State>,
    model_path: String,
}

impl App {
    pub fn new(model_path: String) -> Self {
        Self {
            state: None,
            model_path,
        }
    }
}

impl ApplicationHandler<State> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes().with_title("Solarxy");
        let window = match event_loop.create_window(window_attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("Failed to create window: {}", e);
                event_loop.exit();
                return;
            }
        };
        match pollster::block_on(State::new(window, self.model_path.clone())) {
            Ok(state) => self.state = Some(state),
            Err(e) => {
                eprintln!("Failed to initialize renderer: {}", e);
                event_loop.exit();
            }
        }
    }

    #[allow(unused_mut)]
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, mut event: State) {
        self.state = Some(event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: winit::window::WindowId, event: WindowEvent) {
        let state = match &mut self.state {
            Some(canvas) => canvas,
            None => return,
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                state.update();
                match state.render() {
                    Ok(_) => {}
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
                                    eprintln!("Surface timeout when rendering: {:?}", surface_error);
                                }
                                other => {
                                    eprintln!("Unhandled surface error when rendering: {:?}", other);
                                }
                            }
                        } else {
                            eprintln!("Unable to render: {:?}", e);
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                state.handle_mouse_button(button, btn_state.is_pressed());
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.handle_mouse_move(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32 * 0.01,
                };
                state.handle_scroll(scroll);
            }
            WindowEvent::KeyboardInput { ref event, .. } => {
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
            _ => {}
        }
    }
}

pub fn run_viewer(model_path: String) -> anyhow::Result<()> {
    let event_loop = EventLoop::with_user_event().build()?;
    let mut app = App::new(model_path);
    event_loop.run_app(&mut app)?;
    Ok(())
}
