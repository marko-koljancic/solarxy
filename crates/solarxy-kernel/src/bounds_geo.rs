//! AABB geometry (the `bounds` node's kernel).
//!
//! `box` emits a solid triangulated box matching the input's AABB; `center`
//! emits a single true point primitive at its center (the catalog's original
//! intent, realized once point topology landed; it supersedes the 0.7.x
//! marker-cube substitute).
//!
//! Callers must handle the empty-input case themselves: `compute_bounds(&[])`
//! returns a (-1,-1,-1)..(1,1,1) unit box rather than a zero box, so blindly
//! boxing an empty set would emit a misleading unit cube. [`bounds_box`] and
//! [`center_point`] take an [`AABB`] and trust it; the node checks
//! `is_renderable_empty` first and warns.

use solarxy_core::AABB;

use crate::primitives::generate_box;
use crate::set::{GeometrySet, KernelMesh};
use crate::transform::bake_transform;

/// A solid box matching `aabb`. Degenerate extents (a flat or empty AABB) are
/// floored to a hairline so the result still has non-zero area and renders.
#[must_use]
pub fn bounds_box(aabb: &AABB) -> GeometrySet {
    let size = aabb.size();
    let (w, h, d) = (
        size.x.max(MIN_EXTENT),
        size.y.max(MIN_EXTENT),
        size.z.max(MIN_EXTENT),
    );
    let center = aabb.center();
    boxed("bounds", w, h, d, [center.x, center.y, center.z])
}

/// A single point primitive at `aabb`'s center (Points topology). The true
/// center marker; its on-screen size is the renderer's uniform point size.
#[must_use]
pub fn center_point(aabb: &AABB) -> GeometrySet {
    let c = aabb.center();
    GeometrySet::from_mesh(KernelMesh::points("bounds_center", vec![[c.x, c.y, c.z]]))
}

/// A hairline floor for degenerate extents: a zero-size box would have no
/// area, no usable normals, and would read as "the node is broken".
const MIN_EXTENT: f32 = 1e-4;

fn boxed(name: &str, w: f32, h: f32, d: f32, center: [f32; 3]) -> GeometrySet {
    let mut mesh: KernelMesh = generate_box(w, h, d, 1, 1, 1);
    mesh.name = name.to_string();
    let set = GeometrySet::from_mesh(mesh);
    // generate_box is origin-centered; slide it onto the AABB's center. The
    // translation is never singular, so the bake cannot fail here.
    bake_transform(&set, &cgmath::Matrix4::from_translation(center.into())).unwrap_or(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::generate_box as gen_box;
    use crate::transform::bake_transform as bake;
    use cgmath::Matrix4;

    #[test]
    fn box_mode_matches_the_input_aabb() {
        // A 2x4x6 box centered at (10, 0, 0).
        let set = GeometrySet::from_mesh(gen_box(2.0, 4.0, 6.0, 1, 1, 1));
        let set = bake(&set, &Matrix4::from_translation([10.0, 0.0, 0.0].into())).unwrap();

        let out = bounds_box(&set.bounds);
        assert_eq!(out.mesh_count(), 1);
        let b = out.bounds;
        assert!((b.min.x - 9.0).abs() < 1e-4, "{b:?}");
        assert!((b.max.x - 11.0).abs() < 1e-4, "{b:?}");
        assert!((b.min.y - -2.0).abs() < 1e-4, "{b:?}");
        assert!((b.max.y - 2.0).abs() < 1e-4, "{b:?}");
        assert!((b.min.z - -3.0).abs() < 1e-4, "{b:?}");
        assert!((b.max.z - 3.0).abs() < 1e-4, "{b:?}");
    }

    #[test]
    fn center_mode_emits_one_point_primitive_at_the_center() {
        let set = GeometrySet::from_mesh(gen_box(2.0, 2.0, 2.0, 1, 1, 1));
        let set = bake(&set, &Matrix4::from_translation([4.0, 6.0, 8.0].into())).unwrap();

        let out = center_point(&set.bounds);
        assert_eq!(out.mesh_count(), 1);
        let mesh = &out.meshes[0];
        assert_eq!(
            mesh.topology,
            solarxy_core::geometry::MeshTopology::Points,
            "center mode is a true point primitive, not a marker cube"
        );
        assert_eq!(mesh.positions.len(), 1);
        let p = mesh.positions[0];
        assert!(
            (p[0] - 4.0).abs() < 1e-4 && (p[1] - 6.0).abs() < 1e-4 && (p[2] - 8.0).abs() < 1e-4
        );
        assert!(!out.is_renderable_empty(), "a lone point renders");
    }

    #[test]
    fn a_flat_aabb_still_produces_renderable_geometry() {
        // A plane has zero thickness on one axis; the box must not collapse.
        let set = GeometrySet::from_mesh(crate::primitives::generate_plane(2.0, 2.0, 1, 1));
        let out = bounds_box(&set.bounds);
        assert!(!out.is_renderable_empty());
        assert!(out.triangle_count() > 0);
    }

    #[test]
    fn the_output_carries_no_material() {
        let set = GeometrySet::from_mesh(gen_box(1.0, 1.0, 1.0, 1, 1, 1));
        let out = bounds_box(&set.bounds);
        assert!(out.materials.is_empty());
        assert_eq!(out.meshes[0].material_index, None);
    }
}
