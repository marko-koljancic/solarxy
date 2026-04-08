use cgmath::InnerSpace;
use wgpu::util::DeviceExt;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use super::camera::{
    camera_from_bounds, camera_from_bounds_axis, Camera, CameraController, CameraUniform,
};
use crate::preferences::ProjectionMode;
use super::model::AABB;

struct CameraTransition {
    dest_eye: cgmath::Point3<f32>,
    dest_target: cgmath::Point3<f32>,
    dest_up: cgmath::Vector3<f32>,
    dest_ortho_scale: f32,
}

pub struct CameraState {
    pub camera: Camera,
    uniform: CameraUniform,
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    controller: CameraController,
    transition: Option<CameraTransition>,
}

impl CameraState {
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        bounds: &AABB,
        aspect: f32,
    ) -> Self {
        let camera = camera_from_bounds(bounds, aspect);
        let mut uniform = CameraUniform::new();
        uniform.update_view_proj(&camera);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let controller = CameraController::new(0.2);
        Self {
            camera,
            uniform,
            buffer,
            bind_group,
            controller,
            transition: None,
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, dt: f32) {
        if let Some(ref transition) = self.transition {
            let factor = 1.0 - (1.0 - 0.18_f32).powf(dt * 60.0);

            self.camera.eye = lerp_point3(self.camera.eye, transition.dest_eye, factor);
            self.camera.target = lerp_point3(self.camera.target, transition.dest_target, factor);
            self.camera.up = lerp_vec3(self.camera.up, transition.dest_up, factor).normalize();
            self.camera.ortho_scale =
                lerp_f32(self.camera.ortho_scale, transition.dest_ortho_scale, factor);

            let eye_done = (self.camera.eye - transition.dest_eye).magnitude2() < 0.01;
            let target_done =
                (self.camera.target - transition.dest_target).magnitude2() < 0.01;
            if eye_done && target_done {
                self.camera.eye = transition.dest_eye;
                self.camera.target = transition.dest_target;
                self.camera.up = transition.dest_up;
                self.camera.ortho_scale = transition.dest_ortho_scale;
                self.transition = None;
            }
        }

        self.controller.update_camera(&mut self.camera);
        self.uniform.update_view_proj(&self.camera);
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
    }

    pub fn resize(&mut self, aspect: f32) {
        self.camera.aspect = aspect;
    }

    pub fn reset_to_bounds(&mut self, bounds: &AABB) {
        let dest = camera_from_bounds(bounds, self.camera.aspect);
        self.start_transition(&dest);
    }

    pub fn reset_to_bounds_axis(
        &mut self,
        bounds: &AABB,
        direction: cgmath::Vector3<f32>,
        up: cgmath::Vector3<f32>,
    ) {
        let dest = camera_from_bounds_axis(bounds, self.camera.aspect, direction, up);
        self.start_transition(&dest);
    }

    fn start_transition(&mut self, dest: &Camera) {
        if dest.projection == ProjectionMode::Orthographic
            && self.camera.projection != ProjectionMode::Orthographic
        {
            let dist = (self.camera.target - self.camera.eye).magnitude();
            self.camera.ortho_scale = dist * (self.camera.fovy / 2.0).to_radians().tan();
        }
        self.camera.projection = dest.projection;

        self.transition = Some(CameraTransition {
            dest_eye: dest.eye,
            dest_target: dest.target,
            dest_up: dest.up,
            dest_ortho_scale: dest.ortho_scale,
        });
        self.controller = CameraController::new(0.2);
    }

    pub fn set_projection(&mut self, mode: ProjectionMode) {
        if mode == ProjectionMode::Orthographic
            && self.camera.projection != ProjectionMode::Orthographic
        {
            let dist = (self.camera.target - self.camera.eye).magnitude();
            self.camera.ortho_scale = dist * (self.camera.fovy / 2.0).to_radians().tan();
        }
        self.camera.projection = mode;
    }

    pub fn handle_key(&mut self, code: KeyCode, is_pressed: bool) -> bool {
        if self.transition.is_some() {
            return false;
        }
        self.controller.handle_key(code, is_pressed)
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if self.transition.is_some() {
            return;
        }
        self.controller.handle_mouse_button(button, pressed);
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        if self.transition.is_some() {
            return;
        }
        self.controller.handle_mouse_move(x, y);
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        if self.transition.is_some() {
            return;
        }
        self.controller.handle_scroll(delta);
    }

    pub fn is_orbiting(&self) -> bool {
        self.controller.is_orbiting()
    }

    pub fn inject_orbit_yaw(&mut self, yaw: f32) {
        self.controller.inject_orbit_yaw(yaw);
    }

    pub fn clone_with_new_resources(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let camera = self.camera;
        let mut uniform = CameraUniform::new();
        uniform.update_view_proj(&camera);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer (secondary)"),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group (secondary)"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            camera,
            uniform,
            buffer,
            bind_group,
            controller: CameraController::new(0.2),
            transition: None,
        }
    }

    pub fn write_with_aspect(&mut self, queue: &wgpu::Queue, aspect: f32) {
        let saved = self.camera.aspect;
        self.camera.aspect = aspect;
        self.uniform.update_view_proj(&self.camera);
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
        self.camera.aspect = saved;
    }
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_point3(a: cgmath::Point3<f32>, b: cgmath::Point3<f32>, t: f32) -> cgmath::Point3<f32> {
    cgmath::Point3::new(
        lerp_f32(a.x, b.x, t),
        lerp_f32(a.y, b.y, t),
        lerp_f32(a.z, b.z, t),
    )
}

fn lerp_vec3(a: cgmath::Vector3<f32>, b: cgmath::Vector3<f32>, t: f32) -> cgmath::Vector3<f32> {
    cgmath::Vector3::new(
        lerp_f32(a.x, b.x, t),
        lerp_f32(a.y, b.y, t),
        lerp_f32(a.z, b.z, t),
    )
}
