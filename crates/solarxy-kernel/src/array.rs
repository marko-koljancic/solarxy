//! Linear and radial duplication (the `array` node's kernel, Phase 15).
//!
//! Both modes are compositions of machinery that already exists: each copy's
//! placement is a [`compose_trs`] matrix, each copy is a [`bake_transform`],
//! and the copies concatenate through [`merge`](crate::merge::merge), whose
//! content-hash material dedup means the N copies of a textured input share
//! one material entry rather than N identical ones.
//!
//! `count` includes the original, so `count == 1` is an identity copy.

use std::sync::Arc;

use crate::error::KernelError;
use crate::merge::merge;
use crate::set::{GeometrySet, KernelMesh};
use crate::transform::{RotateOrder, compose_trs};

/// The output-triangle ceiling, mirroring [`crate::subdivide::MAX_OUTPUT_TRIANGLES`]:
/// a runaway `count` errors before a single copy is allocated rather than
/// stalling the cook or exhausting memory.
pub const MAX_OUTPUT_TRIANGLES: usize = 8_000_000;

/// The axis a radial array revolves about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    X,
    #[default]
    Y,
    Z,
}

impl Axis {
    /// The unit vector along this axis.
    #[must_use]
    pub fn unit(self) -> [f32; 3] {
        match self {
            Axis::X => [1.0, 0.0, 0.0],
            Axis::Y => [0.0, 1.0, 0.0],
            Axis::Z => [0.0, 0.0, 1.0],
        }
    }

    /// The axis a radial copy is offset along before it revolves: the first
    /// coordinate axis that is not this one, in XYZ order. So a Y-axis array
    /// (the common "spin things around the up axis" case) pushes copies out
    /// along +X, and a Z-axis array does too; only an X-axis array uses +Y.
    #[must_use]
    pub fn reference(self) -> [f32; 3] {
        match self {
            Axis::X => [0.0, 1.0, 0.0],
            Axis::Y | Axis::Z => [1.0, 0.0, 0.0],
        }
    }

    /// This axis as a Euler-angle triple carrying `radians` about itself.
    fn euler(self, radians: f32) -> [f32; 3] {
        match self {
            Axis::X => [radians, 0.0, 0.0],
            Axis::Y => [0.0, radians, 0.0],
            Axis::Z => [0.0, 0.0, radians],
        }
    }
}

/// How the copies are placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrayMode {
    /// Copy `i` translates by `i * offset`.
    Linear { offset: [f32; 3] },
    /// Copy `i` sits at `radius` along the axis's reference direction, then
    /// revolves about `axis` by `i * sweep / count`. Orientation follows the
    /// revolution, so the copies fan out like spokes.
    ///
    /// The step is `sweep / count` rather than `sweep / (count - 1)` so that a
    /// full 360-degree sweep tiles evenly without stacking a duplicate copy on
    /// top of the original at the seam.
    Radial {
        axis: Axis,
        radius: f32,
        /// Total sweep, in radians (the node resolves its Degrees param).
        sweep_rad: f32,
    },
}

/// Duplicates `set` `count` times (the original included) and concatenates the
/// copies. Errors when the projected triangle count would exceed
/// [`MAX_OUTPUT_TRIANGLES`], before any copy is allocated.
///
/// # Errors
/// Returns a user-facing message when the output would exceed the ceiling, and
/// [`KernelError::SingularTransform`] can surface from the bake (it cannot in
/// practice here: every placement matrix is a rigid motion).
pub fn array(set: &GeometrySet, count: u32, mode: ArrayMode) -> Result<GeometrySet, String> {
    if count <= 1 {
        return Ok(set.clone());
    }

    let input_tris: usize = set.meshes.iter().map(KernelMesh::triangle_count).sum();
    let projected = input_tris.saturating_mul(count as usize);
    if projected > MAX_OUTPUT_TRIANGLES {
        return Err(format!(
            "array would produce {projected} triangles (over the {MAX_OUTPUT_TRIANGLES} \
             ceiling); lower the count"
        ));
    }

    let mut copies: Vec<Arc<GeometrySet>> = Vec::with_capacity(count as usize);
    for i in 0..count {
        let matrix = placement(i, count, mode);
        let baked = crate::transform::bake_transform(set, &matrix).map_err(|e: KernelError| {
            // Unreachable for the rigid placements above, but a bake error is
            // data, not a panic.
            e.to_string()
        })?;
        copies.push(Arc::new(baked));
    }
    Ok(merge(&copies))
}

/// Copy `i`'s placement matrix.
fn placement(i: u32, count: u32, mode: ArrayMode) -> cgmath::Matrix4<f32> {
    let step = i as f32;
    match mode {
        ArrayMode::Linear { offset } => compose_trs(
            [offset[0] * step, offset[1] * step, offset[2] * step],
            [0.0; 3],
            RotateOrder::default(),
            [1.0; 3],
            [0.0; 3],
        ),
        ArrayMode::Radial {
            axis,
            radius,
            sweep_rad,
        } => {
            let angle = sweep_rad * step / count as f32;
            let r = axis.reference();
            // The copy is pushed out to `radius` along the reference axis, then
            // the whole thing revolves about `axis` through the origin. Feeding
            // the offset as `translate` and the revolution as `rotate` about a
            // zero pivot gives exactly that: compose_trs is T * R * S about the
            // pivot, so R acts on the already-offset copy.
            let offset = [r[0] * radius, r[1] * radius, r[2] * radius];
            let rot = compose_trs(
                [0.0; 3],
                axis.euler(angle),
                RotateOrder::default(),
                [1.0; 3],
                [0.0; 3],
            );
            let out = compose_trs(offset, [0.0; 3], RotateOrder::default(), [1.0; 3], [0.0; 3]);
            rot * out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{generate_box, generate_plane};

    fn unit_box() -> GeometrySet {
        GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1))
    }

    #[test]
    fn count_one_is_an_identity_copy() {
        let set = unit_box();
        let out = array(&set, 1, ArrayMode::Linear { offset: [5.0; 3] }).unwrap();
        assert_eq!(out.mesh_count(), 1);
        assert!((out.bounds.min.x - set.bounds.min.x).abs() < 1e-5);
    }

    #[test]
    fn linear_steps_by_offset_and_grows_the_bounds() {
        let set = unit_box(); // spans -0.5..0.5
        let out = array(
            &set,
            3,
            ArrayMode::Linear {
                offset: [2.0, 0.0, 0.0],
            },
        )
        .unwrap();
        assert_eq!(out.mesh_count(), 3, "one mesh per copy");
        // Copies at x-offsets 0, 2, 4 -> the last spans 3.5..4.5.
        assert!((out.bounds.min.x - -0.5).abs() < 1e-5);
        assert!((out.bounds.max.x - 4.5).abs() < 1e-5);
    }

    #[test]
    fn radial_full_sweep_does_not_duplicate_at_the_seam() {
        // 4 copies over 360 degrees about Y at radius 2 => 0, 90, 180, 270.
        let set = GeometrySet::from_mesh(generate_plane(0.2, 0.2, 1, 1));
        let out = array(
            &set,
            4,
            ArrayMode::Radial {
                axis: Axis::Y,
                radius: 2.0,
                sweep_rad: std::f32::consts::TAU,
            },
        )
        .unwrap();
        assert_eq!(out.mesh_count(), 4);
        // The ring reaches +/-2 (plus the plane's half-extent) on both X and Z:
        // a duplicate at the seam would still pass this, so also check that no
        // two copies share a centroid.
        assert!(out.bounds.max.x > 2.0 && out.bounds.min.x < -2.0);
        assert!(out.bounds.max.z > 2.0 && out.bounds.min.z < -2.0);

        let centroids: Vec<[f32; 3]> = out.meshes.iter().map(centroid).collect();
        for i in 0..centroids.len() {
            for j in (i + 1)..centroids.len() {
                let d = dist(centroids[i], centroids[j]);
                assert!(d > 1e-3, "copies {i} and {j} coincide (seam duplicate)");
            }
        }
    }

    #[test]
    fn radial_orientation_follows_the_revolution() {
        // A single quarter-turn copy about Y: a point at +X lands at -Z
        // (cgmath's right-handed +Y rotation takes +X toward -Z).
        let set = GeometrySet::from_mesh(generate_plane(0.1, 0.1, 1, 1));
        let out = array(
            &set,
            2,
            ArrayMode::Radial {
                axis: Axis::Y,
                radius: 3.0,
                sweep_rad: std::f32::consts::PI, // 2 copies over 180 => step 90
            },
        )
        .unwrap();
        let c0 = centroid(&out.meshes[0]);
        let c1 = centroid(&out.meshes[1]);
        assert!((c0[0] - 3.0).abs() < 1e-4, "copy 0 sits at +X radius");
        assert!(c0[2].abs() < 1e-4);
        assert!(c1[0].abs() < 1e-4, "copy 1 revolved a quarter turn off +X");
        assert!((c1[2].abs() - 3.0).abs() < 1e-4);
    }

    #[test]
    fn the_triangle_ceiling_errors_before_allocating() {
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 200, 200, 200));
        let err = array(&set, 100_000, ArrayMode::Linear { offset: [1.0; 3] }).unwrap_err();
        assert!(err.contains("ceiling"), "got: {err}");
    }

    #[test]
    fn materials_survive_duplication_and_dedup_to_one_entry() {
        let mut set = unit_box();
        set.materials = vec![Arc::new(solarxy_core::geometry::RawMaterialData {
            name: "red".to_string(),
            ..Default::default()
        })];
        set.meshes[0].material_index = Some(0);

        let out = array(
            &set,
            4,
            ArrayMode::Linear {
                offset: [2.0, 0.0, 0.0],
            },
        )
        .unwrap();
        assert_eq!(out.mesh_count(), 4);
        assert_eq!(
            out.materials.len(),
            1,
            "merge dedups the four identical materials by content hash"
        );
        for mesh in &out.meshes {
            assert_eq!(mesh.material_index, Some(0));
        }
    }

    fn centroid(mesh: &KernelMesh) -> [f32; 3] {
        let n = mesh.positions.len() as f32;
        let sum = mesh.positions.iter().fold([0.0f32; 3], |mut a, p| {
            a[0] += p[0];
            a[1] += p[1];
            a[2] += p[2];
            a
        });
        [sum[0] / n, sum[1] / n, sum[2] / n]
    }

    fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }
}
