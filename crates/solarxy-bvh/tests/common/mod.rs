//! What the integration tests need that the library deliberately does not
//! expose.
//!
//! The meshes and the ray corpus moved into `solarxy_bvh::corpus`, because the
//! WGSL comparison lives in another crate and has to draw from the same one.
//! What stays here is the bridge to `solarxy_core::raycast`, which speaks
//! `cgmath` types this crate does not take as a dependency.

// Each integration test binary compiles this module separately, so anything
// only one of them uses reads as dead code in the others.
#![allow(dead_code)]

use cgmath::Point3;
use solarxy_core::aabb::AABB;

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

/// The same box, transformed by a column-major matrix and re-fitted.
///
/// The raycaster tests a mesh's bounds before its triangles, so a transformed
/// instance needs the box its transformed positions actually occupy.
pub fn transformed_bounds(positions: &[[f32; 3]], world: &[[f32; 4]; 4]) -> AABB {
    let moved: Vec<[f32; 3]> = positions
        .iter()
        .map(|p| {
            [
                world[0][0] * p[0] + world[1][0] * p[1] + world[2][0] * p[2] + world[3][0],
                world[0][1] * p[0] + world[1][1] * p[1] + world[2][1] * p[2] + world[3][1],
                world[0][2] * p[0] + world[1][2] * p[1] + world[2][2] * p[2] + world[3][2],
            ]
        })
        .collect();
    bounds_of(&moved)
}
