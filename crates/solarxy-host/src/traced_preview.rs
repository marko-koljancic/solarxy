//! The interactive traced preview: what a pane's path tracer runs at, and
//! what a camera has to keep the same for an accumulation to stay valid.
//!
//! Both graphical shells drive a per-pane preview on the shared tracer, and
//! these are the terms they drive it on. They live here rather than in
//! either host so the two cannot converge to different budgets, the way the
//! marker projection lives here so they cannot cull differently. The
//! housekeeping around them (when to reset, when to re-assert) reads an
//! engine and writes a backend, so it stays a per-shell twin; the numbers
//! and the pure rule do not.

use solarxy_core::preferences::ProjectionMode;
use solarxy_renderer::camera::Camera;
use solarxy_renderer::pathtrace::backend::TraceSettings;

/// What the traced preview converges to. High enough that a resting pane
/// keeps improving for minutes at one sample per frame, low enough that
/// the counter's target still means something.
pub const PREVIEW_TARGET_SAMPLES: u32 = 4096;

/// The preview traces at half resolution and the resolve upscales the
/// mean into the pane, so a sample costs a quarter of the rays a full-size
/// one would and the window stays responsive at one sample per frame.
pub const PREVIEW_RESOLUTION_SCALE: f32 = 0.5;

/// The traced preview's settings: one sample per frame (the pacing that
/// keeps the shell responsive), half resolution, and the edge-aware filter,
/// which defaults on because a one-sample frame is unusable without it.
/// Asserted before every preview encode rather than held, since the still
/// job authors its own settings on the same backend.
///
/// The filter is the one value a person can turn off, and the reason to is
/// judging what the tracer actually produced rather than what the filter
/// made of it, which matters most at the sample counts where the filter is
/// doing the most work.
#[must_use]
pub fn preview_trace_settings(denoise: bool) -> TraceSettings {
    TraceSettings {
        samples: PREVIEW_TARGET_SAMPLES,
        chunk: 1,
        denoise,
        resolution_scale: PREVIEW_RESOLUTION_SCALE,
        ..TraceSettings::default()
    }
}

/// The fields of a camera a traced accumulation is valid under. Aspect
/// included, because a resize reshapes every ray; the projection kind
/// rides as a discriminant.
#[must_use]
pub fn camera_key(c: &Camera) -> [f32; 13] {
    [
        c.eye.x,
        c.eye.y,
        c.eye.z,
        c.target.x,
        c.target.y,
        c.target.z,
        c.up.x,
        c.up.y,
        c.up.z,
        c.fovy,
        c.aspect,
        c.ortho_scale,
        match c.projection {
            ProjectionMode::Perspective => 0.0,
            ProjectionMode::Orthographic => 1.0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        Camera {
            eye: cgmath::Point3::new(0.0, 1.0, 5.0),
            target: cgmath::Point3::new(0.0, 0.0, 0.0),
            up: cgmath::Vector3::new(0.0, 1.0, 0.0),
            aspect: 1.5,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
            projection: ProjectionMode::Perspective,
            ortho_scale: 1.0,
        }
    }

    /// The key is an identity, compared exactly at the encode, so the tests
    /// compare it exactly too: by bit pattern, which is what the encode's
    /// comparison amounts to.
    fn bits(c: &Camera) -> [u32; 13] {
        camera_key(c).map(f32::to_bits)
    }

    #[test]
    fn the_preview_states_its_own_terms_and_leaves_the_rest_default() {
        let on = preview_trace_settings(true);
        assert_eq!(on.samples, PREVIEW_TARGET_SAMPLES);
        assert_eq!(on.chunk, 1, "one sample per frame");
        assert!(on.denoise);
        assert!((on.resolution_scale - PREVIEW_RESOLUTION_SCALE).abs() < f32::EPSILON);
        let off = preview_trace_settings(false);
        assert!(
            !off.denoise,
            "the filter is the one value a person can turn off"
        );
        let defaults = TraceSettings::default();
        assert_eq!(on.bounces, defaults.bounces);
        assert_eq!(on.transmissive_bounces, defaults.transmissive_bounces);
        assert_eq!(on.transparent_background, defaults.transparent_background);
    }

    #[test]
    fn a_resize_changes_the_camera_key() {
        let before = bits(&camera());
        let mut resized = camera();
        resized.aspect = 2.0;
        assert_ne!(bits(&resized), before, "a resize reshapes every ray");
    }

    #[test]
    fn the_projection_kind_changes_the_camera_key() {
        let before = bits(&camera());
        let mut ortho = camera();
        ortho.projection = ProjectionMode::Orthographic;
        assert_ne!(bits(&ortho), before);
    }

    #[test]
    fn a_clip_plane_change_does_not_change_the_camera_key() {
        // The near and far planes do not move a ray; only the pose, the
        // field of view, the aspect and the projection do.
        let before = bits(&camera());
        let mut planes = camera();
        planes.znear = 0.5;
        planes.zfar = 50.0;
        assert_eq!(bits(&planes), before);
    }
}
