//! Axis-aligned bounding box (`AABB`) primitives used by the renderer
//! (camera framing, bounds overlay) and the analyzer (mesh extents).

use cgmath::{InnerSpace, Point3, Vector3};

/// Axis-aligned bounding box with `f32` extents.
///
/// Used for camera auto-framing, bounds-overlay drawing, and shadow-frustum
/// sizing. [`AABB::diagonal`] is the canonical "model size" scalar that
/// drives auto-framing distance.
#[derive(Debug, Clone, Copy)]
pub struct AABB {
    pub min: Point3<f32>,
    pub max: Point3<f32>,
}

impl AABB {
    /// Geometric center: midpoint of `min` and `max`.
    pub fn center(&self) -> Point3<f32> {
        Point3::new(
            f32::midpoint(self.min.x, self.max.x),
            f32::midpoint(self.min.y, self.max.y),
            f32::midpoint(self.min.z, self.max.z),
        )
    }

    /// Length of the box's space diagonal — the canonical "model size" scalar.
    pub fn diagonal(&self) -> f32 {
        (self.max - self.min).magnitude()
    }

    /// Half-extents along each axis (= `size() / 2`).
    pub fn half_extents(&self) -> Vector3<f32> {
        Vector3::new(
            (self.max.x - self.min.x) * 0.5,
            (self.max.y - self.min.y) * 0.5,
            (self.max.z - self.min.z) * 0.5,
        )
    }

    /// Per-axis extent (max − min).
    pub fn size(&self) -> Vector3<f32> {
        Vector3::new(
            self.max.x - self.min.x,
            self.max.y - self.min.y,
            self.max.z - self.min.z,
        )
    }

    /// The smallest box containing both `self` and `other`.
    ///
    /// Exists for viewports fed by two independent sources of geometry - the
    /// desktop shell draws a file-loaded model beside its cooked scene
    /// objects - where camera framing, the depth-range fit and the shadow
    /// frustum all have to cover everything on screen rather than one half
    /// of it.
    pub fn union(&self, other: &AABB) -> AABB {
        AABB {
            min: Point3::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Point3::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }

    /// Eight corners ordered as `(min, max)` combinations along x, then y,
    /// then z (z slowest-varying). `corners()[0] == min` and `corners()[7] == max`.
    pub fn corners(&self) -> [Point3<f32>; 8] {
        let (mn, mx) = (self.min, self.max);
        [
            Point3::new(mn.x, mn.y, mn.z),
            Point3::new(mx.x, mn.y, mn.z),
            Point3::new(mn.x, mx.y, mn.z),
            Point3::new(mx.x, mx.y, mn.z),
            Point3::new(mn.x, mn.y, mx.z),
            Point3::new(mx.x, mn.y, mx.z),
            Point3::new(mn.x, mx.y, mx.z),
            Point3::new(mx.x, mx.y, mx.z),
        ]
    }

    /// The smallest axis-aligned box containing this one after `transform`.
    ///
    /// Every corner is carried through and re-bounded, so rotation and
    /// non-uniform scale are handled rather than translation alone. Under
    /// rotation the result is looser than the exact bounds of the rotated
    /// geometry, which is the same approximation every other bounds consumer
    /// here already accepts: the callers are camera framing, the depth
    /// range's fit and the shadow frustum's extent, and all three want a box
    /// that certainly covers the object rather than the tightest one.
    ///
    /// The perspective row is ignored. These are object placements, which are
    /// affine by construction.
    #[must_use]
    pub fn transformed(&self, transform: &cgmath::Matrix4<f32>) -> AABB {
        let mut corners = self
            .corners()
            .into_iter()
            .map(|c| Point3::from_homogeneous(transform * c.to_homogeneous()));
        // `corners()` yields eight, so the first is always present and the
        // accumulator needs no sentinel infinities.
        let first = corners.next().unwrap_or(self.min);
        let mut acc = AABB {
            min: first,
            max: first,
        };
        for c in corners {
            acc.min = Point3::new(acc.min.x.min(c.x), acc.min.y.min(c.y), acc.min.z.min(c.z));
            acc.max = Point3::new(acc.max.x.max(c.x), acc.max.y.max(c.y), acc.max.z.max(c.z));
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use cgmath::SquareMatrix;

    use super::*;

    fn unit_cube() -> AABB {
        AABB {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(1.0, 1.0, 1.0),
        }
    }

    #[test]
    fn size_and_derived_metrics() {
        let aabb = AABB {
            min: Point3::new(-2.0, 0.0, 3.0),
            max: Point3::new(4.0, 6.0, 9.0),
        };
        let s = aabb.size();
        assert!((s.x - 6.0).abs() < f32::EPSILON);
        assert!((s.y - 6.0).abs() < f32::EPSILON);
        assert!((s.z - 6.0).abs() < f32::EPSILON);

        let he = aabb.half_extents();
        assert!((he.x - 3.0).abs() < f32::EPSILON);
        assert!((he.y - 3.0).abs() < f32::EPSILON);

        let c = aabb.center();
        assert!((c.x - 1.0).abs() < f32::EPSILON);
        assert!((c.y - 3.0).abs() < f32::EPSILON);
        assert!((c.z - 6.0).abs() < f32::EPSILON);

        let d = aabb.diagonal();
        let expected = (6.0_f32 * 6.0 + 6.0 * 6.0 + 6.0 * 6.0).sqrt();
        assert!((d - expected).abs() < 1e-6);
    }

    #[test]
    fn zero_volume_aabb() {
        let aabb = AABB {
            min: Point3::new(3.0, 4.0, 5.0),
            max: Point3::new(3.0, 4.0, 5.0),
        };
        assert!((aabb.diagonal()).abs() < f32::EPSILON);
        assert!((aabb.size().x).abs() < f32::EPSILON);
        assert_eq!(aabb.center(), Point3::new(3.0, 4.0, 5.0));
    }

    #[test]
    fn corners_ordering() {
        let c = unit_cube().corners();
        assert_eq!(c[0], Point3::new(0.0, 0.0, 0.0));
        assert_eq!(c[1], Point3::new(1.0, 0.0, 0.0));
        assert_eq!(c[2], Point3::new(0.0, 1.0, 0.0));
        assert_eq!(c[3], Point3::new(1.0, 1.0, 0.0));
        assert_eq!(c[4], Point3::new(0.0, 0.0, 1.0));
        assert_eq!(c[5], Point3::new(1.0, 0.0, 1.0));
        assert_eq!(c[6], Point3::new(0.0, 1.0, 1.0));
        assert_eq!(c[7], Point3::new(1.0, 1.0, 1.0));

        let aabb = AABB {
            min: Point3::new(-3.0, -2.0, -1.0),
            max: Point3::new(4.0, 5.0, 6.0),
        };
        let c2 = aabb.corners();
        assert_eq!(c2[0], aabb.min);
        assert_eq!(c2[7], aabb.max);
    }

    #[test]
    fn transformed_by_identity_is_unchanged() {
        let a = AABB {
            min: Point3::new(-3.0, -2.0, -1.0),
            max: Point3::new(4.0, 5.0, 6.0),
        };
        let t = a.transformed(&cgmath::Matrix4::identity());
        assert!((t.min - a.min).magnitude() < 1e-6);
        assert!((t.max - a.max).magnitude() < 1e-6);
    }

    #[test]
    fn transformed_carries_translation() {
        // The case that made this necessary: a placed object used to report
        // the box it would occupy at the origin, so framing, the depth fit
        // and the shadow frustum all pointed at empty space.
        let moved = unit_cube().transformed(&cgmath::Matrix4::from_translation(Vector3::new(
            10.0, 0.0, -5.0,
        )));
        assert!((moved.min - Point3::new(10.0, 0.0, -5.0)).magnitude() < 1e-6);
        assert!((moved.max - Point3::new(11.0, 1.0, -4.0)).magnitude() < 1e-6);
    }

    #[test]
    fn transformed_handles_non_uniform_scale() {
        let scaled =
            unit_cube().transformed(&cgmath::Matrix4::from_nonuniform_scale(2.0, 3.0, 4.0));
        assert!((scaled.min - Point3::new(0.0, 0.0, 0.0)).magnitude() < 1e-6);
        assert!((scaled.max - Point3::new(2.0, 3.0, 4.0)).magnitude() < 1e-6);
    }

    #[test]
    fn transformed_rotation_stays_axis_aligned_and_covering() {
        // A 45-degree turn about Y widens the axis-aligned box to the
        // diagonal. Looser than the exact bounds of the rotated geometry,
        // which is the documented tradeoff; what must hold is that it still
        // covers every corner.
        let cube = AABB {
            min: Point3::new(-1.0, -1.0, -1.0),
            max: Point3::new(1.0, 1.0, 1.0),
        };
        let m = cgmath::Matrix4::from_angle_y(cgmath::Deg(45.0));
        let r = cube.transformed(&m);
        let half = 2.0_f32.sqrt();
        assert!((r.max.x - half).abs() < 1e-5, "got {}", r.max.x);
        assert!(
            (r.max.y - 1.0).abs() < 1e-5,
            "y is untouched by a Y rotation"
        );
        for corner in cube.corners() {
            let p = Point3::from_homogeneous(m * corner.to_homogeneous());
            assert!(p.x >= r.min.x - 1e-5 && p.x <= r.max.x + 1e-5);
            assert!(p.y >= r.min.y - 1e-5 && p.y <= r.max.y + 1e-5);
            assert!(p.z >= r.min.z - 1e-5 && p.z <= r.max.z + 1e-5);
        }
    }

    #[test]
    fn union_covers_both_boxes() {
        let a = AABB {
            min: Point3::new(-1.0, 0.0, 0.0),
            max: Point3::new(1.0, 2.0, 1.0),
        };
        let b = AABB {
            min: Point3::new(0.0, -3.0, 5.0),
            max: Point3::new(4.0, 1.0, 6.0),
        };
        let u = a.union(&b);
        assert_eq!(u.min, Point3::new(-1.0, -3.0, 0.0));
        assert_eq!(u.max, Point3::new(4.0, 2.0, 6.0));

        // Order-independent, and unioning a box with itself is the identity:
        // the property the single-source case relies on to stay pixel-exact.
        let flipped = b.union(&a);
        assert_eq!(flipped.min, u.min);
        assert_eq!(flipped.max, u.max);
        let self_union = a.union(&a);
        assert_eq!(self_union.min, a.min);
        assert_eq!(self_union.max, a.max);
    }
}
