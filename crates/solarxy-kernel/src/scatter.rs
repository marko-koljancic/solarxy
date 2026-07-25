//! Surface scattering (the `scatter` node's kernel): area-weighted random
//! sampling of a set's triangle surfaces into a Points-topology cloud.
//!
//! Sampling is the classic three-draw scheme: a prefix sum over triangle
//! areas turns one uniform draw into an area-proportional triangle pick,
//! and two more draws fold into barycentric coordinates inside it. Every
//! draw comes from the seeded avalanche hash in [`crate::rng`], so a given
//! `(input, count, seed)` always produces the same cloud.
//!
//! Scattered points inherit the source surface: the reserved `N` lane
//! carries the interpolated vertex normal (or the face normal when the
//! source has none), and `uv` / `color` lanes are interpolated when any
//! source mesh carries them. Line and point meshes have no area and are
//! skipped as sample sources.
//!
//! The point-count ceiling is owned by the scatter node's param hard range
//! (1M), not re-checked here: points are the cheapest primitive, and the
//! resolver clamp is the settled guard.

use std::sync::Arc;

use solarxy_core::geometry::MeshTopology;

use crate::rng;
use crate::set::{AttributeData, GeometrySet, KernelMesh, reserved};

/// Scatters `count` points over the set's triangle surfaces, area-weighted
/// and deterministic in `seed`. Returns the empty set when no triangle has
/// positive area (the node warns; the cook gate treats it as
/// renderable-empty).
#[must_use]
pub fn scatter(set: &GeometrySet, count: u32, seed: u32) -> GeometrySet {
    // One entry per positive-area triangle: owning mesh, triangle index,
    // and the running area total (the prefix sum the pick searches).
    struct Candidate {
        mesh: usize,
        tri: usize,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut prefix: Vec<f64> = Vec::new();
    let mut total = 0.0_f64;

    for (mesh_index, mesh) in set.meshes.iter().enumerate() {
        if mesh.topology != MeshTopology::Triangles {
            continue;
        }
        for (tri, corners) in mesh.indices.chunks_exact(3).enumerate() {
            let [a, b, c] = triangle_positions(mesh, corners);
            let area = f64::from(cross_length(sub(b, a), sub(c, a))) * 0.5;
            if area.is_finite() && area > 0.0 {
                total += area;
                candidates.push(Candidate {
                    mesh: mesh_index,
                    tri,
                });
                prefix.push(total);
            }
        }
    }
    if candidates.is_empty() || total <= 0.0 {
        return GeometrySet::empty();
    }

    // Lanes are emitted when any sampled mesh can feed them; sources
    // without the channel contribute the documented defaults ([0, 0] UV,
    // white color) so the lanes stay position-count.
    let source_has_uvs = |m: &KernelMesh| {
        m.tex_coords
            .as_ref()
            .is_some_and(|t| t.len() == m.positions.len())
    };
    let source_colors = |m: &KernelMesh| match m.attributes.get(reserved::COLOR) {
        Some(AttributeData::Vec4(v)) if v.len() == m.positions.len() => Some(Arc::clone(v)),
        _ => None,
    };
    let any_uv = set
        .meshes
        .iter()
        .any(|m| m.topology == MeshTopology::Triangles && source_has_uvs(m));
    let any_color = set
        .meshes
        .iter()
        .any(|m| m.topology == MeshTopology::Triangles && source_colors(m).is_some());

    let n = count as usize;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(if any_uv { n } else { 0 });
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(if any_color { n } else { 0 });

    for i in 0..u64::from(count) {
        let pick = rng::unit_f64(i, 0, seed) * total;
        let index = prefix
            .partition_point(|&p| p <= pick)
            .min(candidates.len() - 1);
        let chosen = &candidates[index];
        let mesh = &set.meshes[chosen.mesh];
        let corners = &mesh.indices[chosen.tri * 3..chosen.tri * 3 + 3];
        let [a, b, c] = triangle_positions(mesh, corners);

        // Two uniform draws folded onto the u + v <= 1 half give a uniform
        // barycentric point without a square root.
        let mut u = rng::unit_f32(i, 1, seed);
        let mut v = rng::unit_f32(i, 2, seed);
        if u + v > 1.0 {
            u = 1.0 - u;
            v = 1.0 - v;
        }
        let w = 1.0 - u - v;

        positions.push(bary(a, b, c, w, u, v));

        let smooth = mesh
            .normals
            .as_ref()
            .filter(|buf| buf.len() == mesh.positions.len())
            .map(|buf| {
                let [na, nb, nc] = triangle_values(buf, corners);
                normalize_or(bary(na, nb, nc, w, u, v), face_normal(a, b, c))
            });
        normals.push(smooth.unwrap_or_else(|| face_normal(a, b, c)));

        if any_uv {
            uvs.push(
                match mesh
                    .tex_coords
                    .as_ref()
                    .filter(|t| t.len() == mesh.positions.len())
                {
                    Some(buf) => {
                        let [ta, tb, tc] = triangle_values(buf, corners);
                        [
                            w * ta[0] + u * tb[0] + v * tc[0],
                            w * ta[1] + u * tb[1] + v * tc[1],
                        ]
                    }
                    None => [0.0, 0.0],
                },
            );
        }
        if any_color {
            colors.push(match source_colors(mesh) {
                Some(buf) => {
                    let [ca, cb, cc] = triangle_values(&buf, corners);
                    [
                        w * ca[0] + u * cb[0] + v * cc[0],
                        w * ca[1] + u * cb[1] + v * cc[1],
                        w * ca[2] + u * cb[2] + v * cc[2],
                        w * ca[3] + u * cb[3] + v * cc[3],
                    ]
                }
                None => [1.0, 1.0, 1.0, 1.0],
            });
        }
    }

    let mut mesh = KernelMesh::points("scatter", positions);
    mesh.attributes.insert(
        reserved::NORMAL.to_string(),
        AttributeData::Vec3(Arc::new(normals)),
    );
    if any_uv {
        mesh.attributes
            .insert(reserved::UV.to_string(), AttributeData::Vec2(Arc::new(uvs)));
    }
    if any_color {
        mesh.attributes.insert(
            reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::new(colors)),
        );
    }
    GeometrySet::from_mesh(mesh)
}

/// The three corner positions of one triangle.
fn triangle_positions(mesh: &KernelMesh, corners: &[u32]) -> [[f32; 3]; 3] {
    triangle_values(&mesh.positions, corners)
}

/// The three corner values of one triangle from any per-point buffer.
fn triangle_values<const N: usize>(buffer: &[[f32; N]], corners: &[u32]) -> [[f32; N]; 3] {
    [
        buffer[corners[0] as usize],
        buffer[corners[1] as usize],
        buffer[corners[2] as usize],
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn cross_length(a: [f32; 3], b: [f32; 3]) -> f32 {
    let c = cross(a, b);
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
}

fn bary<const N: usize>(a: [f32; N], b: [f32; N], c: [f32; N], w: f32, u: f32, v: f32) -> [f32; N] {
    std::array::from_fn(|i| w * a[i] + u * b[i] + v * c[i])
}

/// The unit face normal under the frozen CCW/Y-up winding. The caller only
/// asks for triangles it already measured as positive-area, so the cross
/// product cannot vanish.
fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let n = cross(sub(b, a), sub(c, a));
    normalize_or(n, [0.0, 1.0, 0.0])
}

fn normalize_or(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-12 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::generate_plane;

    fn lane_vec3<'a>(mesh: &'a KernelMesh, name: &str) -> &'a [[f32; 3]] {
        match mesh.attributes.get(name) {
            Some(AttributeData::Vec3(v)) => v,
            other => panic!("expected a Vec3 lane {name}, got {other:?}"),
        }
    }

    #[test]
    fn same_seed_reproduces_and_reseeding_diverges() {
        let set = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        let a = scatter(&set, 64, 5);
        let b = scatter(&set, 64, 5);
        assert_eq!(a.meshes[0].positions, b.meshes[0].positions);
        let c = scatter(&set, 64, 6);
        assert_ne!(a.meshes[0].positions, c.meshes[0].positions);
    }

    #[test]
    fn output_is_a_points_cloud_on_the_source_surface() {
        let set = GeometrySet::from_mesh(generate_plane(2.0, 4.0, 1, 1));
        let out = scatter(&set, 200, 0);
        let mesh = &out.meshes[0];
        assert_eq!(mesh.topology, MeshTopology::Points);
        assert_eq!(mesh.positions.len(), 200);
        assert!(mesh.is_renderable());
        // The plane primitive spans XY facing +Z, so samples sit at z = 0
        // inside the half extents.
        for p in mesh.positions.iter() {
            assert!(p[2].abs() < 1e-5, "plane points sit at z = 0: {p:?}");
            assert!(
                p[0].abs() <= 1.0 + 1e-5 && p[1].abs() <= 2.0 + 1e-5,
                "{p:?}"
            );
        }
    }

    #[test]
    fn sampling_is_area_weighted() {
        // Two disjoint right triangles in the XZ plane: one with 9x the
        // area of the other, separated along X so samples classify by
        // position. The big one should draw ~90% of the points.
        let positions = vec![
            // Small: legs of length 1 around x = -10.
            [-11.0, 0.0, 0.0],
            [-10.0, 0.0, 0.0],
            [-11.0, 0.0, -1.0],
            // Big: legs of length 3 around x = +10.
            [10.0, 0.0, 0.0],
            [13.0, 0.0, 0.0],
            [10.0, 0.0, -3.0],
        ];
        let set = GeometrySet::from_mesh(KernelMesh::new("t", positions, vec![0, 1, 2, 3, 4, 5]));
        let out = scatter(&set, 2000, 1);
        let big = out.meshes[0]
            .positions
            .iter()
            .filter(|p| p[0] > 0.0)
            .count();
        let share = big as f32 / 2000.0;
        assert!((0.85..0.95).contains(&share), "big-triangle share {share}");
    }

    #[test]
    fn points_inherit_interpolated_attributes() {
        // One triangle with distinct per-corner UV and color; scattered
        // points must interpolate inside the simplex (colors sum to 1).
        let mut mesh = KernelMesh::new(
            "t",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            vec![0, 1, 2],
        );
        mesh.tex_coords = Some(Arc::new(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]));
        mesh.attributes.insert(
            reserved::COLOR.to_string(),
            AttributeData::Vec4(Arc::new(vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ])),
        );
        let out = scatter(&GeometrySet::from_mesh(mesh), 100, 3);
        let scattered = &out.meshes[0];
        let normals = lane_vec3(scattered, reserved::NORMAL);
        assert_eq!(normals.len(), 100);
        for n in normals {
            // Face normal of the CCW XZ triangle points up.
            assert!((n[1] - 1.0).abs() < 1e-5, "{n:?}");
        }
        let Some(AttributeData::Vec2(uvs)) = scattered.attributes.get(reserved::UV) else {
            panic!("uv lane expected");
        };
        for uv in uvs.iter() {
            assert!(
                uv[0] >= -1e-5 && uv[1] >= -1e-5 && uv[0] + uv[1] <= 1.0 + 1e-4,
                "{uv:?}"
            );
        }
        let Some(AttributeData::Vec4(colors)) = scattered.attributes.get(reserved::COLOR) else {
            panic!("color lane expected");
        };
        for c in colors.iter() {
            let sum = c[0] + c[1] + c[2];
            assert!(
                (sum - 1.0).abs() < 1e-4,
                "barycentric weights sum to 1: {c:?}"
            );
            assert!((c[3] - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn smooth_normals_interpolate_when_present() {
        let set = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        let out = scatter(&set, 16, 0);
        for n in lane_vec3(&out.meshes[0], reserved::NORMAL) {
            assert!((n[2] - 1.0).abs() < 1e-5, "plane normals are +Z: {n:?}");
        }
    }

    #[test]
    fn inputs_without_triangle_area_scatter_to_the_empty_set() {
        let points = GeometrySet::from_mesh(KernelMesh::points("p", vec![[0.0; 3]; 8]));
        assert!(scatter(&points, 100, 0).is_renderable_empty());

        let line = GeometrySet::from_mesh(KernelMesh::polyline(
            "l",
            vec![[0.0; 3], [1.0, 0.0, 0.0]],
            vec![0, 1],
        ));
        assert!(scatter(&line, 100, 0).is_renderable_empty());

        assert!(scatter(&GeometrySet::empty(), 100, 0).is_renderable_empty());
    }

    #[test]
    fn degenerate_triangles_are_never_sampled() {
        // A real triangle at x > 0 plus a zero-area sliver at x = -5: every
        // sample must land on the real one.
        let positions = vec![
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [1.0, 0.0, -1.0],
            [-5.0, 0.0, 0.0],
            [-5.0, 0.0, 0.0],
            [-5.0, 0.0, 0.0],
        ];
        let set = GeometrySet::from_mesh(KernelMesh::new("t", positions, vec![0, 1, 2, 3, 4, 5]));
        let out = scatter(&set, 100, 2);
        for p in out.meshes[0].positions.iter() {
            assert!(p[0] > 0.0, "sample landed on the degenerate sliver: {p:?}");
        }
    }
}
