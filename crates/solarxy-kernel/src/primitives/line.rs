//! Line generator: a straight polyline between two endpoints, evenly
//! subdivided. The first Lines-topology primitive: no surface, so no
//! normals or UVs; it draws unlit and exists to carry points for deforms
//! and as a wire in its own right.

use crate::set::KernelMesh;

/// A polyline from `start` to `end` with `points` evenly spaced vertices
/// (clamped to at least 2), connected by `points - 1` segments.
#[must_use]
pub fn generate_line(start: [f32; 3], end: [f32; 3], points: u32) -> KernelMesh {
    let count = points.max(2) as usize;
    let mut positions = Vec::with_capacity(count);
    for i in 0..count {
        let t = i as f32 / (count - 1) as f32;
        positions.push([
            start[0] + (end[0] - start[0]) * t,
            start[1] + (end[1] - start[1]) * t,
            start[2] + (end[2] - start[2]) * t,
        ]);
    }
    let mut indices = Vec::with_capacity((count - 1) * 2);
    for i in 0..count - 1 {
        indices.push(i as u32);
        indices.push(i as u32 + 1);
    }
    KernelMesh::polyline("line", positions, indices)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use solarxy_core::geometry::MeshTopology;

    #[test]
    fn endpoints_are_exact_and_interior_evenly_spaced() {
        let mesh = generate_line([0.0, 0.0, 0.0], [0.0, 3.0, 0.0], 4);
        assert_eq!(mesh.topology, MeshTopology::Lines);
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.primitive_count(), 3);
        assert_eq!(mesh.positions[0], [0.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[3], [0.0, 3.0, 0.0]);
        assert!((mesh.positions[1][1] - 1.0).abs() < 1e-6);
        assert!((mesh.positions[2][1] - 2.0).abs() < 1e-6);
        assert_eq!(*mesh.indices, vec![0, 1, 1, 2, 2, 3]);
        assert!(mesh.is_renderable());
    }

    #[test]
    fn the_two_point_minimum_is_a_single_segment() {
        let mesh = generate_line([1.0, 0.0, 0.0], [2.0, 0.0, 0.0], 0);
        assert_eq!(mesh.vertex_count(), 2, "clamped to the minimum");
        assert_eq!(*mesh.indices, vec![0, 1]);
        assert!(mesh.normals.is_none() && mesh.tex_coords.is_none());
    }
}
