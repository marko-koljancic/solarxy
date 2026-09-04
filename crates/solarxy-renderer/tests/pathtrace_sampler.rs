//! The sampler's decorrelation on a real device: do two pixels draw two point
//! sets, and does stratification survive the rotation that separates them.
//!
//! The defect class this file exists for is invisible to every other probe
//! test. They fix the pixel and vary the sample index, which is the right
//! shape for comparing a density to its sampler and a shape that cannot see
//! correlation across the image. The stratified sampler's permutations are
//! bijections: they reorder the order a pixel visits its cells and never
//! change which cells it visits, so without a per-pixel rotation every pixel
//! integrates every dimension with one identical quadrature rule and the
//! residual at the target count is a stationary pattern rather than noise.
//!
//! Both properties are asserted through the real kernel bindings, because the
//! sampler is WGSL and the browser's front end and the desktop's have already
//! disagreed once about what parses.

mod common;

use solarxy_renderer::pathtrace::probe::{ColorPoll, RandProbe, RandTap};

/// The sample count under test: the authored "good" preset, where the grid is
/// eight by eight and the defect was reported.
const STRATA: u32 = 64;

/// The stratification grid's side at that count.
const GRID: u32 = 8;

/// The dimension the taps draw. Any label works, because the probe's subject
/// is the pixel; this is the hemisphere direction, the dimension the defect
/// was most visible through.
const DIM: u32 = 3;

/// One pixel's full sequence at `STRATA` samples, through the pair path or the
/// scalar one.
fn draws(
    gpu: &common::Gpu,
    probe: &RandProbe,
    pixel: [u32; 2],
    seed: u32,
    scalar: bool,
) -> Vec<[f32; 2]> {
    let taps: Vec<RandTap> = (0..STRATA)
        .map(|i| RandTap {
            pixel,
            sample_index: i,
            strata: STRATA,
            seed,
            dim: DIM,
            scalar: u32::from(scalar),
            _pad: 0,
        })
        .collect();
    let mut readback = probe.submit(&gpu.device, &gpu.queue, &taps);
    for _ in 0..2000 {
        match readback.poll(&gpu.device) {
            ColorPoll::Ready(values) => return values.iter().map(|v| [v[0], v[1]]).collect(),
            ColorPoll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            ColorPoll::Failed => panic!("rand readback failed"),
        }
    }
    panic!("rand readback never resolved");
}

/// The multiset of grid cells a sequence visited, sorted so two sequences
/// compare as sets.
fn cell_multiset(points: &[[f32; 2]]) -> Vec<(u32, u32)> {
    let mut cells: Vec<(u32, u32)> = points
        .iter()
        .map(|p| {
            let g = GRID as f32;
            (
                ((p[0] * g) as u32).min(GRID - 1),
                ((p[1] * g) as u32).min(GRID - 1),
            )
        })
        .collect();
    cells.sort_unstable();
    cells
}

/// A scatter of pixels: neighbours, a diagonal pair, and far-apart ones, so a
/// decorrelation that only keys on one coordinate or only on locality fails.
const PIXELS: [[u32; 2]; 8] = [
    [0, 0],
    [1, 0],
    [0, 1],
    [1, 1],
    [7, 3],
    [64, 64],
    [123, 456],
    [800, 600],
];

#[test]
fn different_pixels_draw_different_point_sets() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let probe = RandProbe::new(&gpu.device);

    // The comparison is over cell multisets rather than raw values, and that
    // choice is the test. Raw sequences differed across pixels before the
    // rotation too, because the jitter inside each cell is seeded per pixel;
    // asserting on them would have passed against the defect. What was
    // byte-identical across every pixel was the set of cells visited, so the
    // set of cells is what has to differ.
    let sets: Vec<Vec<(u32, u32)>> = PIXELS
        .iter()
        .map(|&pixel| {
            let points = draws(&gpu, &probe, pixel, 1, false);
            for p in &points {
                assert!(
                    (0.0..1.0).contains(&p[0]) && (0.0..1.0).contains(&p[1]),
                    "draw {p:?} for pixel {pixel:?} left the unit square"
                );
            }
            cell_multiset(&points)
        })
        .collect();

    for a in 0..sets.len() {
        for b in (a + 1)..sets.len() {
            assert_ne!(
                sets[a], sets[b],
                "pixels {:?} and {:?} drew the same cell set; the sampler is \
                 integrating both with one quadrature rule",
                PIXELS[a], PIXELS[b]
            );
        }
    }
}

#[test]
fn a_pixels_draws_stay_stratified_on_each_axis() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let probe = RandProbe::new(&gpu.device);

    // A rigid toroidal shift of a stratified set is still stratified: each
    // shifted one-in-sixty-four bin holds exactly one point, so the largest
    // toroidal gap between consecutive sorted values stays under two bins.
    // The canonical once-per-stratum assertion is deliberately not made,
    // because the shift legitimately splits the two bins at its seam; the gap
    // formulation is the property convergence actually rests on, and white
    // noise fails it overwhelmingly.
    let bound = 2.0 / STRATA as f32 + 1e-4;
    for &pixel in &PIXELS {
        let points = draws(&gpu, &probe, pixel, 1, false);
        for axis in 0..2 {
            let mut values: Vec<f32> = points.iter().map(|p| p[axis]).collect();
            assert!(
                toroidal_gap(&mut values) <= bound,
                "pixel {pixel:?} axis {axis} exceeds the gap bound of {bound}; \
                 the rotation broke stratification"
            );
        }
    }
}

/// The largest gap between consecutive sorted values, wrapping around one.
fn toroidal_gap(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let mut max_gap = values[0] + 1.0 - values[values.len() - 1];
    for w in values.windows(2) {
        max_gap = max_gap.max(w[1] - w[0]);
    }
    max_gap
}

#[test]
fn different_pixels_draw_different_scalar_combs() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let probe = RandProbe::new(&gpu.device);

    // The scalar path carries the lobe pick, the roulette and the light pick,
    // and its permutation has the same property as the pair path's: it
    // reorders the comb and never moves it. The comparison is over stratum
    // multisets for the same reason the pair test compares cell sets.
    let combs: Vec<Vec<u32>> = PIXELS
        .iter()
        .map(|&pixel| {
            let values = draws(&gpu, &probe, pixel, 1, true);
            let mut comb: Vec<u32> = values
                .iter()
                .map(|v| ((v[0] * STRATA as f32) as u32).min(STRATA - 1))
                .collect();
            comb.sort_unstable();
            comb
        })
        .collect();

    for a in 0..combs.len() {
        for b in (a + 1)..combs.len() {
            assert_ne!(
                combs[a], combs[b],
                "pixels {:?} and {:?} drew the same scalar comb",
                PIXELS[a], PIXELS[b]
            );
        }
    }
}

#[test]
fn a_pixels_scalar_draws_stay_stratified() {
    let Some(gpu) = common::gpu_or_skip() else {
        return;
    };
    let probe = RandProbe::new(&gpu.device);

    let bound = 2.0 / STRATA as f32 + 1e-4;
    for &pixel in &PIXELS {
        let mut values: Vec<f32> = draws(&gpu, &probe, pixel, 1, true)
            .iter()
            .map(|v| v[0])
            .collect();
        assert!(
            toroidal_gap(&mut values) <= bound,
            "pixel {pixel:?} exceeds the scalar gap bound of {bound}; the \
             rotation broke stratification"
        );
    }
}
