//! CPU raycaster — picks faces of CPU-side mesh data for review-mode click
//! anchoring, viewport picking, and any future 3D ↔ UV selection sync.
//!
//! Pure math, no GPU. Möller-Trumbore for triangle-ray intersection, slab
//! method for the AABB early-reject. Callers build [`MeshView`] slices over
//! their CPU mesh copies and pass them in.
//!
//! Lives in `solarxy-core` (moved from `solarxy-app` in the web milestone's
//! because web picking runs in Rust — cooked geometry never
//! crosses into JavaScript, so `engine.pick()` needs this crate-neutral.
//!
//! Performance budget: < 5ms for ~100K-triangle scenes on Apple Silicon
//! / equivalent class hardware (asserted by `tests::dragon_perf_budget`
//! when the `xyzrgb_dragon.obj` fixture is present).

use cgmath::{InnerSpace, Matrix4, Point3, SquareMatrix, Vector3, Vector4};

use crate::aabb::AABB;

/// A picking ray in world space.
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    /// Ray origin (usually the camera eye for click-picking).
    pub origin: Point3<f32>,
    /// Unit-length ray direction.
    pub direction: Vector3<f32>,
}

/// Result of a successful pick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaycastHit {
    /// Index into the slice of [`MeshView`] passed to [`raycast_meshes`].
    pub mesh_index: u32,
    /// Triangle index within that mesh (each tri = 3 consecutive indices).
    pub face_index: u32,
    /// Barycentric coordinates `[w, u, v]` where the triangle is
    /// `v0*w + v1*u + v2*v` and `w + u + v ≈ 1`. Matches the standard
    /// Möller-Trumbore convention.
    pub barycentric: [f32; 3],
    /// World-space point of intersection.
    pub world_pos: Point3<f32>,
    /// Distance along the ray (`t` in `origin + direction * t`).
    pub distance: f32,
}

/// CPU-side view over one mesh's geometry for raycasting purposes.
///
/// Borrowed positions + index buffer + pre-computed AABB. Construction
/// is the responsibility of the caller — the renderer keeps GPU-only
/// `Mesh` structs, so a parallel CPU mirror is needed (added when
/// wiring review mode into `ModelScene` / `State`).
#[derive(Debug, Clone, Copy)]
pub struct MeshView<'a> {
    pub positions: &'a [[f32; 3]],
    pub indices: &'a [u32],
    pub bounds: AABB,
}

/// Construct a world-space picking ray from pixel coordinates within a
/// viewport rect.
///
/// - `cursor_px` — pixel position relative to the viewport's top-left
///   corner. (For multi-pane layouts, subtract the pane origin before
///   passing in.)
/// - `viewport_size_px` — width / height of the viewport rect.
/// - `view_proj` — the camera's `clip = view_proj * world` matrix; the
///   same one written to the GPU camera uniform (wgpu clip convention,
///   near plane at NDC z = 0).
///
/// The ray originates on the NEAR plane under the pixel and aims at the
/// far-plane point under the same pixel. Unprojecting both endpoints is
/// what makes this exact for orthographic cameras too: their rays are
/// parallel with the origin varying per pixel, which an eye-anchored ray
/// cannot express (every pixel would collapse onto the view axis — the
/// bug that froze gizmo drags in axis views). Consequence: a hit's
/// `distance` is measured from the near plane, not the eye; the shift is
/// uniform along any one ray, so nearest-hit ordering is unaffected.
pub fn screen_to_world_ray(
    cursor_px: (f32, f32),
    viewport_size_px: (f32, f32),
    view_proj: Matrix4<f32>,
) -> Ray {
    let inv = view_proj.invert().unwrap_or_else(Matrix4::identity);
    let ndc_x = (cursor_px.0 / viewport_size_px.0) * 2.0 - 1.0;
    let ndc_y = 1.0 - (cursor_px.1 / viewport_size_px.1) * 2.0;
    let unproject = |ndc_z: f32| {
        let h = inv * Vector4::new(ndc_x, ndc_y, ndc_z, 1.0);
        Point3::new(h.x / h.w, h.y / h.w, h.z / h.w)
    };
    let near_world = unproject(0.0);
    let far_world = unproject(1.0);

    Ray {
        origin: near_world,
        direction: (far_world - near_world).normalize(),
    }
}

/// Slab-method AABB intersection. Returns `(t_min, t_max)` where the ray
/// enters and exits the box (clamped so `t_min >= 0`), or `None` if the
/// ray misses or hits only behind the origin.
pub fn intersect_aabb(ray: &Ray, aabb: &AABB) -> Option<(f32, f32)> {
    let inv_d = Vector3::new(
        1.0 / ray.direction.x,
        1.0 / ray.direction.y,
        1.0 / ray.direction.z,
    );

    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;

    let (t1, t2) = (
        (aabb.min.x - ray.origin.x) * inv_d.x,
        (aabb.max.x - ray.origin.x) * inv_d.x,
    );
    let (lo, hi) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
    tmin = tmin.max(lo);
    tmax = tmax.min(hi);

    let (t1, t2) = (
        (aabb.min.y - ray.origin.y) * inv_d.y,
        (aabb.max.y - ray.origin.y) * inv_d.y,
    );
    let (lo, hi) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
    tmin = tmin.max(lo);
    tmax = tmax.min(hi);

    let (t1, t2) = (
        (aabb.min.z - ray.origin.z) * inv_d.z,
        (aabb.max.z - ray.origin.z) * inv_d.z,
    );
    let (lo, hi) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
    tmin = tmin.max(lo);
    tmax = tmax.min(hi);

    if tmax >= tmin.max(0.0) {
        Some((tmin.max(0.0), tmax))
    } else {
        None
    }
}

/// Closest approach between a ray and a finite segment.
///
/// Returns `(t_ray, s_seg, distance)`:
/// - `t_ray` — distance along the ray of the closest point (clamped to `>= 0`),
/// - `s_seg` — normalized position along the segment, clamped to `0..=1`,
/// - `distance` — the world-space gap between those two closest points.
///
/// This one function serves the translate gizmo twice over: it is the hit test
/// for an axis arrow (a capsule is "segment plus radius", so a hit is
/// `distance <= radius`), and it is the drag solver for that axis (the pointer
/// ray is re-parametrized against the axis line, and `s_seg` is how far along
/// the axis the pointer now sits).
///
/// Degenerate cases fall back gracefully: a zero-length segment reduces to a
/// point, and a ray parallel to the segment picks the segment start.
#[must_use]
pub fn closest_points_ray_segment(ray: &Ray, a: Point3<f32>, b: Point3<f32>) -> (f32, f32, f32) {
    let seg = b - a;
    let seg_len2 = seg.magnitude2();
    let w0 = ray.origin - a;

    // Standard segment-segment closest approach, with the ray as an
    // infinite half-line and the segment finite.
    let a_dot = ray.direction.dot(ray.direction); // 1.0 for a unit ray, but do not assume
    let b_dot = ray.direction.dot(seg);
    let c_dot = seg_len2;
    let d_dot = ray.direction.dot(w0);
    let e_dot = seg.dot(w0);

    let denom = a_dot * c_dot - b_dot * b_dot;

    // The unconstrained closest point along the segment's INFINITE line.
    let s_line = if denom.abs() < 1e-8 || seg_len2 < 1e-12 {
        // Parallel, or a degenerate segment: anchor at the segment start.
        0.0
    } else {
        (a_dot * e_dot - b_dot * d_dot) / denom
    };

    // Clamp to the real, finite segment, then re-solve the ray parameter against
    // that clamped point. Doing it in this order is what makes the returned gap
    // the distance to the CAPPED segment rather than to its infinite line, which
    // is what stops a click far out past the arrow tip from grabbing the arrow.
    let s_seg = s_line.clamp(0.0, 1.0);
    let closest_on_seg = a + seg * s_seg;
    let t_ray = (ray.direction.dot(closest_on_seg - ray.origin) / a_dot.max(1e-8)).max(0.0);

    let p_ray = ray.origin + ray.direction * t_ray;
    (t_ray, s_seg, (p_ray - closest_on_seg).magnitude())
}

/// The point on an INFINITE line closest to a ray.
///
/// The gizmo's axis drags need this rather than [`closest_points_ray_segment`]:
/// a drag must be able to run past the end of the drawn arrow, so the axis is a
/// line, not a segment. (The HIT test still uses the segment, which is what
/// stops a click far past the arrow tip from grabbing it.)
///
/// Solved analytically. Faking an infinite line by stretching a segment to some
/// huge length costs real precision: at a half-length of 1e5, an f32 carries
/// only about 7 significant digits, so the answer lands several millimetres off
/// and an axis drag visibly lags the cursor.
///
/// Returns `None` when the ray is near-parallel to the line, where the solution
/// is unbounded and the object would shoot off to infinity.
#[must_use]
pub fn closest_point_ray_line(
    ray: &Ray,
    point: Point3<f32>,
    dir: Vector3<f32>,
) -> Option<Point3<f32>> {
    let u = dir.normalize();
    let b = ray.direction.dot(u);
    // 1 - b^2 is the squared sine of the angle between them: zero when parallel.
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-6 {
        return None;
    }
    let w0 = ray.origin - point;
    let d = ray.direction.dot(w0);
    let e = u.dot(w0);
    let s = (e - b * d) / denom;
    Some(point + u * s)
}

/// Ray-plane intersection. Returns the distance along the ray, or `None` when
/// the ray is parallel to the plane or the plane sits behind the origin.
///
/// The translate gizmo's plane handles drag on exactly this: the pointer ray is
/// intersected with the handle's plane and the hit point follows the cursor.
#[must_use]
pub fn intersect_plane(ray: &Ray, point: Point3<f32>, normal: Vector3<f32>) -> Option<f32> {
    let denom = normal.dot(ray.direction);
    if denom.abs() < 1e-6 {
        return None; // parallel: no single intersection
    }
    let t = normal.dot(point - ray.origin) / denom;
    (t >= 0.0).then_some(t)
}

/// Ray-quad intersection for a planar, axis-aligned-in-its-own-basis quad
/// anchored at `origin` and spanned by `u` and `v` (each scaled to the quad's
/// full extent along that edge). Returns the distance along the ray.
///
/// This is the plane-handle hit test: the little square between two axes.
#[must_use]
pub fn intersect_quad(
    ray: &Ray,
    origin: Point3<f32>,
    u: Vector3<f32>,
    v: Vector3<f32>,
) -> Option<f32> {
    let normal = u.cross(v);
    if normal.magnitude2() < 1e-12 {
        return None; // degenerate quad
    }
    let t = intersect_plane(ray, origin, normal.normalize())?;
    let hit = ray.origin + ray.direction * t;
    let d = hit - origin;
    // Project onto each edge; inside iff both normalized coordinates are 0..=1.
    let uu = u.magnitude2();
    let vv = v.magnitude2();
    let su = d.dot(u) / uu;
    let sv = d.dot(v) / vv;
    ((0.0..=1.0).contains(&su) && (0.0..=1.0).contains(&sv)).then_some(t)
}

/// Ray-versus-ring-band: the rotate gizmo's rings.
///
/// A ring is the circle of radius `radius` about `center` lying in the plane
/// with the given `normal`; the band is that circle thickened by `tolerance`
/// (a pixel tolerance converted to world units by the caller, through the same
/// `world_per_pixel` the vertex generator uses, so the grab zone IS the drawn
/// ring).
///
/// Returns the distance along the ray to the hit point, or `None` when the ray
/// misses the band or is edge-on to the ring's plane, where the intersection is
/// ill-conditioned and a hit would be a coin flip.
#[must_use]
pub fn intersect_ring_band(
    ray: &Ray,
    center: Point3<f32>,
    normal: Vector3<f32>,
    radius: f32,
    tolerance: f32,
) -> Option<f32> {
    // Edge-on: `intersect_plane` would still solve, but the solution slides
    // wildly along the plane for a sub-pixel pointer move, so refuse it.
    if normal.normalize().dot(ray.direction).abs() < 1e-3 {
        return None;
    }
    let t = intersect_plane(ray, center, normal.normalize())?;
    let hit = ray.origin + ray.direction * t;
    let distance_from_center = (hit - center).magnitude();
    ((distance_from_center - radius).abs() <= tolerance).then_some(t)
}

/// Ray-versus-oriented-box: the scale gizmo's cube handles.
///
/// `axes` are the box's three orthonormal local axes and `half_extents` its
/// half-size along each. An OBB rather than an AABB because a locally-oriented
/// gizmo's cubes sit on the object's axes, not the world's.
///
/// Returns the distance along the ray to the near face. The classic slab test,
/// run in the box's own frame.
#[must_use]
pub fn intersect_obb(
    ray: &Ray,
    center: Point3<f32>,
    axes: [Vector3<f32>; 3],
    half_extents: [f32; 3],
) -> Option<f32> {
    let delta = center - ray.origin;
    let mut t_min = f32::NEG_INFINITY;
    let mut t_max = f32::INFINITY;

    for i in 0..3 {
        let axis = axes[i];
        let e = axis.dot(delta);
        let f = axis.dot(ray.direction);

        if f.abs() > 1e-6 {
            // Where the ray crosses this slab's two planes.
            let mut t1 = (e - half_extents[i]) / f;
            let mut t2 = (e + half_extents[i]) / f;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_min > t_max {
                return None;
            }
        } else if -e - half_extents[i] > 0.0 || -e + half_extents[i] < 0.0 {
            // Parallel to this slab AND outside it: no hit is possible.
            return None;
        }
    }

    // A ray starting inside the box hits at t = 0, not behind itself.
    if t_max < 0.0 {
        return None;
    }
    Some(if t_min < 0.0 { 0.0 } else { t_min })
}

/// Möller-Trumbore ray-triangle intersection.
///
/// Returns `Some((t, [w, u, v]))` where `t` is distance along the ray and
/// `[w, u, v]` are barycentric coords (`w + u + v = 1`,
/// `hit = v0*w + v1*u + v2*v`). Returns `None` for:
/// - ray parallel to triangle plane
/// - intersection outside the triangle
/// - intersection behind the ray origin
pub fn intersect_triangle(
    ray: &Ray,
    v0: Point3<f32>,
    v1: Point3<f32>,
    v2: Point3<f32>,
) -> Option<(f32, [f32; 3])> {
    const EPS: f32 = 1e-7;

    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = ray.direction.cross(edge2);
    let a = edge1.dot(h);
    if a.abs() < EPS {
        return None;
    }
    let f = 1.0 / a;
    let s = ray.origin - v0;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = f * ray.direction.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * edge2.dot(q);
    if t > EPS {
        let w = 1.0 - u - v;
        Some((t, [w, u, v]))
    } else {
        None
    }
}

/// Cast a ray against a collection of meshes; return the nearest hit
/// across all of them. AABB early-reject + nearest-so-far pruning.
pub fn raycast_meshes(ray: &Ray, meshes: &[MeshView<'_>]) -> Option<RaycastHit> {
    let mut best: Option<RaycastHit> = None;

    for (mi, mesh) in meshes.iter().enumerate() {
        let Some((aabb_tmin, _)) = intersect_aabb(ray, &mesh.bounds) else {
            continue;
        };
        if let Some(ref hit) = best
            && aabb_tmin > hit.distance
        {
            continue;
        }

        let num_tris = mesh.indices.len() / 3;
        for tri_idx in 0..num_tris {
            let base = tri_idx * 3;
            let i0 = mesh.indices[base] as usize;
            let i1 = mesh.indices[base + 1] as usize;
            let i2 = mesh.indices[base + 2] as usize;

            if i0 >= mesh.positions.len()
                || i1 >= mesh.positions.len()
                || i2 >= mesh.positions.len()
            {
                continue;
            }

            let p0 = Point3::from(mesh.positions[i0]);
            let p1 = Point3::from(mesh.positions[i1]);
            let p2 = Point3::from(mesh.positions[i2]);

            if let Some((t, bary)) = intersect_triangle(ray, p0, p1, p2) {
                let better = best.is_none_or(|h| t < h.distance);
                if better {
                    let world_pos = Point3::new(
                        ray.origin.x + ray.direction.x * t,
                        ray.origin.y + ray.direction.y * t,
                        ray.origin.z + ray.direction.z * t,
                    );
                    best = Some(RaycastHit {
                        mesh_index: mi as u32,
                        face_index: tri_idx as u32,
                        barycentric: bary,
                        world_pos,
                        distance: t,
                    });
                }
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, perspective};

    fn ray(origin: [f32; 3], dir: [f32; 3]) -> Ray {
        Ray {
            origin: Point3::from(origin),
            direction: Vector3::from(dir).normalize(),
        }
    }

    fn aabb(min: [f32; 3], max: [f32; 3]) -> AABB {
        AABB {
            min: Point3::from(min),
            max: Point3::from(max),
        }
    }

    // ---- gizmo primitives ----

    #[test]
    fn ray_segment_hits_an_axis_arrow_dead_on() {
        // Looking down -Z at the origin; the X axis arrow runs 0..1 along +X.
        let r = ray([0.5, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let (t, s, dist) =
            closest_points_ray_segment(&r, Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0));
        assert!(dist < 1e-5, "ray passes through the segment: {dist}");
        assert!((s - 0.5).abs() < 1e-5, "halfway along the axis: {s}");
        assert!((t - 5.0).abs() < 1e-4, "5 units down the ray: {t}");
    }

    #[test]
    fn ray_segment_reports_the_gap_when_it_misses() {
        // Same look direction, but offset 0.25 in Y: a capsule of radius 0.1
        // must NOT be hit, one of radius 0.3 must be.
        let r = ray([0.5, 0.25, 5.0], [0.0, 0.0, -1.0]);
        let (_, s, dist) =
            closest_points_ray_segment(&r, Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0));
        assert!((dist - 0.25).abs() < 1e-5, "gap is the Y offset: {dist}");
        assert!((s - 0.5).abs() < 1e-5);
        assert!(dist > 0.1 && dist < 0.3, "radius decides the hit");
    }

    #[test]
    fn ray_segment_clamps_past_the_arrow_tip() {
        // Aiming beyond the far end: the closest point is the tip (s == 1), and
        // the gap is measured to the TIP, not to the infinite axis line -- which
        // is what stops a click far out along +X from grabbing the arrow.
        let r = ray([5.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let (_, s, dist) =
            closest_points_ray_segment(&r, Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0));
        assert!((s - 1.0).abs() < 1e-6, "clamped to the tip: {s}");
        assert!(
            (dist - 4.0).abs() < 1e-4,
            "gap to the tip, not the line: {dist}"
        );
    }

    #[test]
    fn ray_segment_survives_a_degenerate_segment() {
        let r = ray([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let p = Point3::new(0.0, 0.0, 0.0);
        let (_, s, dist) = closest_points_ray_segment(&r, p, p);
        assert!(dist < 1e-5);
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn plane_intersection_and_parallel_miss() {
        let r = ray([0.0, 5.0, 0.0], [0.0, -1.0, 0.0]);
        let t = intersect_plane(&r, Point3::new(0.0, 0.0, 0.0), Vector3::unit_y());
        assert!((t.unwrap() - 5.0).abs() < 1e-5);

        // Parallel to the plane: no single intersection.
        let parallel = ray([0.0, 5.0, 0.0], [1.0, 0.0, 0.0]);
        assert!(
            intersect_plane(&parallel, Point3::new(0.0, 0.0, 0.0), Vector3::unit_y()).is_none()
        );

        // Behind the origin: not a hit.
        let behind = ray([0.0, 5.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(intersect_plane(&behind, Point3::new(0.0, 0.0, 0.0), Vector3::unit_y()).is_none());
    }

    #[test]
    fn quad_hit_inside_and_miss_outside() {
        // The XY plane handle: a unit quad at the origin spanned by +X and +Y.
        let origin = Point3::new(0.0, 0.0, 0.0);
        let u = Vector3::unit_x();
        let v = Vector3::unit_y();

        let inside = ray([0.5, 0.5, 5.0], [0.0, 0.0, -1.0]);
        assert!((intersect_quad(&inside, origin, u, v).unwrap() - 5.0).abs() < 1e-4);

        // Just past the far edge in X.
        let outside = ray([1.5, 0.5, 5.0], [0.0, 0.0, -1.0]);
        assert!(intersect_quad(&outside, origin, u, v).is_none());

        // Behind the quad's near edge.
        let negative = ray([-0.2, 0.5, 5.0], [0.0, 0.0, -1.0]);
        assert!(intersect_quad(&negative, origin, u, v).is_none());
    }

    #[test]
    fn quad_rejects_a_degenerate_span() {
        let r = ray([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let u = Vector3::unit_x();
        assert!(intersect_quad(&r, Point3::new(0.0, 0.0, 0.0), u, u).is_none());
    }

    #[test]
    fn triangle_hit_centroid() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(1.0, 0.0, 0.0);
        let v2 = Point3::new(0.0, 1.0, 0.0);
        let r = ray([1.0 / 3.0, 1.0 / 3.0, 1.0], [0.0, 0.0, -1.0]);
        let hit = intersect_triangle(&r, v0, v1, v2).expect("centroid should hit");
        assert!((hit.0 - 1.0).abs() < 1e-5, "t should be 1.0, got {}", hit.0);
        for c in hit.1 {
            assert!((c - 1.0 / 3.0).abs() < 1e-4, "bary off: {:?}", hit.1);
        }
    }

    #[test]
    fn triangle_hit_corner_carries_extreme_barycentric() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(1.0, 0.0, 0.0);
        let v2 = Point3::new(0.0, 1.0, 0.0);
        let r = ray([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]);
        let (_, bary) = intersect_triangle(&r, v0, v1, v2).expect("hit at v0");
        assert!((bary[0] - 1.0).abs() < 1e-4);
        assert!(bary[1].abs() < 1e-4);
        assert!(bary[2].abs() < 1e-4);
    }

    #[test]
    fn triangle_miss_outside() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(1.0, 0.0, 0.0);
        let v2 = Point3::new(0.0, 1.0, 0.0);
        let r = ray([5.0, 5.0, 1.0], [0.0, 0.0, -1.0]);
        assert!(intersect_triangle(&r, v0, v1, v2).is_none());
    }

    #[test]
    fn triangle_parallel_ray_misses() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(1.0, 0.0, 0.0);
        let v2 = Point3::new(0.0, 1.0, 0.0);
        let r = ray([-1.0, 0.5, 0.0], [1.0, 0.0, 0.0]);
        assert!(intersect_triangle(&r, v0, v1, v2).is_none());
    }

    #[test]
    fn triangle_degenerate_returns_none() {
        let v = Point3::new(0.5, 0.5, 0.0);
        let r = ray([0.5, 0.5, 1.0], [0.0, 0.0, -1.0]);
        assert!(intersect_triangle(&r, v, v, v).is_none());
    }

    #[test]
    fn triangle_behind_ray_origin_rejected() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(1.0, 0.0, 0.0);
        let v2 = Point3::new(0.0, 1.0, 0.0);
        let r = ray([0.3, 0.3, -1.0], [0.0, 0.0, -1.0]);
        assert!(intersect_triangle(&r, v0, v1, v2).is_none());
    }

    #[test]
    fn aabb_hit_unit_cube_from_outside() {
        let r = ray([0.0, 0.0, 2.0], [0.0, 0.0, -1.0]);
        let a = aabb([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]);
        let (tmin, tmax) = intersect_aabb(&r, &a).expect("ray hits cube");
        assert!((tmin - 1.5).abs() < 1e-5, "tmin {}", tmin);
        assert!((tmax - 2.5).abs() < 1e-5, "tmax {}", tmax);
    }

    #[test]
    fn aabb_miss_above() {
        let r = ray([0.0, 5.0, 2.0], [0.0, 0.0, -1.0]);
        let a = aabb([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]);
        assert!(intersect_aabb(&r, &a).is_none());
    }

    #[test]
    fn aabb_box_entirely_behind_ray() {
        let r = ray([0.0, 0.0, -5.0], [0.0, 0.0, -1.0]);
        let a = aabb([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]);
        assert!(intersect_aabb(&r, &a).is_none());
    }

    #[test]
    fn aabb_origin_inside_box() {
        let r = ray([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        let a = aabb([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]);
        let (tmin, tmax) = intersect_aabb(&r, &a).expect("inside-origin always hits");
        assert!((tmin - 0.0).abs() < 1e-5);
        assert!((tmax - 0.5).abs() < 1e-5);
    }

    fn quad_mesh() -> (Vec<[f32; 3]>, Vec<u32>) {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        let indices = vec![0, 1, 2, 2, 1, 3];
        (positions, indices)
    }

    #[test]
    fn scene_picks_nearest_of_two_layered_quads() {
        let (p_front, i_front) = quad_mesh();
        let p_back: Vec<[f32; 3]> = p_front.iter().map(|p| [p[0], p[1], p[2] - 1.0]).collect();
        let meshes = [
            MeshView {
                positions: &p_front,
                indices: &i_front,
                bounds: AABB {
                    min: Point3::new(0.0, 0.0, 0.0),
                    max: Point3::new(1.0, 1.0, 0.0),
                },
            },
            MeshView {
                positions: &p_back,
                indices: &i_front,
                bounds: AABB {
                    min: Point3::new(0.0, 0.0, -1.0),
                    max: Point3::new(1.0, 1.0, -1.0),
                },
            },
        ];
        let r = ray([0.25, 0.25, 5.0], [0.0, 0.0, -1.0]);
        let hit = raycast_meshes(&r, &meshes).expect("must hit a face");
        assert_eq!(hit.mesh_index, 0, "should pick the nearer mesh");
        assert!((hit.distance - 5.0).abs() < 1e-4);
        assert_eq!(hit.face_index, 0);
    }

    #[test]
    fn scene_aabb_miss_skips_inner_iteration() {
        let (pos, idx) = quad_mesh();
        let meshes = [MeshView {
            positions: &pos,
            indices: &idx,
            bounds: AABB {
                min: Point3::new(0.0, 0.0, 0.0),
                max: Point3::new(1.0, 1.0, 0.0),
            },
        }];
        let r = ray([10.0, 10.0, 5.0], [0.0, 0.0, -1.0]);
        assert!(raycast_meshes(&r, &meshes).is_none());
    }

    #[test]
    fn scene_handles_corrupt_indices_gracefully() {
        let pos = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let idx = vec![0, 1, 99];
        let meshes = [MeshView {
            positions: &pos,
            indices: &idx,
            bounds: AABB {
                min: Point3::new(0.0, 0.0, 0.0),
                max: Point3::new(1.0, 1.0, 0.0),
            },
        }];
        let r = ray([0.25, 0.25, 5.0], [0.0, 0.0, -1.0]);
        assert!(
            raycast_meshes(&r, &meshes).is_none(),
            "must skip the bad tri"
        );
    }

    #[test]
    fn scene_empty_meshes_returns_none() {
        let r = ray([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        assert!(raycast_meshes(&r, &[]).is_none());
    }

    // The perspective assertions are clip-convention independent (direction
    // only), so those tests keep raw cgmath matrices. The ortho tests mirror
    // production instead: callers pass a wgpu-convention view_proj (near
    // plane at NDC z = 0), which is what puts the ray origin on the near
    // plane rather than mid-frustum.

    #[test]
    fn center_pixel_hits_origin_for_perspective_looking_down_z() {
        let eye = Point3::new(0.0, 0.0, 5.0);
        let view = Matrix4::look_at_rh(eye, Point3::new(0.0, 0.0, 0.0), Vector3::unit_y());
        let proj = perspective(Deg(45.0_f32), 1.0, 0.1, 100.0);
        let view_proj = proj * view;
        let r = screen_to_world_ray((400.0, 300.0), (800.0, 600.0), view_proj);
        assert!((r.direction.x).abs() < 1e-4, "dx {}", r.direction.x);
        assert!((r.direction.y).abs() < 1e-4, "dy {}", r.direction.y);
        assert!((r.direction.z - -1.0).abs() < 1e-4, "dz {}", r.direction.z);
    }

    #[test]
    fn corner_pixels_produce_diverging_rays() {
        let eye = Point3::new(0.0, 0.0, 5.0);
        let view = Matrix4::look_at_rh(eye, Point3::new(0.0, 0.0, 0.0), Vector3::unit_y());
        let proj = perspective(Deg(60.0_f32), 16.0 / 9.0, 0.1, 100.0);
        let view_proj = proj * view;
        let r_tl = screen_to_world_ray((0.0, 0.0), (1600.0, 900.0), view_proj);
        let r_br = screen_to_world_ray((1600.0, 900.0), (1600.0, 900.0), view_proj);
        assert!(r_tl.direction.x < 0.0 && r_tl.direction.y > 0.0);
        assert!(r_br.direction.x > 0.0 && r_br.direction.y < 0.0);
    }

    fn ortho_front_view_proj() -> Matrix4<f32> {
        // A front view: eye on +Z looking at the origin, 4 x 3 world units
        // visible (the ortho half-extents below), remapped to the wgpu clip
        // convention exactly like the renderer's view_projection matrix.
        let eye = Point3::new(0.0, 0.0, 5.0);
        let view = Matrix4::look_at_rh(eye, Point3::new(0.0, 0.0, 0.0), Vector3::unit_y());
        let proj = cgmath::ortho(-2.0, 2.0, -1.5, 1.5, 0.1, 100.0);
        #[rustfmt::skip]
        let gl_to_wgpu = Matrix4::new(
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 0.5, 0.0,
            0.0, 0.0, 0.5, 1.0,
        );
        gl_to_wgpu * proj * view
    }

    #[test]
    fn ortho_rays_are_parallel_and_origins_track_the_pixel() {
        // The bug this pins: an eye-anchored ray gives every pixel the same
        // origin, so ortho gizmo drags read a near-constant world point.
        let vp = ortho_front_view_proj();
        let center = screen_to_world_ray((400.0, 300.0), (800.0, 600.0), vp);
        let right = screen_to_world_ray((600.0, 300.0), (800.0, 600.0), vp);
        let up = screen_to_world_ray((400.0, 150.0), (800.0, 600.0), vp);

        assert!(center.direction.dot(right.direction) > 1.0 - 1e-5, "parallel");
        assert!(center.direction.dot(up.direction) > 1.0 - 1e-5, "parallel");
        // 200 px right = a quarter of the 800 px width = 1.0 world unit of
        // the 4-unit visible span; 150 px up = 0.75 world units of 3.
        assert!((right.origin.x - center.origin.x - 1.0).abs() < 1e-4);
        assert!((up.origin.y - center.origin.y - 0.75).abs() < 1e-4);
    }

    #[test]
    fn ortho_center_pixel_ray_runs_down_the_view_axis() {
        let vp = ortho_front_view_proj();
        let r = screen_to_world_ray((400.0, 300.0), (800.0, 600.0), vp);
        assert!(r.origin.x.abs() < 1e-4 && r.origin.y.abs() < 1e-4);
        assert!((r.direction.z - -1.0).abs() < 1e-4, "dz {}", r.direction.z);
    }

    #[test]
    fn ortho_pick_hits_the_quad_under_the_pixel() {
        // A unit quad at the origin in the XY plane; the pixel a quarter
        // width right of center must hit at world x = 1.0 exactly, which
        // the old eye-anchored ray missed entirely.
        let pos = vec![
            [0.5, -0.5, 0.0],
            [1.5, -0.5, 0.0],
            [1.5, 0.5, 0.0],
            [0.5, 0.5, 0.0],
        ];
        let idx = vec![0, 1, 2, 0, 2, 3];
        let meshes = [MeshView {
            positions: &pos,
            indices: &idx,
            bounds: AABB {
                min: Point3::new(0.5, -0.5, 0.0),
                max: Point3::new(1.5, 0.5, 0.0),
            },
        }];
        let vp = ortho_front_view_proj();
        let r = screen_to_world_ray((600.0, 300.0), (800.0, 600.0), vp);
        let hit = raycast_meshes(&r, &meshes).expect("the quad under the pixel");
        assert!((hit.world_pos[0] - 1.0).abs() < 1e-4, "x {}", hit.world_pos[0]);
        assert!(hit.world_pos[1].abs() < 1e-4, "y {}", hit.world_pos[1]);
    }

    /// Loads `res/models/xyzrgb_dragon.obj` (~720K triangles) and asserts
    /// that a single raycast completes within a generous 50ms budget. The
    /// realistic budget on typical desktop hardware is <5ms; the loose
    /// assertion guards CI runners + debug builds without flaking.
    ///
    /// Skipped if the fixture isn't present (e.g. fresh checkout where
    /// the model wasn't downloaded).
    #[test]
    fn dragon_perf_budget() {
        use std::path::PathBuf;
        use std::time::Instant;

        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop(); // crates
        path.pop(); // workspace root
        path.push("res/models/xyzrgb_dragon.obj");
        if !path.exists() {
            eprintln!(
                "skipping dragon_perf_budget: fixture not at {}",
                path.display()
            );
            return;
        }

        let raw =
            solarxy_formats::obj::load_obj(path.to_str().expect("utf-8 path")).expect("OBJ loads");
        assert!(!raw.meshes.is_empty(), "dragon has meshes");

        let bounds: Vec<AABB> = raw
            .meshes
            .iter()
            .map(|m| crate::geometry::compute_bounds(&m.positions))
            .collect();
        let views: Vec<MeshView<'_>> = raw
            .meshes
            .iter()
            .zip(bounds.iter())
            .map(|(m, b)| MeshView {
                positions: &m.positions,
                indices: &m.indices,
                bounds: *b,
            })
            .collect();

        let center = crate::geometry::compute_bounds(
            &views
                .iter()
                .flat_map(|v| v.positions.iter().copied())
                .collect::<Vec<_>>(),
        )
        .center();
        let extent = (views[0].bounds.max - views[0].bounds.min)
            .magnitude()
            .max(1.0);
        let origin = Point3::new(center.x, center.y, center.z + extent * 5.0);
        let r = ray([origin.x, origin.y, origin.z], [0.0, 0.0, -1.0]);

        let total_tris: usize = views.iter().map(|v| v.indices.len() / 3).sum();

        let t = Instant::now();
        let _hit = raycast_meshes(&r, &views);
        let elapsed = t.elapsed();

        eprintln!(
            "dragon raycast: {} tris, {} meshes, {:.2}ms",
            total_tris,
            views.len(),
            elapsed.as_secs_f64() * 1e3,
        );
        assert!(
            elapsed.as_millis() < 50,
            "raycast too slow: {}ms over {} tris",
            elapsed.as_millis(),
            total_tris
        );
    }

    // ---- gizmo primitives (rotate rings, scale cubes) ----

    #[test]
    fn a_ring_is_grabbable_on_its_band_and_not_inside_or_outside_it() {
        // The XY-plane ring (normal +Z) of radius 1, centred at the origin.
        let center = Point3::new(0.0, 0.0, 0.0);
        let normal = Vector3::unit_z();

        // Straight down -Z, landing exactly ON the ring at (1, 0).
        let on = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        assert!(intersect_ring_band(&on, center, normal, 1.0, 0.05).is_some());

        // Through the ring's empty middle: a miss, which is what makes the
        // three rings independently grabbable rather than one filled disc.
        let inside = ray([0.2, 0.0, 5.0], [0.0, 0.0, -1.0]);
        assert!(intersect_ring_band(&inside, center, normal, 1.0, 0.05).is_none());

        // Well outside the ring: also a miss.
        let outside = ray([1.6, 0.0, 5.0], [0.0, 0.0, -1.0]);
        assert!(intersect_ring_band(&outside, center, normal, 1.0, 0.05).is_none());
    }

    #[test]
    fn the_ring_band_widens_with_the_pixel_tolerance() {
        // Same reason as the axis shafts: a ring far from the camera must stay
        // as easy to click, and the tolerance is what carries that.
        let center = Point3::new(0.0, 0.0, 0.0);
        let normal = Vector3::unit_z();
        let near_miss = ray([1.1, 0.0, 5.0], [0.0, 0.0, -1.0]);
        assert!(intersect_ring_band(&near_miss, center, normal, 1.0, 0.05).is_none());
        assert!(intersect_ring_band(&near_miss, center, normal, 1.0, 0.2).is_some());
    }

    #[test]
    fn a_ring_seen_edge_on_refuses_to_solve() {
        // Looking along the ring's plane, the hit point slides wildly for a
        // sub-pixel pointer move. Better to refuse than to grab a coin flip.
        let center = Point3::new(0.0, 0.0, 0.0);
        let normal = Vector3::unit_z();
        let edge_on = ray([5.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
        assert!(intersect_ring_band(&edge_on, center, normal, 1.0, 0.05).is_none());
    }

    #[test]
    fn an_obb_is_hit_along_its_own_axes() {
        let world = [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()];
        let center = Point3::new(2.0, 0.0, 0.0);
        let half = [0.25_f32; 3];

        let hit = ray([2.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let t = intersect_obb(&hit, center, world, half).expect("dead centre");
        // The near face sits 0.25 in front of the centre.
        assert!((t - 4.75).abs() < 1e-4, "near face, got {t}");

        let miss = ray([2.0, 1.0, 5.0], [0.0, 0.0, -1.0]);
        assert!(intersect_obb(&miss, center, world, half).is_none());
    }

    #[test]
    fn a_rotated_obb_is_hit_where_it_actually_sits_not_where_an_aabb_would_be() {
        // The whole reason this is an OBB: under local orientation the scale
        // cubes ride the object's axes. A box turned 45 degrees about Z reaches
        // further along its own diagonal than its axis-aligned twin does.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let rotated = [
            Vector3::new(s, s, 0.0),
            Vector3::new(-s, s, 0.0),
            Vector3::unit_z(),
        ];
        let center = Point3::new(0.0, 0.0, 0.0);
        let half = [0.5_f32, 0.5, 0.5];

        // The rotated box's corner reaches out to ~0.707 along +X; the
        // axis-aligned one stops at 0.5. A ray down -Z at x = 0.6 therefore
        // hits the rotated box and misses the world-aligned one.
        let probe = ray([0.6, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let world = [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()];
        assert!(intersect_obb(&probe, center, rotated, half).is_some());
        assert!(intersect_obb(&probe, center, world, half).is_none());
    }

    #[test]
    fn an_infinite_line_solves_exactly_far_past_the_segment() {
        // The gizmo's axis drag runs way past the drawn arrow. Faking the line
        // with a huge segment used to cost ~4mm of f32 precision here, which the
        // user sees as the object lagging the cursor.
        let line_dir = Vector3::unit_x();
        let origin = Point3::new(0.0, 0.0, 0.0);
        for x in [1.0_f32, 37.5, 1000.0] {
            let r = ray([x, 0.0, 5.0], [0.0, 0.0, -1.0]);
            let p = closest_point_ray_line(&r, origin, line_dir).unwrap();
            assert!(
                (p.x - x).abs() < 1e-4,
                "exact at x = {x}, got {} (off by {})",
                p.x,
                (p.x - x).abs()
            );
            assert!(p.y.abs() < 1e-4 && p.z.abs() < 1e-4, "on the line");
        }
    }

    #[test]
    fn a_ray_down_the_line_has_no_closest_point() {
        // Sighting along the axis: the solution is unbounded, and a naive solve
        // sends the object to infinity.
        let r = ray([5.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
        assert!(
            closest_point_ray_line(&r, Point3::new(0.0, 0.0, 0.0), Vector3::unit_x()).is_none()
        );
    }
}
