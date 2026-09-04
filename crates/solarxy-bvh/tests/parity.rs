//! The parity corpus: this crate's hierarchy traversal against the CPU
//! raycaster the picking path has used since the web milestone.
//!
//! Three implementations of the same intersection will exist by the end of the
//! milestone: `solarxy_core::raycast` (brute force over a mesh, already
//! shipping), `solarxy_bvh` (hierarchy traversal, this crate), and the WGSL
//! kernel. Only the first two can be unit tested against each other, so this
//! file is the joint that carries the whole chain: it pins the hierarchy to a
//! shipped implementation, and the shader is then written as a line-for-line
//! twin of the hierarchy.
//!
//! The corpus is generated from a fixed seed rather than sampled, so a failure
//! names one reproducible ray rather than "sometimes".

mod common;

use cgmath::{Deg, InnerSpace, Matrix4, Point3, SquareMatrix, Vector3};
use common::{bounds_of, transformed_bounds};
use solarxy_bvh::{Bvh, Instanced, corpus};
use solarxy_core::raycast::{MeshView, Ray, raycast_meshes};

/// Compare the hierarchy against brute force over one mesh, for `count` rays.
///
/// Returns how many of them hit, so a test can assert the corpus actually
/// exercised the traversal rather than missing everything and passing.
fn compare_corpus(positions: &[[f32; 3]], indices: &[u32], seed: u32, count: u32) -> u32 {
    let bvh = Bvh::build_triangles(positions, indices);
    let meshes = [MeshView {
        positions,
        indices,
        bounds: bounds_of(positions),
    }];
    let mut hits = 0;

    for ray in corpus::rays(seed, count) {
        let i = ray.index;
        let origin = ray.origin;
        let dir = ray.direction;

        let expect = raycast_meshes(
            &Ray {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                direction: Vector3::new(dir[0], dir[1], dir[2]),
            },
            &meshes,
        );
        let got = bvh.intersect_triangles(origin, dir, f32::INFINITY, positions, indices);

        match (expect, got) {
            (None, None) => {}
            (Some(want), Some(have)) => {
                hits += 1;
                assert!(
                    (want.distance - have.t).abs() < 1e-4,
                    "ray {i}: distance {} vs {}",
                    want.distance,
                    have.t
                );
                for k in 0..3 {
                    assert!(
                        (want.barycentric[k] - have.bary[k]).abs() < 1e-3,
                        "ray {i}: barycentric {:?} vs {:?}",
                        want.barycentric,
                        have.bary
                    );
                }
                assert_eq!(want.face_index, have.prim, "ray {i}: different triangle");
            }
            (a, b) => {
                panic!("ray {i} from {origin:?} toward {dir:?}: raycaster {a:?}, hierarchy {b:?}")
            }
        }

        // The any-hit query must agree with the closest-hit one about whether
        // anything is there at all. It orders no children and returns early,
        // so it is a genuinely different traversal answering the same
        // question.
        assert_eq!(
            expect.is_some(),
            bvh.occluded_triangles(origin, dir, f32::INFINITY, positions, indices),
            "ray {i}: any-hit disagrees with closest-hit"
        );
    }

    hits
}

#[test]
fn hierarchy_matches_the_raycaster_on_a_sphere() {
    let (positions, indices) = corpus::sphere(48, 24);
    let hits = compare_corpus(&positions, &indices, 0x9E37_79B9, 2000);
    assert!(hits > 200, "corpus barely hit anything ({hits} hits)");
}

#[test]
fn hierarchy_matches_the_raycaster_on_flat_coplanar_geometry() {
    // Every triangle shares one plane, so the root box is degenerate on an
    // axis and the slab test divides by an infinite reciprocal on every query.
    // This is the case that separates a robust slab test from a plausible one.
    let (positions, indices) = corpus::coplanar_grid(30, 0.1);
    let hits = compare_corpus(&positions, &indices, 0x1357_9BDF, 2000);
    assert!(hits > 100, "corpus barely hit anything ({hits} hits)");
}

#[test]
fn hierarchy_matches_the_raycaster_when_every_triangle_is_the_same_triangle() {
    // Degenerate for the builder rather than the traversal: no split reduces
    // area, so the median fallback shapes the whole tree. The answers must
    // still line up.
    let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let indices: Vec<u32> = std::iter::repeat_n([0u32, 1, 2], 200).flatten().collect();
    let bvh = Bvh::build_triangles(&positions, &indices);

    // Brute force resolves the tie by first-wins; the hierarchy visits leaves
    // in permutation order, so only the distance is meaningfully comparable
    // here. That the two agree a hit exists, at the same distance, is the
    // whole claim.
    let meshes = [MeshView {
        positions: &positions,
        indices: &indices,
        bounds: bounds_of(&positions),
    }];
    let expect = raycast_meshes(
        &Ray {
            origin: Point3::new(0.25, 0.25, 4.0),
            direction: Vector3::new(0.0, 0.0, -1.0),
        },
        &meshes,
    );
    let got = bvh.intersect_triangles(
        [0.25, 0.25, 4.0],
        [0.0, 0.0, -1.0],
        f32::INFINITY,
        &positions,
        &indices,
    );
    let (Some(want), Some(have)) = (expect, got) else {
        panic!("both implementations should hit: {expect:?} / {got:?}");
    };
    assert!((want.distance - have.t).abs() < 1e-4);
}

// ---------------------------------------------------------------------------
// The two-level structure.
//
// The reference and the hierarchy answer the same question from opposite
// directions: `raycast_meshes` transforms the geometry into world space and
// intersects there, while the hierarchy transforms the ray into each instance's
// object space and intersects there. They are the same answer in exact
// arithmetic and only close in floating point, which is the point of comparing
// them. Barycentric coordinates are affine-invariant so they compare directly,
// and `t` stays in world units at both levels precisely because the transformed
// direction is not renormalized.
// ---------------------------------------------------------------------------

/// The pair the two sides need: the world transform for the reference's
/// geometry, and its inverse for the hierarchy's ray.
type Placement = ([[f32; 4]; 4], [[f32; 4]; 4]);

/// One placement, from the parts a reader can picture.
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

/// Compare the two-level traversal against brute force over the same scene
/// expressed as one transformed mesh per instance.
///
/// Every instance shares one hierarchy and one position buffer, which is the
/// case the structure exists for: an instanced mesh is one BLAS and N entries
/// in the TLAS.
fn compare_instanced_corpus(
    positions: &[[f32; 3]],
    indices: &[u32],
    placements: &[Placement],
    seed: u32,
    count: u32,
) -> u32 {
    let blas = Bvh::build_triangles(positions, indices);

    let world_positions: Vec<Vec<[f32; 3]>> = placements
        .iter()
        .map(|(world, _)| {
            positions
                .iter()
                .map(|p| {
                    [
                        world[0][0] * p[0] + world[1][0] * p[1] + world[2][0] * p[2] + world[3][0],
                        world[0][1] * p[0] + world[1][1] * p[1] + world[2][1] * p[2] + world[3][1],
                        world[0][2] * p[0] + world[1][2] * p[1] + world[2][2] * p[2] + world[3][2],
                    ]
                })
                .collect()
        })
        .collect();

    let boxes: Vec<_> = placements
        .iter()
        .map(|(world, _)| transformed_bounds(positions, world))
        .collect();
    let tlas = Bvh::build_tlas(&boxes);

    let instances: Vec<Instanced<'_>> = placements
        .iter()
        .map(|(_, inv_world)| Instanced {
            inv_world: *inv_world,
            blas: &blas,
            positions,
            indices,
        })
        .collect();
    let meshes: Vec<MeshView<'_>> = world_positions
        .iter()
        .zip(&boxes)
        .map(|(p, bounds)| MeshView {
            positions: p,
            indices,
            bounds: *bounds,
        })
        .collect();

    let mut hits = 0;
    for ray in corpus::rays(seed, count) {
        let i = ray.index;
        let origin = ray.origin;
        let dir = ray.direction;

        let expect = raycast_meshes(
            &Ray {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                direction: Vector3::new(dir[0], dir[1], dir[2]),
            },
            &meshes,
        );
        let got = tlas.intersect_instances(origin, dir, f32::INFINITY, &instances);

        match (expect, got) {
            (None, None) => {}
            (Some(want), Some(have)) => {
                hits += 1;
                assert!(
                    (want.distance - have.t).abs() < 1e-3,
                    "ray {i}: distance {} vs {}",
                    want.distance,
                    have.t
                );
                // Which surface was hit is only meaningful when the two agree
                // there is one surface to name. Two instances that overlap, or
                // two coincident triangles, put the answer on a tie the two
                // implementations are free to break differently; the distance
                // assertion above is what carries the claim there.
                if want.mesh_index == have.instance && want.face_index == have.prim {
                    for k in 0..3 {
                        assert!(
                            (want.barycentric[k] - have.bary[k]).abs() < 1e-3,
                            "ray {i}: barycentric {:?} vs {:?}",
                            want.barycentric,
                            have.bary
                        );
                    }
                }
            }
            (a, b) => {
                panic!("ray {i} from {origin:?} toward {dir:?}: raycaster {a:?}, hierarchy {b:?}")
            }
        }

        assert_eq!(
            expect.is_some(),
            tlas.occluded_instances(origin, dir, f32::INFINITY, &instances),
            "ray {i}: instanced any-hit disagrees with closest-hit"
        );
    }

    hits
}

#[test]
fn the_two_level_traversal_matches_the_raycaster_across_placements() {
    let (positions, indices) = corpus::sphere(24, 12);
    // Translated, rotated and non-uniformly scaled, and deliberately not
    // overlapping, so the closest-hit answer names one instance without a tie.
    // Non-uniform scale is the case that fails if the transformed direction is
    // renormalized, because `t` then means something different per instance.
    let placements = [
        placement([-1.2, 0.0, 0.0], [0.0, 1.0, 0.0], 0.0, [0.45, 0.45, 0.45]),
        placement([1.2, 0.0, 0.0], [1.0, 0.0, 0.0], 37.0, [0.6, 0.25, 0.4]),
        placement([0.0, 1.3, 0.0], [0.3, 0.5, 0.8], 115.0, [0.3, 0.7, 0.3]),
    ];
    let hits = compare_instanced_corpus(&positions, &indices, &placements, 0x2545_F491, 3000);
    assert!(hits > 150, "corpus barely hit anything ({hits} hits)");
}

#[test]
fn a_single_identity_instance_agrees_with_the_one_level_traversal() {
    // The two-level walk over one untransformed instance must be the same
    // answer as the one-level walk. Anything the outer level gets wrong shows
    // up here with nothing else to blame.
    let (positions, indices) = corpus::sphere(32, 16);
    let blas = Bvh::build_triangles(&positions, &indices);
    let tlas = Bvh::build_tlas(&[bounds_of(&positions)]);
    let identity: [[f32; 4]; 4] = Matrix4::identity().into();
    let instances = [Instanced {
        inv_world: identity,
        blas: &blas,
        positions: &positions,
        indices: &indices,
    }];

    let mut compared = 0;
    for ray in corpus::rays(0x9E37_79B9, 2000) {
        let flat = blas.intersect_triangles(
            ray.origin,
            ray.direction,
            f32::INFINITY,
            &positions,
            &indices,
        );
        let nested = tlas.intersect_instances(ray.origin, ray.direction, f32::INFINITY, &instances);
        match (flat, nested) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                compared += 1;
                assert_eq!(a.prim, b.prim, "ray {}: different triangle", ray.index);
                assert_eq!(b.instance, 0, "ray {}: wrong instance", ray.index);
                assert!((a.t - b.t).abs() < 1e-6, "ray {}: different t", ray.index);
                assert_eq!(a.bary, b.bary, "ray {}: different barycentric", ray.index);
            }
            (a, b) => panic!("ray {}: one level {a:?}, two level {b:?}", ray.index),
        }
        assert_eq!(
            blas.occluded_triangles(
                ray.origin,
                ray.direction,
                f32::INFINITY,
                &positions,
                &indices
            ),
            tlas.occluded_instances(ray.origin, ray.direction, f32::INFINITY, &instances),
            "ray {}: any-hit disagrees across levels",
            ray.index
        );
    }
    assert!(
        compared > 200,
        "corpus barely hit anything ({compared} hits)"
    );
}

#[test]
fn an_instance_index_the_scene_does_not_carry_is_skipped() {
    // The TLAS is built over three boxes but handed two instances, which is
    // what a scene mid-edit looks like. Skipping beats panicking, and beats
    // reading past the end.
    let (positions, indices) = corpus::sphere(16, 8);
    let blas = Bvh::build_triangles(&positions, &indices);
    let boxes = [
        bounds_of(&positions),
        bounds_of(&positions),
        bounds_of(&positions),
    ];
    let tlas = Bvh::build_tlas(&boxes);
    let identity: [[f32; 4]; 4] = Matrix4::identity().into();
    let instances = [
        Instanced {
            inv_world: identity,
            blas: &blas,
            positions: &positions,
            indices: &indices,
        },
        Instanced {
            inv_world: identity,
            blas: &blas,
            positions: &positions,
            indices: &indices,
        },
    ];

    let hit =
        tlas.intersect_instances([0.0, 0.0, 4.0], [0.0, 0.0, -1.0], f32::INFINITY, &instances);
    assert!(
        hit.is_some(),
        "the two live instances should still be found"
    );
    assert!(hit.expect("checked").instance < 2);
    assert!(tlas.occluded_instances([0.0, 0.0, 4.0], [0.0, 0.0, -1.0], f32::INFINITY, &instances));
}
