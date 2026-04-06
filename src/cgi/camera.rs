use winit::{event::MouseButton, keyboard::KeyCode};
use super::model;
use crate::preferences::ProjectionMode;

#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::from_cols(
    cgmath::Vector4::new(1.0, 0.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 1.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 1.0),
);

#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: cgmath::Point3<f32>,
    pub target: cgmath::Point3<f32>,
    pub up: cgmath::Vector3<f32>,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
    pub projection: ProjectionMode,
    pub ortho_scale: f32,
}

impl Camera {
    pub fn build_view_matrix(&self) -> cgmath::Matrix4<f32> {
        cgmath::Matrix4::look_at_rh(self.eye, self.target, self.up)
    }

    pub fn build_proj_matrix(&self) -> cgmath::Matrix4<f32> {
        match self.projection {
            ProjectionMode::Perspective => {
                cgmath::perspective(cgmath::Deg(self.fovy), self.aspect, self.znear, self.zfar)
            }
            ProjectionMode::Orthographic => {
                let half_h = self.ortho_scale;
                let half_w = half_h * self.aspect;
                cgmath::ortho(-half_w, half_w, -half_h, half_h, self.znear, self.zfar)
            }
        }
    }

    pub fn build_view_projection_matrix(&self) -> cgmath::Matrix4<f32> {
        OPENGL_TO_WGPU_MATRIX * self.build_proj_matrix() * self.build_view_matrix()
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
        projection: ProjectionMode::Perspective,
        ortho_scale: distance * (fovy / 2.0).to_radians().tan(),
    }
}

pub fn camera_from_bounds_axis(
    bounds: &model::AABB,
    aspect: f32,
    direction: cgmath::Vector3<f32>,
    up: cgmath::Vector3<f32>,
) -> Camera {
    use cgmath::InnerSpace;

    let center = bounds.center();
    let half_ext = bounds.half_extents();
    let fovy = 45.0_f32;
    let extent = bounds.diagonal() / 2.0;
    let distance = (extent / (fovy / 2.0).to_radians().tan()) * 1.5;

    let dir_n = direction.normalize();
    let right = dir_n.cross(up).normalize();
    let up_n = right.cross(dir_n);
    let half_w =
        half_ext.x * right.x.abs() + half_ext.y * right.y.abs() + half_ext.z * right.z.abs();
    let half_h = half_ext.x * up_n.x.abs() + half_ext.y * up_n.y.abs() + half_ext.z * up_n.z.abs();

    let ortho_scale = half_h.max(half_w / aspect) * 1.2;

    Camera {
        eye: center + direction * distance,
        target: center,
        up,
        aspect,
        fovy,
        znear: (distance / 100.0).max(0.01),
        zfar: distance * 20.0,
        projection: ProjectionMode::Orthographic,
        ortho_scale,
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    view_position: [f32; 4],
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    inv_proj: [[f32; 4]; 4],
    near: f32,
    far: f32,
    _pad: [f32; 2],
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraUniform {
    pub fn new() -> Self {
        use cgmath::SquareMatrix;
        let identity: [[f32; 4]; 4] = cgmath::Matrix4::identity().into();
        Self {
            view_position: [0.0; 4],
            view_proj: identity,
            view: identity,
            proj: identity,
            inv_proj: identity,
            near: 0.01,
            far: 100.0,
            _pad: [0.0; 2],
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera) {
        use cgmath::SquareMatrix;
        self.view_position = camera.eye.to_homogeneous().into();
        let view = camera.build_view_matrix();
        let proj = OPENGL_TO_WGPU_MATRIX * camera.build_proj_matrix();
        self.view_proj = (proj * view).into();
        self.view = view.into();
        self.proj = proj.into();
        self.inv_proj = proj.invert().unwrap_or(cgmath::Matrix4::identity()).into();
        self.near = camera.znear;
        self.far = camera.zfar;
    }
}

pub struct CameraController {
    speed: f32,
    is_forward_pressed: bool,
    is_backward_pressed: bool,
    is_left_pressed: bool,
    is_right_pressed: bool,
    is_left_mouse_pressed: bool,
    last_mouse_pos: Option<(f32, f32)>,
    orbit_delta: (f32, f32),
    is_middle_mouse_pressed: bool,
    pan_delta: (f32, f32),
    zoom_delta: f32,
}

impl CameraController {
    pub fn is_orbiting(&self) -> bool {
        self.is_left_mouse_pressed
    }

    pub fn inject_orbit_yaw(&mut self, yaw: f32) {
        self.orbit_delta.0 += yaw;
    }

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
            KeyCode::ArrowUp => {
                self.is_forward_pressed = is_pressed;
                true
            }
            KeyCode::ArrowLeft => {
                self.is_left_pressed = is_pressed;
                true
            }
            KeyCode::ArrowDown => {
                self.is_backward_pressed = is_pressed;
                true
            }
            KeyCode::ArrowRight => {
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

        if self.orbit_delta.0 != 0.0 || self.orbit_delta.1 != 0.0 {
            let offset = camera.eye - camera.target;
            let r = offset.magnitude();
            let mut yaw = f32::atan2(offset.x, offset.z);
            let horiz = (offset.x * offset.x + offset.z * offset.z).sqrt();
            let mut pitch = f32::atan2(offset.y, horiz);

            yaw += self.orbit_delta.0;
            pitch =
                (pitch + self.orbit_delta.1).clamp(-89.0_f32.to_radians(), 89.0_f32.to_radians());

            camera.eye = camera.target
                + cgmath::Vector3::new(
                    r * pitch.cos() * yaw.sin(),
                    r * pitch.sin(),
                    r * pitch.cos() * yaw.cos(),
                );
            self.orbit_delta = (0.0, 0.0);
        }

        if self.pan_delta.0 != 0.0 || self.pan_delta.1 != 0.0 {
            let fwd = (camera.target - camera.eye).normalize();
            let right = fwd.cross(camera.up).normalize();
            let up = right.cross(fwd);
            let scale = match camera.projection {
                ProjectionMode::Perspective => (camera.target - camera.eye).magnitude() * 0.001,
                ProjectionMode::Orthographic => camera.ortho_scale * 0.002,
            };
            let shift = right * (-self.pan_delta.0 * scale) + up * (self.pan_delta.1 * scale);
            camera.eye += shift;
            camera.target += shift;
            self.pan_delta = (0.0, 0.0);
        }

        if self.zoom_delta != 0.0 {
            match camera.projection {
                ProjectionMode::Perspective => {
                    let fwd = camera.target - camera.eye;
                    let fwd_norm = fwd.normalize();
                    let dist = fwd.magnitude();
                    let min_dist = 0.01;
                    let new_dist = (dist - self.zoom_delta * self.speed * 5.0).max(min_dist);
                    camera.eye = camera.target - fwd_norm * new_dist;
                }
                ProjectionMode::Orthographic => {
                    let zoom_factor = 1.0 - self.zoom_delta * self.speed * 0.5;
                    camera.ortho_scale = (camera.ortho_scale * zoom_factor).max(0.01);
                }
            }
            self.zoom_delta = 0.0;
        }

        let dist = (camera.target - camera.eye).magnitude();
        camera.znear = (dist / 100.0).max(0.01);
        camera.zfar = dist * 50.0;
    }
}
