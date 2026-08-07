//! The builder's own axis-aligned box.
//!
//! `solarxy_core::aabb::AABB` is the workspace's bounding box and this crate
//! converts to and from it at its public edges. It is not used *inside* the
//! builder for one reason: its fields are `cgmath::Point3`, and every line of
//! this crate is written on `[f32; 3]` so it ports to WGSL unchanged. A
//! builder that reached for a math crate would produce traversal code the
//! shader could not mirror term for term, which is the whole point of keeping
//! a CPU twin.

use solarxy_core::aabb::AABB;

/// An axis-aligned box, built up by union.
///
/// The empty box is `min = +inf`, `max = -inf`, so unioning anything into it
/// yields that thing. [`Bounds::surface_area`] reports zero for it rather than
/// a negative or NaN area, which is what lets the SAH sweep evaluate a bin
/// nothing landed in without a special case.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Default for Bounds {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Bounds {
    /// The identity for [`Bounds::union`].
    pub const EMPTY: Self = Self {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };

    /// A degenerate box at the origin.
    ///
    /// Used for the single node an empty hierarchy still emits. Infinities
    /// would work for traversal (nothing intersects an inverted box) but they
    /// reach the GPU as buffer contents, and a zero box reads as obviously
    /// empty in a capture where `+inf` reads as a bug.
    pub const ZERO: Self = Self {
        min: [0.0; 3],
        max: [0.0; 3],
    };

    /// The tightest box containing one point.
    #[must_use]
    pub fn from_point(p: [f32; 3]) -> Self {
        Self { min: p, max: p }
    }

    /// Whether no point has been added yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1] || self.min[2] > self.max[2]
    }

    /// Grow to contain `p`.
    pub fn expand(&mut self, p: [f32; 3]) {
        for axis in 0..3 {
            self.min[axis] = self.min[axis].min(p[axis]);
            self.max[axis] = self.max[axis].max(p[axis]);
        }
    }

    /// Grow to contain `other`.
    pub fn union(&mut self, other: &Bounds) {
        for axis in 0..3 {
            self.min[axis] = self.min[axis].min(other.min[axis]);
            self.max[axis] = self.max[axis].max(other.max[axis]);
        }
    }

    /// Per-axis extent, clamped at zero so an empty box reports no size
    /// rather than a negative one.
    #[must_use]
    pub fn extent(&self) -> [f32; 3] {
        [
            (self.max[0] - self.min[0]).max(0.0),
            (self.max[1] - self.min[1]).max(0.0),
            (self.max[2] - self.min[2]).max(0.0),
        ]
    }

    /// Midpoint. Meaningless for an empty box; callers only ask for it after
    /// adding at least one point.
    #[must_use]
    pub fn centre(&self) -> [f32; 3] {
        [
            f32::midpoint(self.min[0], self.max[0]),
            f32::midpoint(self.min[1], self.max[1]),
            f32::midpoint(self.min[2], self.max[2]),
        ]
    }

    /// Total surface area, the SAH's measure of how likely a ray is to enter
    /// this box. Zero for an empty box.
    #[must_use]
    pub fn surface_area(&self) -> f32 {
        let e = self.extent();
        2.0 * e[2].mul_add(e[0], e[0].mul_add(e[1], e[1] * e[2]))
    }

    /// The axis with the largest extent, as an index into `[x, y, z]`.
    #[must_use]
    pub fn largest_axis(&self) -> usize {
        let e = self.extent();
        if e[0] >= e[1] && e[0] >= e[2] {
            0
        } else if e[1] >= e[2] {
            1
        } else {
            2
        }
    }
}

impl From<&AABB> for Bounds {
    fn from(a: &AABB) -> Self {
        Self {
            min: [a.min.x, a.min.y, a.min.z],
            max: [a.max.x, a.max.y, a.max.z],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Bounds;

    #[test]
    fn empty_has_no_area_and_no_extent() {
        let b = Bounds::EMPTY;
        assert!(b.is_empty());
        assert!(b.extent().iter().all(|e| e.abs() < f32::EPSILON));
        assert!(b.surface_area().abs() < f32::EPSILON);
    }

    #[test]
    fn union_with_empty_is_identity() {
        let unit = Bounds {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 2.0, 3.0],
        };
        let mut acc = Bounds::EMPTY;
        acc.union(&unit);
        assert_eq!(acc, unit);
    }

    #[test]
    fn surface_area_matches_the_box_formula() {
        let b = Bounds {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 2.0, 3.0],
        };
        // 2 * (1*2 + 2*3 + 3*1)
        assert!((b.surface_area() - 22.0).abs() < 1e-6);
    }

    #[test]
    fn largest_axis_picks_the_longest_side() {
        let b = Bounds {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 5.0, 3.0],
        };
        assert_eq!(b.largest_axis(), 1);
    }
}
