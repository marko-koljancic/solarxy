//! Bakes the linearly-transformed-cosine tables the rect-area light shades
//! through. The committed blob is `src/shaders/ltc_lut.rgba16f`;
//! regenerate it with:
//!
//! ```bash
//! cargo run --release -p solarxy-renderer --example gen_ltc_lut -- \
//!     crates/solarxy-renderer/src/shaders/ltc_lut.rgba16f
//! ```
//!
//! Release matters: this is a numerical fit, not a rasterization, and a
//! debug build takes tens of minutes where release takes about one.
//!
//! **What is being fitted.** Heitz, Dupuy, Hill and Neubelt, *Real-Time
//! Polygonal-Light Shading with Linearly Transformed Cosines* (SIGGRAPH
//! 2016). For each roughness and view elevation, find the 3x3 matrix `M`
//! that best maps a clamped-cosine distribution onto the GGX lobe. Shading
//! a polygon then reduces to transforming its corners by `M^-1` and
//! integrating a cosine over the result, which has a closed form. That is
//! what makes a rectangle's extent and orientation reach the shading at
//! all, which is the whole point of `rect_area_light` v3.
//!
//! **What is stored.** Two 64x64 RGBA tables, indexed by
//! `u = perceptual roughness` and `v = sqrt(1 - dot(N, V))`:
//!
//! - table 1: the four varying entries of `M^-1`, normalized so its middle
//!   entry is exactly 1 (so it need not be stored).
//! - table 2: the lobe's magnitude and its Fresnel term, which together
//!   rebuild the specular colour the split-sum path would have produced.
//!
//! **Why fit rather than embed.** The published tables are MIT licensed and
//! could simply be copied. Fitting keeps the data ours and regenerable at
//! any resolution, and the fit is checked numerically against the published
//! tables by `tests/ltc_fit.rs` rather than by eye, so a wrong fit fails a
//! test instead of shipping as slightly wrong-looking highlights.

#![allow(clippy::cast_precision_loss)]

mod fit;

use fit::{Ltc, N, alpha_for_column, compute_avg_terms, compute_error, nelder_mead, view_for_row};
use half::f16;

fn main() -> anyhow::Result<()> {
    let out_path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: gen_ltc_lut <output.rgba16f>"))?;

    // The packed output, and the raw parameters kept alongside so each fit
    // can warm-start from its neighbour.
    let mut table1 = vec![[0.0f64; 4]; N * N];
    let mut table2 = vec![[0.0f64; 4]; N * N];
    let mut params = vec![[1.0f64, 1.0, 0.0]; N * N];

    let mut ltc = Ltc::new();

    // Sweep from the easy corner to the hard one, each fit warm-started
    // from its already solved neighbour: that is what keeps a 4096-point
    // search stable. Theta ASCENDS from normal incidence (t = 0, where the
    // lobe is symmetric and the fit is nearly free) toward grazing;
    // roughness DESCENDS from 1 (a broad lobe) toward the mirror, which is
    // the hardest fit in the table and is reached with the best possible
    // guess in hand.
    for t in 0..N {
        let v = view_for_row(t);

        for a in (0..N).rev() {
            let alpha = alpha_for_column(a);

            let (magnitude, fresnel, average_dir) = compute_avg_terms(v, alpha);
            ltc.magnitude = magnitude;
            ltc.fresnel = fresnel;

            let isotropic = if t == 0 {
                // Normal incidence: the lobe is symmetric about the normal,
                // so the frame is the shading frame, m13 is zero, and
                // m11 == m22 by symmetry. Fitting those two independently
                // here only lets sampling noise break a symmetry the
                // physics guarantees.
                ltc.x = [1.0, 0.0, 0.0];
                ltc.y = [0.0, 1.0, 0.0];
                ltc.z = [0.0, 0.0, 1.0];
                let seed = if a == N - 1 {
                    [1.0, 1.0, 0.0]
                } else {
                    params[a + 1]
                };
                ltc.m11 = seed[0];
                ltc.m22 = seed[0];
                ltc.m13 = 0.0;
                true
            } else {
                // Off-normal: fit in the frame around the average direction,
                // and let m13 tilt the lobe back toward the surface.
                let l = average_dir;
                ltc.x = [l[2], 0.0, -l[0]];
                ltc.y = [0.0, 1.0, 0.0];
                ltc.z = l;
                let seed = params[a + (t - 1) * N];
                ltc.m11 = seed[0];
                ltc.m22 = seed[1];
                ltc.m13 = seed[2];
                false
            };
            ltc.update();

            // m11 and m22 are searched in LOG space. They are strictly
            // positive and span five orders of magnitude across the table
            // (about 1 at roughness 1, about 2e-5 at the mirror), so a
            // fixed additive step either crawls at the rough end or cannot
            // resolve the mirror end at all. A fixed step in log space is
            // a fixed PROPORTION, which is the right notion of "nearby"
            // for a lobe width. m13 stays linear: it is a signed tilt of
            // order 0.1, not a scale.
            let start = [ltc.m11.max(1.0e-9).ln(), ltc.m22.max(1.0e-9).ln(), ltc.m13];
            {
                let ltc = &mut ltc;
                let mut objective = |p: &[f64; 3]| -> f64 {
                    let m11 = p[0].exp();
                    if isotropic {
                        ltc.m11 = m11;
                        ltc.m22 = m11;
                        ltc.m13 = 0.0;
                    } else {
                        ltc.m11 = m11;
                        ltc.m22 = p[1].exp();
                        ltc.m13 = p[2];
                    }
                    ltc.update();
                    compute_error(ltc, v, alpha)
                };
                let fitted = nelder_mead(start, 0.25, 1.0e-7, 150, &mut objective);
                // Leave the lobe holding the winning parameters.
                objective(&fitted);
            }
            params[a + t * N] = [ltc.m11, ltc.m22, ltc.m13];

            // Pack M^-1, normalized so its middle entry is 1. The shader
            // rebuilds it as
            //   [ x  0  z ]
            //   [ 0  1  0 ]
            //   [ y  0  w ]
            let inv = ltc.inv_m;
            let mid = inv[1][1];
            let s = if mid.abs() > 1.0e-30 { 1.0 / mid } else { 1.0 };
            table1[a + t * N] = [inv[0][0] * s, inv[2][0] * s, inv[0][2] * s, inv[2][2] * s];
            table2[a + t * N] = [ltc.magnitude, ltc.fresnel, 0.0, 0.0];
        }
        if t % 8 == 0 {
            println!("  theta row {t:>3} of {N} fitted");
        }
    }

    // Two tables back to back, RGBA half floats, row-major with roughness
    // varying fastest. The contract with `solarxy_renderer::ltc`.
    let mut bytes = Vec::with_capacity(N * N * 4 * 2 * 2);
    for table in [&table1, &table2] {
        for texel in table.iter() {
            for &c in texel {
                bytes.extend_from_slice(&f16::from_f64(c).to_le_bytes());
            }
        }
    }

    std::fs::write(&out_path, &bytes)?;
    println!("wrote {out_path}: {} bytes", bytes.len());
    Ok(())
}
