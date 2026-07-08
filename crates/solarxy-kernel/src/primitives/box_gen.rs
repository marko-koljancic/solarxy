//! Box generator: six independently subdivided faces (24 vertices at
//! segment count 1, so every face carries its own flat normal).

use super::assemble;
use crate::set::KernelMesh;

/// `width` along X, `height` along Y, `depth` along Z, centered at the
/// origin. Each face is a `useg x vseg` grid with its own UV unit square.
#[must_use]
pub fn generate_box(
    width: f32,
    height: f32,
    depth: f32,
    width_segments: u32,
    height_segments: u32,
    depth_segments: u32,
) -> KernelMesh {
    // Each face: (normal N, u-axis U, v-axis V) with U x V = N so the grid
    // winds CCW seen from outside. (u_size, v_size) are the face extents
    // along U/V; n_half offsets the face plane along N.
    struct Face {
        n: [f32; 3],
        u: [f32; 3],
        v: [f32; 3],
        u_size: f32,
        v_size: f32,
        n_half: f32,
        useg: usize,
        vseg: usize,
    }

    let ws = width_segments.max(1) as usize;
    let hs = height_segments.max(1) as usize;
    let ds = depth_segments.max(1) as usize;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    let faces = [
        // +Z and -Z: U spans width, V spans height.
        Face {
            n: [0.0, 0.0, 1.0],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            u_size: width,
            v_size: height,
            n_half: depth / 2.0,
            useg: ws,
            vseg: hs,
        },
        Face {
            n: [0.0, 0.0, -1.0],
            u: [-1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            u_size: width,
            v_size: height,
            n_half: depth / 2.0,
            useg: ws,
            vseg: hs,
        },
        // +X and -X: U spans depth, V spans height.
        Face {
            n: [1.0, 0.0, 0.0],
            u: [0.0, 0.0, -1.0],
            v: [0.0, 1.0, 0.0],
            u_size: depth,
            v_size: height,
            n_half: width / 2.0,
            useg: ds,
            vseg: hs,
        },
        Face {
            n: [-1.0, 0.0, 0.0],
            u: [0.0, 0.0, 1.0],
            v: [0.0, 1.0, 0.0],
            u_size: depth,
            v_size: height,
            n_half: width / 2.0,
            useg: ds,
            vseg: hs,
        },
        // +Y and -Y: U spans width, V spans depth.
        Face {
            n: [0.0, 1.0, 0.0],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 0.0, -1.0],
            u_size: width,
            v_size: depth,
            n_half: height / 2.0,
            useg: ws,
            vseg: ds,
        },
        Face {
            n: [0.0, -1.0, 0.0],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 0.0, 1.0],
            u_size: width,
            v_size: depth,
            n_half: height / 2.0,
            useg: ws,
            vseg: ds,
        },
    ];

    for face in &faces {
        let base = positions.len() as u32;
        let half_u = face.u_size / 2.0;
        let half_v = face.v_size / 2.0;
        for j in 0..=face.vseg {
            let fv = j as f32 / face.vseg as f32;
            let sv = -half_v + fv * face.v_size;
            for i in 0..=face.useg {
                let fu = i as f32 / face.useg as f32;
                let su = -half_u + fu * face.u_size;
                positions.push([
                    face.n[0] * face.n_half + face.u[0] * su + face.v[0] * sv,
                    face.n[1] * face.n_half + face.u[1] * su + face.v[1] * sv,
                    face.n[2] * face.n_half + face.u[2] * su + face.v[2] * sv,
                ]);
                normals.push(face.n);
                uvs.push([fu, fv]);
            }
        }
        let stride = (face.useg + 1) as u32;
        for j in 0..face.vseg as u32 {
            for i in 0..face.useg as u32 {
                let a = base + j * stride + i;
                let b = base + j * stride + i + 1;
                let c = base + (j + 1) * stride + i + 1;
                let d = base + (j + 1) * stride + i;
                // With N toward the viewer and U right / V up, a is
                // bottom-left: (a,b,c) + (a,c,d) wind CCW.
                indices.extend_from_slice(&[a, b, c, a, c, d]);
            }
        }
    }

    assemble("box", positions, normals, uvs, indices)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn segmented_box_counts() {
        // 2x3x4 segments: faces are (2x3)x2 + (4x3)x2 + (2x4)x2 quads.
        let mesh = generate_box(1.0, 1.0, 1.0, 2, 3, 4);
        let expect_quads = 2 * (2 * 3 + 4 * 3 + 2 * 4);
        assert_eq!(mesh.triangle_count(), expect_quads * 2);
        let expect_verts = 2 * ((3 * 4) + (5 * 4) + (3 * 5));
        assert_eq!(mesh.vertex_count(), expect_verts);
    }

    #[test]
    fn each_face_has_constant_flat_normal() {
        let mesh = generate_box(1.0, 1.0, 1.0, 1, 1, 1);
        let normals = mesh.normals.as_ref().unwrap();
        // 6 faces x 4 vertices, grouped: all 4 in a group share the normal.
        for face in 0..6 {
            let n0 = normals[face * 4];
            for v in 1..4 {
                assert_eq!(normals[face * 4 + v], n0);
            }
            let len = (n0[0] * n0[0] + n0[1] * n0[1] + n0[2] * n0[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-6);
        }
    }
}
