//! Torus generator: a tube swept around Z, lying in the XY plane
//! (the Minimystix/Three.js orientation).

use std::f32::consts::PI;

use super::assemble;
use crate::set::KernelMesh;

/// `radius` is the distance from the torus center to the tube center,
/// `tube` the tube radius. `radial_segments` subdivide the tube
/// cross-section, `tubular_segments` the sweep. Normals are exact
/// (radially out of the tube).
#[must_use]
pub fn generate_torus(
    radius: f32,
    tube: f32,
    radial_segments: u32,
    tubular_segments: u32,
) -> KernelMesh {
    let rs = radial_segments.max(3) as usize;
    let ts = tubular_segments.max(3) as usize;

    let vert_count = (rs + 1) * (ts + 1);
    let mut positions = Vec::with_capacity(vert_count);
    let mut normals = Vec::with_capacity(vert_count);
    let mut uvs = Vec::with_capacity(vert_count);

    for j in 0..=rs {
        // Angle around the tube cross-section.
        let v = j as f32 / rs as f32 * 2.0 * PI;
        let (sin_v, cos_v) = v.sin_cos();
        for i in 0..=ts {
            // Angle around the main axis (Z).
            let u = i as f32 / ts as f32 * 2.0 * PI;
            let (sin_u, cos_u) = u.sin_cos();
            let ring = radius + tube * cos_v;
            positions.push([ring * cos_u, ring * sin_u, tube * sin_v]);
            normals.push([cos_v * cos_u, cos_v * sin_u, sin_v]);
            uvs.push([i as f32 / ts as f32, j as f32 / rs as f32]);
        }
    }

    let mut indices = Vec::with_capacity(rs * ts * 6);
    let stride = (ts + 1) as u32;
    for j in 1..=rs as u32 {
        for i in 1..=ts as u32 {
            let a = j * stride + i - 1;
            let b = (j - 1) * stride + i - 1;
            let c = (j - 1) * stride + i;
            let d = j * stride + i;
            indices.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }

    assemble("torus", positions, normals, uvs, indices)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn outermost_ring_vertex_faces_outward() {
        let mesh = generate_torus(1.0, 0.25, 8, 8);
        // j = 0, i = 0: v = 0, u = 0 -> position (radius + tube, 0, 0),
        // normal +X.
        assert_eq!(mesh.positions[0], [1.25, 0.0, 0.0]);
        let n = mesh.normals.as_ref().unwrap()[0];
        assert!((n[0] - 1.0).abs() < 1e-6);
        assert!(n[1].abs() < 1e-6 && n[2].abs() < 1e-6);
    }

    #[test]
    fn lies_in_the_xy_plane() {
        let mesh = generate_torus(0.5, 0.2, 16, 32);
        let b = mesh.bounds();
        assert!((b.min.z + 0.2).abs() < 1e-4);
        assert!((b.max.z - 0.2).abs() < 1e-4);
    }
}
