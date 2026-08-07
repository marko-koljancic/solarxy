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

use cgmath::{InnerSpace, Point3, Vector3};
use common::{bounds_of, sphere};
use solarxy_bvh::Bvh;
use solarxy_core::raycast::{MeshView, Ray, raycast_meshes};

/// Deterministic unit-interval draws. xorshift32, because the corpus has to be
/// identical on every machine that runs the gate.
struct Rng(u32);

impl Rng {
    fn next_unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32
    }

    fn next_range(&mut self, lo: f32, hi: f32) -> f32 {
        self.next_unit().mul_add(hi - lo, lo)
    }
}

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
    let mut rng = Rng(seed);
    let mut hits = 0;

    for i in 0..count {
        let origin = [
            rng.next_range(-3.0, 3.0),
            rng.next_range(-3.0, 3.0),
            rng.next_range(-3.0, 3.0),
        ];
        // Half the rays are aimed near the geometry and half are free. A
        // fully random direction from a 6-unit cube finds a unit sphere only
        // about one time in sixteen, which would spend most of the corpus
        // proving that a ray into empty space misses. The aimed half is what
        // actually drives the traversal down to the leaves; the free half is
        // what keeps the box rejections honest.
        let raw = if i % 2 == 0 {
            Vector3::new(
                rng.next_range(-1.2, 1.2) - origin[0],
                rng.next_range(-1.2, 1.2) - origin[1],
                rng.next_range(-1.2, 1.2) - origin[2],
            )
        } else {
            Vector3::new(
                rng.next_range(-1.0, 1.0),
                rng.next_range(-1.0, 1.0),
                rng.next_range(-1.0, 1.0),
            )
        };
        if raw.magnitude() < 1e-3 {
            continue;
        }
        let dir = raw.normalize();

        let expect = raycast_meshes(
            &Ray {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                direction: dir,
            },
            &meshes,
        );
        let got = bvh.intersect_triangles(
            origin,
            [dir.x, dir.y, dir.z],
            f32::INFINITY,
            positions,
            indices,
        );

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
            bvh.occluded_triangles(
                origin,
                [dir.x, dir.y, dir.z],
                f32::INFINITY,
                positions,
                indices
            ),
            "ray {i}: any-hit disagrees with closest-hit"
        );
    }

    hits
}

#[test]
fn hierarchy_matches_the_raycaster_on_a_sphere() {
    let (positions, indices) = sphere(48, 24);
    let hits = compare_corpus(&positions, &indices, 0x9E37_79B9, 2000);
    assert!(hits > 200, "corpus barely hit anything ({hits} hits)");
}

#[test]
fn hierarchy_matches_the_raycaster_on_flat_coplanar_geometry() {
    // Every triangle shares one plane, so the root box is degenerate on an
    // axis and the slab test divides by an infinite reciprocal on every query.
    // This is the case that separates a robust slab test from a plausible one.
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let n = 30u32;
    for y in 0..=n {
        for x in 0..=n {
            positions.push([x as f32 * 0.1 - 1.5, y as f32 * 0.1 - 1.5, 0.0]);
        }
    }
    let stride = n + 1;
    for y in 0..n {
        for x in 0..n {
            let a = y * stride + x;
            indices.extend_from_slice(&[a, a + 1, a + stride]);
            indices.extend_from_slice(&[a + 1, a + stride + 1, a + stride]);
        }
    }
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
