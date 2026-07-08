//! UV sphere generator: latitude/longitude grid with poles along Y.

use std::f32::consts::PI;

use super::assemble;
use crate::set::KernelMesh;

/// Poles at `(0, +/-radius, 0)`. `width_segments` is the longitude count
/// (minimum 3), `height_segments` the latitude count (minimum 2). Pole rows
/// duplicate one vertex per column so each column keeps its own UV, and
/// their degenerate triangles are skipped.
#[must_use]
pub fn generate_sphere(radius: f32, width_segments: u32, height_segments: u32) -> KernelMesh {
    let ws = width_segments.max(3) as usize;
    let hs = height_segments.max(2) as usize;

    let vert_count = (ws + 1) * (hs + 1);
    let mut positions = Vec::with_capacity(vert_count);
    let mut normals = Vec::with_capacity(vert_count);
    let mut uvs = Vec::with_capacity(vert_count);

    for iy in 0..=hs {
        let v = iy as f32 / hs as f32;
        // Polar angle from the +Y pole; pole rows snap exactly so the
        // duplicated pole vertices are bit-identical.
        let (sin_theta, cos_theta) = if iy == 0 {
            (0.0, 1.0)
        } else if iy == hs {
            (0.0, -1.0)
        } else {
            (v * PI).sin_cos()
        };
        for ix in 0..=ws {
            let u = ix as f32 / ws as f32;
            // Azimuth from +X toward +Z.
            let phi = u * 2.0 * PI;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let n = [sin_theta * cos_phi, cos_theta, sin_theta * sin_phi];
            positions.push([radius * n[0], radius * n[1], radius * n[2]]);
            normals.push(n);
            uvs.push([u, 1.0 - v]);
        }
    }

    let mut indices = Vec::with_capacity(ws * (2 * hs - 2) * 3);
    let stride = (ws + 1) as u32;
    for iy in 0..hs as u32 {
        for ix in 0..ws as u32 {
            let a = iy * stride + ix;
            let b = iy * stride + ix + 1;
            let c = (iy + 1) * stride + ix + 1;
            let d = (iy + 1) * stride + ix;
            // Top row: a == b positionally (pole), skip that triangle;
            // bottom row: c == d positionally, skip the other.
            if iy != 0 {
                indices.extend_from_slice(&[a, b, c]);
            }
            if iy != hs as u32 - 1 {
                indices.extend_from_slice(&[a, c, d]);
            }
        }
    }

    assemble("sphere", positions, normals, uvs, indices)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn poles_sit_on_the_y_axis() {
        let mesh = generate_sphere(2.0, 8, 4);
        // First row is the +Y pole, last row the -Y pole.
        for ix in 0..=8 {
            assert_eq!(mesh.positions[ix], [0.0, 2.0, 0.0]);
        }
        let last_row = mesh.vertex_count() - 9;
        for ix in 0..=8 {
            assert_eq!(mesh.positions[last_row + ix], [0.0, -2.0, 0.0]);
        }
    }

    #[test]
    fn normals_equal_normalized_positions() {
        let mesh = generate_sphere(3.0, 8, 6);
        let normals = mesh.normals.as_ref().unwrap();
        for (p, n) in mesh.positions.iter().zip(normals.iter()) {
            for k in 0..3 {
                assert!((p[k] / 3.0 - n[k]).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn minimum_segments_are_clamped() {
        let mesh = generate_sphere(1.0, 0, 0);
        // Clamped to 3 x 2.
        assert_eq!(mesh.vertex_count(), 4 * 3);
        assert_eq!(mesh.triangle_count(), 3 * 2);
    }
}
