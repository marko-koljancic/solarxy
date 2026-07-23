//! Point-deforming operators (the `displace` node kernel): position
//! rewrites driven by per-point directions and amplitudes. The kernel
//! stays graph-blind: the caller hands closures answering "direction of
//! point i" and "amplitude of point i", however it resolved them (fixed
//! normals, attribute lanes, a constant vector).

use std::sync::Arc;

use crate::set::KernelMesh;

/// Rewrites `mesh`'s positions as `p + dir(i) * amp(i)`, optionally
/// normalizing each direction first. A `None` direction, or a zero-length
/// one under `normalize`, leaves that point in place; the math never
/// produces a NaN from degenerate input. Only the positions buffer is
/// replaced; every other buffer (normals included, which go stale by
/// design) rides on the returned mesh by refcount.
#[must_use]
pub fn displace_mesh(
    mesh: &KernelMesh,
    mut dir_at: impl FnMut(usize) -> Option<[f32; 3]>,
    mut amp_at: impl FnMut(usize) -> f32,
    normalize: bool,
) -> KernelMesh {
    let positions: Vec<[f32; 3]> = mesh
        .positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let Some(dir) = dir_at(i) else {
                return *p;
            };
            let dir = if normalize {
                let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
                if len <= f32::EPSILON {
                    return *p;
                }
                [dir[0] / len, dir[1] / len, dir[2] / len]
            } else {
                dir
            };
            let a = amp_at(i);
            [
                p[0] + dir[0] * a,
                p[1] + dir[1] * a,
                p[2] + dir[2] * a,
            ]
        })
        .collect();
    let mut out = mesh.clone();
    out.positions = Arc::new(positions);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::primitives::generate_plane;

    #[test]
    fn displaces_along_a_constant_vector() {
        let mesh = generate_plane(1.0, 1.0, 1, 1);
        let out = displace_mesh(&mesh, |_| Some([0.0, 2.0, 0.0]), |_| 0.5, true);
        for (a, b) in mesh.positions.iter().zip(out.positions.iter()) {
            assert_eq!(b[1], a[1] + 0.5, "unit direction times amplitude");
            assert_eq!(b[0], a[0]);
            assert_eq!(b[2], a[2]);
        }
    }

    #[test]
    fn unnormalized_directions_scale_with_their_length() {
        let mesh = generate_plane(1.0, 1.0, 1, 1);
        let out = displace_mesh(&mesh, |_| Some([0.0, 2.0, 0.0]), |_| 0.5, false);
        for (a, b) in mesh.positions.iter().zip(out.positions.iter()) {
            assert_eq!(b[1], a[1] + 1.0, "2.0 direction times 0.5 amplitude");
        }
    }

    #[test]
    fn per_point_direction_and_amplitude_apply_by_index() {
        let mesh = generate_plane(1.0, 1.0, 1, 1);
        let out = displace_mesh(
            &mesh,
            |i| Some(if i == 0 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] }),
            |i| i as f32,
            true,
        );
        assert_eq!(out.positions[0], mesh.positions[0], "amplitude 0 holds");
        assert_eq!(out.positions[1][2], mesh.positions[1][2] + 1.0);
        assert_eq!(out.positions[2][2], mesh.positions[2][2] + 2.0);
    }

    #[test]
    fn zero_length_and_missing_directions_leave_points_in_place() {
        let mesh = generate_plane(1.0, 1.0, 1, 1);
        let zeroed = displace_mesh(&mesh, |_| Some([0.0, 0.0, 0.0]), |_| 5.0, true);
        assert_eq!(*zeroed.positions, *mesh.positions);
        assert!(zeroed.positions.iter().flatten().all(|v| v.is_finite()));
        let missing = displace_mesh(&mesh, |_| None, |_| 5.0, true);
        assert_eq!(*missing.positions, *mesh.positions);
    }

    #[test]
    fn untouched_buffers_ride_by_refcount() {
        let mesh = generate_plane(1.0, 1.0, 2, 2);
        let out = displace_mesh(&mesh, |_| Some([0.0, 1.0, 0.0]), |_| 1.0, true);
        assert!(Arc::ptr_eq(&out.indices, &mesh.indices));
        assert!(!Arc::ptr_eq(&out.positions, &mesh.positions));
    }
}
