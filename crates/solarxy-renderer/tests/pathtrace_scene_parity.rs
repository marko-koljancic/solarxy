//! The traced scene against the raster one, over the same delta stream.
//!
//! Both consumers read `SceneDelta` and neither knows about the other, so the
//! only thing keeping them describing one scene is a test that feeds them the
//! same ops and compares what they built. The failure this guards is quiet:
//! a traced still that is missing an object the viewport shows, or that has one
//! at a position the viewport moved it away from.
//!
//! The two legitimately differ in three ways, and naming all three is the point
//! of the comparison rather than an apology for it.
//!
//! 1. The raster path keeps an identity row in an object's instance buffer when
//!    the object would otherwise contribute none, because wgpu panics on a
//!    zero-length buffer slice. That row is a buffer artefact no mesh draws, so
//!    it is invisible to a per-mesh placement count and is not compared.
//! 2. The raster path keeps polylines and point clouds; the tracer takes
//!    triangles only. That difference is asserted to equal the reported skip
//!    counts rather than waved through.
//! 3. Both keep invisible objects, with a flag rather than by removal. That is
//!    not a divergence, so it is asserted.
//!
//! The script ends with a real traversal. Every CPU assertion here would still
//! pass with a bind group left pointing at the previous allocation after a
//! buffer grew, because nothing on the CPU side can see that; only rays can.

mod common;

use std::sync::Arc;

use cgmath::{Matrix4, SquareMatrix};
use solarxy_core::geometry::MeshTopology;
use solarxy_core::scene::{
    CookedGeometry, CookedMesh, InstanceXform, SceneDelta, SceneObjectId, SceneOp,
};
use solarxy_renderer::pathtrace::probe::{CorpusHit, CorpusRay, HitPoll, TraversalProbe};
use solarxy_renderer::pathtrace::scene::TraceSceneCache;
use solarxy_renderer::pathtrace::{TraceScene, probe::HitReadback};
use solarxy_renderer::scene_objects::SceneObjects;

/// What both consumers must agree about, per object.
#[derive(Debug, PartialEq)]
struct ObjectFingerprint {
    id: u64,
    transform: [[f32; 4]; 4],
    visible: bool,
    cast_shadow: bool,
    /// One entry per triangle mesh, in cooked order: its cooked index and how
    /// many placements it carries.
    meshes: Vec<(u32, u32)>,
}

fn cube(scale: f32) -> (Vec<[f32; 3]>, Vec<u32>) {
    let s = scale;
    let positions = vec![
        [-s, -s, -s],
        [s, -s, -s],
        [s, s, -s],
        [-s, s, -s],
        [-s, -s, s],
        [s, -s, s],
        [s, s, s],
        [-s, s, s],
    ];
    let indices = vec![
        0, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6, 0, 4, 5, 0, 5, 1, 3, 2, 6, 3, 6, 7, 0, 3, 7, 0, 7, 4,
        1, 5, 6, 1, 6, 2,
    ];
    (positions, indices)
}

fn mesh(
    name: &str,
    topology: MeshTopology,
    scale: f32,
    placements: Option<Vec<InstanceXform>>,
) -> CookedMesh {
    let (positions, indices) = cube(scale);
    let indices = match topology {
        MeshTopology::Points => Vec::new(),
        _ => indices,
    };
    CookedMesh {
        name: name.into(),
        positions: Arc::new(positions),
        normals: None,
        tex_coords: None,
        indices: Arc::new(indices),
        material_index: None,
        topology,
        colors: None,
        instances: placements.map(Arc::new),
    }
}

fn geometry(meshes: Vec<CookedMesh>) -> Arc<CookedGeometry> {
    let mut bounds = solarxy_core::aabb::AABB {
        min: cgmath::Point3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
        max: cgmath::Point3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
    };
    for m in &meshes {
        for p in m.positions.iter() {
            bounds.min.x = bounds.min.x.min(p[0]);
            bounds.min.y = bounds.min.y.min(p[1]);
            bounds.min.z = bounds.min.z.min(p[2]);
            bounds.max.x = bounds.max.x.max(p[0]);
            bounds.max.y = bounds.max.y.max(p[1]);
            bounds.max.z = bounds.max.z.max(p[2]);
        }
    }
    Arc::new(CookedGeometry {
        meshes,
        materials: Vec::new(),
        bounds,
    })
}

fn translation(x: f32) -> InstanceXform {
    let mut m = InstanceXform::IDENTITY;
    m.0[3] = [x, 0.0, 0.0, 1.0];
    m
}

fn raster_fingerprints(objects: &SceneObjects) -> Vec<ObjectFingerprint> {
    objects
        .iter()
        .map(|(id, obj)| {
            let remap = objects.raw_to_gpu(*id).unwrap_or(&[]);
            let meshes = remap
                .iter()
                .enumerate()
                .filter_map(|(cooked, gpu)| {
                    let gpu = (*gpu)?;
                    let m = &obj.model.meshes[gpu];
                    (m.topology == MeshTopology::Triangles)
                        .then_some((u32::try_from(cooked).unwrap_or(u32::MAX), m.instance_count))
                })
                .collect();
            ObjectFingerprint {
                id: id.0,
                transform: obj.transform.into(),
                visible: obj.visible,
                cast_shadow: obj.cast_shadow,
                meshes,
            }
        })
        .collect()
}

fn traced_fingerprints(cache: &TraceSceneCache) -> Vec<ObjectFingerprint> {
    cache
        .iter()
        .map(|(id, obj)| ObjectFingerprint {
            id: id.0,
            transform: obj.transform.into(),
            visible: obj.visible,
            cast_shadow: obj.cast_shadow,
            meshes: obj.meshes().collect(),
        })
        .collect()
}

/// How many drawable meshes the raster path holds, of any topology.
fn raster_drawable_meshes(objects: &SceneObjects) -> u32 {
    objects
        .iter()
        .map(|(_, obj)| u32::try_from(obj.model.meshes.len()).unwrap_or(u32::MAX))
        .sum()
}

fn spin(device: &wgpu::Device, readback: &mut HitReadback) -> Vec<CorpusHit> {
    for _ in 0..2000 {
        match readback.poll(device) {
            HitPoll::Ready(hits) => return hits,
            HitPoll::Failed => panic!("probe readback failed"),
            // Yield, which the sibling harnesses all do and this one did not.
            // A non-blocking poll returns in microseconds, so a bare loop is a
            // busy-wait that races the GPU rather than waiting for it: ten
            // thousand iterations came and went in under twenty milliseconds
            // and the test reported a readback that "never completed" when it
            // had simply not been given time to.
            HitPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
        }
    }
    panic!("probe readback never completed");
}

#[test]
fn both_consumers_describe_the_same_scene_across_a_delta_script() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    let mut objects = SceneObjects::new();
    let mut cache = TraceSceneCache::new();
    let mut scene = TraceScene::new(&gpu.device, &gpu.pathtrace);

    let plain = geometry(vec![mesh("solid", MeshTopology::Triangles, 1.0, None)]);
    let instanced = geometry(vec![mesh(
        "copies",
        MeshTopology::Triangles,
        0.5,
        Some(vec![translation(0.0), translation(4.0), translation(8.0)]),
    )]);
    let mixed = geometry(vec![
        mesh("solid", MeshTopology::Triangles, 1.0, None),
        mesh("wire", MeshTopology::Lines, 1.0, None),
        mesh("dots", MeshTopology::Points, 1.0, None),
    ]);

    // Each batch is compared on its own, rather than only at the end, so an op
    // that diverges fails at the step that introduced it.
    let script: Vec<(&str, SceneDelta)> = vec![
        (
            "one plain object",
            SceneDelta {
                ops: vec![SceneOp::UpsertGeometry {
                    id: SceneObjectId(1),
                    geometry: Arc::clone(&plain),
                }],
            },
        ),
        (
            "an instanced object beside it",
            SceneDelta {
                ops: vec![SceneOp::UpsertGeometry {
                    id: SceneObjectId(2),
                    geometry: Arc::clone(&instanced),
                }],
            },
        ),
        (
            "moving the instanced one",
            SceneDelta {
                ops: vec![SceneOp::SetTransform {
                    id: SceneObjectId(2),
                    transform: Matrix4::from_translation(cgmath::Vector3::new(0.0, 3.0, 0.0))
                        .into(),
                }],
            },
        ),
        (
            "a mixed-topology object, which the two treat differently",
            SceneDelta {
                ops: vec![SceneOp::UpsertGeometry {
                    id: SceneObjectId(3),
                    geometry: Arc::clone(&mixed),
                }],
            },
        ),
        (
            "hiding one and clearing another's shadow flag",
            SceneDelta {
                ops: vec![
                    SceneOp::SetVisible {
                        id: SceneObjectId(1),
                        visible: false,
                    },
                    SceneOp::SetCastShadow {
                        id: SceneObjectId(3),
                        cast_shadow: false,
                    },
                ],
            },
        ),
        (
            "replacing the instanced object's geometry with a plain one",
            SceneDelta {
                ops: vec![SceneOp::UpsertGeometry {
                    id: SceneObjectId(2),
                    geometry: Arc::clone(&plain),
                }],
            },
        ),
        (
            "removing an object, which shrinks the arena",
            SceneDelta {
                ops: vec![SceneOp::Remove {
                    id: SceneObjectId(3),
                }],
            },
        ),
        (
            "an op neither consumer packs",
            SceneDelta {
                ops: vec![SceneOp::SetValidation {
                    id: SceneObjectId(1),
                    validation: None,
                }],
            },
        ),
    ];

    for (step, delta) in &script {
        objects
            .apply(&gpu.device, &gpu.queue, &gpu.layouts, delta)
            .unwrap_or_else(|e| panic!("{step}: raster apply failed: {e}"));
        cache.apply(delta);
        if let Some(arena) = cache.repack() {
            scene.sync(&gpu.device, &gpu.queue, &gpu.pathtrace, arena);
        }

        assert_eq!(
            raster_fingerprints(&objects),
            traced_fingerprints(&cache),
            "{step}: the two consumers disagree about the scene"
        );

        let stats = cache.stats();
        // Against the traced meshes *before* the pack dedupes them, not
        // against `stats.meshes`, which counts distinct packed vertex ranges:
        // two objects displaying one prototype share a range and would look
        // like a mesh the tracer had dropped.
        let traced = traced_fingerprints(&cache);
        let traced_meshes: u32 = traced
            .iter()
            .map(|f| u32::try_from(f.meshes.len()).unwrap_or(u32::MAX))
            .sum();
        assert_eq!(
            raster_drawable_meshes(&objects) - traced_meshes,
            stats.skipped_lines + stats.skipped_points,
            "{step}: the mesh difference is not accounted for by the skip counts"
        );
        assert!(
            stats.meshes <= traced_meshes,
            "{step}: dedupe cannot produce more packed ranges than there are meshes"
        );

        let expected_instances: u32 = traced
            .iter()
            .flat_map(|f| f.meshes.iter().map(|(_, count)| *count))
            .sum();
        assert_eq!(
            u32::try_from(cache.arena().instances().len()).unwrap_or(u32::MAX),
            expected_instances - stats.singular_placements,
            "{step}: packed instances do not match the placements the objects carry"
        );

        let arena = cache.arena();
        for instance in arena.instances() {
            assert!(
                (instance.bvh_root as usize) < arena.nodes().len()
                    && (instance.prim_base as usize) < arena.prim_indices().len()
                    && (instance.index_base as usize) < arena.prim_indices().len()
                    && (instance.vertex_base as usize) < arena.vertex_pos().len(),
                "{step}: an instance base points outside its buffer: {instance:?}"
            );
        }
        assert_eq!(scene.instance_count() as usize, arena.instances().len());
    }

    // The traversal, last. Nothing above this line could tell that the bind
    // group still points at a buffer some earlier reallocation replaced: every
    // CPU assertion reads the arena, and the arena is right either way.
    //
    // Two unit cubes survive the script. The one at the origin was hidden at
    // step five and the one three units up was not, so this also shows the
    // visibility flag reaching the kernel rather than stopping at the arena.
    let probe = TraversalProbe::new(&gpu.device, &gpu.pathtrace.scene);
    let down_z = |y: f32| CorpusRay {
        origin: [0.0, y, 10.0, 0.0],
        direction: [0.0, 0.0, -1.0, 0.0],
    };
    let rays = vec![down_z(0.0), down_z(3.0), down_z(50.0)];

    let mut readback = probe.submit(&gpu.device, &gpu.queue, &scene, &rays);
    let hits = spin(&gpu.device, &mut readback);
    assert!(!hits[0].hit(), "the hidden object must not be traced");
    assert!(hits[1].hit(), "the visible object should be hit");
    assert!((hits[1].t - 9.0).abs() < 1e-3, "hit at {}", hits[1].t);
    assert!(!hits[2].hit(), "a ray through empty space must miss");

    // And back the other way, so the miss above is the flag rather than an
    // object that never made it into the arena at all.
    let show = SceneDelta {
        ops: vec![SceneOp::SetVisible {
            id: SceneObjectId(1),
            visible: true,
        }],
    };
    cache.apply(&show);
    let arena = cache.repack().expect("showing an object is a change");
    scene.sync(&gpu.device, &gpu.queue, &gpu.pathtrace, arena);

    let mut readback = probe.submit(&gpu.device, &gpu.queue, &scene, &rays);
    let hits = spin(&gpu.device, &mut readback);
    assert!(
        hits[0].hit(),
        "the object should be traced once shown again"
    );
    assert!((hits[0].t - 9.0).abs() < 1e-3, "hit at {}", hits[0].t);
    assert!(hits[1].hit(), "the other object is still there");
    assert!(!hits[2].hit(), "a ray through empty space must still miss");
}

#[test]
fn an_identity_transform_survives_both_consumers_unchanged() {
    // The composition is `transform * placement` on both sides. An object left
    // at the identity is the case where a transposed matrix would still look
    // right, so it is asserted separately from the moved objects above.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let mut objects = SceneObjects::new();
    let mut cache = TraceSceneCache::new();

    let delta = SceneDelta {
        ops: vec![SceneOp::UpsertGeometry {
            id: SceneObjectId(7),
            geometry: geometry(vec![mesh("solid", MeshTopology::Triangles, 1.0, None)]),
        }],
    };
    objects
        .apply(&gpu.device, &gpu.queue, &gpu.layouts, &delta)
        .expect("raster apply");
    cache.apply(&delta);
    cache.repack();

    let identity: [[f32; 4]; 4] = Matrix4::identity().into();
    assert_eq!(cache.arena().instances()[0].world, identity);
    assert_eq!(cache.arena().instances()[0].inv_world, identity);
    assert_eq!(raster_fingerprints(&objects), traced_fingerprints(&cache));
}
