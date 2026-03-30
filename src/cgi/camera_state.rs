use wgpu::util::DeviceExt;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use super::camera::{camera_from_bounds, camera_from_bounds_axis, Camera, CameraController, CameraUniform, ProjectionMode};
use super::model::AABB;

pub struct CameraState {
    pub camera: Camera,
    uniform: CameraUniform,
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    controller: CameraController,
}

impl CameraState {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, bounds: &AABB, aspect: f32) -> Self {
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
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue) {
        self.controller.update_camera(&mut self.camera);
        self.uniform.update_view_proj(&self.camera);
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
    }

    pub fn resize(&mut self, aspect: f32) {
        self.camera.aspect = aspect;
    }

    pub fn reset_to_bounds(&mut self, bounds: &AABB) {
        self.camera = camera_from_bounds(bounds, self.camera.aspect);
        self.controller = CameraController::new(0.2);
    }

    pub fn reset_to_bounds_axis(&mut self, bounds: &AABB, direction: cgmath::Vector3<f32>, up: cgmath::Vector3<f32>) {
        self.camera = camera_from_bounds_axis(bounds, self.camera.aspect, direction, up);
        self.controller = CameraController::new(0.2);
    }

    pub fn set_projection(&mut self, mode: ProjectionMode) {
        if mode == ProjectionMode::Orthographic && self.camera.projection != ProjectionMode::Orthographic {
            use cgmath::InnerSpace;
            let dist = (self.camera.target - self.camera.eye).magnitude();
            self.camera.ortho_scale = dist * (self.camera.fovy / 2.0).to_radians().tan();
        }
        self.camera.projection = mode;
    }

    pub fn handle_key(&mut self, code: KeyCode, is_pressed: bool) -> bool {
        self.controller.handle_key(code, is_pressed)
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        self.controller.handle_mouse_button(button, pressed);
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        self.controller.handle_mouse_move(x, y);
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        self.controller.handle_scroll(delta);
    }
}
