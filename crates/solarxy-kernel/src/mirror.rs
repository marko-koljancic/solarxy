//! Reflection across an axis-aligned plane (the `mirror` node's kernel).
//!
//! The reflection itself is a [`bake_transform`](crate::transform::bake_transform)
//! with a negative-determinant matrix (still invertible, so it does not trip
//! `SingularTransform`). Two things are worth stating because they are easy to
//! get wrong in opposite directions:
//!
//! - **Normals are already correct and must not be negated.** `bake_transform`
//!   transforms normals by the inverse-transpose; for an orthogonal reflection
//!   `R`, `(R^-1)^T` reduces to `R` itself, so the normals come out correctly
//!   reflected. Negating them again would undo that.
//! - **Winding is not correct and must be fixed.** A negative determinant
//!   reverses triangle orientation, so the index order ends up disagreeing with
//!   those (correct) normals. Swapping two indices per triangle restores the
//!   kernel's frozen invariant: CCW front faces with outward normals.
//!
//! `mirror_tests::flipped_winding_agrees_with_the_transformed_normals` is the
//! test that pins both halves of that.

use std::sync::Arc;

use cgmath::Matrix4;

use crate::merge::merge;
use crate::set::GeometrySet;

/// The plane a mirror reflects across, given as an axis plus an offset along
/// it (so `Axis::X` with offset 2.0 is the plane `x = 2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    #[default]
    X,
    Y,
    Z,
}

/// Reflects `set` across the plane `axis = offset`, optionally keeping the
/// original (which then precedes the reflection in the output).
///
/// # Errors
/// Propagates a bake failure as a user-facing message. A reflection is never
/// singular, so this is defensive rather than reachable.
pub fn mirror(
    set: &GeometrySet,
    axis: Axis,
    offset: f32,
    keep_original: bool,
) -> Result<GeometrySet, String> {
    let reflected = reflect(set, axis, offset)?;
    if !keep_original {
        return Ok(reflected);
    }
    Ok(merge(&[Arc::new(set.clone()), Arc::new(reflected)]))
}

/// The reflection alone, winding fixed.
fn reflect(set: &GeometrySet, axis: Axis, offset: f32) -> Result<GeometrySet, String> {
    // Reflect through `axis = offset`: translate the plane to the origin,
    // negate that axis, translate back. As a single matrix, that is a -1 scale
    // on the axis plus a 2*offset translation along it.
    let (scale, translate) = match axis {
        Axis::X => ([-1.0, 1.0, 1.0], [2.0 * offset, 0.0, 0.0]),
        Axis::Y => ([1.0, -1.0, 1.0], [0.0, 2.0 * offset, 0.0]),
        Axis::Z => ([1.0, 1.0, -1.0], [0.0, 0.0, 2.0 * offset]),
    };
    let matrix = Matrix4::from_translation(translate.into())
        * Matrix4::from_nonuniform_scale(scale[0], scale[1], scale[2]);

    let mut out = crate::transform::bake_transform(set, &matrix).map_err(|e| e.to_string())?;

    // The determinant is negative, so every triangle now reads clockwise in the
    // reflected frame. Swap two indices to restore CCW. Normals are already
    // right (see the module docs) and are deliberately left alone. Winding is
    // a triangle concept: polylines and point clouds reflect by positions
    // alone, and swapping inside their pair/empty index lists would corrupt
    // them.
    for mesh in &mut out.meshes {
        if mesh.topology != solarxy_core::geometry::MeshTopology::Triangles {
            continue;
        }
        let mut indices = (*mesh.indices).clone();
        for tri in indices.chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
        mesh.indices = Arc::new(indices);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{generate_box, generate_plane};
    use crate::set::KernelMesh;

    #[test]
    fn reflection_mirrors_the_bounds_across_the_plane() {
        // A box spanning 1..2 on X, mirrored across x = 0, lands at -2..-1.
        let mut set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        set = crate::transform::bake_transform(
            &set,
            &Matrix4::from_translation([1.5, 0.0, 0.0].into()),
        )
        .unwrap();

        let out = mirror(&set, Axis::X, 0.0, false).unwrap();
        assert!((out.bounds.min.x - -2.0).abs() < 1e-5, "{:?}", out.bounds);
        assert!((out.bounds.max.x - -1.0).abs() < 1e-5, "{:?}", out.bounds);
    }

    /// Reflection maps `x` to `2d - x`, so a box spanning -0.5..0.5 mirrored
    /// across `x = 3` lands at 5.5..6.5 (centered on 6), NOT at 2.5..3.5 -
    /// landing on the plane would be a translation, not a reflection.
    #[test]
    fn a_nonzero_offset_reflects_across_that_plane() {
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let out = mirror(&set, Axis::X, 3.0, false).unwrap();
        assert!((out.bounds.min.x - 5.5).abs() < 1e-5, "{:?}", out.bounds);
        assert!((out.bounds.max.x - 6.5).abs() < 1e-5, "{:?}", out.bounds);
    }

    #[test]
    fn keep_original_merges_both_halves() {
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let out = mirror(&set, Axis::X, 2.0, true).unwrap();
        assert_eq!(out.mesh_count(), 2, "original then reflection");
        // Original at -0.5..0.5, reflection at 3.5..4.5.
        assert!((out.bounds.min.x - -0.5).abs() < 1e-5);
        assert!((out.bounds.max.x - 4.5).abs() < 1e-5);
    }

    /// The invariant this whole module exists to protect: after the flip, the
    /// face normal implied by the triangle's index order must agree with the
    /// mesh's stored (transformed) vertex normals. If we had negated normals as
    /// well as flipping winding, every face here would disagree.
    #[test]
    fn flipped_winding_agrees_with_the_transformed_normals() {
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let out = mirror(&set, Axis::X, 0.0, false).unwrap();
        let mesh = &out.meshes[0];
        let normals = mesh.normals.as_ref().expect("box carries normals");

        for tri in mesh.indices.chunks_exact(3) {
            let (a, b, c) = (
                mesh.positions[tri[0] as usize],
                mesh.positions[tri[1] as usize],
                mesh.positions[tri[2] as usize],
            );
            let face = cross(sub(b, a), sub(c, a));
            // The stored normal at each corner of a box face is the face normal.
            let stored = normals[tri[0] as usize];
            assert!(
                dot(face, stored) > 0.0,
                "winding disagrees with the normal: face {face:?} vs stored {stored:?}"
            );
        }
    }

    #[test]
    fn winding_is_flipped_relative_to_the_input() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let out = mirror(&set, Axis::X, 0.0, false).unwrap();
        let a = &set.meshes[0].indices;
        let b = &out.meshes[0].indices;
        assert_eq!(a[0], b[0]);
        assert_eq!(a[1], b[2], "the swap is (1, 2)");
        assert_eq!(a[2], b[1]);
    }

    #[test]
    fn non_triangle_meshes_reflect_without_index_surgery() {
        let set = GeometrySet::from_parts(
            vec![
                KernelMesh::points("p", vec![[1.0, 0.0, 0.0]]),
                KernelMesh::polyline("l", vec![[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], vec![0, 1]),
            ],
            vec![],
        );
        let out = mirror(&set, Axis::X, 0.0, false).unwrap();
        assert!((out.meshes[0].positions[0][0] - -1.0).abs() < 1e-5);
        assert_eq!(
            *out.meshes[1].indices,
            vec![0, 1],
            "a pair list gets no winding swap"
        );
        assert!((out.meshes[1].positions[1][0] - -2.0).abs() < 1e-5);
        assert_eq!(
            out.meshes[0].topology,
            solarxy_core::geometry::MeshTopology::Points
        );
        assert_eq!(
            out.meshes[1].topology,
            solarxy_core::geometry::MeshTopology::Lines
        );
    }

    #[test]
    fn materials_survive() {
        let mut set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        set.materials = vec![Arc::new(solarxy_core::geometry::RawMaterialData {
            name: "red".to_string(),
            ..Default::default()
        })];
        set.meshes[0].material_index = Some(0);

        let out = mirror(&set, Axis::X, 2.0, true).unwrap();
        assert_eq!(out.materials.len(), 1, "one entry after dedup");
        for mesh in &out.meshes {
            assert_eq!(mesh.material_index, Some(0));
        }
    }

    #[test]
    fn attributes_and_uvs_ride_along() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let vcount = set.meshes[0].vertex_count();
        let out = mirror(&set, Axis::Z, 0.0, false).unwrap();
        let mesh: &KernelMesh = &out.meshes[0];
        assert_eq!(mesh.vertex_count(), vcount);
        assert!(mesh.tex_coords.is_some(), "UVs survive the reflection");
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
    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
}
