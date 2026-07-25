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

/// How a promotion combines the several source values that land on one
/// destination element (a primitive's corner points, or a point's
/// incident primitives).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromoteMethod {
    Average,
    Min,
    Max,
    First,
}

/// The concrete lane type a copy converts into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneType {
    Float,
    Vec2,
    Vec3,
    Vec4,
}

impl LaneType {
    /// The type's enum key, matching the attribute nodes' `type` params
    /// and the lane-summary type strings.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            LaneType::Float => "float",
            LaneType::Vec2 => "vec2",
            LaneType::Vec3 => "vec3",
            LaneType::Vec4 => "vec4",
        }
    }
}

/// The corner points of primitive `prim` under the mesh's own topology:
/// triangle triples, segment pairs, or the point itself for a point
/// cloud. Returns (indices, count).
fn prim_verts(mesh: &KernelMesh, prim: usize) -> ([usize; 3], usize) {
    use solarxy_core::geometry::MeshTopology;
    match mesh.topology {
        MeshTopology::Triangles => {
            let i = prim * 3;
            (
                [
                    mesh.indices[i] as usize,
                    mesh.indices[i + 1] as usize,
                    mesh.indices[i + 2] as usize,
                ],
                3,
            )
        }
        MeshTopology::Lines => {
            let i = prim * 2;
            (
                [mesh.indices[i] as usize, mesh.indices[i + 1] as usize, 0],
                2,
            )
        }
        MeshTopology::Points => ([prim, 0, 0], 1),
    }
}

/// One promoted value per primitive: the combination of the primitive's
/// corner-point values under `method`.
fn point_to_prim_components<const N: usize>(
    mesh: &KernelMesh,
    get: impl Fn(usize) -> [f32; N],
    method: PromoteMethod,
) -> Vec<[f32; N]> {
    (0..mesh.primitive_count())
        .map(|prim| {
            let (vs, n) = prim_verts(mesh, prim);
            let mut acc = get(vs[0]);
            match method {
                PromoteMethod::First => {}
                PromoteMethod::Average => {
                    for &v in vs.iter().take(n).skip(1) {
                        let val = get(v);
                        for c in 0..N {
                            acc[c] += val[c];
                        }
                    }
                    for a in &mut acc {
                        *a /= n as f32;
                    }
                }
                PromoteMethod::Min | PromoteMethod::Max => {
                    for &v in vs.iter().take(n).skip(1) {
                        let val = get(v);
                        for c in 0..N {
                            acc[c] = if method == PromoteMethod::Min {
                                acc[c].min(val[c])
                            } else {
                                acc[c].max(val[c])
                            };
                        }
                    }
                }
            }
            acc
        })
        .collect()
}

/// One promoted value per point: the combination of the point's incident
/// primitives' values under `method`. A point no primitive touches gets
/// zeros (there is nothing honest to write there).
fn prim_to_point_components<const N: usize>(
    mesh: &KernelMesh,
    get: impl Fn(usize) -> [f32; N],
    method: PromoteMethod,
) -> Vec<[f32; N]> {
    let count = mesh.positions.len();
    let mut acc = vec![[0.0f32; N]; count];
    let mut touched = vec![0u32; count];
    for prim in 0..mesh.primitive_count() {
        let (vs, n) = prim_verts(mesh, prim);
        let val = get(prim);
        for &v in vs.iter().take(n) {
            let first_touch = touched[v] == 0;
            touched[v] += 1;
            match method {
                PromoteMethod::Average => {
                    for c in 0..N {
                        acc[v][c] += val[c];
                    }
                }
                PromoteMethod::First => {
                    if first_touch {
                        acc[v] = val;
                    }
                }
                PromoteMethod::Min | PromoteMethod::Max => {
                    if first_touch {
                        acc[v] = val;
                    } else {
                        for c in 0..N {
                            acc[v][c] = if method == PromoteMethod::Min {
                                acc[v][c].min(val[c])
                            } else {
                                acc[v][c].max(val[c])
                            };
                        }
                    }
                }
            }
        }
    }
    if method == PromoteMethod::Average {
        for (a, &t) in acc.iter_mut().zip(&touched) {
            if t > 1 {
                for c in a.iter_mut() {
                    *c /= t as f32;
                }
            }
        }
    }
    acc
}

/// Promotes a point-domain lane to one value per primitive. The source
/// buffer must hold one element per point (the caller guards).
#[must_use]
pub fn promote_point_to_primitive(
    mesh: &KernelMesh,
    data: &AttributeData,
    method: PromoteMethod,
) -> AttributeData {
    match data {
        AttributeData::Float(v) => AttributeData::Float(Arc::new(
            point_to_prim_components(mesh, |i| [v[i]], method)
                .into_iter()
                .map(|[x]| x)
                .collect(),
        )),
        AttributeData::Vec2(v) => {
            AttributeData::Vec2(Arc::new(point_to_prim_components(mesh, |i| v[i], method)))
        }
        AttributeData::Vec3(v) => {
            AttributeData::Vec3(Arc::new(point_to_prim_components(mesh, |i| v[i], method)))
        }
        AttributeData::Vec4(v) => {
            AttributeData::Vec4(Arc::new(point_to_prim_components(mesh, |i| v[i], method)))
        }
    }
}

/// Promotes a primitive-domain lane to one value per point. The source
/// buffer must hold one element per primitive (the caller guards).
#[must_use]
pub fn promote_primitive_to_point(
    mesh: &KernelMesh,
    data: &AttributeData,
    method: PromoteMethod,
) -> AttributeData {
    match data {
        AttributeData::Float(v) => AttributeData::Float(Arc::new(
            prim_to_point_components(mesh, |i| [v[i]], method)
                .into_iter()
                .map(|[x]| x)
                .collect(),
        )),
        AttributeData::Vec2(v) => {
            AttributeData::Vec2(Arc::new(prim_to_point_components(mesh, |i| v[i], method)))
        }
        AttributeData::Vec3(v) => {
            AttributeData::Vec3(Arc::new(prim_to_point_components(mesh, |i| v[i], method)))
        }
        AttributeData::Vec4(v) => {
            AttributeData::Vec4(Arc::new(prim_to_point_components(mesh, |i| v[i], method)))
        }
    }
}

/// Converts a lane to `target`. Same-type conversion is an `Arc` bump
/// (no copy). Widening pads: a float broadcasts to x/y/z, missing
/// components fill with zero, and a vec4 target's w fills with 1.0 (the
/// `color` case: alpha zero would display invisible). Narrowing to float
/// takes the magnitude over every component; other narrowing drops the
/// trailing components.
#[must_use]
pub fn convert_lane(data: &AttributeData, target: LaneType) -> AttributeData {
    fn mag<const N: usize>(v: [f32; N]) -> f32 {
        v.iter().map(|c| c * c).sum::<f32>().sqrt()
    }
    let widen = |v: &AttributeData, i: usize| -> [f32; 4] {
        match v {
            AttributeData::Float(a) => [a[i], a[i], a[i], 1.0],
            AttributeData::Vec2(a) => [a[i][0], a[i][1], 0.0, 1.0],
            AttributeData::Vec3(a) => [a[i][0], a[i][1], a[i][2], 1.0],
            AttributeData::Vec4(a) => a[i],
        }
    };
    let same = matches!(
        (data, target),
        (AttributeData::Float(_), LaneType::Float)
            | (AttributeData::Vec2(_), LaneType::Vec2)
            | (AttributeData::Vec3(_), LaneType::Vec3)
            | (AttributeData::Vec4(_), LaneType::Vec4)
    );
    if same {
        return data.clone();
    }
    let n = data.len();
    match target {
        LaneType::Float => AttributeData::Float(Arc::new(
            (0..n)
                .map(|i| match data {
                    AttributeData::Float(a) => a[i],
                    AttributeData::Vec2(a) => mag(a[i]),
                    AttributeData::Vec3(a) => mag(a[i]),
                    AttributeData::Vec4(a) => mag(a[i]),
                })
                .collect(),
        )),
        LaneType::Vec2 => AttributeData::Vec2(Arc::new(
            (0..n)
                .map(|i| {
                    let w = widen(data, i);
                    [w[0], w[1]]
                })
                .collect(),
        )),
        LaneType::Vec3 => AttributeData::Vec3(Arc::new(
            (0..n)
                .map(|i| {
                    let w = widen(data, i);
                    [w[0], w[1], w[2]]
                })
                .collect(),
        )),
        LaneType::Vec4 => AttributeData::Vec4(Arc::new((0..n).map(|i| widen(data, i)).collect())),
    }
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
    fn promote_point_to_primitive_combines_triangle_corners() {
        // One triangle (0,1,2) with float values 1, 2, 6.
        let mesh = KernelMesh::new(
            "tri",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let lane = AttributeData::Float(Arc::new(vec![1.0, 2.0, 6.0]));
        let get = |d: &AttributeData| match d {
            AttributeData::Float(v) => v[0],
            other => panic!("float expected, got {other:?}"),
        };
        assert_eq!(
            get(&promote_point_to_primitive(
                &mesh,
                &lane,
                PromoteMethod::Average
            )),
            3.0
        );
        assert_eq!(
            get(&promote_point_to_primitive(
                &mesh,
                &lane,
                PromoteMethod::Min
            )),
            1.0
        );
        assert_eq!(
            get(&promote_point_to_primitive(
                &mesh,
                &lane,
                PromoteMethod::Max
            )),
            6.0
        );
        assert_eq!(
            get(&promote_point_to_primitive(
                &mesh,
                &lane,
                PromoteMethod::First
            )),
            1.0
        );
    }

    #[test]
    fn promote_respects_line_and_point_topologies() {
        let line = KernelMesh::polyline(
            "line",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            vec![0, 1, 1, 2],
        );
        let lane = AttributeData::Float(Arc::new(vec![0.0, 4.0, 8.0]));
        let AttributeData::Float(prims) =
            promote_point_to_primitive(&line, &lane, PromoteMethod::Average)
        else {
            panic!("float expected");
        };
        assert_eq!(*prims, vec![2.0, 6.0], "one value per segment");

        let points = KernelMesh::points("pts", vec![[0.0; 3], [1.0, 0.0, 0.0]]);
        let lane = AttributeData::Float(Arc::new(vec![3.0, 5.0]));
        let AttributeData::Float(prims) =
            promote_point_to_primitive(&points, &lane, PromoteMethod::Max)
        else {
            panic!("float expected");
        };
        assert_eq!(*prims, vec![3.0, 5.0], "each point is its own primitive");
    }

    #[test]
    fn promote_primitive_to_point_averages_incident_primitives() {
        // Two triangles sharing the edge 1-2: values 2 and 4. Points 1 and
        // 2 average to 3; points 0 and 3 take their single primitive.
        let mesh = KernelMesh::new(
            "quad",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            vec![0, 1, 2, 1, 3, 2],
        );
        let lane = AttributeData::Float(Arc::new(vec![2.0, 4.0]));
        let AttributeData::Float(pts) =
            promote_primitive_to_point(&mesh, &lane, PromoteMethod::Average)
        else {
            panic!("float expected");
        };
        assert_eq!(*pts, vec![2.0, 3.0, 3.0, 4.0]);
        let AttributeData::Float(firsts) =
            promote_primitive_to_point(&mesh, &lane, PromoteMethod::First)
        else {
            panic!("float expected");
        };
        assert_eq!(*firsts, vec![2.0, 2.0, 2.0, 4.0], "first-touch wins");
    }

    #[test]
    fn promote_primitive_to_point_zeros_orphan_points() {
        // Point 2 belongs to no segment.
        let mut mesh = KernelMesh::polyline(
            "line",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [9.0, 9.0, 9.0]],
            vec![0, 1],
        );
        mesh.primitive_attributes
            .insert("v".into(), AttributeData::Vec2(Arc::new(vec![[1.0, 2.0]])));
        let lane = mesh.primitive_attributes.get("v").unwrap();
        let AttributeData::Vec2(pts) = promote_primitive_to_point(&mesh, lane, PromoteMethod::Max)
        else {
            panic!("vec2 expected");
        };
        assert_eq!(*pts, vec![[1.0, 2.0], [1.0, 2.0], [0.0, 0.0]]);
    }

    #[test]
    fn convert_lane_covers_the_matrix() {
        let vec3 = AttributeData::Vec3(Arc::new(vec![[3.0, 0.0, 4.0]]));
        // Same type: an Arc bump, not a copy.
        let AttributeData::Vec3(same) = convert_lane(&vec3, LaneType::Vec3) else {
            panic!("vec3 expected");
        };
        let AttributeData::Vec3(orig) = &vec3 else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(&same, orig));
        // Narrow to float: the magnitude.
        let AttributeData::Float(m) = convert_lane(&vec3, LaneType::Float) else {
            panic!("float expected");
        };
        assert_eq!(*m, vec![5.0]);
        // Widen to vec4: w pads 1.0 (the color case).
        let AttributeData::Vec4(v4) = convert_lane(&vec3, LaneType::Vec4) else {
            panic!("vec4 expected");
        };
        assert_eq!(*v4, vec![[3.0, 0.0, 4.0, 1.0]]);
        // Float broadcasts to xyz, w = 1.
        let f = AttributeData::Float(Arc::new(vec![0.25]));
        let AttributeData::Vec4(fv4) = convert_lane(&f, LaneType::Vec4) else {
            panic!("vec4 expected");
        };
        assert_eq!(*fv4, vec![[0.25, 0.25, 0.25, 1.0]]);
        // Vec2 pads z with zero; vec4 drops w on the way down.
        let v2 = AttributeData::Vec2(Arc::new(vec![[1.0, 2.0]]));
        let AttributeData::Vec3(v2to3) = convert_lane(&v2, LaneType::Vec3) else {
            panic!("vec3 expected");
        };
        assert_eq!(*v2to3, vec![[1.0, 2.0, 0.0]]);
        let v4 = AttributeData::Vec4(Arc::new(vec![[1.0, 2.0, 3.0, 4.0]]));
        let AttributeData::Vec3(v4to3) = convert_lane(&v4, LaneType::Vec3) else {
            panic!("vec3 expected");
        };
        assert_eq!(*v4to3, vec![[1.0, 2.0, 3.0]]);
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
