//! Where a review marker lands on a pane.
//!
//! The engine resolves each note's anchor to a world point every frame;
//! this is the one projection both shells run over it, so the two cannot
//! disagree about which markers a pane shows or where. The answer is in the
//! pane's own pixel units, from its top-left corner: the browser divides by
//! its device pixel ratio afterwards, and the desktop's pane rect is already
//! in logical pixels, so each caller scales rather than this function
//! guessing which unit it was handed.
//!
//! The culls are the browser's, which the desktop used to lack: a little
//! slack beyond the frustum so a pin fades at the edge instead of popping
//! exactly on it, and the clip-space depth range, which is what rejects a
//! point behind an orthographic camera, where `clip.w` is a constant one and
//! the usual sign test says nothing.

use cgmath::{Matrix4, Vector4};

/// Screen-edge slack in NDC: a little beyond the frustum so pins fade at
/// the edge instead of popping exactly on it.
pub const NDC_XY_SLACK: f32 = 1.05;
/// The wgpu clip-space depth range with the same slack. The z cull is what
/// rejects behind-camera points under orthographic projection.
pub const NDC_Z_MIN: f32 = -0.05;
/// See [`NDC_Z_MIN`].
pub const NDC_Z_MAX: f32 = 1.05;

/// Project a world point through a pane's view-projection into the pane's
/// pixels, `(x, y)` from the pane's top-left, with `pane_size` in the same
/// units the answer is wanted in. `None` when the point is behind a
/// perspective camera, outside the frustum beyond the slack, or outside the
/// depth range.
#[must_use]
pub fn project_to_pane(
    view_proj: &Matrix4<f32>,
    world: [f32; 3],
    pane_size: (f32, f32),
) -> Option<(f32, f32)> {
    let clip = view_proj * Vector4::new(world[0], world[1], world[2], 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = (clip.x / clip.w, clip.y / clip.w, clip.z / clip.w);
    if ndc.0.abs() > NDC_XY_SLACK
        || ndc.1.abs() > NDC_XY_SLACK
        || !(NDC_Z_MIN..=NDC_Z_MAX).contains(&ndc.2)
    {
        return None;
    }
    Some((
        f32::midpoint(ndc.0, 1.0) * pane_size.0,
        (1.0 - ndc.1) * 0.5 * pane_size.1,
    ))
}

#[cfg(test)]
mod tests {
    use cgmath::SquareMatrix;

    use super::*;

    const PANE: (f32, f32) = (200.0, 100.0);

    fn project(world: [f32; 3]) -> Option<(f32, f32)> {
        project_to_pane(&Matrix4::identity(), world, PANE)
    }

    #[test]
    fn the_origin_lands_at_the_pane_centre_under_identity() {
        let (x, y) = project([0.0, 0.0, 0.0]).expect("inside");
        assert!((x - 100.0).abs() < 1e-4, "x = {x}");
        assert!((y - 50.0).abs() < 1e-4, "y = {y}");
    }

    #[test]
    fn a_point_beyond_the_frustum_survives_within_the_slack_and_not_past_it() {
        assert!(
            project([1.04, 0.0, 0.0]).is_some(),
            "x just inside the slack"
        );
        assert!(project([1.06, 0.0, 0.0]).is_none(), "x just past the slack");
        assert!(
            project([0.0, -1.04, 0.0]).is_some(),
            "y just inside the slack"
        );
        assert!(
            project([0.0, -1.06, 0.0]).is_none(),
            "y just past the slack"
        );
    }

    #[test]
    fn the_depth_range_rejects_behind_the_near_plane_and_past_the_far_plane() {
        assert!(
            project([0.0, 0.0, -0.04]).is_some(),
            "z just inside the near slack"
        );
        assert!(
            project([0.0, 0.0, -0.06]).is_none(),
            "z behind the near plane"
        );
        assert!(
            project([0.0, 0.0, 1.04]).is_some(),
            "z just inside the far slack"
        );
        assert!(project([0.0, 0.0, 1.06]).is_none(), "z past the far plane");
    }

    #[test]
    fn a_point_behind_a_perspective_camera_is_rejected_by_its_sign() {
        let mut m = Matrix4::identity();
        m[3][3] = -1.0;
        assert!(project_to_pane(&m, [0.0, 0.0, 0.0], PANE).is_none());
    }

    #[test]
    fn the_answer_is_in_the_panes_own_units() {
        // The same point at twice the pane size lands twice as far from the
        // corner: the caller chooses the unit by choosing the size.
        let (x, y) = project([0.5, 0.5, 0.0]).expect("inside");
        let (x2, y2) =
            project_to_pane(&Matrix4::identity(), [0.5, 0.5, 0.0], (400.0, 200.0)).expect("inside");
        assert!((x2 - 2.0 * x).abs() < 1e-4);
        assert!((y2 - 2.0 * y).abs() < 1e-4);
    }
}
