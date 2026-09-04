//! The WGSL traversal against its CPU twin, over one shared ray corpus.
//!
//! This is the third and last link of a chain. `solarxy_core::raycast` is the
//! brute-force intersector the picking path has shipped since the web
//! milestone. `solarxy_bvh` writes the hierarchy traversal in Rust and pins it
//! to that. The WGSL kernel is written as a twin of the Rust one, and this
//! pins it to that. Break any link and a path tracer renders a plausible image
//! of the wrong geometry, at full speed, with nothing failing.
//!
//! The corpus comes from `solarxy_bvh::corpus` rather than from here, so both
//! comparisons drive the same rays through the same meshes and a change to the
//! ray policy cannot land in one and not the other.

mod common;

use cgmath::{Deg, InnerSpace, Matrix4, Point3, SquareMatrix, Vector3};
use solarxy_bvh::{Bvh, Instanced, corpus};
use solarxy_core::aabb::AABB;
use solarxy_renderer::pathtrace::arena::{ArenaMesh, ArenaPlacement, INSTANCE_VISIBLE, TraceArena};
use solarxy_renderer::pathtrace::probe::{CorpusRay, HitPoll, TraversalProbe};
use solarxy_renderer::pathtrace::{TraceScene, probe::CorpusHit};

/// The ray budget both sides pass. Not infinity: WGSL has no infinity literal,
/// and a finite bound on one side against an infinite one on the other is a
/// difference between the implementations rather than between their answers.
const T_MAX: f32 = 1e30;

/// The world transform and its inverse.
type Placement = ([[f32; 4]; 4], [[f32; 4]; 4]);

fn placement(translate: [f32; 3], axis: [f32; 3], degrees: f32, scale: [f32; 3]) -> Placement {
    let world = Matrix4::from_translation(Vector3::new(translate[0], translate[1], translate[2]))
        * Matrix4::from_axis_angle(
            Vector3::new(axis[0], axis[1], axis[2]).normalize(),
            Deg(degrees),
        )
        * Matrix4::from_nonuniform_scale(scale[0], scale[1], scale[2]);
    let inv = world.invert().expect("placement must be invertible");
    (world.into(), inv.into())
}

fn transformed_bounds(positions: &[[f32; 3]], world: &[[f32; 4]; 4]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        let q = [
            world[0][0] * p[0] + world[1][0] * p[1] + world[2][0] * p[2] + world[3][0],
            world[0][1] * p[0] + world[1][1] * p[1] + world[2][1] * p[2] + world[3][1],
            world[0][2] * p[0] + world[1][2] * p[1] + world[2][2] * p[2] + world[3][2],
        ];
        for axis in 0..3 {
            min[axis] = min[axis].min(q[axis]);
            max[axis] = max[axis].max(q[axis]);
        }
    }
    AABB {
        min: Point3::new(min[0], min[1], min[2]),
        max: Point3::new(max[0], max[1], max[2]),
    }
}

/// Runs one corpus through both traversals and asserts they agree.
///
/// Returns how many rays hit, so a case can assert the corpus actually
/// exercised the traversal rather than missing everything and passing.
fn compare(
    positions: &[[f32; 3]],
    indices: &[u32],
    placements: &[Placement],
    seed: u32,
    count: u32,
) -> u32 {
    let Some(gpu) = common::gpu_or_skip() else {
        // A skip returns zero hits; the caller's assertion would then fire on a
        // machine that simply has no adapter, so report a value that passes.
        return u32::MAX;
    };

    let blas = Bvh::build_triangles(positions, indices);
    let boxes: Vec<AABB> = placements
        .iter()
        .map(|(world, _)| transformed_bounds(positions, world))
        .collect();
    let tlas = Bvh::build_tlas(&boxes);

    let mesh = ArenaMesh {
        bvh: &blas,
        positions,
        indices,
        normals: None,
        uv0: None,
    };
    let arena_placements: Vec<ArenaPlacement> = placements
        .iter()
        .map(|(world, inv_world)| ArenaPlacement {
            mesh: 0,
            world: *world,
            inv_world: *inv_world,
            material_base: 0,
            flags: INSTANCE_VISIBLE,
        })
        .collect();
    let arena = TraceArena::build(&tlas, &[mesh], &arena_placements);
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    assert_eq!(scene.instance_count() as usize, placements.len());

    let rays = corpus::rays(seed, count);
    let gpu_rays: Vec<CorpusRay> = rays
        .iter()
        .map(|r| CorpusRay {
            origin: [r.origin[0], r.origin[1], r.origin[2], 0.0],
            direction: [r.direction[0], r.direction[1], r.direction[2], 0.0],
        })
        .collect();

    let probe = TraversalProbe::new(&gpu.device, &gpu.pathtrace.scene);
    let mut readback = probe.submit(&gpu.device, &gpu.queue, &scene, &gpu_rays);
    let hits = spin(&gpu.device, &mut readback);
    assert_eq!(hits.len(), rays.len());

    let instances: Vec<Instanced<'_>> = placements
        .iter()
        .map(|(_, inv_world)| Instanced {
            inv_world: *inv_world,
            blas: &blas,
            positions,
            indices,
        })
        .collect();

    let mut hit_count = 0;
    for (ray, have) in rays.iter().zip(&hits) {
        let i = ray.index;
        let want = tlas.intersect_instances(ray.origin, ray.direction, T_MAX, &instances);

        assert_eq!(
            want.is_some(),
            have.hit(),
            "ray {i} from {:?} toward {:?}: cpu {want:?}, gpu {have:?}",
            ray.origin,
            ray.direction
        );

        if let Some(want) = want {
            hit_count += 1;
            assert!(
                (want.t - have.t).abs() < 1e-3,
                "ray {i}: distance {} vs {}",
                want.t,
                have.t
            );
            // Which surface was hit is only meaningful where the two agree
            // there is one surface to name. Two coincident triangles, or two
            // instances that touch, put the answer on a tie the two are free to
            // break differently; the distance above is what carries the claim
            // there, and the barycentrics only mean something once the triangle
            // matches.
            if want.instance == have.instance && want.prim == have.prim {
                for k in 0..3 {
                    assert!(
                        (want.bary[k] - have.bary[k]).abs() < 1e-3,
                        "ray {i}: barycentric {:?} vs {:?}",
                        want.bary,
                        have.bary
                    );
                }
            }
        }

        // The any-hit walk orders no children and returns early, so it is a
        // genuinely different traversal answering the same question. Both
        // implementations of both walks have to agree, which is four answers
        // to one question.
        assert_eq!(
            tlas.occluded_instances(ray.origin, ray.direction, T_MAX, &instances),
            have.occluded(),
            "ray {i}: any-hit disagrees across the language boundary"
        );
    }

    hit_count
}

/// Pumps the readback to completion.
///
/// A test may spin where the renderer may not: the shipped path polls once a
/// frame and gets on with something else, because WebGPU has no blocking wait.
fn spin(
    device: &wgpu::Device,
    readback: &mut solarxy_renderer::pathtrace::probe::HitReadback,
) -> Vec<CorpusHit> {
    // Sleeping rather than yielding, which is what the seven sibling spins in
    // this suite already do. The bound is a count of polls, so the wait it
    // actually buys is the count times whatever one poll costs: with a yield
    // that is a few milliseconds on an idle machine, and it is nothing at all
    // on a loaded one, where ten thousand yields can pass before a queue that
    // is sharing its GPU has retired the copy. This gave up on a busy CI runner
    // while the readback was still perfectly healthy. A millisecond a poll
    // makes the same bound ten seconds of real time.
    for _ in 0..10_000 {
        match readback.poll(device) {
            HitPoll::Ready(hits) => return hits,
            HitPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            HitPoll::Failed => panic!("corpus readback failed"),
        }
    }
    panic!("corpus readback never resolved");
}

#[test]
fn the_kernel_agrees_with_the_cpu_traversal_on_a_single_identity_instance() {
    // One untransformed instance: anything the two-level walk gets wrong shows
    // up here with no transform to blame it on.
    let (positions, indices) = corpus::sphere(32, 16);
    let identity: [[f32; 4]; 4] = Matrix4::identity().into();
    let hits = compare(
        &positions,
        &indices,
        &[(identity, identity)],
        0x9E37_79B9,
        2048,
    );
    assert!(hits > 150, "corpus barely hit anything ({hits} hits)");
}

#[test]
fn the_kernel_agrees_with_the_cpu_traversal_across_placements() {
    // Non-uniform scale is the case that fails if the transformed ray direction
    // is renormalized, because `t` then means something different per instance
    // and the top-level walk stops being able to compare hits.
    let (positions, indices) = corpus::sphere(24, 12);
    let placements = [
        placement([-1.2, 0.0, 0.0], [0.0, 1.0, 0.0], 0.0, [0.45, 0.45, 0.45]),
        placement([1.2, 0.0, 0.0], [1.0, 0.0, 0.0], 37.0, [0.6, 0.25, 0.4]),
        placement([0.0, 1.3, 0.0], [0.3, 0.5, 0.8], 115.0, [0.3, 0.7, 0.3]),
    ];
    let hits = compare(&positions, &indices, &placements, 0x2545_F491, 3072);
    assert!(hits > 150, "corpus barely hit anything ({hits} hits)");
}

#[test]
fn the_kernel_agrees_with_the_cpu_traversal_on_flat_coplanar_geometry() {
    // Every triangle shares one plane, so the box is degenerate on an axis and
    // the slab test divides by an infinite reciprocal on every query. The CPU
    // side spells the test as a per-axis loop and the shader as vectorized
    // min/max; this is the case that decides whether those are really the same
    // function, NaN handling included.
    let (positions, indices) = corpus::coplanar_grid(30, 0.1);
    let identity: [[f32; 4]; 4] = Matrix4::identity().into();
    let hits = compare(
        &positions,
        &indices,
        &[(identity, identity)],
        0x1357_9BDF,
        2048,
    );
    assert!(hits > 80, "corpus barely hit anything ({hits} hits)");
}

#[test]
fn an_empty_scene_traverses_without_hitting_anything() {
    // An empty arena is an ordinary editing state, and every storage buffer is
    // padded rather than zero-sized because a zero-sized binding is invalid.
    // What must not happen is a hit against the padding.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let tlas = Bvh::build_tlas(&[]);
    let arena = TraceArena::build(&tlas, &[], &[]);
    let scene = TraceScene::upload(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
    assert_eq!(scene.instance_count(), 0);

    let probe = TraversalProbe::new(&gpu.device, &gpu.pathtrace.scene);
    let rays: Vec<CorpusRay> = corpus::rays(0x9E37_79B9, 128)
        .iter()
        .map(|r| CorpusRay {
            origin: [r.origin[0], r.origin[1], r.origin[2], 0.0],
            direction: [r.direction[0], r.direction[1], r.direction[2], 0.0],
        })
        .collect();
    let mut readback = probe.submit(&gpu.device, &gpu.queue, &scene, &rays);
    for hit in spin(&gpu.device, &mut readback) {
        assert!(!hit.hit(), "empty scene reported a hit: {hit:?}");
        assert!(!hit.occluded(), "empty scene reported an occluder: {hit:?}");
    }
}

#[test]
fn a_scene_that_grows_and_shrinks_keeps_tracing_the_geometry_it_holds() {
    // The buffers carry headroom and are rewritten in place while a repack
    // fits, so a bind group outlives most changes and is rebuilt only when one
    // of them had to be reallocated. The failure this guards is the one where
    // it is not rebuilt: the group still points at the old allocation, the
    // dispatch is valid, and the kernel traverses the previous scene at full
    // speed with nothing failing. Only a real traversal after each step can
    // see that, which is why this probes rather than inspecting sizes.
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };

    let identity: [[f32; 4]; 4] = Matrix4::identity().into();
    let probe = TraversalProbe::new(&gpu.device, &gpu.pathtrace.scene);
    let mut scene = TraceScene::new(&gpu.device, &gpu.pathtrace);

    // Small, then large enough to force a reallocation, then small again so a
    // shrink writes into a buffer with a long tail of the previous scene still
    // sitting in it.
    for subdivisions in [4u32, 24, 4] {
        let (positions, indices) = corpus::sphere(subdivisions, subdivisions / 2);
        let blas = Bvh::build_triangles(&positions, &indices);
        let tlas = Bvh::build_tlas(&[transformed_bounds(&positions, &identity)]);
        let arena = TraceArena::build(
            &tlas,
            &[ArenaMesh {
                bvh: &blas,
                positions: &positions,
                indices: &indices,
                normals: None,
                uv0: None,
            }],
            &[ArenaPlacement {
                mesh: 0,
                world: identity,
                inv_world: identity,
                material_base: 0,
                flags: INSTANCE_VISIBLE,
            }],
        );
        scene.sync(&gpu.device, &gpu.queue, &gpu.pathtrace, &arena);
        assert_eq!(scene.instance_count(), 1);

        let rays = corpus::rays(0x0BAD_F00D, 512);
        let gpu_rays: Vec<CorpusRay> = rays
            .iter()
            .map(|r| CorpusRay {
                origin: [r.origin[0], r.origin[1], r.origin[2], 0.0],
                direction: [r.direction[0], r.direction[1], r.direction[2], 0.0],
            })
            .collect();
        let mut readback = probe.submit(&gpu.device, &gpu.queue, &scene, &gpu_rays);
        let hits = spin(&gpu.device, &mut readback);

        let instances = [Instanced {
            inv_world: identity,
            blas: &blas,
            positions: &positions,
            indices: &indices,
        }];
        let mut hit_count = 0;
        for (ray, have) in rays.iter().zip(&hits) {
            let want = tlas.intersect_instances(ray.origin, ray.direction, T_MAX, &instances);
            assert_eq!(
                want.is_some(),
                have.hit(),
                "at {subdivisions} subdivisions, ray {} disagrees: cpu {want:?}, gpu {have:?}",
                ray.index
            );
            if let Some(want) = want {
                assert!(
                    (want.t - have.t).abs() <= 1e-3 * want.t.abs().max(1.0),
                    "at {subdivisions} subdivisions, ray {} hit a different distance: \
                     cpu {}, gpu {}",
                    ray.index,
                    want.t,
                    have.t
                );
                hit_count += 1;
            }
        }
        assert!(
            hit_count > 20,
            "at {subdivisions} subdivisions the corpus barely hit anything ({hit_count})"
        );
    }
}
