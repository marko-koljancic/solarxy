//! The transform manipulator overlay (phase 11): the translate gizmo.
//!
//! Pull-based, exactly like the rest of the renderer's overlay state: the host
//! hands over a [`ManipulatorState`] (or `None`) and the renderer draws it. The
//! desktop shell never calls it, so the desktop is unaffected until it wants the
//! same feature, at which point it wires the same three pieces.
//!
//! Geometry is generated on the CPU into the existing [`GizmoVertex`] format and
//! drawn through the existing gizmo shader, so this adds no shader and no bind
//! group -- only two pipeline variants that draw ON TOP of the scene
//! (`depth_compare: Always`), because a handle buried inside the mesh it is
//! meant to move is useless.
//!
//! Sizing is the caller's job: `scale` is world units per gizmo unit, obtained
//! from [`crate::camera::Camera::world_per_pixel`], which the hit-tester also
//! uses. That shared helper is what keeps the grab zones aligned with the
//! visuals.

use cgmath::{Matrix4, Point3, Transform, Vector3};

use crate::model::GizmoVertex;

/// One grabbable part of the manipulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    AxisX,
    AxisY,
    AxisZ,
    /// The little square between two axes: drags in that plane.
    PlaneXY,
    PlaneYZ,
    PlaneZX,
}

impl Handle {
    /// The two axis indices a plane handle spans; `None` for an axis handle.
    #[must_use]
    pub fn plane_axes(self) -> Option<(usize, usize)> {
        match self {
            Handle::PlaneXY => Some((0, 1)),
            Handle::PlaneYZ => Some((1, 2)),
            Handle::PlaneZX => Some((2, 0)),
            _ => None,
        }
    }

    /// The axis index an axis handle runs along; `None` for a plane handle.
    #[must_use]
    pub fn axis(self) -> Option<usize> {
        match self {
            Handle::AxisX => Some(0),
            Handle::AxisY => Some(1),
            Handle::AxisZ => Some(2),
            _ => None,
        }
    }

    /// Every handle, in hit-test priority order: plane handles first, so the
    /// small square between two axes stays grabbable where it overlaps them.
    #[must_use]
    pub fn all() -> [Handle; 6] {
        [
            Handle::PlaneXY,
            Handle::PlaneYZ,
            Handle::PlaneZX,
            Handle::AxisX,
            Handle::AxisY,
            Handle::AxisZ,
        ]
    }
}

/// Which tool the manipulator is showing. Rotate and Scale are Phase 12; the
/// variants exist so the host can carry the mode without a second enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManipulatorTool {
    Translate,
}

/// Everything the renderer needs to draw the manipulator this frame.
#[derive(Debug, Clone, Copy)]
pub struct ManipulatorState {
    /// World matrix of the thing being manipulated. Only the translation is used
    /// in phase 11 (the gizmo is world-oriented; local orientation is Phase 12).
    pub anchor: Matrix4<f32>,
    pub tool: ManipulatorTool,
    pub hovered: Option<Handle>,
    pub active: Option<Handle>,
    /// World units per gizmo unit: `GIZMO_PX * world_per_pixel(...)`. Recomputed
    /// per pane, since a pane's camera and height decide it.
    pub scale: f32,
}

// ---- geometry constants (in gizmo units; multiplied by `scale`) ----

/// Arrow length, in pixels once scaled. The rest of the gizmo is proportional.
pub const GIZMO_PX: f32 = 90.0;
/// Click tolerance around an axis shaft, in pixels.
pub const HIT_PX: f32 = 9.0;

/// Where the shaft stops and the arrowhead begins.
const HEAD_START: f32 = 0.78;
const HEAD_RADIUS: f32 = 0.055;
/// Plane handles sit out at this fraction, spanning this much.
const PLANE_OFFSET: f32 = 0.30;
const PLANE_SIZE: f32 = 0.22;
/// The shaft starts slightly out from the origin so the three axes do not
/// smear into one blob at the centre.
const SHAFT_START: f32 = 0.12;

/// The colours, chosen to survive the ACES tone mapping the overlay rides
/// through (it draws inside the main HDR pass, like the axis gizmo) and to stay
/// below the bloom threshold.
const X_COLOR: [f32; 3] = [0.85, 0.22, 0.28];
const Y_COLOR: [f32; 3] = [0.35, 0.75, 0.28];
const Z_COLOR: [f32; 3] = [0.25, 0.48, 0.90];
/// Hover and grab both go amber, matching the app's accent.
const HILIGHT: [f32; 3] = [1.0, 0.78, 0.28];

fn axis_dir(axis: usize) -> Vector3<f32> {
    match axis {
        0 => Vector3::unit_x(),
        1 => Vector3::unit_y(),
        _ => Vector3::unit_z(),
    }
}

impl ManipulatorState {
    /// The colour a handle draws in, accounting for hover and grab.
    fn color_for(&self, handle: Handle) -> [f32; 3] {
        // While a drag is live the grabbed handle wins and hover is ignored: the
        // user is committed, and a stray hover highlight would be a lie.
        if self.active == Some(handle) || (self.active.is_none() && self.hovered == Some(handle)) {
            return HILIGHT;
        }
        // A plane handle takes the colour of the axis it is NORMAL to, the way
        // Blender and Maya both do it, so the YZ square reads as "the X plane".
        match handle {
            Handle::AxisX | Handle::PlaneYZ => X_COLOR,
            Handle::AxisY | Handle::PlaneZX => Y_COLOR,
            Handle::AxisZ | Handle::PlaneXY => Z_COLOR,
        }
    }

    /// The world-space origin of the manipulator.
    #[must_use]
    pub fn origin(&self) -> Point3<f32> {
        Point3::new(self.anchor.w.x, self.anchor.w.y, self.anchor.w.z)
    }

    /// The world-space endpoints of an axis shaft: what the ray-capsule hit test
    /// runs against, and what the drag re-parametrizes along.
    ///
    /// Shared with the hit tester on purpose -- the grab zone IS the drawn shaft.
    #[must_use]
    pub fn axis_segment(&self, axis: usize) -> (Point3<f32>, Point3<f32>) {
        let o = self.origin();
        let d = axis_dir(axis) * self.scale;
        (o + d * SHAFT_START, o + d)
    }

    /// A plane handle's quad: origin corner plus the two edge vectors.
    #[must_use]
    pub fn plane_quad(&self, a: usize, b: usize) -> (Point3<f32>, Vector3<f32>, Vector3<f32>) {
        let o = self.origin();
        let (ua, ub) = (axis_dir(a) * self.scale, axis_dir(b) * self.scale);
        let corner = o + ua * PLANE_OFFSET + ub * PLANE_OFFSET;
        (corner, ua * PLANE_SIZE, ub * PLANE_SIZE)
    }

    /// Builds this frame's overlay geometry: `(lines, triangles)`.
    ///
    /// Lines carry the shafts; triangles carry the arrowheads and the plane
    /// quads. Two lists because they need different primitive topologies, not
    /// because they are conceptually different.
    #[must_use]
    pub fn build_vertices(&self) -> (Vec<GizmoVertex>, Vec<GizmoVertex>) {
        let mut lines = Vec::with_capacity(6);
        let mut tris = Vec::with_capacity(3 * 8 * 3 + 6 * 3);

        for axis in 0..3 {
            let handle = match axis {
                0 => Handle::AxisX,
                1 => Handle::AxisY,
                _ => Handle::AxisZ,
            };
            let color = self.color_for(handle);
            let (start, end) = self.axis_segment(axis);

            lines.push(GizmoVertex {
                position: start.into(),
                color,
            });
            lines.push(GizmoVertex {
                position: end.into(),
                color,
            });

            self.push_arrowhead(&mut tris, axis, color);
        }

        for (a, b, handle) in [
            (0usize, 1usize, Handle::PlaneXY),
            (1, 2, Handle::PlaneYZ),
            (2, 0, Handle::PlaneZX),
        ] {
            let color = self.color_for(handle);
            let (corner, u, v) = self.plane_quad(a, b);
            // Two triangles, wound both ways: the quad must read from either
            // side, and the pipeline does not cull.
            let p0 = corner;
            let p1 = corner + u;
            let p2 = corner + u + v;
            let p3 = corner + v;
            for p in [p0, p1, p2, p0, p2, p3] {
                tris.push(GizmoVertex {
                    position: p.into(),
                    color,
                });
            }
        }

        (lines, tris)
    }

    /// A cone at the tip of an axis, as a triangle fan around its base plus a
    /// cap, so it reads as solid from any angle.
    fn push_arrowhead(&self, tris: &mut Vec<GizmoVertex>, axis: usize, color: [f32; 3]) {
        const SEGMENTS: usize = 8;
        let o = self.origin();
        let dir = axis_dir(axis);
        let base_center = o + dir * (self.scale * HEAD_START);
        let tip = o + dir * self.scale;
        let radius = self.scale * HEAD_RADIUS;

        // Any two vectors perpendicular to the axis span its base circle.
        let u = axis_dir((axis + 1) % 3) * radius;
        let v = axis_dir((axis + 2) % 3) * radius;

        let ring: Vec<Point3<f32>> = (0..SEGMENTS)
            .map(|i| {
                let theta = (i as f32) * std::f32::consts::TAU / (SEGMENTS as f32);
                base_center + u * theta.cos() + v * theta.sin()
            })
            .collect();

        for i in 0..SEGMENTS {
            let a = ring[i];
            let b = ring[(i + 1) % SEGMENTS];
            // Side.
            for p in [a, b, tip] {
                tris.push(GizmoVertex {
                    position: p.into(),
                    color,
                });
            }
            // Cap, so the cone is not hollow when seen from behind.
            for p in [b, a, base_center] {
                tris.push(GizmoVertex {
                    position: p.into(),
                    color,
                });
            }
        }
    }
}

/// Transforms a world point by a matrix. Small helper so callers do not have to
/// import cgmath's `Transform` just to place a handle.
#[must_use]
pub fn transform_point(m: &Matrix4<f32>, p: Point3<f32>) -> Point3<f32> {
    m.transform_point(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, SquareMatrix};

    /// Colours are f32 arrays; clippy (rightly) forbids `==` on those.
    fn same(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    fn state(scale: f32) -> ManipulatorState {
        ManipulatorState {
            anchor: Matrix4::from_translation(Vector3::new(1.0, 2.0, 3.0)),
            tool: ManipulatorTool::Translate,
            hovered: None,
            active: None,
            scale,
        }
    }

    #[test]
    fn the_gizmo_sits_at_its_anchor() {
        let s = state(1.0);
        assert_eq!(s.origin(), Point3::new(1.0, 2.0, 3.0));

        let identity = ManipulatorState {
            anchor: Matrix4::identity(),
            ..s
        };
        assert_eq!(identity.origin(), Point3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn axis_segments_scale_with_the_camera_distance() {
        // `scale` is world-units-per-gizmo-unit, so doubling it doubles the
        // shaft: that is what keeps the gizmo the same SIZE on screen as the
        // camera pulls back.
        let near = state(1.0);
        let far = state(2.0);
        let (_, near_end) = near.axis_segment(0);
        let (_, far_end) = far.axis_segment(0);
        assert!((near_end.x - near.origin().x - 1.0).abs() < 1e-6);
        assert!((far_end.x - far.origin().x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_three_axes_run_along_the_three_world_axes() {
        let s = state(1.0);
        let o = s.origin();
        let axes: [(usize, Vector3<f32>); 3] = [
            (0, Vector3::unit_x()),
            (1, Vector3::unit_y()),
            (2, Vector3::unit_z()),
        ];
        for (axis, expected) in axes {
            let (start, end) = s.axis_segment(axis);
            let along = end - start;
            // Pointing the right way, and starting off the origin so the three
            // shafts do not smear together at the centre.
            assert!(along.x * expected.x + along.y * expected.y + along.z * expected.z > 0.0);
            assert!((start - o).magnitude() > 0.0);
        }
    }

    #[test]
    fn a_plane_handle_spans_its_two_axes_and_sits_off_the_origin() {
        let s = state(1.0);
        let (corner, u, v) = s.plane_quad(0, 1);
        // Offset out along both axes, so it does not sit under the shafts.
        assert!(corner.x > s.origin().x);
        assert!(corner.y > s.origin().y);
        assert!((corner.z - s.origin().z).abs() < 1e-6);
        // Spanned by X and Y, so its normal is Z.
        assert!(u.x > 0.0 && u.y.abs() < 1e-6);
        assert!(v.y > 0.0 && v.x.abs() < 1e-6);
    }

    #[test]
    fn hover_and_grab_light_the_handle_up() {
        let mut s = state(1.0);
        assert!(same(s.color_for(Handle::AxisX), X_COLOR));

        s.hovered = Some(Handle::AxisX);
        assert!(same(s.color_for(Handle::AxisX), HILIGHT));
        assert!(
            same(s.color_for(Handle::AxisY), Y_COLOR),
            "only the hovered one"
        );

        // While dragging, the grabbed handle wins and hover is ignored: the user
        // is committed, and a stray hover highlight would be a lie.
        s.active = Some(Handle::AxisZ);
        assert!(same(s.color_for(Handle::AxisZ), HILIGHT));
        assert!(same(s.color_for(Handle::AxisX), X_COLOR));
    }

    #[test]
    fn build_vertices_emits_shafts_and_solid_heads() {
        let (lines, tris) = state(1.0).build_vertices();
        assert_eq!(lines.len(), 6, "three shafts, two endpoints each");
        // Three cones (8 segments x 2 tris x 3 verts) plus three quads (2 tris).
        assert_eq!(tris.len(), 3 * 8 * 2 * 3 + 3 * 2 * 3);
    }

    #[test]
    fn a_plane_handle_takes_the_colour_of_the_axis_it_faces() {
        // Blender/Maya convention: the YZ plane reads as "the X plane".
        let s = state(1.0);
        assert!(same(s.color_for(Handle::PlaneYZ), X_COLOR));
        assert!(same(s.color_for(Handle::PlaneZX), Y_COLOR));
        assert!(same(s.color_for(Handle::PlaneXY), Z_COLOR));
    }
}
