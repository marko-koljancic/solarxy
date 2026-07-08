//! A minimal orbit camera for the single-pane web viewport.
//!
//! Owns yaw/pitch/distance around a target and produces the `view_proj`
//! matrix and eye position the forward renderer and picking need. Pointer
//! gestures map to orbit (left drag), pan (right/middle drag), and dolly
//! (wheel), mirroring the desktop camera's feel without winit.

use cgmath::{InnerSpace, Matrix4, Point3, Rad, Vector3, perspective};

/// An orbit camera around a target point.
pub struct OrbitCamera {
    pub target: Point3<f32>,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: Rad<f32>,
    pub aspect: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Point3::new(0.0, 0.0, 0.0),
            yaw: 0.7,
            pitch: 0.5,
            distance: 6.0,
            fov_y: Rad(std::f32::consts::FRAC_PI_4),
            aspect: 1.0,
        }
    }
}

impl OrbitCamera {
    /// The eye position derived from the orbit angles.
    pub fn eye(&self) -> Point3<f32> {
        let cp = self.pitch.cos();
        let dir = Vector3::new(cp * self.yaw.sin(), self.pitch.sin(), cp * self.yaw.cos());
        self.target + dir * self.distance
    }

    /// The `clip = view_proj * world` matrix (right-handed, y-up).
    pub fn view_proj(&self) -> Matrix4<f32> {
        let eye = self.eye();
        let view = Matrix4::look_at_rh(eye, self.target, Vector3::unit_y());
        let proj = perspective(self.fov_y, self.aspect.max(0.01), 0.05, 1000.0);
        proj * view
    }

    /// Orbits by pointer deltas in pixels (scaled to radians), clamping
    /// pitch away from the poles.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-1.4, 1.4);
    }

    /// Pans the target in the camera's screen plane.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let eye = self.eye();
        let forward = (self.target - eye).normalize();
        let right = forward.cross(Vector3::unit_y()).normalize();
        let up = right.cross(forward).normalize();
        let scale = self.distance * 0.0015;
        self.target += (-right * dx + up * dy) * scale;
    }

    /// Dollies in/out by a wheel delta (positive zooms in).
    pub fn dolly(&mut self, amount: f32) {
        self.distance = (self.distance * (1.0 - amount * 0.1)).clamp(0.2, 500.0);
    }
}
