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

/// Extracts Euler angles in **radians** from a pure rotation matrix: the exact
/// inverse of [`rotation_matrix`] for the same `order`.
///
/// `m` must be orthonormal. A matrix still carrying scale decomposes to
/// nonsense, so a caller holding a `T * R * S` basis divides the scale out
/// first.
///
/// At the gimbal pole (the middle rotation at +/- 90 degrees) the two outer
/// angles stop being independently recoverable: only their sum or difference is
/// determined. The convention there, inherited from the same Three.js `Euler`
/// this module's order naming follows, is to fold the free rotation into one
/// angle and zero the other. Feeding the result back through
/// [`rotation_matrix`] still reproduces the input matrix, which is the property
/// the gizmo actually depends on.
#[must_use]
pub fn decompose_rotation(m: Matrix3<f32>, order: RotateOrder) -> [f32; 3] {
    // How close the middle angle's sine may get to 1 before the outer pair
    // degenerates and the pole branch has to take over.
    const POLE: f32 = 0.999_999_5;

    // cgmath is column-major (`m.<col>.<row>`), so the textbook `m<row><col>`
    // names are unpacked once here. Keeping the textbook names makes the six
    // arms below checkable line-by-line against any Euler reference.
    let (m11, m12, m13) = (m.x.x, m.y.x, m.z.x);
    let (m21, m22, m23) = (m.x.y, m.y.y, m.z.y);
    let (m31, m32, m33) = (m.x.z, m.y.z, m.z.z);

    let (x, y, z) = match order {
        RotateOrder::Xyz => {
            let y = m13.clamp(-1.0, 1.0).asin();
            if m13.abs() < POLE {
                ((-m23).atan2(m33), y, (-m12).atan2(m11))
            } else {
                (m32.atan2(m22), y, 0.0)
            }
        }
        RotateOrder::Xzy => {
            let z = (-m12).clamp(-1.0, 1.0).asin();
            if m12.abs() < POLE {
                (m32.atan2(m22), m13.atan2(m11), z)
            } else {
                ((-m23).atan2(m33), 0.0, z)
            }
        }
        RotateOrder::Yxz => {
            let x = (-m23).clamp(-1.0, 1.0).asin();
            if m23.abs() < POLE {
                (x, m13.atan2(m33), m21.atan2(m22))
            } else {
                (x, (-m31).atan2(m11), 0.0)
            }
        }
        RotateOrder::Yzx => {
            let z = m21.clamp(-1.0, 1.0).asin();
            if m21.abs() < POLE {
                ((-m23).atan2(m22), (-m31).atan2(m11), z)
            } else {
                (0.0, m13.atan2(m33), z)
            }
        }
        RotateOrder::Zxy => {
            let x = m32.clamp(-1.0, 1.0).asin();
            if m32.abs() < POLE {
                (x, (-m31).atan2(m33), (-m12).atan2(m22))
            } else {
                (x, 0.0, m21.atan2(m11))
            }
        }
        RotateOrder::Zyx => {
            let y = (-m31).clamp(-1.0, 1.0).asin();
            if m31.abs() < POLE {
                (m32.atan2(m33), y, m21.atan2(m11))
            } else {
                (0.0, y, (-m12).atan2(m22))
            }
        }
    };
    [x, y, z]
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
                topology: mesh.topology,
                attributes: mesh.attributes.clone(),
                primitive_attributes: mesh.primitive_attributes.clone(),
                instances: None,
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

    #[test]
    fn a_point_cloud_bakes_positions_like_any_mesh() {
        let set =
            GeometrySet::from_mesh(crate::set::KernelMesh::points("p", vec![[1.0, 0.0, 0.0]]));
        let m = compose_trs(
            [0.0, 2.0, 0.0],
            [0.0; 3],
            RotateOrder::default(),
            [1.0; 3],
            [0.0; 3],
        );
        let out = bake_transform(&set, &m).unwrap();
        assert_vec_eq(out.meshes[0].positions[0], [1.0, 2.0, 0.0], 1e-5);
        assert_eq!(
            out.meshes[0].topology,
            solarxy_core::geometry::MeshTopology::Points,
            "the bake is topology-agnostic and keeps the tag"
        );
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

    const ALL_ORDERS: [RotateOrder; 6] = [
        RotateOrder::Xyz,
        RotateOrder::Xzy,
        RotateOrder::Yxz,
        RotateOrder::Yzx,
        RotateOrder::Zxy,
        RotateOrder::Zyx,
    ];

    fn assert_mat_eq(a: Matrix3<f32>, b: Matrix3<f32>, tol: f32, what: &str) {
        for c in 0..3 {
            for r in 0..3 {
                assert!(
                    (a[c][r] - b[c][r]).abs() < tol,
                    "{what}: [{c}][{r}] {} != {}",
                    a[c][r],
                    b[c][r]
                );
            }
        }
    }

    /// The property the rotate gizmo stands on: decomposing a rotation matrix
    /// gives back the angles that built it, for every order. Swept over a grid
    /// rather than a lucky triple, because a transposed index or a swapped
    /// `atan2` argument survives a single sample far too easily.
    #[test]
    fn decompose_rotation_inverts_rotation_matrix() {
        // All well clear of +/- 90 degrees, so no order's MIDDLE angle (which is
        // the one that gimbals) lands on the pole. The pole has its own test.
        let grid = [-1.2_f32, -0.5, 0.0, 0.3, 1.1];
        for order in ALL_ORDERS {
            for x in grid {
                for y in grid {
                    for z in grid {
                        let angles = [x, y, z];
                        let m = rotation_matrix(angles, order);
                        let back = decompose_rotation(m, order);
                        assert_vec_eq(back, angles, 1e-3);
                        // The stronger statement, and the one that actually
                        // matters: the angles rebuild the same orientation.
                        assert_mat_eq(rotation_matrix(back, order), m, 1e-5, "round-trip");
                    }
                }
            }
        }
    }

    /// At the pole the outer pair collapses (only their sum is determined), so
    /// the ANGLES cannot round-trip and asserting that they do would be wrong.
    /// The matrix still must, and that is what the gizmo consumes.
    #[test]
    fn decompose_rotation_round_trips_the_matrix_at_the_gimbal_pole() {
        for order in ALL_ORDERS {
            for middle in [FRAC_PI_2, -FRAC_PI_2] {
                // Whichever lane is the middle one for this order, driving all
                // three to the pole angle guarantees we hit it.
                for angles in [
                    [middle, middle, middle],
                    [0.4, middle, 0.9],
                    [middle, 0.4, middle],
                ] {
                    let m = rotation_matrix(angles, order);
                    let back = decompose_rotation(m, order);
                    assert_mat_eq(
                        rotation_matrix(back, order),
                        m,
                        1e-5,
                        &format!("{order:?} at the pole"),
                    );
                }
            }
        }
    }

    /// A geo's basis is `R * S`; the gizmo divides the scale out before
    /// decomposing. Proves that recovers the rotation exactly, which is what
    /// makes local-orientation handles land on the object's real axes.
    #[test]
    fn a_scaled_basis_decomposes_once_the_scale_is_divided_out() {
        let angles = [0.3_f32, -0.8, 1.1];
        let r = rotation_matrix(angles, RotateOrder::Xyz);
        let scale = [3.0_f32, 0.5, 2.0];
        let scaled = r * Matrix3::from_diagonal(Vector3::from(scale));

        // Each column carries one scale factor; normalizing them recovers R.
        let normalized = Matrix3::from_cols(
            scaled.x.normalize(),
            scaled.y.normalize(),
            scaled.z.normalize(),
        );
        assert_vec_eq(
            decompose_rotation(normalized, RotateOrder::Xyz),
            angles,
            1e-4,
        );
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
