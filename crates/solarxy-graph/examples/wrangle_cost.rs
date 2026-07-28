//! What a wrangle costs per element (0.8.1 milestone).
//!
//! The question this answers. The milestone requires "a documented
//! per-element ceiling with a cook warning above it", and says explicitly
//! that the number is measured at implementation because open item 3 (what
//! scene sizes users actually hit) is unresolved: "the doc must not carry a
//! guessed figure dressed as a decision".
//!
//! What the number means. The wrangle is single-threaded and on web runs in
//! the browser's main wasm instance, so the interesting threshold is not
//! where it fails but where a re-cook stops feeling instant. A parameter
//! drag re-cooks continuously, so the budget that matters is one frame at
//! 60fps (16.7 ms); a click-to-edit re-cook can afford more.
//!
//! ```text
//! cargo run --release -p solarxy-graph --example wrangle_cost
//! ```
//!
//! Honest limitation: this is a native release build. wasm is slower, and
//! the ratio is not fixed. The ceiling that ships is therefore set below
//! the native number rather than at it, and the reasoning is recorded in
//! the milestone amendment rather than left in a comment here.

use std::time::Instant;

use solarxy_graph::expr::{EvalCtx, Runner, SceneTime, parse_program};
use solarxy_kernel::wrangle::wrangle;
use solarxy_kernel::{AttributeDomain, GeometrySet};

/// Point counts to sweep, bracketing the scales a user plausibly reaches:
/// a primitive, a dense primitive, a scanned mesh, a heavy scan.
const SIZES: &[u32] = &[8, 16, 32, 64, 128, 256];

/// The programs, chosen to span the realistic range rather than to flatter
/// the result: one trivial assignment, the demonstrable colour-by-position
/// default, and a heavier one with maths and a local.
const PROGRAMS: &[(&str, &str)] = &[
    ("trivial", "@v = 1;"),
    (
        "default (colour by position)",
        "@Cd = set(@P.x + 0.5, @P.y + 0.5, @P.z + 0.5);",
    ),
    (
        "heavy (local + trig + displace)",
        "float d = length(@P); @P = @P * (1 + 0.1 * sin(d * 8 + $T)); @Cd = set(d, 1 - d, 0.5);",
    ),
];

fn main() {
    println!("Wrangle cost: microseconds per element, native release build.\n");

    for (label, source) in PROGRAMS {
        println!("## {label}");
        println!("`{source}`\n");
        println!("| points | total ms | us/element | elements in 16.7ms |");
        println!("|---|---|---|---|");

        for &seg in SIZES {
            let set = grid(seg);
            let points: usize = set.meshes.iter().map(|m| m.positions.len()).sum();

            let program = match parse_program(source) {
                Ok(p) => p,
                Err(e) => {
                    println!("| {points} | parse failed: {} | | |", e.message);
                    continue;
                }
            };
            let base = EvalCtx::new(SceneTime::default());
            let mut runner = Runner::new(&program, base, source);
            let bindings = program.lane_bindings();

            // One warm pass so the first measurement is not paying for
            // first-touch page faults on the freshly built buffers.
            let _ = wrangle(&set, AttributeDomain::Point, &bindings, &mut runner);

            let start = Instant::now();
            let out = wrangle(&set, AttributeDomain::Point, &bindings, &mut runner);
            let elapsed = start.elapsed();
            assert!(out.is_ok(), "the program must run");

            let ms = elapsed.as_secs_f64() * 1e3;
            let us_each = elapsed.as_secs_f64() * 1e6 / points as f64;
            let in_a_frame = (16.7 / ms * points as f64) as u64;
            println!("| {points} | {ms:.2} | {us_each:.3} | {in_a_frame} |");
        }
        println!();
    }

    println!(
        "The last column is the honest ceiling: how many elements this program \n\
         finishes inside one 60fps frame. A parameter drag re-cooks continuously, \n\
         so that is the budget a wrangle upstream of a dragged param has to fit."
    );
}

/// A subdivided plane: `seg x seg` quads, so point count grows as the
/// square and the sweep reaches real scan sizes without a fixture file.
fn grid(seg: u32) -> GeometrySet {
    GeometrySet::from_mesh(solarxy_kernel::primitives::generate_plane(
        2.0, 2.0, seg, seg,
    ))
}
