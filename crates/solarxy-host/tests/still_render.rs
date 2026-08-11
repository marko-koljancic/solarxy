//! The still render end to end: does an image assembled from tiles match the
//! one rendered in a single pass, and does it survive being run repeatedly.
//!
//! The comparison is the whole test. A tiled render has three chances to be
//! subtly wrong and none of them looks like a crash: the camera can be windowed
//! incorrectly, so each tile is a slightly different shot and the seams step;
//! the tracer's dispatch offset can disagree with its storage coordinate, so a
//! tile draws the wrong part of the picture; and the apron can be cropped from
//! the wrong corner, so every tile is offset by a constant. Against a
//! single-pass render of the same scene, all three are immediately visible.
//!
//! Both engines are driven, because they tile by entirely different mechanisms
//! and share only the job that paces them.

mod common;

use common::{
    Harness, SKY_DOWN, SKY_UP, display_settings, harness, pane_settings, skip_or, sphere_delta,
};
use solarxy_core::preferences::BackgroundMode;
use solarxy_host::still::{
    StillEngine, StillRenderJob, StillSpec, StillStep, StillTile, TILE_BUDGET_PIXELS, TilePlan,
};
use solarxy_host::{RasterBackend, StillCtx};
use solarxy_core::AABB;
use solarxy_core::preferences::ResolvedBackground;
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};
use solarxy_renderer::backend::RenderBackend;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::placeholder_bounds;
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};
use solarxy_renderer::scene::BackgroundModeExt;

/// Small enough that a whole run is seconds, large enough that the forced tile
/// budget below cuts it into a grid rather than a single tile.
const W: u32 = 96;
const H: u32 = 72;

/// A budget that forces tiling at a testable size.
///
/// The shipped budget is four megapixels, which would make any image a test can
/// afford to render a single tile and would exercise none of this. The plan is
/// built by hand here for that reason; everything downstream of it is the
/// shipped code.
const TEST_BUDGET: u32 = 32 * 32;

fn spec(engine: StillEngine, samples: u32, budget: u32) -> StillSpec {
    StillSpec {
        width: W,
        height: H,
        engine,
        samples,
        screen_space_post: false,
        tile_budget: budget,
        readback: solarxy_host::still::StillReadback::Display8,
    }
}

/// A budget that leaves the whole image in one tile, so a tiled render has
/// something to be compared against.
///
/// The square of the *longer* edge: the plan derives a square tile edge from
/// the budget's square root, so a budget of exactly the pixel count of a
/// non-square image gives an edge shorter than its long side and two tiles.
const WHOLE: u32 = W * W;

/// Runs a job to completion, resizing the targets per tile the way a shell
/// does, and returns the assembled image as RGBA8.
fn run(h: &mut Harness, job: &mut StillRenderJob, backend: &mut dyn RenderBackend) -> Vec<u8> {
    let pds: PaneDisplaySettings = pane_settings();
    let display: DisplaySettings = display_settings();
    let background: ResolvedBackground = BackgroundMode::GRADIENT.resolve(&[]);
    let bounds: AABB = placeholder_bounds();
    let format = h.format;

    let mut image = vec![0u8; (W * H * 4) as usize];
    let mut guard = 0;
    loop {
        guard += 1;
        assert!(guard < 100_000, "the job never finished");
        let Some(tile) = job.current() else {
            break;
        };
        // The shell's job: size the shared targets to the tile before the job
        // renders into them.
        h.renderer
            .resize_targets(&h.device, tile.render.width, tile.render.height);
        let step = {
            let mut ctx = StillCtx {
                device: &h.device,
                queue: &h.queue,
                renderer: &mut h.renderer,
                camera: &mut h.camera,
                env: &h.env,
                pds: &pds,
                display: &display,
                background,
                bounds: Some(&bounds),
                look: CompositeLook::default(),
                format,
                scene_present: true,
            };
            job.advance(&mut ctx, backend)
        };
        match step {
            StillStep::Working => {}
            StillStep::Tile => {
                while let Some(t) = job.take_tile() {
                    blit(&mut image, &t);
                }
            }
            StillStep::Done => break,
            StillStep::Failed => panic!("a tile readback failed"),
        }
    }
    while let Some(t) = job.take_tile() {
        blit(&mut image, &t);
    }
    image
}

fn blit(image: &mut [u8], tile: &StillTile) {
    let row = tile.rect.width as usize * 4;
    for y in 0..tile.rect.height as usize {
        let dst = ((tile.rect.y as usize + y) * W as usize + tile.rect.x as usize) * 4;
        let src = y * row;
        image[dst..dst + row].copy_from_slice(&tile.pixels[src..src + row]);
    }
}

fn traced(h: &Harness, samples: u32) -> PathBackend {
    let mut backend = PathBackend::new(&h.device, &h.queue);
    backend.apply(&h.device, &h.queue, &sphere_delta());
    backend.set_sky(SKY_UP, SKY_DOWN);
    backend.set_settings(TraceSettings {
        samples,
        // Everything in one call, so the test is about tiling rather than about
        // pacing; the pacing is the shell's and is measured by the endurance
        // run.
        chunk: samples,
        ..TraceSettings::default()
    });
    backend
}

fn raster(h: &Harness) -> RasterBackend {
    let mut backend = RasterBackend::new(std::sync::Arc::clone(&h.renderer.layouts));
    backend.apply(&h.device, &h.queue, &sphere_delta());
    backend
}

/// Mean absolute difference between two RGBA8 images, over the colour lanes.
fn difference(a: &[u8], b: &[u8]) -> f64 {
    let mut total = 0.0f64;
    let mut count = 0u64;
    for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for c in 0..3 {
            total += f64::from(i32::from(p[c]) - i32::from(q[c])).abs();
            count += 1;
        }
    }
    total / count.max(1) as f64
}

#[test]
fn a_tiled_raster_still_matches_the_same_image_rendered_in_one_pass() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    let mut backend = raster(&h);

    let mut whole = StillRenderJob::new(spec(StillEngine::Raster, 1, WHOLE));
    let one_pass = run(&mut h, &mut whole, &mut backend);
    assert_eq!(whole.plan().len(), 1, "the whole image should be one tile");

    let mut tiled = StillRenderJob::new(spec(StillEngine::Raster, 1, TEST_BUDGET));
    assert!(
        tiled.plan().len() >= 4,
        "the forced budget should cut this into a grid, not {} tile(s)",
        tiled.plan().len()
    );
    let assembled = run(&mut h, &mut tiled, &mut backend);

    // Rasterizing the same geometry through two different projections is not
    // bit-exact: the triangles are rasterized against different pixel centres
    // and an edge lands on one side or the other. What must not happen is a
    // step at a tile boundary, which is a whole-tile shift and shows up here as
    // an average an order of magnitude larger than this.
    let diff = difference(&one_pass, &assembled);
    eprintln!("raster tiled against one pass: {diff:.3} of 255");
    // Measures exactly zero here, which is stronger than the comment above
    // expected: the windowed frustum is the same frustum, so the rasterizer
    // lands on the same pixel centres and the multisample resolve sees the same
    // coverage. The tolerance is kept small rather than zero because that is a
    // property of this hardware's rasterization rules and not of the design.
    assert!(
        diff < 0.5,
        "a tiled raster still differs from a single-pass one by {diff:.3} of 255"
    );
}

#[test]
fn a_tiled_traced_still_matches_the_same_image_rendered_in_one_pass() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    // Enough samples that the estimator's own noise is below the difference a
    // tiling mistake would produce.
    const SPP: u32 = 64;

    let mut backend = traced(&h, SPP);
    let mut whole = StillRenderJob::new(spec(StillEngine::PathTraced, SPP, WHOLE));
    let one_pass = run(&mut h, &mut whole, &mut backend);
    assert_eq!(whole.plan().len(), 1);

    backend.invalidate();
    let mut tiled = StillRenderJob::new(spec(StillEngine::PathTraced, SPP, TEST_BUDGET));
    assert!(tiled.plan().len() >= 4);
    let assembled = run(&mut h, &mut tiled, &mut backend);

    // A traced tile draws the same rays the untiled render would have drawn for
    // those pixels, because the sampler is seeded from the pixel's place in the
    // whole image rather than in the tile. So this is far tighter than the
    // raster comparison: what is left is float reassociation.
    let diff = difference(&one_pass, &assembled);
    eprintln!("traced tiled against one pass: {diff:.4} of 255");
    assert!(
        diff < 0.5,
        "a tiled traced still differs from a single-pass one by {diff:.4} of 255; \
         the tiles are not drawing the samples their pixels would have drawn"
    );
}

/// The seam test, on a field with nothing in it to hide one.
///
/// A background gradient is the adversarial case: it varies smoothly down the
/// picture with no detail to mask a discontinuity, so a tile that drew its own
/// sweep instead of its slice of the image's shows up as a hard band at every
/// horizontal boundary.
///
/// **The horizontal boundaries are the ones that matter here**, and saying so
/// is the point: the gradient is constant along a row, so a test that only
/// checked vertical boundaries would compare zero against zero and pass no
/// matter what the tiling did. It is checked in both directions below, with
/// that asymmetry stated rather than left for a reader to notice.
#[test]
fn a_flat_field_assembles_without_a_seam() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    // No geometry at all: every pixel is the background.
    let mut backend = RasterBackend::new(std::sync::Arc::clone(&h.renderer.layouts));

    let mut job = StillRenderJob::new(spec(StillEngine::Raster, 1, TEST_BUDGET));
    let columns: Vec<u32> = job
        .plan()
        .tiles
        .iter()
        .map(|t| t.image.x)
        .filter(|x| *x > 0)
        .collect();
    let rows: Vec<u32> = job
        .plan()
        .tiles
        .iter()
        .map(|t| t.image.y)
        .filter(|y| *y > 0)
        .collect();
    assert!(
        !rows.is_empty(),
        "the plan has no horizontal boundary, so this tests nothing"
    );
    let image = run(&mut h, &mut job, &mut backend);

    let at = |x: usize, y: usize| ((y * W as usize) + x) * 4;
    let step = |a: usize, b: usize| {
        (0..3)
            .map(|c| f64::from(i32::from(image[a + c]) - i32::from(image[b + c])).abs())
            .sum::<f64>()
            / 3.0
    };
    let row_step = |y: u32| {
        (0..W as usize)
            .map(|x| step(at(x, y as usize - 1), at(x, y as usize)))
            .sum::<f64>()
            / f64::from(W)
    };
    let column_step = |x: u32| {
        (0..H as usize)
            .map(|y| step(at(x as usize - 1, y), at(x as usize, y)))
            .sum::<f64>()
            / f64::from(H)
    };

    // The gradient varies down the picture, so an ordinary neighbouring row
    // already differs a little. A seam is a step much larger than that.
    let ordinary_rows: f64 = (1..H)
        .filter(|y| !rows.contains(y))
        .map(row_step)
        .sum::<f64>()
        / f64::from(H - 1 - rows.len() as u32);
    for y in &rows {
        let seam = row_step(*y);
        eprintln!("horizontal seam at y={y}: {seam:.3} against an ordinary {ordinary_rows:.3}");
        assert!(
            seam <= ordinary_rows + 1.0,
            "the step across the tile boundary at y={y} is {seam:.3}, against \
             {ordinary_rows:.3} between ordinary rows"
        );
    }
    for x in &columns {
        let seam = column_step(*x);
        eprintln!("vertical seam at x={x}: {seam:.3}");
        assert!(seam <= 1.0, "a vertical seam at x={x} of {seam:.3}");
    }
}

#[test]
fn the_shipped_budget_leaves_a_four_megapixel_still_in_whole_tiles() {
    // Not a GPU test: the arithmetic that decides how a real still is cut,
    // pinned against the size Chain B names.
    let plan = TilePlan::new(4096, 2304, TILE_BUDGET_PIXELS, 0);
    assert!(plan.len() > 1, "a 4096x2304 still should tile");
    for tile in &plan.tiles {
        assert!(tile.render.area() <= u64::from(TILE_BUDGET_PIXELS));
    }
}
