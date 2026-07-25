//! Circle generator: a closed Lines-topology loop around one coordinate
//! axis. Like [`super::line`] it has no surface, so no normals or UVs; it
//! is the profile source the extrude family will consume, and a wire
//! primitive in its own right.

use crate::array::Axis;
use crate::set::KernelMesh;

/// A closed loop of `segments` points (clamped to at least 3) at `radius`
/// in the plane perpendicular to `axis`, centered on the origin. The loop
/// winds counter-clockwise seen from the positive side of its axis
/// (right-hand rule), matching the frozen orientation conventions.
#[must_use]
pub fn generate_circle(radius: f32, segments: u32, axis: Axis) -> KernelMesh {
    let count = segments.max(3) as usize;
    // The plane's basis follows the cyclic axis order (x -> yz, y -> zx,
    // z -> xy), which is exactly the right-hand CCW winding per axis.
    let (u, v): ([f32; 3], [f32; 3]) = match axis {
        Axis::X => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        Axis::Y => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        Axis::Z => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    };
    let mut positions = Vec::with_capacity(count);
    for i in 0..count {
        let theta = std::f32::consts::TAU * i as f32 / count as f32;
        let (s, c) = theta.sin_cos();
        positions.push([
            radius * (c * u[0] + s * v[0]),
            radius * (c * u[1] + s * v[1]),
            radius * (c * u[2] + s * v[2]),
        ]);
    }
    let mut indices = Vec::with_capacity(count * 2);
    for i in 0..count {
        indices.push(i as u32);
        indices.push(((i + 1) % count) as u32);
    }
    KernelMesh::polyline("circle", positions, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::geometry::MeshTopology;

    fn length(p: [f32; 3]) -> f32 {
        (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
    }

    #[test]
    fn a_default_axis_circle_lies_in_the_ground_plane_at_radius() {
        let mesh = generate_circle(2.0, 32, Axis::Y);
        assert_eq!(mesh.topology, MeshTopology::Lines);
        assert_eq!(mesh.vertex_count(), 32);
        assert_eq!(
            mesh.primitive_count(),
            32,
            "closed: as many segments as points"
        );
        for p in mesh.positions.iter() {
            assert!(p[1].abs() < 1e-6, "Y-axis circle lies in XZ: {p:?}");
            assert!((length(*p) - 2.0).abs() < 1e-5, "on the radius: {p:?}");
        }
    }

    #[test]
    fn the_loop_closes_back_to_the_first_point() {
        let mesh = generate_circle(1.0, 8, Axis::Z);
        let last_pair = &mesh.indices[mesh.indices.len() - 2..];
        assert_eq!(last_pair, &[7, 0], "the final segment closes the loop");
        for p in mesh.positions.iter() {
            assert!(p[2].abs() < 1e-6, "Z-axis circle lies in XY: {p:?}");
        }
    }

    #[test]
    fn each_axis_spans_its_own_plane_and_segments_clamp() {
        let x = generate_circle(1.0, 16, Axis::X);
        for p in x.positions.iter() {
            assert!(p[0].abs() < 1e-6, "X-axis circle lies in YZ: {p:?}");
        }
        let clamped = generate_circle(1.0, 0, Axis::Y);
        assert_eq!(clamped.vertex_count(), 3, "clamped to the triangle minimum");
        assert!(clamped.is_renderable());
    }
}
