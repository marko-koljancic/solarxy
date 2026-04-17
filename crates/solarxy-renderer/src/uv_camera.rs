use wgpu::util::DeviceExt;

use super::camera::{CameraUniform, OPENGL_TO_WGPU_MATRIX};

pub struct UvCameraState {
    uniform: CameraUniform,
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl UvCameraState {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let uniform = CameraUniform::new();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("UV Camera Buffer"),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uv_camera_bind_group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            uniform,
            buffer,
            bind_group,
        }
    }

    pub fn write(&mut self, queue: &wgpu::Queue, offset: [f32; 2], zoom: f32, aspect: f32) {
        let cx = 0.5 + offset[0];
        let cy = 0.5 + offset[1];
        let half_h = 0.6 / zoom;
        let half_w = half_h * aspect;

        let proj = OPENGL_TO_WGPU_MATRIX
            * cgmath::ortho(
                cx - half_w,
                cx + half_w,
                cy - half_h,
                cy + half_h,
                -1.0,
                1.0,
            );

        self.uniform.set_uv_projection(proj.into());
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
    }
}
