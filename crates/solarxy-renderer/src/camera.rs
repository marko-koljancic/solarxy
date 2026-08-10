//! Orbit camera + the `CameraUniform` GPU struct that drives every shader's
//! view/projection matrices and material-override switching.

use super::input::{CameraKey, PointerButton};
use super::model;
use solarxy_core::preferences::ProjectionMode;

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

    /// The same projection restricted to a rectangle of the image, which is how
    /// a still render draws one tile of a picture too large to draw at once.
    ///
    /// `origin` and `size` are the tile in pixels, `full` the whole image, with
    /// the origin at the top left the way every rect in this renderer is. The
    /// result is an **asymmetric** frustum: a tile off to one side sees the
    /// same cone of the world it would have occupied in the whole image, viewed
    /// off the view axis, which is exactly what makes the tiles reassemble into
    /// one picture rather than into a grid of separate renders each looking
    /// straight ahead.
    ///
    /// [`Camera::aspect`] is read as the **whole image's** aspect, not the
    /// tile's. The frustum is derived from the picture and then cut down; a
    /// tile that recomputed its own aspect would be a different camera.
    ///
    /// Windowing the whole rect reproduces [`Camera::build_proj_matrix`], which
    /// is asserted rather than assumed: it is what says a one-tile render and
    /// an untiled one are the same render.
    #[must_use]
    pub fn build_proj_matrix_windowed(
        &self,
        origin: [f32; 2],
        size: [f32; 2],
        full: [f32; 2],
    ) -> cgmath::Matrix4<f32> {
        let (fw, fh) = (full[0].max(1.0), full[1].max(1.0));
        // Fractions of the image the tile spans. `y` is measured down from the
        // top, and the frustum's is measured up from the bottom, which is the
        // one place the flip has to happen.
        let x0 = origin[0] / fw;
        let x1 = (origin[0] + size[0]) / fw;
        let y0 = origin[1] / fh;
        let y1 = (origin[1] + size[1]) / fh;

        let (left, right, bottom, top) = match self.projection {
            ProjectionMode::Perspective => {
                let top = self.znear * (self.fovy.to_radians() * 0.5).tan();
                let right = top * self.aspect;
                (-right, right, -top, top)
            }
            ProjectionMode::Orthographic => {
                let half_h = self.ortho_scale;
                let half_w = half_h * self.aspect;
                (-half_w, half_w, -half_h, half_h)
            }
        };
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let l = lerp(left, right, x0);
        let r = lerp(left, right, x1);
        // Top first, because the tile's `y0` edge is the upper one.
        let t = lerp(top, bottom, y0);
        let b = lerp(top, bottom, y1);

        match self.projection {
            ProjectionMode::Perspective => cgmath::frustum(l, r, b, t, self.znear, self.zfar),
            ProjectionMode::Orthographic => cgmath::ortho(l, r, b, t, self.znear, self.zfar),
        }
    }

    pub fn build_view_projection_matrix(&self) -> cgmath::Matrix4<f32> {
        OPENGL_TO_WGPU_MATRIX * self.build_proj_matrix() * self.build_view_matrix()
    }

    /// World units per screen pixel at `world_point`, for a pane `pane_height_px`
    /// tall.
    ///
    /// This is the whole of "constant screen size": a manipulator handle drawn
    /// `N * world_per_pixel` long occupies N pixels no matter how far the camera
    /// is, and a click tolerance of `M * world_per_pixel` is M pixels wide. Both
    /// the vertex generator and the hit tester call this one function, so the
    /// grab zones can never drift out of step with what the user sees -- the
    /// same reason `geo_world_matrix` is shared between scene lowering and
    /// picking.
    ///
    /// Perspective scales with the point's depth along the view axis;
    /// orthographic does not (its half-height IS `ortho_scale`, see
    /// [`Camera::build_proj_matrix`]).
    #[must_use]
    /// The camera's forward direction, and so the axis the view-aligned rotate
    /// ring turns about. One definition, because the renderer draws that ring
    /// and the host hit-tests it, and a disagreement between the two would let
    /// you grab a ring somewhere other than where it is drawn.
    pub fn forward(&self) -> cgmath::Vector3<f32> {
        use cgmath::InnerSpace;
        let d = self.target - self.eye;
        if d.magnitude2() < 1e-12 {
            cgmath::Vector3::unit_z()
        } else {
            d.normalize()
        }
    }

    pub fn world_per_pixel(&self, world_point: cgmath::Point3<f32>, pane_height_px: f32) -> f32 {
        use cgmath::InnerSpace;
        let height_px = pane_height_px.max(1.0);
        match self.projection {
            ProjectionMode::Perspective => {
                let forward = (self.target - self.eye).normalize();
                // Depth along the view axis, not the raw distance: a handle off
                // to the side of the frustum must not shrink.
                let depth = (world_point - self.eye).dot(forward).max(1e-4);
                let half_fov = cgmath::Rad::from(cgmath::Deg(self.fovy * 0.5)).0;
                2.0 * depth * half_fov.tan() / height_px
            }
            ProjectionMode::Orthographic => 2.0 * self.ortho_scale / height_px,
        }
    }

    /// Switches projection while keeping the model the same apparent size at
    /// the target: Persp to Ortho derives `ortho_scale` from the eye distance,
    /// Ortho to Persp inverts that math and moves the eye to the matching
    /// distance along the current view direction. Without the inverse step a
    /// toggle back to perspective reuses whatever distance the ortho eye
    /// happened to sit at (a preset parks it far out) and the framing jumps.
    pub fn set_projection_preserving_framing(&mut self, mode: ProjectionMode) {
        use cgmath::InnerSpace;
        if mode == self.projection {
            return;
        }
        let half_fov_tan = (self.fovy / 2.0).to_radians().tan();
        match mode {
            ProjectionMode::Orthographic => {
                let dist = (self.target - self.eye).magnitude();
                self.ortho_scale = dist * half_fov_tan;
            }
            ProjectionMode::Perspective => {
                let dist = (self.ortho_scale / half_fov_tan.max(1e-6)).max(0.01);
                self.eye = self.target - self.forward() * dist;
            }
        }
        self.projection = mode;
    }
}

/// The up vector of a world-Y turntable at `(yaw, pitch)`: `+Y` on the
/// horizon, tilting to `-Z` at the top pose (`pitch` +90 deg, yaw 0) and `+Z`
/// at the bottom pose -- exactly the ups the view presets use. Orthonormal to
/// the eye direction by construction, so the orbit can keep `Camera::up` in
/// lockstep with the angles it rebuilds the eye from.
pub fn turntable_up(yaw: f32, pitch: f32) -> cgmath::Vector3<f32> {
    cgmath::Vector3::new(
        -pitch.sin() * yaw.sin(),
        pitch.cos(),
        -pitch.sin() * yaw.cos(),
    )
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
    inspection_mode: u32,
    texel_density_target: f32,
    material_override: u32,
    depth_near: f32,
    depth_far: f32,
    roughness_scale: f32,
    metallic_scale: f32,
    hdri_rotation: f32,
    _pad: [f32; 2],
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(
    std::mem::size_of::<CameraUniform>().is_multiple_of(16),
    "CameraUniform must be 16-byte aligned for WGSL uniform buffer layout",
);

impl CameraUniform {
    pub const SIZE: usize = std::mem::size_of::<Self>();
    pub const INSPECTION_OFFSET: u64 = std::mem::offset_of!(Self, inspection_mode) as u64;

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
            inspection_mode: 0,
            texel_density_target: 1.0,
            material_override: 0,
            depth_near: 0.01,
            depth_far: 100.0,
            roughness_scale: 1.0,
            metallic_scale: 1.0,
            hdri_rotation: 0.0,
            _pad: [0.0; 2],
        }
    }

    pub fn set_uv_projection(&mut self, view_proj: [[f32; 4]; 4]) {
        use cgmath::SquareMatrix;
        self.view_position = [0.0; 4];
        self.view_proj = view_proj;
        let identity: [[f32; 4]; 4] = cgmath::Matrix4::identity().into();
        self.view = identity;
        self.proj = view_proj;
        self.inv_proj = identity;
        self.near = -1.0;
        self.far = 1.0;
        self.inspection_mode = 0;
        self.texel_density_target = 1.0;
        self.material_override = 0;
    }

    /// The camera restricted to one tile of a larger image. Everything else is
    /// identical to [`CameraUniform::update_view_proj`], which is what makes a
    /// tile a window on the same shot rather than a different one.
    pub fn update_view_proj_windowed(
        &mut self,
        camera: &Camera,
        origin: [f32; 2],
        size: [f32; 2],
        full: [f32; 2],
    ) {
        self.write_view_proj(
            camera,
            OPENGL_TO_WGPU_MATRIX * camera.build_proj_matrix_windowed(origin, size, full),
        );
    }

    pub fn update_view_proj(&mut self, camera: &Camera) {
        self.write_view_proj(camera, OPENGL_TO_WGPU_MATRIX * camera.build_proj_matrix());
    }

    fn write_view_proj(&mut self, camera: &Camera, proj: cgmath::Matrix4<f32>) {
        use cgmath::SquareMatrix;
        self.view_position = camera.eye.to_homogeneous().into();
        let view = camera.build_view_matrix();
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

    pub fn handle_key(&mut self, code: CameraKey, is_pressed: bool) -> bool {
        match code {
            CameraKey::ArrowUp => {
                self.is_forward_pressed = is_pressed;
                true
            }
            CameraKey::ArrowLeft => {
                self.is_left_pressed = is_pressed;
                true
            }
            CameraKey::ArrowDown => {
                self.is_backward_pressed = is_pressed;
                true
            }
            CameraKey::ArrowRight => {
                self.is_right_pressed = is_pressed;
                true
            }
        }
    }

    pub fn handle_mouse_button(&mut self, button: PointerButton, pressed: bool) {
        match button {
            PointerButton::Left => {
                self.is_left_mouse_pressed = pressed;
                if !pressed {
                    self.last_mouse_pos = None;
                }
            }
            PointerButton::Middle => {
                self.is_middle_mouse_pressed = pressed;
                if !pressed {
                    self.last_mouse_pos = None;
                }
            }
            PointerButton::Right | PointerButton::Other => {}
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
            let horiz = (offset.x * offset.x + offset.z * offset.z).sqrt();
            // At a pole pose (top/bottom view) the offset alone leaves yaw
            // undefined (atan2(0, 0)); recover the heading from the up vector
            // instead, so the first drag out of a top view tilts in place
            // rather than snapping to a fixed azimuth.
            let mut yaw = if horiz < r * 1e-4 {
                if offset.y > 0.0 {
                    f32::atan2(-camera.up.x, -camera.up.z)
                } else {
                    f32::atan2(camera.up.x, camera.up.z)
                }
            } else {
                f32::atan2(offset.x, offset.z)
            };
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
            // The turntable owns the frame: a view preset may have left a
            // tilted up (top view parks it at -Z), and rebuilding only the eye
            // against that stale up rolls the horizon on every later drag.
            camera.up = turntable_up(yaw, pitch);
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
                    let zoom_factor = (-self.zoom_delta * self.speed * 0.5_f32).exp();
                    let new_dist = (dist * zoom_factor).max(min_dist);
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

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::AABB;

    fn default_camera() -> Camera {
        Camera {
            eye: cgmath::Point3::new(0.0, 0.0, 3.0),
            target: cgmath::Point3::new(0.0, 0.0, 0.0),
            up: cgmath::Vector3::new(0.0, 1.0, 0.0),
            aspect: 16.0 / 9.0,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
            projection: ProjectionMode::Perspective,
            ortho_scale: 5.0,
        }
    }

    #[test]
    fn perspective_projection_not_degenerate() {
        let cam = default_camera();
        let proj = cam.build_proj_matrix();
        let m: [[f32; 4]; 4] = proj.into();
        assert!(m[0][0].abs() > 0.0);
        assert!(m[1][1].abs() > 0.0);
        assert!(m[2][2].abs() > 0.0);
        let ratio = m[1][1] / m[0][0];
        assert!((ratio - cam.aspect).abs() < 1e-5);
    }

    #[test]
    fn orthographic_projection_symmetry() {
        let mut cam = default_camera();
        cam.projection = ProjectionMode::Orthographic;
        let proj = cam.build_proj_matrix();
        let m: [[f32; 4]; 4] = proj.into();
        assert!(m[0][0].abs() > 0.0);
        assert!(m[1][1].abs() > 0.0);
        assert!((m[3][3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn view_matrix_look_at_origin() {
        let cam = default_camera();
        let view = cam.build_view_matrix();
        let m: [[f32; 4]; 4] = view.into();
        assert!((m[3][2] - (-3.0)).abs() < 1e-5);
    }

    #[test]
    fn camera_from_bounds_frames_unit_cube() {
        let bounds = AABB {
            min: cgmath::Point3::new(0.0, 0.0, 0.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        };
        let cam = camera_from_bounds(&bounds, 16.0 / 9.0);
        let center = bounds.center();
        assert!((cam.target.x - center.x).abs() < 1e-5);
        assert!((cam.target.y - center.y).abs() < 1e-5);
        assert!((cam.target.z - center.z).abs() < 1e-5);
        let dist = ((cam.eye.x - cam.target.x).powi(2)
            + (cam.eye.y - cam.target.y).powi(2)
            + (cam.eye.z - cam.target.z).powi(2))
        .sqrt();
        assert!(dist > bounds.diagonal() * 0.5);
    }

    #[test]
    fn camera_from_bounds_different_aspects() {
        let bounds = AABB {
            min: cgmath::Point3::new(-1.0, -1.0, -1.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        };
        let cam_wide = camera_from_bounds(&bounds, 2.0);
        let cam_tall = camera_from_bounds(&bounds, 0.5);
        assert!((cam_wide.target.x - cam_tall.target.x).abs() < 1e-5);
        assert!((cam_wide.target.y - cam_tall.target.y).abs() < 1e-5);
        assert!((cam_wide.aspect - 2.0).abs() < 1e-5);
        assert!((cam_tall.aspect - 0.5).abs() < 1e-5);
    }

    fn unit_cube_bounds() -> AABB {
        AABB {
            min: cgmath::Point3::new(0.0, 0.0, 0.0),
            max: cgmath::Point3::new(1.0, 1.0, 1.0),
        }
    }

    #[test]
    fn camera_from_bounds_axis_placement() {
        let bounds = unit_cube_bounds();
        let center = bounds.center();

        let cam = camera_from_bounds_axis(
            &bounds,
            1.0,
            cgmath::Vector3::new(0.0, 0.0, 1.0),
            cgmath::Vector3::new(0.0, 1.0, 0.0),
        );
        assert!(cam.eye.z > center.z, "eye should be in front of center");
        assert!((cam.eye.x - center.x).abs() < 1e-5, "no X offset");
        assert!((cam.eye.y - center.y).abs() < 1e-5, "no Y offset");
        assert!((cam.target.x - center.x).abs() < 1e-5);
        assert!((cam.target.y - center.y).abs() < 1e-5);

        let cam = camera_from_bounds_axis(
            &bounds,
            1.0,
            cgmath::Vector3::new(0.0, 1.0, 0.0),
            cgmath::Vector3::new(0.0, 0.0, -1.0),
        );
        assert!(cam.eye.y > center.y, "eye should be above center");
        assert!((cam.eye.x - center.x).abs() < 1e-5, "no X offset");

        let cam = camera_from_bounds_axis(
            &bounds,
            1.0,
            cgmath::Vector3::new(1.0, 0.0, 0.0),
            cgmath::Vector3::new(0.0, 1.0, 0.0),
        );
        assert!(cam.eye.x > center.x, "eye should be right of center");
        assert!((cam.eye.y - center.y).abs() < 1e-5, "no Y offset");
    }

    #[test]
    fn camera_from_bounds_axis_uses_orthographic() {
        let bounds = unit_cube_bounds();
        let cam = camera_from_bounds_axis(
            &bounds,
            1.0,
            cgmath::Vector3::new(0.0, 0.0, 1.0),
            cgmath::Vector3::new(0.0, 1.0, 0.0),
        );
        assert!(
            matches!(cam.projection, ProjectionMode::Orthographic),
            "axis camera should use Orthographic"
        );
        assert!(cam.ortho_scale > 0.0);
        assert!(cam.znear > 0.0);
        assert!(cam.zfar > cam.znear);
    }

    #[test]
    fn camera_from_bounds_zero_volume() {
        let bounds = AABB {
            min: cgmath::Point3::new(5.0, 5.0, 5.0),
            max: cgmath::Point3::new(5.0, 5.0, 5.0),
        };
        let cam = camera_from_bounds(&bounds, 1.0);
        assert!(cam.eye.x.is_finite());
        assert!(cam.eye.y.is_finite());
        assert!(cam.eye.z.is_finite());
        assert!(cam.znear > 0.0);

        let cam2 = camera_from_bounds_axis(
            &bounds,
            1.0,
            cgmath::Vector3::new(0.0, 0.0, 1.0),
            cgmath::Vector3::new(0.0, 1.0, 0.0),
        );
        assert!(cam2.eye.z.is_finite());
        assert!(cam2.ortho_scale.is_finite());
    }

    #[test]
    fn world_per_pixel_is_the_constant_screen_size_contract() {
        // A perspective camera 10 units back, 60 degree vertical fov, 800px pane.
        let mut cam = Camera {
            eye: cgmath::Point3::new(0.0, 0.0, 10.0),
            target: cgmath::Point3::new(0.0, 0.0, 0.0),
            up: cgmath::Vector3::unit_y(),
            aspect: 1.0,
            fovy: 60.0,
            znear: 0.1,
            zfar: 100.0,
            projection: ProjectionMode::Perspective,
            ortho_scale: 1.0,
        };

        // The visible half-height at the origin is 10 * tan(30deg); a pixel is
        // that span over half the pane.
        let wpp = cam.world_per_pixel(cgmath::Point3::new(0.0, 0.0, 0.0), 800.0);
        let expected = 2.0 * 10.0 * (30.0f32).to_radians().tan() / 800.0;
        assert!((wpp - expected).abs() < 1e-6, "{wpp} vs {expected}");

        // Twice as far away => a handle must be twice as large in world units to
        // cover the same pixels.
        let far = cam.world_per_pixel(cgmath::Point3::new(0.0, 0.0, -10.0), 800.0);
        assert!((far / wpp - 2.0).abs() < 1e-4, "ratio {}", far / wpp);

        // Orthographic ignores depth entirely: its half-height IS ortho_scale.
        cam.projection = ProjectionMode::Orthographic;
        cam.ortho_scale = 4.0;
        let near_o = cam.world_per_pixel(cgmath::Point3::new(0.0, 0.0, 0.0), 800.0);
        let far_o = cam.world_per_pixel(cgmath::Point3::new(0.0, 0.0, -50.0), 800.0);
        assert!((near_o - 2.0 * 4.0 / 800.0).abs() < 1e-6);
        assert!(
            (near_o - far_o).abs() < 1e-9,
            "ortho does not scale with depth"
        );
    }

    #[test]
    fn turntable_up_matches_preset_ups() {
        use std::f32::consts::FRAC_PI_2;
        let top = turntable_up(0.0, FRAC_PI_2);
        assert!(top.x.abs() < 1e-6 && top.y.abs() < 1e-6 && (top.z + 1.0).abs() < 1e-6);
        let bottom = turntable_up(0.0, -FRAC_PI_2);
        assert!(bottom.x.abs() < 1e-6 && bottom.y.abs() < 1e-6 && (bottom.z - 1.0).abs() < 1e-6);
        let horizon = turntable_up(0.7, 0.0);
        assert!(horizon.x.abs() < 1e-6 && (horizon.y - 1.0).abs() < 1e-6);
    }

    /// Drives one orbit drag: press, an anchor move, a delta move, update.
    fn drag(cam: &mut Camera, ctl: &mut CameraController, dx: f32, dy: f32) {
        ctl.handle_mouse_button(PointerButton::Left, true);
        ctl.handle_mouse_move(500.0, 500.0);
        ctl.handle_mouse_move(500.0 + dx, 500.0 + dy);
        ctl.update_camera(cam);
        ctl.handle_mouse_button(PointerButton::Left, false);
    }

    #[test]
    fn orbit_after_top_preset_keeps_a_continuous_up() {
        use cgmath::InnerSpace;
        let bounds = unit_cube_bounds();
        // The top preset: looking straight down, up parked at -Z.
        let mut cam = camera_from_bounds_axis(
            &bounds,
            1.0,
            cgmath::Vector3::unit_y(),
            -cgmath::Vector3::unit_z(),
        );
        let mut ctl = CameraController::new(0.2);

        // Tilt away from the pole (drag up = pitch down).
        drag(&mut cam, &mut ctl, 0.0, -30.0);
        let forward = (cam.target - cam.eye).normalize();
        assert!((cam.up.magnitude() - 1.0).abs() < 1e-4, "up stays unit");
        assert!(cam.up.dot(forward).abs() < 1e-4, "up stays orthogonal");
        let right = forward.cross(cam.up);
        assert!(right.y.abs() < 1e-4, "no roll: the right vector is level");
        assert!(cam.up.z < -0.9, "a small tilt keeps up near -Z, no flip");
        assert!(cam.up.y > 0.0, "tilting down starts raising up toward +Y");

        // Continue down to the horizon: up must arrive upright, still no roll.
        drag(&mut cam, &mut ctl, 0.0, -284.0);
        let forward = (cam.target - cam.eye).normalize();
        assert!(cam.up.y > 0.95, "at the horizon up is world +Y");
        let right = forward.cross(cam.up);
        assert!(right.y.abs() < 1e-4, "still no roll after the full sweep");
    }

    #[test]
    fn orbit_at_pole_seeds_yaw_from_up() {
        use std::f32::consts::FRAC_PI_2;
        let bounds = unit_cube_bounds();
        let mut cam = camera_from_bounds_axis(
            &bounds,
            1.0,
            cgmath::Vector3::unit_y(),
            -cgmath::Vector3::unit_z(),
        );
        // A top view whose heading was yaw0 when it reached the pole (a
        // restored .slxy pose, or a preset entered from an angled orbit).
        let yaw0 = 1.0_f32;
        cam.up = turntable_up(yaw0, FRAC_PI_2);

        let mut ctl = CameraController::new(0.2);
        drag(&mut cam, &mut ctl, 0.0, -40.0);

        let off = cam.eye - cam.target;
        let yaw = off.x.atan2(off.z);
        assert!(
            (yaw - yaw0).abs() < 1e-3,
            "first drag continues the stored heading: {yaw} vs {yaw0}"
        );
    }

    #[test]
    fn projection_toggle_round_trips_framing() {
        use cgmath::InnerSpace;
        let mut cam = default_camera();
        let target = cam.target;
        let d0 = (cam.eye - cam.target).magnitude();
        let wpp0 = cam.world_per_pixel(target, 800.0);

        cam.set_projection_preserving_framing(ProjectionMode::Orthographic);
        let wpp_ortho = cam.world_per_pixel(target, 800.0);
        assert!(
            (wpp_ortho - wpp0).abs() < 1e-6,
            "ortho keeps the apparent size at the target"
        );

        cam.set_projection_preserving_framing(ProjectionMode::Perspective);
        let d1 = (cam.eye - cam.target).magnitude();
        let wpp1 = cam.world_per_pixel(target, 800.0);
        assert!((d1 - d0).abs() < 1e-4, "distance round-trips: {d1} vs {d0}");
        assert!((wpp1 - wpp0).abs() < 1e-6, "apparent size round-trips");
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use solarxy_core::preferences::ProjectionMode;

    fn cam(projection: ProjectionMode) -> Camera {
        Camera {
            eye: cgmath::Point3::new(1.0, 2.0, 6.0),
            target: cgmath::Point3::new(0.0, 0.5, 0.0),
            up: cgmath::Vector3::unit_y(),
            aspect: 16.0 / 9.0,
            fovy: 42.0,
            znear: 0.1,
            zfar: 120.0,
            projection,
            ortho_scale: 3.0,
        }
    }

    fn close(a: cgmath::Matrix4<f32>, b: cgmath::Matrix4<f32>, tol: f32, what: &str) {
        let (a, b): ([[f32; 4]; 4], [[f32; 4]; 4]) = (a.into(), b.into());
        for c in 0..4 {
            for r in 0..4 {
                assert!(
                    (a[c][r] - b[c][r]).abs() <= tol,
                    "{what}: [{c}][{r}] {} against {}",
                    a[c][r],
                    b[c][r]
                );
            }
        }
    }

    /// The property the whole tiling rests on: one tile covering the image is
    /// the image.
    ///
    /// If this fails, every tiled render is a slightly different shot from the
    /// untiled one, which shows up as a seam only where two tiles meet and as
    /// nothing at all in a single-tile render.
    #[test]
    fn windowing_the_whole_image_is_the_unwindowed_projection() {
        for projection in [ProjectionMode::Perspective, ProjectionMode::Orthographic] {
            let c = cam(projection);
            close(
                c.build_proj_matrix_windowed([0.0, 0.0], [1920.0, 1080.0], [1920.0, 1080.0]),
                c.build_proj_matrix(),
                1e-5,
                "the whole rect",
            );
        }
    }

    /// Four tiles reassemble into the same frustum they were cut from.
    ///
    /// Checked at the corners rather than on the matrices, because that is the
    /// statement that matters: a point on the shared edge of two tiles has to
    /// land on the edge of both, or the assembled image has a seam.
    #[test]
    fn adjacent_tiles_agree_on_the_edge_between_them() {
        let c = cam(ProjectionMode::Perspective);
        let full = [1024.0, 512.0];
        let left = c.build_proj_matrix_windowed([0.0, 0.0], [512.0, 512.0], full);
        let right = c.build_proj_matrix_windowed([512.0, 0.0], [512.0, 512.0], full);

        // A point on the vertical seam projects to the right edge of the left
        // tile and the left edge of the right one.
        let view = c.build_view_matrix();
        let world = cgmath::Point3::new(0.0, 0.0, 0.0);
        let eye = view * world.to_homogeneous();
        let ndc = |m: cgmath::Matrix4<f32>| {
            let p = m * eye;
            p.x / p.w
        };
        assert!(
            (ndc(left) - 1.0).abs() < 1e-4,
            "the seam is at {} of the left tile rather than its right edge",
            ndc(left)
        );
        assert!(
            (ndc(right) + 1.0).abs() < 1e-4,
            "the seam is at {} of the right tile rather than its left edge",
            ndc(right)
        );
    }

    /// A tile is a window, not a zoom: the top-left tile has to look up and
    /// left, which is what an asymmetric frustum is for and what a symmetric
    /// one with a narrower field of view would get wrong.
    #[test]
    fn a_corner_tile_looks_off_axis() {
        let c = cam(ProjectionMode::Perspective);
        let m: [[f32; 4]; 4] = c
            .build_proj_matrix_windowed([0.0, 0.0], [256.0, 256.0], [1024.0, 1024.0])
            .into();
        // In a symmetric frustum the third column's x and y are zero; in an
        // off-axis one they carry the shear that aims it.
        assert!(
            m[2][0].abs() > 0.1,
            "the corner tile's frustum is not sheared horizontally"
        );
        assert!(
            m[2][1].abs() > 0.1,
            "the corner tile's frustum is not sheared vertically"
        );
    }
}
