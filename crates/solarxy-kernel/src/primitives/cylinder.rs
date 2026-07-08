//! Cylinder generator, also the unified tip machinery for [`generate_cone`]
//! (`crate::primitives::generate_cone`): a zero top or bottom radius
//! collapses that ring to a tip, skips its degenerate torso triangles, and
//! omits its cap. The catalog deliberately allows `radius_top = 0` on the
//! cylinder node (a capped cone), which Minimystix's clamp prevented.

use std::f32::consts::PI;

use super::assemble;
use crate::set::KernelMesh;

/// Runs along Y, centered: top ring at `+height/2`, bottom at `-height/2`.
/// Torso normals lean by the slope `(radius_bottom - radius_top) / height`,
/// so cone tips shade correctly without a special case.
#[must_use]
pub fn generate_cylinder(
    radius_top: f32,
    radius_bottom: f32,
    height: f32,
    radial_segments: u32,
    height_segments: u32,
) -> KernelMesh {
    let rs = radial_segments.max(3) as usize;
    let hs = height_segments.max(1) as usize;
    let half_h = height / 2.0;
    let slope = if height == 0.0 {
        0.0
    } else {
        (radius_bottom - radius_top) / height
    };

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Torso rows from the top ring down.
    let inv_slope_len = 1.0 / (1.0 + slope * slope).sqrt();
    for iy in 0..=hs {
        let v = iy as f32 / hs as f32;
        let y = half_h - v * height;
        let radius = radius_top + v * (radius_bottom - radius_top);
        for ix in 0..=rs {
            let u = ix as f32 / rs as f32;
            let phi = u * 2.0 * PI;
            let (sin_phi, cos_phi) = phi.sin_cos();
            positions.push([radius * sin_phi, y, radius * cos_phi]);
            normals.push([
                sin_phi * inv_slope_len,
                slope * inv_slope_len,
                cos_phi * inv_slope_len,
            ]);
            uvs.push([u, 1.0 - v]);
        }
    }

    let stride = (rs + 1) as u32;
    for iy in 0..hs as u32 {
        for ix in 0..rs as u32 {
            let a = iy * stride + ix;
            let b = (iy + 1) * stride + ix;
            let c = (iy + 1) * stride + ix + 1;
            let d = iy * stride + ix + 1;
            // A collapsed ring makes one triangle of the quad degenerate:
            // (a,b,d) has a == d on a collapsed top, (b,c,d) has b == c on
            // a collapsed bottom.
            if radius_top > 0.0 || iy != 0 {
                indices.extend_from_slice(&[a, b, d]);
            }
            if radius_bottom > 0.0 || iy != hs as u32 - 1 {
                indices.extend_from_slice(&[b, c, d]);
            }
        }
    }

    // Caps: one center vertex plus a duplicated ring (cap normals differ
    // from torso normals, so vertices cannot be shared with the torso).
    let mut build_cap = |radius: f32, top: bool| {
        if radius <= 0.0 {
            return;
        }
        let sign = if top { 1.0 } else { -1.0 };
        let y = sign * half_h;
        let center = positions.len() as u32;
        positions.push([0.0, y, 0.0]);
        normals.push([0.0, sign, 0.0]);
        uvs.push([0.5, 0.5]);
        for ix in 0..=rs {
            let phi = ix as f32 / rs as f32 * 2.0 * PI;
            let (sin_phi, cos_phi) = phi.sin_cos();
            positions.push([radius * sin_phi, y, radius * cos_phi]);
            normals.push([0.0, sign, 0.0]);
            // Radial cap mapping; v mirrored on the bottom so the texture
            // reads unflipped from below.
            uvs.push([0.5 + 0.5 * sin_phi, 0.5 + 0.5 * sign * cos_phi]);
        }
        for ix in 0..rs as u32 {
            let ring = center + 1;
            if top {
                indices.extend_from_slice(&[center, ring + ix, ring + ix + 1]);
            } else {
                indices.extend_from_slice(&[center, ring + ix + 1, ring + ix]);
            }
        }
    };
    build_cap(radius_top, true);
    build_cap(radius_bottom, false);

    assemble("cylinder", positions, normals, uvs, indices)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn capped_cone_via_zero_top_radius() {
        // The degenerate-tip case the catalog newly allows.
        let mesh = generate_cylinder(0.0, 0.5, 1.0, 8, 2);
        // Tip row vertices all collapse onto the +Y tip point.
        for ix in 0..=8 {
            assert_eq!(mesh.positions[ix], [0.0, 0.5, 0.0]);
        }
        // Torso: top row quads contribute 1 triangle, others 2; one cap.
        let torso = 8 + 8 * 2;
        let cap = 8;
        assert_eq!(mesh.triangle_count(), torso + cap);
        // No +Y-facing cap normal at y=+0.5: every normal there leans.
        let normals = mesh.normals.as_ref().unwrap();
        for (p, n) in mesh.positions.iter().zip(normals.iter()) {
            assert!(
                !(p[1] > 0.49 && n[1] > 0.99),
                "found a top-cap normal on a tip-only cone"
            );
        }
    }

    #[test]
    fn torso_normals_lean_by_slope() {
        // radius_bottom > radius_top: surface leans outward-downward...
        let mesh = generate_cylinder(0.2, 0.5, 1.0, 8, 1);
        let normals = mesh.normals.as_ref().unwrap();
        // First torso vertex: phi = 0 -> (sin, slope, cos)/len with
        // slope = (0.5 - 0.2) / 1.
        let slope = 0.3_f32;
        let len = (1.0 + slope * slope).sqrt();
        let n = normals[0];
        assert!((n[0] - 0.0).abs() < 1e-6);
        assert!((n[1] - slope / len).abs() < 1e-6);
        assert!((n[2] - 1.0 / len).abs() < 1e-6);
    }

    #[test]
    fn both_caps_present_for_positive_radii() {
        let mesh = generate_cylinder(0.5, 0.5, 2.0, 6, 1);
        let normals = mesh.normals.as_ref().unwrap();
        let up = normals.iter().filter(|n| n[1] > 0.99).count();
        let down = normals.iter().filter(|n| n[1] < -0.99).count();
        // Each cap: 1 center + 7 ring vertices.
        assert_eq!(up, 8);
        assert_eq!(down, 8);
    }
}
