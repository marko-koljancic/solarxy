//! Transform composition and baking (the Houdini SOP model): the transform
//! node composes `M = T * R(order) * S` about a pivot and bakes it into
//! point positions; normals transform by the inverse-transpose. Chained
//! transforms compose naturally because the geometry carries no object
//! transform (that ambiguity from Minimystix's relative
//! `.add()`/`.multiply()` semantics is dissolved by construction).

use cgmath::{InnerSpace, Matrix, Matrix3, Matrix4, Point3, Rad, SquareMatrix, Transform, Vector3};

use crate::error::KernelError;
use crate::set::{GeometrySet, KernelMesh};

/// Euler rotation orders. The mapping is frozen: the name lists the matrix
/// multiplication left to right, `Xyz` composing `Rx * Ry * Rz` (the
/// Three.js `Euler` convention the catalog inherits), so the rightmost
/// axis rotation applies to vectors first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RotateOrder {
    #[default]
    Xyz,
    Xzy,
    Yxz,
    Yzx,
    Zxy,
    Zyx,
}

/// Rotation matrix for Euler angles in **radians** (the graph's param
/// resolver owns the degrees conversion).
#[must_use]
pub fn rotation_matrix(rotate_rad: [f32; 3], order: RotateOrder) -> Matrix3<f32> {
    let rx = Matrix3::from_angle_x(Rad(rotate_rad[0]));
    let ry = Matrix3::from_angle_y(Rad(rotate_rad[1]));
    let rz = Matrix3::from_angle_z(Rad(rotate_rad[2]));
    match order {
        RotateOrder::Xyz => rx * ry * rz,
        RotateOrder::Xzy => rx * rz * ry,
        RotateOrder::Yxz => ry * rx * rz,
        RotateOrder::Yzx => ry * rz * rx,
        RotateOrder::Zxy => rz * rx * ry,
        RotateOrder::Zyx => rz * ry * rx,
    }
}

/// Composes the transform node's matrix:
/// `M = T(translate) * T(pivot) * R(order) * S(scale) * T(-pivot)` —
/// rotation and scale act about `pivot`, then the whole result translates.
/// The node's `uniform_scale` factor is folded into `scale` by the caller.
#[must_use]
pub fn compose_trs(
    translate: [f32; 3],
    rotate_rad: [f32; 3],
    order: RotateOrder,
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Matrix4<f32> {
    let t = Matrix4::from_translation(Vector3::from(translate));
    let p = Matrix4::from_translation(Vector3::from(pivot));
    let r = Matrix4::from(rotation_matrix(rotate_rad, order));
    let s = Matrix4::from_nonuniform_scale(scale[0], scale[1], scale[2]);
    let p_inv = Matrix4::from_translation(-Vector3::from(pivot));
    t * p * r * s * p_inv
}

/// Bakes `matrix` into a set's point positions, transforming normals by
/// the inverse-transpose (correct under non-uniform scale) and
/// renormalizing. Positions and normals get fresh buffers; UVs, indices,
/// extra attributes, and materials ride along by refcount bump.
pub fn bake_transform(
    set: &GeometrySet,
    matrix: &Matrix4<f32>,
) -> Result<GeometrySet, KernelError> {
    let linear = Matrix3::from_cols(
        matrix.x.truncate(),
        matrix.y.truncate(),
        matrix.z.truncate(),
    );
    let normal_matrix = linear
        .invert()
        .ok_or(KernelError::SingularTransform)?
        .transpose();

    let meshes = set
        .meshes
        .iter()
        .map(|mesh| {
            let positions: Vec<[f32; 3]> = mesh
                .positions
                .iter()
                .map(|p| matrix.transform_point(Point3::from(*p)).into())
                .collect();
            let normals = mesh.normals.as_ref().map(|buf| {
                std::sync::Arc::new(
                    buf.iter()
                        .map(|n| {
                            let v = normal_matrix * Vector3::from(*n);
                            if v.magnitude2() > 0.0 {
                                v.normalize().into()
                            } else {
                                *n
                            }
                        })
                        .collect::<Vec<[f32; 3]>>(),
                )
            });
            KernelMesh {
                name: mesh.name.clone(),
                positions: std::sync::Arc::new(positions),
                normals,
                tex_coords: mesh.tex_coords.clone(),
                indices: mesh.indices.clone(),
                material_index: mesh.material_index,
                attributes: mesh.attributes.clone(),
            }
        })
        .collect();

    Ok(GeometrySet::from_parts(meshes, set.materials.clone()))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;
    use std::sync::Arc;

    use super::*;
    use crate::primitives::generate_plane;

    fn assert_vec_eq(a: [f32; 3], b: [f32; 3], tol: f32) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < tol, "{a:?} != {b:?} at [{i}]");
        }
    }

    /// Freezes the order-name -> multiplication mapping: each variant must
    /// equal the explicit product its name spells.
    #[test]
    fn rotate_order_mapping_is_frozen() {
        let angles = [0.3_f32, 0.7, 1.1];
        let rx = Matrix3::from_angle_x(Rad(angles[0]));
        let ry = Matrix3::from_angle_y(Rad(angles[1]));
        let rz = Matrix3::from_angle_z(Rad(angles[2]));
        let cases = [
            (RotateOrder::Xyz, rx * ry * rz),
            (RotateOrder::Xzy, rx * rz * ry),
            (RotateOrder::Yxz, ry * rx * rz),
            (RotateOrder::Yzx, ry * rz * rx),
            (RotateOrder::Zxy, rz * rx * ry),
            (RotateOrder::Zyx, rz * ry * rx),
        ];
        for (order, expect) in cases {
            let got = rotation_matrix(angles, order);
            for c in 0..3 {
                for r in 0..3 {
                    assert!(
                        (got[c][r] - expect[c][r]).abs() < 1e-6,
                        "{order:?} [{c}][{r}]"
                    );
                }
            }
        }
    }

    /// Orders differ observably: (90deg, 0, 90deg) sends +X to +Z under
    /// Xyz (Rz first) but to +Y under Zyx (Rx first).
    #[test]
    fn rotate_orders_are_semantically_distinct() {
        let angles = [FRAC_PI_2, 0.0, FRAC_PI_2];
        let v = Vector3::unit_x();
        let xyz = rotation_matrix(angles, RotateOrder::Xyz) * v;
        let zyx = rotation_matrix(angles, RotateOrder::Zyx) * v;
        assert_vec_eq(xyz.into(), [0.0, 0.0, 1.0], 1e-6);
        assert_vec_eq(zyx.into(), [0.0, 1.0, 0.0], 1e-6);
    }

    #[test]
    fn translate_moves_positions_and_bounds() {
        let set = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        let m = compose_trs(
            [10.0, 0.0, -3.0],
            [0.0; 3],
            RotateOrder::Xyz,
            [1.0; 3],
            [0.0; 3],
        );
        let out = bake_transform(&set, &m).unwrap();
        assert_vec_eq(out.meshes[0].positions[0], [9.0, 1.0, -3.0], 1e-6);
        assert!((out.bounds.min.x - 9.0).abs() < 1e-6);
        assert!((out.bounds.max.x - 11.0).abs() < 1e-6);
    }

    #[test]
    fn rotation_carries_normals() {
        // Plane faces +Z. Rx(90) maps +Z to -Y (y' = -z, z' = y), so the
        // baked normals must follow.
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let m = compose_trs(
            [0.0; 3],
            [FRAC_PI_2, 0.0, 0.0],
            RotateOrder::Xyz,
            [1.0; 3],
            [0.0; 3],
        );
        let out = bake_transform(&set, &m).unwrap();
        let n = out.meshes[0].normals.as_ref().unwrap()[0];
        assert_vec_eq(n, [0.0, -1.0, 0.0], 1e-6);
    }

    /// Under non-uniform scale the inverse-transpose keeps normals unit
    /// and perpendicular to the surface (a plain linear transform would
    /// shear them off-perpendicular).
    #[test]
    fn non_uniform_scale_uses_inverse_transpose() {
        // A plane rotated 45 degrees about Y has normals (s, 0, s)/len.
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let rot = compose_trs(
            [0.0; 3],
            [0.0, std::f32::consts::FRAC_PI_4, 0.0],
            RotateOrder::Xyz,
            [1.0; 3],
            [0.0; 3],
        );
        let tilted = bake_transform(&set, &rot).unwrap();

        // Now squash X by 4: the surface flattens toward the YZ plane, so
        // the true normal swings TOWARD +X, unlike naively transformed
        // normals which would swing away.
        let squash = compose_trs(
            [0.0; 3],
            [0.0; 3],
            RotateOrder::Xyz,
            [0.25, 1.0, 1.0],
            [0.0; 3],
        );
        let out = bake_transform(&tilted, &squash).unwrap();

        let n = out.meshes[0].normals.as_ref().unwrap()[0];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "normal not renormalized: {len}");

        // Perpendicularity to a transformed surface edge.
        let p0 = out.meshes[0].positions[0];
        let p1 = out.meshes[0].positions[1];
        let edge = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let dot = n[0] * edge[0] + n[1] * edge[1] + n[2] * edge[2];
        assert!(dot.abs() < 1e-5, "normal not perpendicular: {dot}");
        // And it swung toward +X (inverse-transpose direction).
        assert!(n[0] > 0.9, "expected normal dominated by +X, got {n:?}");
    }

    #[test]
    fn pivot_point_stays_fixed_under_scale() {
        let set = GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1));
        // Scale x2 about the plane's top-left corner (-1, 1, 0).
        let m = compose_trs(
            [0.0; 3],
            [0.0; 3],
            RotateOrder::Xyz,
            [2.0; 3],
            [-1.0, 1.0, 0.0],
        );
        let out = bake_transform(&set, &m).unwrap();
        // Vertex 0 is that corner: unmoved.
        assert_vec_eq(out.meshes[0].positions[0], [-1.0, 1.0, 0.0], 1e-6);
        // The opposite corner doubled its distance from the pivot.
        let far = out.meshes[0].positions.last().copied().unwrap();
        assert_vec_eq(far, [3.0, -3.0, 0.0], 1e-6);
    }

    #[test]
    fn unshared_buffers_ride_along_by_refcount() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let m = compose_trs(
            [1.0, 0.0, 0.0],
            [0.0; 3],
            RotateOrder::Xyz,
            [1.0; 3],
            [0.0; 3],
        );
        let out = bake_transform(&set, &m).unwrap();
        // Rewritten buffers are fresh; untouched buffers share.
        assert!(!Arc::ptr_eq(
            &out.meshes[0].positions,
            &set.meshes[0].positions
        ));
        assert!(Arc::ptr_eq(
            out.meshes[0].tex_coords.as_ref().unwrap(),
            set.meshes[0].tex_coords.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(&out.meshes[0].indices, &set.meshes[0].indices));
    }

    #[test]
    fn singular_matrix_is_rejected() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let m = compose_trs(
            [0.0; 3],
            [0.0; 3],
            RotateOrder::Xyz,
            [0.0, 1.0, 1.0],
            [0.0; 3],
        );
        assert!(matches!(
            bake_transform(&set, &m),
            Err(KernelError::SingularTransform)
        ));
    }
}
