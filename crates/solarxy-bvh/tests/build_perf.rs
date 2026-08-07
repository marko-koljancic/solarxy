//! Build-time measurement over a million triangles.
//!
//! Ignored by default, so it never slows the ordinary suite, and run
//! explicitly when a number is needed:
//!
//! ```bash
//! cargo test --release -p solarxy-bvh --test build_perf -- --ignored --nocapture
//! ```
//!
//! This is the native lower bound for the browser gate, not the gate itself.
//! The gate asks how long the build takes inside the import Web Worker, which
//! runs the same code compiled to wasm with no threads and a different
//! allocator, so it will be slower. Measuring natively first says whether the
//! algorithm is anywhere near the budget before a wasm build is worth making,
//! and it is the number that isolates a regression in the builder from one in
//! the wasm toolchain.
//!
//! Three consecutive runs are timed rather than one. The reference machine is
//! fanless, so the drift between the first and the last is the throttling
//! signal, and averaging it away would hide exactly what a sustained-load
//! judgement needs to see.

mod common;

use std::time::Instant;

use solarxy_bvh::Bvh;

/// A sphere with a million triangles: `1000 * 500 * 2`.
const WIDTH: u32 = 1000;
const HEIGHT: u32 = 500;
const RUNS: usize = 3;

#[test]
#[ignore = "measurement, not a regression gate; run with --release --ignored"]
fn build_one_million_triangles() {
    let generate = Instant::now();
    let (positions, indices) = common::sphere(WIDTH, HEIGHT);
    let generate_ms = generate.elapsed().as_secs_f64() * 1000.0;
    let tri_count = indices.len() / 3;
    assert_eq!(tri_count, 1_000_000);

    println!(
        "scene: {tri_count} triangles, {} vertices, generated in {generate_ms:.1} ms",
        positions.len()
    );

    let mut timings = Vec::with_capacity(RUNS);
    let mut last = None;
    for run in 0..RUNS {
        let start = Instant::now();
        let bvh = Bvh::build_triangles(&positions, &indices);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        timings.push(ms);
        println!("run {}: {ms:.1} ms", run + 1);
        last = Some(bvh);
    }

    let Some(bvh) = last else {
        panic!("RUNS must be at least one");
    };
    let stats = bvh.stats();
    let arrays = bvh.to_gpu_arrays();
    println!(
        "nodes: {} ({} KiB), permutation: {} KiB",
        stats.node_count,
        arrays.nodes.len() / 1024,
        arrays.prim_indices.len() / 1024
    );
    println!(
        "leaves: {}, average {:.2} prims, largest {}, deepest {}, depth-capped {}",
        stats.leaf_count,
        f64::from(stats.prim_count) / f64::from(stats.leaf_count),
        stats.max_leaf_size,
        stats.max_depth,
        stats.depth_capped_leaves
    );
    println!(
        "thermal drift across {RUNS} runs: first {:.1} ms, last {:.1} ms ({:+.1}%)",
        timings[0],
        timings[RUNS - 1],
        (timings[RUNS - 1] / timings[0] - 1.0) * 100.0
    );

    // The structural claims hold at scale, not only on the small meshes the
    // ordinary suite uses. A million-triangle tree that exceeded the depth cap
    // would overflow the shader's stack, which is the one failure a CPU test
    // can catch before the GPU shows it as a wrong image.
    assert!(stats.max_depth < solarxy_bvh::MAX_DEPTH);
    assert_eq!(stats.depth_capped_leaves, 0);
    assert_eq!(stats.prim_count, tri_count as u32);
}
