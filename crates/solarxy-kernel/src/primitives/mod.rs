//! The seven parametric primitive generators: box, sphere, cylinder, cone,
//! plane, torus, torus knot.
//!
//! Shared conventions, frozen for the whole engine:
//!
//! - Y-up right-handed coordinates; triangles counter-clockwise
//!   front-facing; normals point out of the enclosed volume.
//! - Every generator emits positions, unit normals, and UVs in `[0, 1]`.
//! - Orientation follows the Minimystix/Three.js executable spec for visual
//!   continuity: primitives are centered at the origin; cylinder and cone
//!   run along Y (top cap at `+h/2`, cone tip at `+h/2`); the plane lies in
//!   the XY plane facing `+Z`; torus and torus knot lie in the XY plane
//!   around Z. Values are spec-conformant, not byte-matched to Three.js.
//! - Dimension validity (positive sizes, segment minimums) is enforced
//!   upstream by the graph's param resolver hard ranges. The generators
//!   additionally clamp segment counts to their mathematical minimums as a
//!   totality guard, never as policy.

mod box_gen;
mod cone;
mod cylinder;
mod plane;
mod sphere;
mod torus;
mod torus_knot;

pub use box_gen::generate_box;
pub use cone::generate_cone;
pub use cylinder::generate_cylinder;
pub use plane::generate_plane;
pub use sphere::generate_sphere;
pub use torus::generate_torus;
pub use torus_knot::generate_torus_knot;

use std::sync::Arc;

use crate::set::{AttributeMap, KernelMesh};

/// Builds a [`KernelMesh`] from freshly generated buffers.
pub(crate) fn assemble(
    name: &str,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
) -> KernelMesh {
    debug_assert_eq!(positions.len(), normals.len());
    debug_assert_eq!(positions.len(), uvs.len());
    debug_assert_eq!(indices.len() % 3, 0);
    KernelMesh {
        name: name.to_string(),
        positions: Arc::new(positions),
        normals: Some(Arc::new(normals)),
        tex_coords: Some(Arc::new(uvs)),
        indices: Arc::new(indices),
        material_index: None,
        attributes: AttributeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    //! Cross-primitive conformance suite: every generator must satisfy the
    //! frozen invariants (unit outward normals, CCW winding, UV range,
    //! index validity) at its catalog-default parameters.

    use super::*;

    /// The seven primitives at their catalog-default parameters, with the
    /// hand-computed expected vertex and triangle counts.
    fn defaults() -> Vec<(KernelMesh, usize, usize)> {
        vec![
            // box 1x1x1, segments 1: 6 faces x 4 verts, 12 tris.
            (generate_box(1.0, 1.0, 1.0, 1, 1, 1), 24, 12),
            // sphere r=0.5, 32x16: 33*17 verts, 32*(2*16-2) tris.
            (generate_sphere(0.5, 32, 16), 561, 960),
            // cylinder r=0.5/0.5, h=1, 32x1: torso 33*2 + caps 2*(1+33);
            // tris torso 64 + caps 64.
            (generate_cylinder(0.5, 0.5, 1.0, 32, 1), 134, 128),
            // cone r=0.5 h=1 32x1: torso 66 + bottom cap 34; torso keeps
            // only the bottom triangle of each quad (tip row degenerate).
            (generate_cone(0.5, 1.0, 32, 1), 100, 64),
            // plane 1x1, 1x1: 4 verts, 2 tris.
            (generate_plane(1.0, 1.0, 1, 1), 4, 2),
            // torus R=0.5 tube=0.2, 16x32: (16+1)*(32+1) verts, 16*32*2 tris.
            (generate_torus(0.5, 0.2, 16, 32), 561, 1024),
            // torus_knot r=0.5 tube=0.2 p=2 q=3, 128x32: 129*33 verts,
            // 128*32*2 tris.
            (generate_torus_knot(0.5, 0.2, 2, 3, 128, 32), 4257, 8192),
        ]
    }

    fn face_normal(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3]) -> [f32; 3] {
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ]
    }

    #[test]
    fn counts_match_catalog_defaults() {
        for (mesh, verts, tris) in defaults() {
            assert_eq!(mesh.vertex_count(), verts, "{}: vertex count", mesh.name);
            assert_eq!(mesh.triangle_count(), tris, "{}: triangle count", mesh.name);
        }
    }

    #[test]
    fn indices_are_in_range_and_triangulated() {
        for (mesh, ..) in defaults() {
            assert_eq!(mesh.indices.len() % 3, 0, "{}", mesh.name);
            let max = mesh.vertex_count() as u32;
            assert!(
                mesh.indices.iter().all(|&i| i < max),
                "{}: index out of range",
                mesh.name
            );
        }
    }

    #[test]
    fn normals_are_unit_length() {
        for (mesh, ..) in defaults() {
            let normals = mesh.normals.as_ref().expect("generator emits normals");
            assert_eq!(normals.len(), mesh.vertex_count(), "{}", mesh.name);
            for (i, n) in normals.iter().enumerate() {
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                assert!(
                    (len - 1.0).abs() < 1e-4,
                    "{}: normal {i} has length {len}",
                    mesh.name
                );
            }
        }
    }

    #[test]
    fn uvs_are_in_unit_range() {
        for (mesh, ..) in defaults() {
            let uvs = mesh.tex_coords.as_ref().expect("generator emits UVs");
            assert_eq!(uvs.len(), mesh.vertex_count(), "{}", mesh.name);
            for uv in uvs.iter() {
                assert!(
                    (-1e-6..=1.0 + 1e-6).contains(&uv[0]) && (-1e-6..=1.0 + 1e-6).contains(&uv[1]),
                    "{}: UV {uv:?} outside [0,1]",
                    mesh.name
                );
            }
        }
    }

    /// The winding + orientation invariant: each triangle's geometric face
    /// normal must agree with its vertices' shading normals (positive dot),
    /// which simultaneously proves CCW-front winding and outward normals.
    #[test]
    fn winding_is_ccw_with_outward_normals() {
        for (mesh, ..) in defaults() {
            let normals = mesh.normals.as_ref().unwrap();
            for (t, tri) in mesh.indices.chunks(3).enumerate() {
                let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
                let fnorm = face_normal(mesh.positions[a], mesh.positions[b], mesh.positions[c]);
                let avg = [
                    (normals[a][0] + normals[b][0] + normals[c][0]) / 3.0,
                    (normals[a][1] + normals[b][1] + normals[c][1]) / 3.0,
                    (normals[a][2] + normals[b][2] + normals[c][2]) / 3.0,
                ];
                let dot = fnorm[0] * avg[0] + fnorm[1] * avg[1] + fnorm[2] * avg[2];
                assert!(
                    dot > 0.0,
                    "{}: triangle {t} winding disagrees with normals (dot {dot})",
                    mesh.name
                );
            }
        }
    }

    #[test]
    fn bounds_match_expected_extents() {
        let cases: Vec<(KernelMesh, [f32; 3], [f32; 3])> = vec![
            (
                generate_box(1.0, 2.0, 3.0, 1, 1, 1),
                [-0.5, -1.0, -1.5],
                [0.5, 1.0, 1.5],
            ),
            (
                generate_sphere(0.5, 32, 16),
                [-0.5, -0.5, -0.5],
                [0.5, 0.5, 0.5],
            ),
            (
                generate_cylinder(0.5, 0.5, 1.0, 32, 1),
                [-0.5, -0.5, -0.5],
                [0.5, 0.5, 0.5],
            ),
            (
                generate_cone(0.5, 1.0, 32, 1),
                [-0.5, -0.5, -0.5],
                [0.5, 0.5, 0.5],
            ),
            (
                generate_plane(2.0, 1.0, 1, 1),
                [-1.0, -0.5, 0.0],
                [1.0, 0.5, 0.0],
            ),
            // torus: XY extent R + tube, Z extent tube.
            (
                generate_torus(0.5, 0.2, 16, 32),
                [-0.7, -0.7, -0.2],
                [0.7, 0.7, 0.2],
            ),
        ];
        for (mesh, expect_min, expect_max) in cases {
            let b = mesh.bounds();
            let (bmin, bmax): ([f32; 3], [f32; 3]) = (b.min.into(), b.max.into());
            for i in 0..3 {
                assert!(
                    (bmin[i] - expect_min[i]).abs() < 1e-3,
                    "{}: min[{i}] = {} expected {}",
                    mesh.name,
                    bmin[i],
                    expect_min[i]
                );
                assert!(
                    (bmax[i] - expect_max[i]).abs() < 1e-3,
                    "{}: max[{i}] = {} expected {}",
                    mesh.name,
                    bmax[i],
                    expect_max[i]
                );
            }
        }
    }
}
