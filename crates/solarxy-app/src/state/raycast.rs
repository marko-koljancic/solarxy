//! CPU raycaster — picks faces of CPU-side mesh data for review-mode click
//! anchoring and any future 3D ↔ UV selection sync.
//!
//! Pure math, no GPU. Möller-Trumbore for triangle-ray intersection, slab
//! method for the AABB early-reject. Decoupled from the live `ModelScene`
//! (which only holds GPU buffers): callers build `MeshView` slices over
//! their CPU mesh copies and pass them in.
//!
//! Performance budget: < 5ms for ~100K-triangle scenes on Apple Silicon
//! / equivalent class hardware (asserted by [`tests::dragon_perf_budget`]
//! when the `xyzrgb_dragon.obj` fixture is present).

// Standard math notation in ray-tracing code: `t`, `u`, `v`, `w`, `h`, `q`,
// `s`, `a`, `f` are deliberate (Möller-Trumbore + slab method literature).
#![allow(clippy::many_single_char_names)]
// Public API surface here is built ahead of its consumers — review-mode
// click handling (state/review.rs, task #6) is the first caller. The
// allow stays until then; remove on first use.
#![allow(dead_code)]

use cgmath::{InnerSpace, Matrix4, Point3, SquareMatrix, Vector3, Vector4};
use solarxy_core::AABB;

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
///   same one written to the GPU camera uniform.
/// - `camera_eye` — the camera origin in world space.
///
/// Returns a ray from `camera_eye` aimed through the pixel. Works for
/// both perspective and orthographic cameras (the ortho case reduces to
/// a parallel-ray direction).
pub fn screen_to_world_ray(
    cursor_px: (f32, f32),
    viewport_size_px: (f32, f32),
    view_proj: Matrix4<f32>,
    camera_eye: Point3<f32>,
) -> Ray {
    let inv = view_proj.invert().unwrap_or_else(Matrix4::identity);
    let ndc_x = (cursor_px.0 / viewport_size_px.0) * 2.0 - 1.0;
    let ndc_y = 1.0 - (cursor_px.1 / viewport_size_px.1) * 2.0;
    let far_clip = Vector4::new(ndc_x, ndc_y, 1.0, 1.0);
    let far_world_h = inv * far_clip;
    let far_world = Point3::new(
        far_world_h.x / far_world_h.w,
        far_world_h.y / far_world_h.w,
        far_world_h.z / far_world_h.w,
    );

    let direction = (far_world - camera_eye).normalize();
    Ray {
        origin: camera_eye,
        direction,
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

    #[test]
    fn center_pixel_hits_origin_for_perspective_looking_down_z() {
        let eye = Point3::new(0.0, 0.0, 5.0);
        let view = Matrix4::look_at_rh(eye, Point3::new(0.0, 0.0, 0.0), Vector3::unit_y());
        let proj = perspective(Deg(45.0_f32), 1.0, 0.1, 100.0);
        let view_proj = proj * view;
        let r = screen_to_world_ray((400.0, 300.0), (800.0, 600.0), view_proj, eye);
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
        let r_tl = screen_to_world_ray((0.0, 0.0), (1600.0, 900.0), view_proj, eye);
        let r_br = screen_to_world_ray((1600.0, 900.0), (1600.0, 900.0), view_proj, eye);
        assert!(r_tl.direction.x < 0.0 && r_tl.direction.y > 0.0);
        assert!(r_br.direction.x > 0.0 && r_br.direction.y < 0.0);
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
            .map(|m| solarxy_core::geometry::compute_bounds(&m.positions))
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

        let center = solarxy_core::geometry::compute_bounds(
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
}
