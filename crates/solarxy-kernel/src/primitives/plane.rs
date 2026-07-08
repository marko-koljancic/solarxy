//! Plane generator: a subdivided rectangle in the XY plane facing `+Z`
//! (the Minimystix/Three.js orientation).

use super::assemble;
use crate::set::KernelMesh;

/// `width` along X, `height` along Y, centered at the origin. UVs run
/// left-to-right, bottom-to-top (`v = 1` at the top edge).
#[must_use]
pub fn generate_plane(
    width: f32,
    height: f32,
    width_segments: u32,
    height_segments: u32,
) -> KernelMesh {
    let wseg = width_segments.max(1) as usize;
    let hseg = height_segments.max(1) as usize;
    let half_w = width / 2.0;
    let half_h = height / 2.0;

    let vert_count = (wseg + 1) * (hseg + 1);
    let mut positions = Vec::with_capacity(vert_count);
    let mut normals = Vec::with_capacity(vert_count);
    let mut uvs = Vec::with_capacity(vert_count);

    // Rows run top (y = +h/2) to bottom so v = 1 - fy maps the top edge
    // to v = 1.
    for iy in 0..=hseg {
        let fy = iy as f32 / hseg as f32;
        let y = half_h - fy * height;
        for ix in 0..=wseg {
            let fx = ix as f32 / wseg as f32;
            positions.push([-half_w + fx * width, y, 0.0]);
            normals.push([0.0, 0.0, 1.0]);
            uvs.push([fx, 1.0 - fy]);
        }
    }

    let mut indices = Vec::with_capacity(wseg * hseg * 6);
    let stride = (wseg + 1) as u32;
    for iy in 0..hseg as u32 {
        for ix in 0..wseg as u32 {
            let a = iy * stride + ix;
            let b = (iy + 1) * stride + ix;
            let c = (iy + 1) * stride + ix + 1;
            let d = iy * stride + ix + 1;
            // CCW viewed from +Z (a top-left, b below it, d to its right).
            indices.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }

    assemble("plane", positions, normals, uvs, indices)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn segmented_plane_counts_and_corner_uvs() {
        let mesh = generate_plane(1.0, 1.0, 4, 2);
        assert_eq!(mesh.vertex_count(), 5 * 3);
        assert_eq!(mesh.triangle_count(), 4 * 2 * 2);

        let uvs = mesh.tex_coords.as_ref().unwrap();
        // First vertex is the top-left corner.
        assert_eq!(mesh.positions[0], [-0.5, 0.5, 0.0]);
        assert_eq!(uvs[0], [0.0, 1.0]);
        // Last vertex is the bottom-right corner.
        let last = mesh.vertex_count() - 1;
        assert_eq!(mesh.positions[last], [0.5, -0.5, 0.0]);
        assert_eq!(uvs[last], [1.0, 0.0]);
    }
}
