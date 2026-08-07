//! Meshes the integration tests build hierarchies over.
//!
//! Generated rather than loaded from `res/models/`: the crate has no
//! filesystem access by design, the largest bundled model is a quarter of the
//! size the gates need, and a generated mesh gives the same triangle count on
//! every machine that reproduces a measurement.

// Each integration test binary compiles this module separately, so anything
// only one of them uses reads as dead code in the others.
#![allow(dead_code)]

use cgmath::Point3;
use solarxy_core::aabb::AABB;

/// A UV sphere of unit radius with `width * height * 2` triangles.
///
/// Closed and curved, so it exercises rays that enter and leave, box tests
/// that reject at an angle, and leaves whose bounds overlap. The degenerate
/// triangles at the poles are left in exactly as a real generator emits them.
pub fn sphere(width: u32, height: u32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut positions = Vec::with_capacity(((width + 1) * (height + 1)) as usize);
    for y in 0..=height {
        let phi = (y as f32 / height as f32) * std::f32::consts::PI;
        for x in 0..=width {
            let theta = (x as f32 / width as f32) * std::f32::consts::TAU;
            positions.push([phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()]);
        }
    }
    let stride = width + 1;
    let mut indices = Vec::with_capacity((width * height * 6) as usize);
    for y in 0..height {
        for x in 0..width {
            let a = y * stride + x;
            indices.extend_from_slice(&[a, a + stride, a + 1]);
            indices.extend_from_slice(&[a + 1, a + stride, a + stride + 1]);
        }
    }
    (positions, indices)
}

/// The tightest box around a position buffer.
pub fn bounds_of(positions: &[[f32; 3]]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    AABB {
        min: Point3::new(min[0], min[1], min[2]),
        max: Point3::new(max[0], max[1], max[2]),
    }
}
