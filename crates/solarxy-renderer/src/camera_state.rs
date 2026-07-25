//! Per-pane [`CameraState`] bundle: a [`crate::camera::Camera`] plus its
//! GPU-side uniform buffer + bind group, refreshed each frame.

use cgmath::InnerSpace;
use wgpu::util::DeviceExt;

use super::camera::{
    camera_from_bounds, camera_from_bounds_axis, Camera, CameraController, CameraUniform,
};
use super::input::{CameraKey, PointerButton};
use solarxy_core::preferences::ProjectionMode;
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
            let target_done = (self.camera.target - transition.dest_target).magnitude2() < 0.01;
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
        self.finish_transition();
        self.camera.set_projection_preserving_framing(mode);
    }

    /// Snaps an in-flight transition to its destination. Input lands here
    /// first: swallowing events while a transition runs meant a mouse press
    /// right after a view preset never registered, so the entire following
    /// drag was dead no matter how far it moved.
    fn finish_transition(&mut self) {
        if let Some(t) = self.transition.take() {
            self.camera.eye = t.dest_eye;
            self.camera.target = t.dest_target;
            self.camera.up = t.dest_up;
            self.camera.ortho_scale = t.dest_ortho_scale;
        }
    }

    /// The camera this state is heading to: the transition destination when
    /// one is in flight, else the current camera. Discrete camera-derived
    /// decisions (the grid plane) key off this so they switch once, at
    /// command time, instead of stepping mid-lerp.
    pub fn destination_camera(&self) -> Camera {
        match &self.transition {
            Some(t) => apply_dest(self.camera, t),
            None => self.camera,
        }
    }

    pub fn handle_key(&mut self, code: CameraKey, is_pressed: bool) -> bool {
        if is_pressed {
            self.finish_transition();
        }
        self.controller.handle_key(code, is_pressed)
    }

    pub fn handle_mouse_button(&mut self, button: PointerButton, pressed: bool) {
        if pressed {
            self.finish_transition();
        }
        self.controller.handle_mouse_button(button, pressed);
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        // Deltas only accumulate while a button is held, and any press has
        // already finished the transition; hover moves must not cancel it.
        self.controller.handle_mouse_move(x, y);
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        self.finish_transition();
        self.controller.handle_scroll(delta);
    }

    pub fn is_orbiting(&self) -> bool {
        self.controller.is_orbiting()
    }

    pub fn inject_orbit_yaw(&mut self, yaw: f32) {
        self.controller.inject_orbit_yaw(yaw);
    }

    #[must_use]
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

/// The transition's endpoint applied to a camera, as a value: the pure half
/// of [`CameraState::destination_camera`], kept free of GPU state so it is
/// unit-testable.
fn apply_dest(mut cam: Camera, t: &CameraTransition) -> Camera {
    cam.eye = t.dest_eye;
    cam.target = t.dest_target;
    cam.up = t.dest_up;
    cam.ortho_scale = t.dest_ortho_scale;
    cam
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_f32_interpolates_with_nonzero_endpoints() {
        assert!((lerp_f32(10.0, 30.0, 0.0) - 10.0).abs() < f32::EPSILON);
        assert!((lerp_f32(10.0, 30.0, 1.0) - 30.0).abs() < f32::EPSILON);
        assert!((lerp_f32(10.0, 30.0, 0.5) - 20.0).abs() < f32::EPSILON);
        assert!((lerp_f32(10.0, 30.0, 0.25) - 15.0).abs() < f32::EPSILON);
        assert!((lerp_f32(-5.0, 5.0, 0.5)).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_dest_reaches_the_exact_destination() {
        let cam = Camera {
            eye: cgmath::Point3::new(0.0, 0.0, 3.0),
            target: cgmath::Point3::new(0.0, 0.0, 0.0),
            up: cgmath::Vector3::unit_y(),
            aspect: 1.0,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
            projection: ProjectionMode::Orthographic,
            ortho_scale: 5.0,
        };
        let t = CameraTransition {
            dest_eye: cgmath::Point3::new(0.0, 9.0, 0.0),
            dest_target: cgmath::Point3::new(1.0, 0.0, 2.0),
            dest_up: -cgmath::Vector3::unit_z(),
            dest_ortho_scale: 2.5,
        };
        let dest = apply_dest(cam, &t);
        assert!((dest.eye.y - 9.0).abs() < f32::EPSILON);
        assert!((dest.target.x - 1.0).abs() < f32::EPSILON);
        assert!((dest.up.z + 1.0).abs() < f32::EPSILON);
        assert!((dest.ortho_scale - 2.5).abs() < f32::EPSILON);
        // The source camera's non-transitioned fields ride along unchanged.
        assert!((dest.fovy - 45.0).abs() < f32::EPSILON);
        assert!(matches!(dest.projection, ProjectionMode::Orthographic));
    }

    #[test]
    fn lerp_point3_and_vec3_with_nonzero_endpoints() {
        let a = cgmath::Point3::new(2.0, 4.0, 6.0);
        let b = cgmath::Point3::new(12.0, 24.0, 36.0);

        let at0 = lerp_point3(a, b, 0.0);
        assert!((at0.x - 2.0).abs() < f32::EPSILON);
        assert!((at0.y - 4.0).abs() < f32::EPSILON);
        assert!((at0.z - 6.0).abs() < f32::EPSILON);

        let mid = lerp_point3(a, b, 0.5);
        assert!((mid.x - 7.0).abs() < f32::EPSILON);
        assert!((mid.y - 14.0).abs() < f32::EPSILON);
        assert!((mid.z - 21.0).abs() < f32::EPSILON);

        let va = cgmath::Vector3::new(2.0, 4.0, 6.0);
        let vb = cgmath::Vector3::new(12.0, 24.0, 36.0);
        let vmid = lerp_vec3(va, vb, 0.5);
        assert!((vmid.x - 7.0).abs() < f32::EPSILON);
        assert!((vmid.y - 14.0).abs() < f32::EPSILON);
    }
}
