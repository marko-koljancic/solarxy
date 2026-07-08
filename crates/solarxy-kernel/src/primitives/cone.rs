//! Cone generator: a cylinder with a zero top radius. Sharing the
//! generator keeps the degenerate-tip handling (skipped tip triangles,
//! sloped tip normals, no top cap) in exactly one place.

use super::generate_cylinder;
use crate::set::KernelMesh;

/// Tip at `(0, +height/2, 0)`, base cap at `-height/2`.
#[must_use]
pub fn generate_cone(
    radius: f32,
    height: f32,
    radial_segments: u32,
    height_segments: u32,
) -> KernelMesh {
    let mut mesh = generate_cylinder(0.0, radius, height, radial_segments, height_segments);
    mesh.name = "cone".to_string();
    mesh
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn cone_is_a_tip_cylinder() {
        let cone = generate_cone(0.5, 1.0, 8, 1);
        let cyl = generate_cylinder(0.0, 0.5, 1.0, 8, 1);
        assert_eq!(cone.vertex_count(), cyl.vertex_count());
        assert_eq!(cone.triangle_count(), cyl.triangle_count());
        assert_eq!(cone.name, "cone");
        // Tip at +h/2, base ring at -h/2.
        assert_eq!(cone.positions[0], [0.0, 0.5, 0.0]);
    }
}
