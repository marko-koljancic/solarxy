//! Torus-knot generator: a tube swept along a (p, q) torus-knot curve.
//!
//! The frame along the curve is the Minimystix/Three.js construction ported
//! exactly (tangent from a small parameter step, seed normal from the
//! chord sum, then two cross products): it is continuous in the curve
//! parameter, so the tube never self-twists between rings. This sampling is
//! part of the executable spec; inventing a different frame (true Frenet,
//! parallel transport) would visibly rotate the tube.

use std::f32::consts::PI;

use cgmath::{InnerSpace, Vector3};

use super::assemble;
use crate::set::KernelMesh;

/// `p` windings around the axis of rotational symmetry, `q` around the
/// torus interior. `radius` scales the curve, `tube` the swept tube.
#[must_use]
pub fn generate_torus_knot(
    radius: f32,
    tube: f32,
    p: u32,
    q: u32,
    tubular_segments: u32,
    radial_segments: u32,
) -> KernelMesh {
    let p = p.max(1) as f32;
    let q = q.max(1) as f32;
    let ts = tubular_segments.max(3) as usize;
    let rs = radial_segments.max(3) as usize;

    // The knot curve (the Three.js parameterization, kept for visual
    // continuity with Minimystix scenes).
    let curve = |u: f32| -> Vector3<f32> {
        let (su, cu) = u.sin_cos();
        let qu_over_p = q / p * u;
        let (sq, cq) = qu_over_p.sin_cos();
        Vector3::new(
            radius * (2.0 + cq) * 0.5 * cu,
            radius * (2.0 + cq) * 0.5 * su,
            radius * sq * 0.5,
        )
    };

    let vert_count = (ts + 1) * (rs + 1);
    let mut positions = Vec::with_capacity(vert_count);
    let mut normals = Vec::with_capacity(vert_count);
    let mut uvs = Vec::with_capacity(vert_count);

    for i in 0..=ts {
        let u = i as f32 / ts as f32 * p * 2.0 * PI;
        let p1 = curve(u);
        let p2 = curve(u + 0.01);

        // The spec frame: tangent, a chord-sum seed normal, then
        // orthogonalize via two crosses.
        let t = p2 - p1;
        let seed = p2 + p1;
        let b = t.cross(seed);
        let n = b.cross(t);
        let b = b.normalize();
        let n = n.normalize();

        for j in 0..=rs {
            let v = j as f32 / rs as f32 * 2.0 * PI;
            let (sin_v, cos_v) = v.sin_cos();
            let cx = -tube * cos_v;
            let cy = tube * sin_v;
            let pos = p1 + n * cx + b * cy;
            positions.push(pos.into());
            let normal = (pos - p1).normalize();
            normals.push(normal.into());
            uvs.push([i as f32 / ts as f32, j as f32 / rs as f32]);
        }
    }

    let mut indices = Vec::with_capacity(ts * rs * 6);
    let stride = (rs + 1) as u32;
    for i in 1..=ts as u32 {
        for j in 1..=rs as u32 {
            let a = (i - 1) * stride + j - 1;
            let b = i * stride + j - 1;
            let c = i * stride + j;
            let d = (i - 1) * stride + j;
            indices.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }

    assemble("torus_knot", positions, normals, uvs, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The no-self-twist invariant: corresponding vertices on adjacent
    /// rings must stay close (a frame flip would displace them by up to a
    /// tube diameter, far beyond one curve step at default resolution).
    #[test]
    fn adjacent_rings_do_not_twist() {
        let tube = 0.2;
        let mesh = generate_torus_knot(0.5, tube, 2, 3, 128, 32);
        let stride = 33;
        let rings = 129;
        for ring in 0..rings - 1 {
            for j in 0..stride {
                let a = mesh.positions[ring * stride + j];
                let b = mesh.positions[(ring + 1) * stride + j];
                let d2 = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
                assert!(
                    d2.sqrt() < tube,
                    "ring {ring} vertex {j} jumps {} (frame twist?)",
                    d2.sqrt()
                );
            }
        }
    }

    #[test]
    fn tube_cross_sections_have_correct_radius() {
        let mesh = generate_torus_knot(0.5, 0.2, 2, 3, 16, 8);
        // Ring 0's center is the curve point at u = 0:
        // (radius * (2 + 1) * 0.5, 0, 0) = (0.75, 0, 0).
        let center = [0.75_f32, 0.0, 0.0];
        for j in 0..=8 {
            let p = mesh.positions[j];
            let d = ((p[0] - center[0]).powi(2)
                + (p[1] - center[1]).powi(2)
                + (p[2] - center[2]).powi(2))
            .sqrt();
            assert!((d - 0.2).abs() < 1e-5, "vertex {j} at distance {d}");
        }
    }
}
