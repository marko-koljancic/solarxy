use winit::{
    event::MouseButton,
    keyboard::KeyCode,
};
use super::model;

#[rustfmt::skip]
const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::from_cols(
    cgmath::Vector4::new(1.0, 0.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 1.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 1.0),
);

pub struct Camera {
    pub eye: cgmath::Point3<f32>,
    pub target: cgmath::Point3<f32>,
    pub up: cgmath::Vector3<f32>,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Camera {
    pub fn build_view_projection_matrix(&self) -> cgmath::Matrix4<f32> {
        let view = cgmath::Matrix4::look_at_rh(self.eye, self.target, self.up);
        let proj = cgmath::perspective(cgmath::Deg(self.fovy), self.aspect, self.znear, self.zfar);
        OPENGL_TO_WGPU_MATRIX * proj * view
    }
}

pub fn camera_from_bounds(bounds: &model::AABB, aspect: f32) -> Camera {
    let center = bounds.center();
    let extent = bounds.diagonal() / 2.0;
    let fovy = 45.0_f32;
    let distance = (extent / (fovy / 2.0).to_radians().tan()) * 1.5;
    Camera {
        eye: center + cgmath::Vector3::new(0.0, extent * 0.4, distance),
        target: center,
        up: cgmath::Vector3::unit_y(),
        aspect,
        fovy,
        znear: (distance / 100.0).max(0.01),
        zfar: distance * 20.0,
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    view_position: [f32; 4],
    view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    pub fn new() -> Self {
        use cgmath::SquareMatrix;
        Self {
            view_position: [0.0; 4],
            view_proj: cgmath::Matrix4::identity().into(),
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera) {
        self.view_position = camera.eye.to_homogeneous().into();
        self.view_proj = (OPENGL_TO_WGPU_MATRIX * camera.build_view_projection_matrix()).into();
    }
}

pub struct CameraController {
    speed: f32,
    is_forward_pressed: bool,
    is_backward_pressed: bool,
    is_left_pressed: bool,
    is_right_pressed: bool,
    // Mouse orbit (left-drag)
    is_left_mouse_pressed: bool,
    last_mouse_pos: Option<(f32, f32)>,
    orbit_delta: (f32, f32),
    // Mouse pan (middle-drag)
    is_middle_mouse_pressed: bool,
    pan_delta: (f32, f32),
    // Zoom
    zoom_delta: f32,
}

impl CameraController {
    pub fn new(speed: f32) -> Self {
        Self {
            speed,
            is_forward_pressed: false,
            is_backward_pressed: false,
            is_left_pressed: false,
            is_right_pressed: false,
            is_left_mouse_pressed: false,
            last_mouse_pos: None,
            orbit_delta: (0.0, 0.0),
            is_middle_mouse_pressed: false,
            pan_delta: (0.0, 0.0),
            zoom_delta: 0.0,
        }
    }

    pub fn handle_key(&mut self, code: KeyCode, is_pressed: bool) -> bool {
        match code {
            KeyCode::KeyW | KeyCode::ArrowUp => {
                self.is_forward_pressed = is_pressed;
                true
            }
            KeyCode::KeyA | KeyCode::ArrowLeft => {
                self.is_left_pressed = is_pressed;
                true
            }
            KeyCode::KeyS | KeyCode::ArrowDown => {
                self.is_backward_pressed = is_pressed;
                true
            }
            KeyCode::KeyD | KeyCode::ArrowRight => {
                self.is_right_pressed = is_pressed;
                true
            }
            _ => false,
        }
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        match button {
            MouseButton::Left => {
                self.is_left_mouse_pressed = pressed;
                if !pressed {
                    self.last_mouse_pos = None;
                }
            }
            MouseButton::Middle => {
                self.is_middle_mouse_pressed = pressed;
                if !pressed {
                    self.last_mouse_pos = None;
                }
            }
            _ => {}
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        if let Some((last_x, last_y)) = self.last_mouse_pos {
            let dx = x - last_x;
            let dy = y - last_y;
            if self.is_left_mouse_pressed {
                self.orbit_delta.0 += dx * 0.005;
                self.orbit_delta.1 += dy * 0.005;
            }
            if self.is_middle_mouse_pressed {
                self.pan_delta.0 += dx;
                self.pan_delta.1 += dy;
            }
        }
        if self.is_left_mouse_pressed || self.is_middle_mouse_pressed {
            self.last_mouse_pos = Some((x, y));
        }
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        self.zoom_delta += delta;
    }

    pub fn update_camera(&mut self, camera: &mut Camera) {
        use cgmath::InnerSpace;

        let forward = camera.target - camera.eye;
        let forward_norm = forward.normalize();
        let forward_mag = forward.magnitude();

        if self.is_forward_pressed && forward_mag > self.speed {
            camera.eye += forward_norm * self.speed;
        }
        if self.is_backward_pressed {
            camera.eye -= forward_norm * self.speed;
        }

        let right = forward_norm.cross(camera.up);
        let forward = camera.target - camera.eye;
        let forward_mag = forward.magnitude();

        if self.is_right_pressed {
            camera.eye = camera.target - (forward + right * self.speed).normalize() * forward_mag;
        }
        if self.is_left_pressed {
            camera.eye = camera.target - (forward - right * self.speed).normalize() * forward_mag;
        }

        // Mouse orbit
        if self.orbit_delta.0 != 0.0 || self.orbit_delta.1 != 0.0 {
            let offset = camera.eye - camera.target;
            let r = offset.magnitude();
            let mut yaw = f32::atan2(offset.x, offset.z);
            let horiz = (offset.x * offset.x + offset.z * offset.z).sqrt();
            let mut pitch = f32::atan2(offset.y, horiz);

            yaw += self.orbit_delta.0;
            pitch = (pitch + self.orbit_delta.1).clamp(
                -89.0_f32.to_radians(),
                89.0_f32.to_radians(),
            );

            camera.eye = camera.target + cgmath::Vector3::new(
                r * pitch.cos() * yaw.sin(),
                r * pitch.sin(),
                r * pitch.cos() * yaw.cos(),
            );
            self.orbit_delta = (0.0, 0.0);
        }

        // Mouse pan
        if self.pan_delta.0 != 0.0 || self.pan_delta.1 != 0.0 {
            let fwd = (camera.target - camera.eye).normalize();
            let right = fwd.cross(camera.up).normalize();
            let up = right.cross(fwd);
            let dist = (camera.target - camera.eye).magnitude();
            let scale = dist * 0.001;
            let shift = right * (-self.pan_delta.0 * scale) + up * (self.pan_delta.1 * scale);
            camera.eye += shift;
            camera.target += shift;
            self.pan_delta = (0.0, 0.0);
        }

        // Mouse zoom
        if self.zoom_delta != 0.0 {
            let fwd = camera.target - camera.eye;
            let fwd_norm = fwd.normalize();
            let dist = fwd.magnitude();
            let min_dist = 0.01;
            let new_dist = (dist - self.zoom_delta * self.speed * 5.0).max(min_dist);
            camera.eye = camera.target - fwd_norm * new_dist;
            self.zoom_delta = 0.0;
        }

        // Keep near/far planes proportional to current distance
        let dist = (camera.target - camera.eye).magnitude();
        camera.znear = (dist / 100.0).max(0.01);
        camera.zfar = dist * 50.0;
    }
}
