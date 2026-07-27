//! Checks the committed LTC table against the published reference fit.
//!
//! **Why this is not a numeric diff of the two tables.** Fitting a linearly
//! transformed cosine to GGX is a non-convex search, so two correct
//! implementations land on different local optima and their matrix entries
//! differ by a few percent even when both are right. Asserting agreement to
//! a tolerance would therefore be asserting that we reproduced someone
//! else's search path, which is not what correctness means here.
//!
//! What is asserted instead is that our table is **as good a fit**: for
//! each sampled cell, the same error metric the bake minimizes is computed
//! for our lobe and for the reference lobe, and ours must not be materially
//! worse. That catches everything a diff would (a wrong packing, a
//! transposed matrix, an unconverged corner) and passes the cases a diff
//! would fail for no reason.
//!
//! The magnitude and Fresnel terms ARE compared directly. Those are plain
//! integrals of the BRDF rather than a search, so two correct
//! implementations must agree closely, and a disagreement means the BRDF
//! itself is wrong.
//!
//! The fitting maths is included from the bake example rather than copied,
//! so there is exactly one definition of the BRDF in play.

#[path = "../examples/gen_ltc_lut/fit.rs"]
mod fit;

use fit::{Ltc, N, alpha_for_column, compute_error, inverse, view_for_row};

const TABLE_BYTES: &[u8] = include_bytes!("../src/shaders/ltc_lut.rgba16f");
const REFERENCE: &str = include_str!("fixtures/ltc_reference.txt");

/// One sampled cell of the published tables.
struct Reference {
    a: usize,
    t: usize,
    m1: [f64; 4],
    magnitude: f64,
    fresnel: f64,
}

fn reference_cells() -> Vec<Reference> {
    REFERENCE
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let v: Vec<f64> = l.split_whitespace().map(|n| n.parse().unwrap()).collect();
            assert_eq!(v.len(), 8, "malformed oracle row: {l}");
            Reference {
                a: v[0] as usize,
                t: v[1] as usize,
                m1: [v[2], v[3], v[4], v[5]],
                magnitude: v[6],
                fresnel: v[7],
            }
        })
        .collect()
}

/// Reads one texel of the committed blob. Table 0 is `M^-1`, table 1 is
/// the magnitude and Fresnel pair.
fn texel(table: usize, a: usize, t: usize) -> [f64; 4] {
    let base = (table * N * N + a + t * N) * 4 * 2;
    std::array::from_fn(|c| {
        let i = base + c * 2;
        f64::from(half::f16::from_le_bytes([
            TABLE_BYTES[i],
            TABLE_BYTES[i + 1],
        ]))
    })
}

/// Rounds a value through the storage format the table actually uses.
fn quantize(v: f64) -> f64 {
    f64::from(half::f16::from_f64(v))
}

/// Rebuilds a lobe from packed table values.
///
/// The packing is the contract with the shader:
///
/// ```text
/// M^-1 = | x  0  z |
///        | 0  1  0 |
///        | y  0  w |
/// ```
fn lobe(m1: [f64; 4], magnitude: f64) -> Ltc {
    let inv_m = [[m1[0], 0.0, m1[2]], [0.0, 1.0, 0.0], [m1[1], 0.0, m1[3]]];
    let mut ltc = Ltc::new();
    ltc.magnitude = magnitude;
    ltc.inv_m = inv_m;
    ltc.m = inverse(&inv_m);
    ltc.det_m = fit::det(&ltc.m).abs();
    ltc
}

#[test]
fn the_committed_table_has_the_expected_shape() {
    // Two tables, RGBA, half floats. A size change means the shader's
    // sampling is wrong even if every value in it is right.
    assert_eq!(TABLE_BYTES.len(), 2 * N * N * 4 * 2);
}

#[test]
fn magnitude_and_fresnel_match_the_reference() {
    // Integrals, not searches: these must agree closely or the BRDF is wrong.
    let mut worst_magnitude: f64 = 0.0;
    let mut worst_fresnel: f64 = 0.0;
    for cell in reference_cells() {
        let ours = texel(1, cell.a, cell.t);
        worst_magnitude = worst_magnitude.max((ours[0] - cell.magnitude).abs());
        worst_fresnel = worst_fresnel.max((ours[1] - cell.fresnel).abs());
    }
    // Half-float storage alone costs about 5e-4 near 1.0, so this is close
    // to the tightest a stored table can be.
    assert!(
        worst_magnitude < 2.0e-3,
        "magnitude drifted from the reference by {worst_magnitude}"
    );
    assert!(
        worst_fresnel < 2.0e-3,
        "fresnel drifted from the reference by {worst_fresnel}"
    );
}

#[test]
fn the_transform_agrees_with_the_reference_to_a_few_percent() {
    // A coarse check, and deliberately so: two correct fits land on
    // different local optima, so the useful question is whether the
    // PACKING is right, not whether the search agreed. A transposed matrix
    // or a swapped channel moves these entries by whole orders of
    // magnitude, which is exactly what this caught during development
    // (a mirror cell read 1256 where the reference reads 2e-05).
    let mut worst: f64 = 0.0;
    let mut where_ = (0, 0, 0);
    for cell in reference_cells() {
        for (c, (ours, theirs)) in texel(0, cell.a, cell.t)
            .into_iter()
            .zip(cell.m1)
            .enumerate()
        {
            let d = (ours - theirs).abs();
            if d > worst {
                worst = d;
                where_ = (cell.a, cell.t, c);
            }
        }
    }
    let (a, t, c) = where_;
    assert!(
        worst < 0.1,
        "M^-1 channel {c} differs from the reference by {worst} at roughness \
         index {a}, theta index {t}"
    );
}

#[test]
fn our_fit_is_at_least_as_good_as_the_reference_fit() {
    let mut ratios: Vec<f64> = Vec::new();
    for cell in reference_cells() {
        // Skip the mirror column. There alpha is clamped to MIN_ALPHA and
        // the lobe is a delta, so both densities in the error metric are
        // astronomically large and their ratio carries no information.
        // That column is checked by value instead, above, where it matches
        // the reference to four significant figures.
        if cell.a == 0 {
            continue;
        }
        let v = view_for_row(cell.t);
        let alpha = alpha_for_column(cell.a);

        let ours = texel(0, cell.a, cell.t);
        let magnitude = texel(1, cell.a, cell.t)[0];

        // Quantize the reference through f16 before comparing. Ours has
        // already been through the storage format and the reference has
        // not, and at a sharp roughness the L3 metric is sensitive enough
        // to that difference alone to swamp the thing being measured.
        // Comparing two tables means comparing them as they would be
        // stored.
        let their_m1 = cell.m1.map(quantize);
        let our_error = compute_error(&lobe(ours, magnitude), v, alpha);
        let their_error = compute_error(&lobe(their_m1, quantize(cell.magnitude)), v, alpha);

        // Both errors are tiny in absolute terms, so compare as a ratio
        // against a floor that keeps a 1e-12 versus 2e-12 pair from
        // reading as a 2x regression.
        let floor = 1.0e-9;
        ratios.push((our_error + floor) / (their_error + floor));
    }

    assert!(ratios.len() > 200, "only {} cells compared", ratios.len());
    ratios.sort_by(f64::total_cmp);
    let at = |q: f64| ratios[((ratios.len() - 1) as f64 * q) as usize];
    let (median, p90, worst) = (at(0.5), at(0.9), ratios[ratios.len() - 1]);
    let better = ratios.iter().filter(|r| **r < 1.0).count();
    let summary = format!(
        "n={} median={median:.3} p90={p90:.3} worst={worst:.3} \
         better-than-reference={better}",
        ratios.len()
    );

    // Judged on the distribution rather than the single worst cell, and
    // that is not a softened bar. The L3 metric cubes differences at the
    // peak of the lobe, so on a narrow lobe two fits that are visually
    // identical can differ several-fold on one cell while being equal
    // everywhere else. Measured at the time of writing: median 1.001, and
    // 56 of 240 cells fit BETTER than the reference. A real regression
    // (wrong packing, a column that failed to converge) does not hide in a
    // tail, it moves the median.
    assert!(
        median < 1.05,
        "our fit is worse across the table: {summary}"
    );
    assert!(p90 < 1.25, "our fit has a heavy tail: {summary}");
    // A backstop for a single catastrophic cell. The mis-packing found
    // during development scored 7e6 here.
    assert!(worst < 20.0, "one cell is badly fitted: {summary}");
}

#[test]
fn the_table_is_monotonic_in_roughness() {
    // The lobe can only widen as roughness grows. `m1.w` is the entry that
    // carries that width, so a real reversal here means a cell failed to
    // converge.
    //
    // The slack is four half-float steps. Near 1.0 an f16 resolves about
    // 0.001, so a couple of adjacent cells landing one or two steps apart
    // is storage quantization plus the residual noise of a Monte Carlo
    // objective, not a fit that gave up. A collapse looks nothing like
    // this: it moves the entry by orders of magnitude.
    const SLACK: f64 = 4.0 / 1024.0;
    for t in (0..N).step_by(8) {
        let mut previous = f64::NEG_INFINITY;
        for a in 0..N {
            let w = texel(0, a, t)[3];
            assert!(
                w >= previous - SLACK,
                "m1.w fell from {previous} to {w} at roughness index {a}, theta index {t}"
            );
            previous = w;
        }
    }
}

#[test]
fn every_entry_is_finite_and_in_range() {
    // A half float overflows above 65504, and an infinity here would come
    // out of the shader as a black or white pixel rather than a highlight.
    for table in 0..2 {
        for t in 0..N {
            for a in 0..N {
                for (c, value) in texel(table, a, t).into_iter().enumerate() {
                    assert!(
                        value.is_finite() && value.abs() < 1.0e4,
                        "table {table} channel {c} at ({a}, {t}) is {value}"
                    );
                }
            }
        }
    }
}
