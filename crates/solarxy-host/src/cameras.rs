//! Per-pane camera lifecycle and the depth-range fit.

use cgmath::Vector3;
use solarxy_core::AABB;
use solarxy_core::preferences::ProjectionMode;
use solarxy_renderer::camera::Camera;
use solarxy_renderer::camera_state::CameraState;

/// The near and far planes that put `bounds` exactly inside the Depth
/// inspection mode's visible range, for `camera`.
///
/// Existed three times before this crate: once in each shell and once more in
/// the golden harness, whose copy carried a comment saying it was the same
/// math. It is pure, so it is the one function here that moved without
/// reconciling anything.
#[must_use]
pub fn depth_bounds(camera: &Camera, bounds: &AABB) -> (f32, f32) {
    let view = camera.build_view_matrix();
    let mut z_min = f32::INFINITY;
    let mut z_max = f32::NEG_INFINITY;
    for corner in &bounds.corners() {
        let vp = view * corner.to_homogeneous();
        let z = -vp.z;
        z_min = z_min.min(z);
        z_max = z_max.max(z);
    }
    z_min = z_min.max(0.001);
    if z_max <= z_min {
        z_max = z_min + 1.0;
    }
    (z_min, z_max)
}

/// Lazily create a [`CameraState`] for every pane slot the layout uses.
///
/// Idempotent: a slot that already holds a camera is skipped, so layout
/// toggles preserve per-slot cameras within a session. Slot 0 is the primary
/// perspective camera; slots 1 to 3 are cloned from it and reset to Top,
/// Front and Left as a one-time convenience the user re-orients afterwards.
///
/// `projection` is the shell's startup preference for slot 0, or `None` where
/// the shell has no such preference and takes the camera's own default. It is
/// deliberately not applied to slots 1 to 3: those are reset to an axis view
/// immediately afterwards, which sets their projection itself.
pub fn ensure_pane_cameras(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    cameras: &mut [Option<CameraState>; 4],
    bounds: &AABB,
    aspect: f32,
    count: usize,
    projection: Option<ProjectionMode>,
) {
    for i in 0..count.min(cameras.len()) {
        if cameras[i].is_some() {
            continue;
        }
        let mut cam = if i == 0 {
            CameraState::new(device, camera_layout, bounds, aspect)
        } else if let Some(src) = cameras[0].as_ref() {
            src.clone_with_new_resources(device, camera_layout)
        } else {
            continue;
        };
        match i {
            0 => {
                if let Some(mode) = projection {
                    cam.set_projection(mode);
                }
            }
            1 => cam.reset_to_bounds_axis(bounds, Vector3::unit_y(), -Vector3::unit_z()),
            2 => cam.reset_to_bounds_axis(bounds, Vector3::unit_z(), Vector3::unit_y()),
            _ => cam.reset_to_bounds_axis(bounds, -Vector3::unit_x(), Vector3::unit_y()),
        }
        cameras[i] = Some(cam);
    }
}
