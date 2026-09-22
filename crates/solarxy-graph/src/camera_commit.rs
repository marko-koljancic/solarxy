//! The no-movement guard on a locked look-through pane's camera commit.
//!
//! Both graphical shells write a navigated pose back to the bound `camera`
//! node as one undo step, and both have to know when a press and release
//! moved nothing, because an unconditional commit pushes an undo step that
//! changes nothing and, on the browser, swallows the click that should have
//! picked. The guard reads the node's own parameters, so it lives here with
//! them rather than in the shared host, which has no engine dependency by
//! design; it was the browser's alone until the desktop wrote poses back.

use std::collections::BTreeMap;

use crate::params::{ParamSource, ParamValue};

/// Whether the pose a locked look-through pane would commit is already the
/// one on its bound camera node.
///
/// Compared in f32, exactly. f32 is the camera's own precision, and the same
/// narrowing the node-to-pane follow applies when it seats the node's pose
/// on the pane camera; comparing there means an untouched pose is equal even
/// when the stored f64 literal is not representable in f32 (a hand-typed
/// 0.1, say). A tolerance would be looser than the round trip needs and
/// would swallow a real one-pixel reframe.
///
/// A node missing either param, or holding an expression, compares as
/// changed: the commit then writes the literal pose, which is what a gesture
/// on such a pane has to mean.
#[must_use]
pub fn pose_unchanged(
    params: &BTreeMap<String, ParamSource>,
    eye: [f32; 3],
    target: [f32; 3],
) -> bool {
    stored_matches(params.get("position"), eye) && stored_matches(params.get("target"), target)
}

// The exact comparison IS the design: see the doc comment above. A tolerance
// is what the lint wants and what this guard must not have.
#[allow(clippy::float_cmp)]
fn stored_matches(stored: Option<&ParamSource>, live: [f32; 3]) -> bool {
    match stored.and_then(ParamSource::literal) {
        Some(ParamValue::Vec3(v)) => [v[0] as f32, v[1] as f32, v[2] as f32] == live,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam_params(position: [f64; 3], target: [f64; 3]) -> BTreeMap<String, ParamSource> {
        BTreeMap::from([
            (
                "position".to_owned(),
                ParamSource::Literal(ParamValue::Vec3(position)),
            ),
            (
                "target".to_owned(),
                ParamSource::Literal(ParamValue::Vec3(target)),
            ),
        ])
    }

    /// The seam the defect lived in: a press and release with no movement
    /// must compare unchanged, so the commit returns nothing and the click
    /// ladder gets its turn.
    #[test]
    fn an_unmoved_pose_compares_unchanged() {
        let params = cam_params([1.0, 2.0, 3.0], [0.0, 0.0, 0.0]);
        assert!(pose_unchanged(&params, [1.0, 2.0, 3.0], [0.0, 0.0, 0.0]));
    }

    /// A hand-typed literal that f32 cannot represent still compares
    /// unchanged against the camera that was seated from it, because the
    /// comparison narrows the way the follow does.
    #[test]
    fn a_hand_typed_pose_survives_the_narrowing() {
        let params = cam_params([0.1, 0.2, 0.3], [10.1, 0.0, 0.0]);
        let eye = [0.1f64 as f32, 0.2f64 as f32, 0.3f64 as f32];
        let target = [10.1f64 as f32, 0.0, 0.0];
        assert!(pose_unchanged(&params, eye, target));
    }

    /// A drag that moved either half of the pose commits.
    #[test]
    fn a_moved_pose_compares_changed() {
        let params = cam_params([1.0, 2.0, 3.0], [0.0, 0.0, 0.0]);
        assert!(!pose_unchanged(&params, [1.0, 2.5, 3.0], [0.0, 0.0, 0.0]));
        assert!(!pose_unchanged(&params, [1.0, 2.0, 3.0], [0.0, 0.1, 0.0]));
    }

    /// A node missing a pose param, or carrying an expression, compares as
    /// changed, so the gesture writes the literal pose.
    #[test]
    fn a_missing_or_expression_pose_compares_changed() {
        assert!(!pose_unchanged(&BTreeMap::new(), [0.0; 3], [0.0; 3]));
        let params = BTreeMap::from([
            (
                "position".to_owned(),
                ParamSource::Expression { expr: "1".into() },
            ),
            (
                "target".to_owned(),
                ParamSource::Literal(ParamValue::Vec3([0.0; 3])),
            ),
        ]);
        assert!(!pose_unchanged(&params, [0.0; 3], [0.0; 3]));
    }
}
