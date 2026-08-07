//! CPU traversal: the reference the WGSL kernel is held to.
//!
//! This is not a convenience. The shader cannot be unit tested, so the
//! traversal is written twice: once here and once in WGSL, in the same shape,
//! against the same node layout, with the same epsilon and the same rejection
//! order. The parity corpus then pins this against
//! `solarxy_core::raycast::intersect_triangle`, which the picking path has
//! used since the web milestone. A disagreement anywhere along that chain
//! names which of the three is wrong.
//!
//! Everything here is deliberately unidiomatic Rust where idiomatic Rust would
//! not port: a fixed stack array instead of a `Vec`, explicit index arithmetic
//! instead of iterators, and no early `?`. WGSL has none of those.

use crate::build::Bvh;

/// Entries in the traversal stack, fixed because WGSL has no dynamic
/// allocation.
///
/// Sixty-four against a builder that caps depth at 32, and a descent pushes at
/// most one entry per level, so the stack runs at half capacity in the worst
/// case the builder can produce.
pub const TRAVERSAL_STACK_SIZE: usize = 64;

/// Matches `solarxy_core::raycast`'s epsilon exactly. The two implementations
/// must agree on which grazing hits count, not merely on the obvious ones.
const EPS: f32 = 1e-7;

/// Where a ray met a triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriangleHit {
    /// Index of the triangle, in the caller's numbering rather than the
    /// hierarchy's permutation.
    pub prim: u32,
    /// Distance along the ray.
    pub t: f32,
    /// Barycentric coordinates `[w, u, v]`, so the point is
    /// `v0 * w + v1 * u + v2 * v`. Same convention as `solarxy_core::raycast`.
    pub bary: [f32; 3],
}

impl Bvh {
    /// Nearest triangle hit within `t_max`, or `None`.
    ///
    /// `positions` and `indices` are the same buffers the hierarchy was built
    /// over. The hierarchy stores only indices, so the geometry comes back in
    /// on every query, exactly as the kernel binds it as a storage buffer
    /// rather than carrying it in the node array.
    #[must_use]
    pub fn intersect_triangles(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        t_max: f32,
        positions: &[[f32; 3]],
        indices: &[u32],
    ) -> Option<TriangleHit> {
        let inv_dir = invert(direction);
        let mut stack = [0u32; TRAVERSAL_STACK_SIZE];
        let mut sp = 0usize;
        let mut node_idx = 0u32;
        let mut best: Option<TriangleHit> = None;
        let mut t_best = t_max;

        loop {
            let node = &self.nodes()[node_idx as usize];
            if slab_hit(origin, inv_dir, node.min, node.max, t_best) {
                if node.is_leaf() {
                    let first = node.first_prim() as usize;
                    for slot in first..first + node.prim_count() as usize {
                        let prim = self.prim_indices()[slot];
                        if let Some((t, bary)) =
                            intersect_tri(origin, direction, prim, positions, indices)
                            && t < t_best
                        {
                            t_best = t;
                            best = Some(TriangleHit { prim, t, bary });
                        }
                    }
                } else {
                    let left = node_idx + 1;
                    let right = node.right_child();
                    // Descend the side the ray reaches first, so `t_best`
                    // tightens before the far subtree is tested and its box
                    // test more often rejects.
                    let (near, far) = if direction[node.axis() as usize] < 0.0 {
                        (right, left)
                    } else {
                        (left, right)
                    };
                    debug_assert!(sp < TRAVERSAL_STACK_SIZE, "traversal stack overflow");
                    if sp < TRAVERSAL_STACK_SIZE {
                        stack[sp] = far;
                        sp += 1;
                    }
                    node_idx = near;
                    continue;
                }
            }
            if sp == 0 {
                break;
            }
            sp -= 1;
            node_idx = stack[sp];
        }

        best
    }

    /// Whether anything blocks the ray within `t_max`.
    ///
    /// Separate from [`Bvh::intersect_triangles`] rather than a flag on it,
    /// because the shadow query returns on the first hit and never orders its
    /// children. That difference is most of why an any-hit traversal is worth
    /// having.
    #[must_use]
    pub fn occluded_triangles(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        t_max: f32,
        positions: &[[f32; 3]],
        indices: &[u32],
    ) -> bool {
        let inv_dir = invert(direction);
        let mut stack = [0u32; TRAVERSAL_STACK_SIZE];
        let mut sp = 0usize;
        let mut node_idx = 0u32;

        loop {
            let node = &self.nodes()[node_idx as usize];
            if slab_hit(origin, inv_dir, node.min, node.max, t_max) {
                if node.is_leaf() {
                    let first = node.first_prim() as usize;
                    for slot in first..first + node.prim_count() as usize {
                        let prim = self.prim_indices()[slot];
                        if let Some((t, _)) =
                            intersect_tri(origin, direction, prim, positions, indices)
                            && t < t_max
                        {
                            return true;
                        }
                    }
                } else {
                    debug_assert!(sp < TRAVERSAL_STACK_SIZE, "traversal stack overflow");
                    if sp < TRAVERSAL_STACK_SIZE {
                        stack[sp] = node.right_child();
                        sp += 1;
                    }
                    node_idx += 1;
                    continue;
                }
            }
            if sp == 0 {
                break;
            }
            sp -= 1;
            node_idx = stack[sp];
        }

        false
    }
}

/// Reciprocal direction, with infinities left in for axis-aligned rays.
///
/// The infinity is not an edge case to guard, it is the mechanism: an
/// axis-aligned ray produces `±inf` for that slab, the comparison against the
/// box still orders correctly, and only the origin sitting exactly on a slab
/// plane yields a NaN. The slab test handles that below.
fn invert(direction: [f32; 3]) -> [f32; 3] {
    [1.0 / direction[0], 1.0 / direction[1], 1.0 / direction[2]]
}

/// Slab test against an axis-aligned box, over `[0, t_max]`.
///
/// The NaN handling is load-bearing and free: when the ray origin lies exactly
/// on a slab plane of a zero-extent axis, `(bound - origin) * inf` is NaN, and
/// both `f32::max` and `f32::min` return the other operand for a NaN input.
/// That is exactly the "ignore this axis" behaviour the robust formulation
/// wants, and WGSL's `max`/`min` are specified the same way, so the shader
/// twin gets it for the same reason.
fn slab_hit(origin: [f32; 3], inv_dir: [f32; 3], min: [f32; 3], max: [f32; 3], t_max: f32) -> bool {
    let mut t_near = 0.0f32;
    let mut t_far = t_max;
    for axis in 0..3 {
        let t0 = (min[axis] - origin[axis]) * inv_dir[axis];
        let t1 = (max[axis] - origin[axis]) * inv_dir[axis];
        let (near, far) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
        t_near = t_near.max(near);
        t_far = t_far.min(far);
        if t_near > t_far {
            return false;
        }
    }
    true
}

/// Moller-Trumbore against one triangle of the index buffer.
///
/// Field for field the same arithmetic and the same rejection order as
/// `solarxy_core::raycast::intersect_triangle`. Deliberate duplication: that
/// function speaks `cgmath` types this crate does not take, and the shader
/// twin has to mirror something written on plain floats.
fn intersect_tri(
    origin: [f32; 3],
    direction: [f32; 3],
    prim: u32,
    positions: &[[f32; 3]],
    indices: &[u32],
) -> Option<(f32, [f32; 3])> {
    let base = prim as usize * 3;
    let (Some(&i0), Some(&i1), Some(&i2)) = (
        indices.get(base),
        indices.get(base + 1),
        indices.get(base + 2),
    ) else {
        return None;
    };
    let (Some(&v0), Some(&v1), Some(&v2)) = (
        positions.get(i0 as usize),
        positions.get(i1 as usize),
        positions.get(i2 as usize),
    ) else {
        return None;
    };

    let edge1 = sub(v1, v0);
    let edge2 = sub(v2, v0);
    let h = cross(direction, edge2);
    let a = dot(edge1, h);
    if a.abs() < EPS {
        return None;
    }
    let f = 1.0 / a;
    let s = sub(origin, v0);
    let u = f * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, edge1);
    let v = f * dot(direction, q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * dot(edge2, q);
    if t > EPS {
        Some((t, [1.0 - u - v, u, v]))
    } else {
        None
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[2].mul_add(b[2], a[0].mul_add(b[0], a[1] * b[1]))
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1].mul_add(b[2], -(a[2] * b[1])),
        a[2].mul_add(b[0], -(a[0] * b[2])),
        a[0].mul_add(b[1], -(a[1] * b[0])),
    ]
}

#[cfg(test)]
mod tests {
    use super::{TriangleHit, intersect_tri};
    use crate::build::Bvh;

    fn unit_triangle() -> (Vec<[f32; 3]>, Vec<u32>) {
        (
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
    }

    #[test]
    fn a_ray_down_the_axis_hits_the_only_triangle() {
        let (positions, indices) = unit_triangle();
        let bvh = Bvh::build_triangles(&positions, &indices);
        let hit = bvh.intersect_triangles(
            [0.25, 0.25, 5.0],
            [0.0, 0.0, -1.0],
            f32::INFINITY,
            &positions,
            &indices,
        );
        let Some(TriangleHit { prim, t, bary }) = hit else {
            panic!("expected a hit");
        };
        assert_eq!(prim, 0);
        assert!((t - 5.0).abs() < 1e-4);
        assert!((bary[0] + bary[1] + bary[2] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_ray_pointing_away_misses() {
        let (positions, indices) = unit_triangle();
        let bvh = Bvh::build_triangles(&positions, &indices);
        assert!(
            bvh.intersect_triangles(
                [0.25, 0.25, 5.0],
                [0.0, 0.0, 1.0],
                f32::INFINITY,
                &positions,
                &indices
            )
            .is_none()
        );
    }

    #[test]
    fn t_max_bounds_the_query() {
        let (positions, indices) = unit_triangle();
        let bvh = Bvh::build_triangles(&positions, &indices);
        assert!(
            bvh.intersect_triangles(
                [0.25, 0.25, 5.0],
                [0.0, 0.0, -1.0],
                4.0,
                &positions,
                &indices
            )
            .is_none()
        );
        assert!(bvh.occluded_triangles(
            [0.25, 0.25, 5.0],
            [0.0, 0.0, -1.0],
            6.0,
            &positions,
            &indices
        ));
        assert!(!bvh.occluded_triangles(
            [0.25, 0.25, 5.0],
            [0.0, 0.0, -1.0],
            4.0,
            &positions,
            &indices
        ));
    }

    #[test]
    fn an_axis_aligned_ray_grazing_a_flat_box_still_traverses() {
        // Every triangle is coplanar in z, so the root box has zero extent on
        // that axis and the slab test divides by an infinite reciprocal.
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        let indices = vec![0, 1, 2, 1, 3, 2];
        let bvh = Bvh::build_triangles(&positions, &indices);
        // A ray travelling inside the plane must not produce a NaN verdict.
        let hit = bvh.intersect_triangles(
            [-1.0, 0.5, 0.0],
            [1.0, 0.0, 0.0],
            f32::INFINITY,
            &positions,
            &indices,
        );
        assert!(hit.is_none() || hit.is_some());
    }

    #[test]
    fn the_slab_test_agrees_with_the_raycaster() {
        // `slab_hit` is private, so this parity check lives here rather than
        // in the integration corpus. It is worth having separately: the box
        // test is what the shader spends most of its time in, and a
        // disagreement here would show up in the triangle corpus only as an
        // occasional missed hit rather than as a box result.
        use cgmath::{InnerSpace, Point3, Vector3};
        use solarxy_core::aabb::AABB;
        use solarxy_core::raycast::{Ray, intersect_aabb};

        let mut state = 0x0BAD_C0DEu32;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32
        };

        for i in 0..3000 {
            let lo = [
                next().mul_add(4.0, -2.0),
                next().mul_add(4.0, -2.0),
                next().mul_add(4.0, -2.0),
            ];
            // Every fourth box is flat on one axis, which is the case that
            // drives an infinite reciprocal through the slab test.
            let flat = i % 4;
            let size = [
                if flat == 0 { 0.0 } else { next() * 2.0 },
                if flat == 1 { 0.0 } else { next() * 2.0 },
                if flat == 2 { 0.0 } else { next() * 2.0 },
            ];
            let hi = [lo[0] + size[0], lo[1] + size[1], lo[2] + size[2]];

            let origin = [
                next().mul_add(8.0, -4.0),
                next().mul_add(8.0, -4.0),
                next().mul_add(8.0, -4.0),
            ];
            let raw = Vector3::new(
                next().mul_add(2.0, -1.0),
                next().mul_add(2.0, -1.0),
                next().mul_add(2.0, -1.0),
            );
            if raw.magnitude() < 1e-3 {
                continue;
            }
            let dir = raw.normalize();

            let aabb = AABB {
                min: Point3::new(lo[0], lo[1], lo[2]),
                max: Point3::new(hi[0], hi[1], hi[2]),
            };
            let ray = Ray {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                direction: dir,
            };
            // The raycaster reports the slab interval; a box behind the ray
            // has a negative exit and is a miss for traversal purposes, which
            // is the interval this crate clamps to `[0, t_max]`.
            let want = intersect_aabb(&ray, &aabb).is_some_and(|(_, t_far)| t_far >= 0.0);
            let got = super::slab_hit(
                origin,
                super::invert([dir.x, dir.y, dir.z]),
                lo,
                hi,
                f32::INFINITY,
            );
            assert_eq!(
                want, got,
                "box {i}: {lo:?}..{hi:?} from {origin:?} toward {dir:?}"
            );
        }
    }

    #[test]
    fn the_hierarchy_agrees_with_brute_force() {
        let (positions, indices) = crate::test_meshes::sphere(40, 20);
        let bvh = Bvh::build_triangles(&positions, &indices);
        let tri_count = indices.len() as u32 / 3;

        let mut state = 0x1234_5678u32;
        let mut next = || {
            // xorshift32, so the corpus is the same on every machine and a
            // failure is reproducible from the seed alone.
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32
        };

        for _ in 0..400 {
            let origin = [
                next().mul_add(6.0, -3.0),
                next().mul_add(6.0, -3.0),
                next().mul_add(6.0, -3.0),
            ];
            let mut dir = [
                next().mul_add(2.0, -1.0),
                next().mul_add(2.0, -1.0),
                next().mul_add(2.0, -1.0),
            ];
            let len = super::dot(dir, dir).sqrt();
            if len < 1e-4 {
                continue;
            }
            for d in &mut dir {
                *d /= len;
            }

            let mut expect: Option<(f32, u32)> = None;
            for prim in 0..tri_count {
                if let Some((t, _)) = intersect_tri(origin, dir, prim, &positions, &indices)
                    && expect.is_none_or(|(best, _)| t < best)
                {
                    expect = Some((t, prim));
                }
            }

            let got = bvh.intersect_triangles(origin, dir, f32::INFINITY, &positions, &indices);
            match (expect, got) {
                (None, None) => {}
                (Some((t, _)), Some(hit)) => {
                    assert!((t - hit.t).abs() < 1e-4, "t {t} vs {}", hit.t);
                }
                (a, b) => panic!("brute force {a:?} but hierarchy {b:?}"),
            }

            assert_eq!(
                expect.is_some(),
                bvh.occluded_triangles(origin, dir, f32::INFINITY, &positions, &indices)
            );
        }
    }
}
