//! User-facing attribute authoring (the `attribute_create` and
//! `attribute_randomize` node kernels): the first operators that write
//! point-domain lanes on request rather than as a side effect.
//!
//! Both write the point domain of every mesh in the set, replacing any
//! existing lane under the same name. Under a reserved name
//! ([`crate::set::reserved`]) with the contractual type, the written lane
//! feeds the same consumers imports feed: a `color` Vec4 lane displays
//! immediately, which is what makes the attribute system visible in one
//! node.

use std::sync::Arc;

use crate::rng;
use crate::set::{AttributeData, GeometrySet, KernelMesh};

/// The constant a lane is filled with, choosing the lane's type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttributeValue {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
}

/// The per-component bounds seeded uniform values are drawn from,
/// choosing the lane's type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RandomRange {
    Float { min: f32, max: f32 },
    Vec3 { min: [f32; 3], max: [f32; 3] },
    Vec4 { min: [f32; 4], max: [f32; 4] },
}

/// Writes a constant point-domain lane named `name` on every mesh,
/// replacing an existing lane of that name.
#[must_use]
pub fn attribute_create(set: &GeometrySet, name: &str, value: AttributeValue) -> GeometrySet {
    map_meshes(set, |mesh| {
        let count = mesh.positions.len();
        let lane = match value {
            AttributeValue::Float(v) => AttributeData::Float(Arc::new(vec![v; count])),
            AttributeValue::Vec2(v) => AttributeData::Vec2(Arc::new(vec![v; count])),
            AttributeValue::Vec3(v) => AttributeData::Vec3(Arc::new(vec![v; count])),
            AttributeValue::Vec4(v) => AttributeData::Vec4(Arc::new(vec![v; count])),
        };
        (name.to_string(), lane)
    })
}

/// Fills a point-domain lane named `name` with seeded per-point uniform
/// draws inside `range`, replacing an existing lane of that name. The
/// point index runs across the whole set, so every point in a multi-mesh
/// set draws independently, and the same seed always reproduces the same
/// values.
#[must_use]
pub fn attribute_randomize(
    set: &GeometrySet,
    name: &str,
    range: RandomRange,
    seed: u32,
) -> GeometrySet {
    let mut next_index: u64 = 0;
    map_meshes(set, |mesh| {
        let count = mesh.positions.len();
        let base = next_index;
        next_index += count as u64;
        let draw = |i: usize, lane: u32, min: f32, max: f32| -> f32 {
            min + rng::unit_f32(base + i as u64, lane, seed) * (max - min)
        };
        let data = match range {
            RandomRange::Float { min, max } => {
                AttributeData::Float(Arc::new((0..count).map(|i| draw(i, 0, min, max)).collect()))
            }
            RandomRange::Vec3 { min, max } => AttributeData::Vec3(Arc::new(
                (0..count)
                    .map(|i| std::array::from_fn(|c| draw(i, c as u32, min[c], max[c])))
                    .collect(),
            )),
            RandomRange::Vec4 { min, max } => AttributeData::Vec4(Arc::new(
                (0..count)
                    .map(|i| std::array::from_fn(|c| draw(i, c as u32, min[c], max[c])))
                    .collect(),
            )),
        };
        (name.to_string(), data)
    })
}

/// Clones the set with one point-domain lane written per mesh; every
/// other buffer rides by refcount.
fn map_meshes(
    set: &GeometrySet,
    mut lane_for: impl FnMut(&KernelMesh) -> (String, AttributeData),
) -> GeometrySet {
    let meshes = set
        .meshes
        .iter()
        .map(|mesh| {
            let mut out = mesh.clone();
            let (name, data) = lane_for(mesh);
            out.attributes.insert(name, data);
            out
        })
        .collect();
    GeometrySet::from_parts(meshes, set.materials.clone())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::primitives::{generate_box, generate_plane};
    use crate::set::reserved;

    #[test]
    fn create_fills_a_constant_lane_on_every_mesh() {
        let set = GeometrySet::from_parts(
            vec![
                generate_box(1.0, 1.0, 1.0, 1, 1, 1),
                generate_plane(1.0, 1.0, 1, 1),
            ],
            Vec::new(),
        );
        let out = attribute_create(&set, "mass", AttributeValue::Float(2.5));
        for mesh in &out.meshes {
            let Some(AttributeData::Float(lane)) = mesh.attributes.get("mass") else {
                panic!("lane written on {}", mesh.name);
            };
            assert_eq!(lane.len(), mesh.positions.len());
            assert!(lane.iter().all(|&v| v == 2.5));
        }
    }

    #[test]
    fn create_replaces_an_existing_lane_and_leaves_buffers_shared() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let once = attribute_create(&set, reserved::COLOR, AttributeValue::Vec4([1.0; 4]));
        let twice = attribute_create(
            &once,
            reserved::COLOR,
            AttributeValue::Vec4([0.0, 0.0, 0.0, 1.0]),
        );
        let Some(AttributeData::Vec4(lane)) = twice.meshes[0].attributes.get(reserved::COLOR)
        else {
            panic!("lane written");
        };
        assert!(lane.iter().all(|&c| c == [0.0, 0.0, 0.0, 1.0]));
        assert!(
            Arc::ptr_eq(&twice.meshes[0].positions, &set.meshes[0].positions),
            "positions ride by refcount"
        );
    }

    #[test]
    fn a_vec4_color_lane_reaches_the_renderer_contract() {
        let set = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let out = attribute_randomize(
            &set,
            reserved::COLOR,
            RandomRange::Vec4 {
                min: [0.0, 0.0, 0.0, 1.0],
                max: [1.0, 1.0, 1.0, 1.0],
            },
            7,
        );
        let cooked = out.to_cooked();
        let colors = cooked.meshes[0].colors.as_ref().expect("colors crossed");
        assert_eq!(colors.len(), 24);
        for c in colors.iter() {
            assert!((0.0..=1.0).contains(&c[0]) && (0.0..=1.0).contains(&c[1]));
            assert_eq!(c[3], 1.0, "alpha pinned by min = max = 1");
        }
    }

    #[test]
    fn randomize_is_seeded_deterministic_and_per_point_distinct() {
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let range = RandomRange::Float {
            min: -1.0,
            max: 1.0,
        };
        let a = attribute_randomize(&set, "jitter", range, 3);
        let b = attribute_randomize(&set, "jitter", range, 3);
        let c = attribute_randomize(&set, "jitter", range, 4);
        let lane = |s: &GeometrySet| match s.meshes[0].attributes.get("jitter") {
            Some(AttributeData::Float(v)) => Arc::clone(v),
            other => panic!("float lane expected, got {other:?}"),
        };
        assert_eq!(lane(&a), lane(&b));
        assert_ne!(lane(&a), lane(&c));
        let values = lane(&a);
        assert!(values.iter().all(|v| (-1.0..1.0).contains(v)));
        assert!(
            values.windows(2).any(|w| w[0] != w[1]),
            "per-point values differ"
        );
    }

    #[test]
    fn randomize_draws_continue_across_meshes() {
        // Two identical meshes in one set must NOT receive identical
        // lanes: the point index runs across the set.
        let mesh = generate_plane(1.0, 1.0, 1, 1);
        let set = GeometrySet::from_parts(vec![mesh.clone(), mesh], Vec::new());
        let out = attribute_randomize(&set, "v", RandomRange::Float { min: 0.0, max: 1.0 }, 0);
        let lanes: Vec<_> = out
            .meshes
            .iter()
            .map(|m| match m.attributes.get("v") {
                Some(AttributeData::Float(v)) => Arc::clone(v),
                other => panic!("float lane expected, got {other:?}"),
            })
            .collect();
        assert_ne!(lanes[0], lanes[1], "the second mesh continues the stream");
    }
}
