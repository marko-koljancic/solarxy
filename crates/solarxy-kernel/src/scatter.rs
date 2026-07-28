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
    scatter_weighted(set, count, seed, None)
}

/// [`scatter`] with an optional per-point density lane biasing where samples
/// land.
///
/// **How a point-domain lane becomes a triangle weight.** Density is stored
/// per vertex, but the sampler picks a triangle from an area prefix sum, so
/// each triangle takes the MEAN of its three corners. The alternative,
/// rejection-sampling against the barycentric-interpolated value, is smoother
/// but needs an unbounded retry loop, and an unbounded loop on a
/// single-threaded cook is the one thing this operator must not have.
///
/// A triangle whose mean density is zero or negative is never picked; density
/// clamps at zero rather than flipping a weight negative. When every weight
/// comes out zero the result is empty, which the node reports rather than
/// silently falling back to uniform scattering that would hide the mistake.
#[must_use]
pub fn scatter_weighted(
    set: &GeometrySet,
    count: u32,
    seed: u32,
    density: Option<&str>,
) -> GeometrySet {
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
            let weight = area * triangle_density(mesh, corners, density);
            if weight.is_finite() && weight > 0.0 {
                total += weight;
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

/// Whether a density lane named `name` would actually bias anything.
///
/// Exists so the NODE can tell "you named a lane that is not there" from "the
/// scatter worked", which the geometry alone cannot: an unresolved lane falls
/// back to area-only weighting and produces a perfectly ordinary even scatter,
/// so a typo looks exactly like success.
///
/// Deliberately a question rather than a second return value from
/// [`scatter_weighted`]: the operator stays a plain `GeometrySet ->
/// GeometrySet`, and this shares [`triangle_density`]'s acceptance rules
/// below, so the two cannot drift into disagreeing about what "resolves"
/// means.
#[must_use]
pub fn density_lane_resolves(set: &GeometrySet, name: &str) -> bool {
    set.meshes.iter().any(|mesh| {
        mesh.topology == MeshTopology::Triangles
            && matches!(
                mesh.attributes.get(name),
                Some(AttributeData::Float(v)) if v.len() == mesh.positions.len()
            )
    })
}

/// The density multiplier for one triangle: the mean of its three corners,
/// clamped at zero.
///
/// Returns 1.0 when no lane is named or the named lane is absent or the wrong
/// type, so an unweighted scatter is exactly area-weighted as before. A lane
/// that exists but is not Float is ignored rather than refused: the node
/// warns, and silently scattering nothing would be worse than scattering
/// evenly.
fn triangle_density(mesh: &KernelMesh, corners: &[u32], density: Option<&str>) -> f64 {
    let Some(name) = density else { return 1.0 };
    let Some(AttributeData::Float(values)) = mesh.attributes.get(name) else {
        return 1.0;
    };
    if values.len() != mesh.positions.len() {
        return 1.0;
    }
    let mut sum = 0.0_f64;
    for &c in corners {
        let Some(v) = values.get(c as usize) else {
            return 1.0;
        };
        sum += f64::from(*v).max(0.0);
    }
    sum / 3.0
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

#[cfg(test)]
mod density_tests {
    use super::*;
    use std::sync::Arc;

    /// Two unit quads side by side in X: the left spans x in [-2,-1], the
    /// right x in [1,2]. Equal area, so an unweighted scatter splits evenly.
    fn two_quads(density: Option<[f32; 8]>) -> GeometrySet {
        let positions = vec![
            [-2.0, 0.0, -0.5],
            [-1.0, 0.0, -0.5],
            [-1.0, 0.0, 0.5],
            [-2.0, 0.0, 0.5],
            [1.0, 0.0, -0.5],
            [2.0, 0.0, -0.5],
            [2.0, 0.0, 0.5],
            [1.0, 0.0, 0.5],
        ];
        let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let mut mesh = KernelMesh::new("quads", positions, indices);
        if let Some(d) = density {
            mesh.attributes.insert(
                "density".to_string(),
                AttributeData::Float(Arc::new(d.to_vec())),
            );
        }
        GeometrySet::from_parts(vec![mesh], Vec::new())
    }

    /// How many scattered points landed on the right-hand quad.
    fn right_share(set: &GeometrySet) -> f32 {
        let pts = &set.meshes[0].positions;
        let right = pts.iter().filter(|p| p[0] > 0.0).count();
        right as f32 / pts.len() as f32
    }

    #[test]
    fn without_a_lane_the_split_is_even_by_area() {
        let out = scatter(&two_quads(None), 2000, 7);
        let share = right_share(&out);
        assert!((share - 0.5).abs() < 0.06, "expected ~0.5, got {share}");
    }

    #[test]
    fn naming_a_lane_biases_where_the_points_land() {
        // Right quad three times the density of the left: it should take
        // about three quarters of the points.
        let set = two_quads(Some([1.0, 1.0, 1.0, 1.0, 3.0, 3.0, 3.0, 3.0]));
        let out = scatter_weighted(&set, 4000, 7, Some("density"));
        let share = right_share(&out);
        assert!((share - 0.75).abs() < 0.06, "expected ~0.75, got {share}");
    }

    #[test]
    fn a_zero_region_receives_nothing() {
        let set = two_quads(Some([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]));
        let out = scatter_weighted(&set, 500, 7, Some("density"));
        assert!(
            out.meshes[0].positions.iter().all(|p| p[0] > 0.0),
            "zero-density region must receive no points"
        );
    }

    #[test]
    fn negative_density_clamps_rather_than_flipping_the_weight() {
        // A negative weight would subtract from the prefix sum and corrupt
        // the search, so it clamps to zero and the region is simply skipped.
        let set = two_quads(Some([-5.0, -5.0, -5.0, -5.0, 1.0, 1.0, 1.0, 1.0]));
        let out = scatter_weighted(&set, 500, 7, Some("density"));
        assert!(out.meshes[0].positions.iter().all(|p| p[0] > 0.0));
    }

    #[test]
    fn density_zero_everywhere_scatters_nothing_rather_than_falling_back() {
        // Falling back to an even scatter would hide the mistake; the node
        // turns this into a warning naming the lane.
        let set = two_quads(Some([0.0; 8]));
        let out = scatter_weighted(&set, 500, 7, Some("density"));
        assert!(out.is_renderable_empty());
    }

    #[test]
    fn a_resolvable_lane_is_reported_as_such() {
        let set = two_quads(Some([1.0; 8]));
        assert!(density_lane_resolves(&set, "density"));
    }

    #[test]
    fn an_absent_lane_is_reported_so_the_node_can_say_so() {
        // The whole point: the GEOMETRY cannot tell you, because an
        // unresolved lane produces a perfectly ordinary even scatter.
        let set = two_quads(Some([1.0; 8]));
        assert!(!density_lane_resolves(&set, "denstiy"));
        assert!(!density_lane_resolves(&two_quads(None), "density"));
    }

    #[test]
    fn resolution_agrees_with_what_the_weighting_actually_accepts() {
        // A lane of the wrong TYPE is ignored by `triangle_density`, so it
        // must also report as unresolved, or the node would stay silent about
        // a scatter that is not being weighted.
        let mut set = two_quads(None);
        set.meshes[0].attributes.insert(
            "density".to_string(),
            AttributeData::Vec3(Arc::new(vec![[1.0, 1.0, 1.0]; 8])),
        );
        assert!(!density_lane_resolves(&set, "density"));
        let out = scatter_weighted(&set, 1000, 7, Some("density"));
        assert!(
            (right_share(&out) - 0.5).abs() < 0.08,
            "and it scattered evenly"
        );
    }

    #[test]
    fn an_absent_or_mistyped_lane_falls_back_to_area_alone() {
        // Naming a lane that is not there must not blank the scatter: the
        // node warns, and an even spread is the recoverable answer.
        let out = scatter_weighted(&two_quads(None), 1000, 7, Some("nope"));
        assert!(!out.is_renderable_empty());
        assert!((right_share(&out) - 0.5).abs() < 0.08);
    }
}
