//! The path tracer driven the way a shell drives it, through the real backend
//! contract and the real post chain.
//!
//! This lives here rather than in the renderer because of what it compares
//! against. The claim under test is that a traced image and a rasterized one
//! reach the composite the same way and get the same look applied, and only
//! this crate can hold both backends at once.
//!
//! Three things it is watching for, all of which passed every renderer-level
//! test while being wrong:
//!
//! 1. A converged pane keeps showing its image. The accumulator ping-pongs, and
//!    a swap on the wrong side of a dispatch leaves the finished frame reading
//!    a slot the run never wrote. Every readback inside the renderer's own
//!    tests reads the same accessor the resolve does, so they agree with each
//!    other while both being stale.
//! 2. `invalidate` actually drops the mean, rather than only being called.
//! 3. The look applies. The composite is shared by construction, so the way to
//!    check it is to composite the same values through both routes and compare.

mod common;

use solarxy_core::preferences::{BackgroundMode, InspectionMode};

use common::{
    HEIGHT, Harness, SKY_DOWN, SKY_UP, WIDTH, display_settings, harness, pane_settings,
    read_surface, skip_or, sphere_delta,
};
use solarxy_renderer::backend::{FrameCtx, FrameOutcome, PaneContent, RenderBackend};
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::placeholder_bounds;
use solarxy_renderer::panes::PaneRect;
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};

/// Encode one pane through the backend and composite it, exactly as a shell's
/// frame loop does.
fn frame(h: &mut Harness, backend: &mut PathBackend, look: CompositeLook) -> FrameOutcome {
    let rect = PaneRect {
        x: 0.0,
        y: 0.0,
        width: WIDTH as f32,
        height: HEIGHT as f32,
    };
    let pds = pane_settings();
    let display = display_settings();
    let background = BackgroundMode::GRADIENT.resolve(&[]);
    let bounds = placeholder_bounds();
    let cam_data = h.camera.camera;
    let mut encoder = h
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Traced Pane Encoder"),
        });
    let target = h.renderer.targets.hdr_resolve_view.clone();
    let outcome = backend.encode(
        &mut FrameCtx {
            device: &h.device,
            queue: &h.queue,
            renderer: &mut h.renderer,
            encoder: &mut encoder,
            index: 0,
            rect,
            is_split: false,
            pds: &pds,
            display: &display,
            background,
            camera: Some(&mut h.camera),
            env: &h.env,
            bounds: Some(&bounds),
            grid_plane: None,
            look,
            scene_present: true,
            outline: false,
            window: None,
            content: PaneContent::Scene {
                extra: None,
                selected: None,
                cam_data,
                shadow: false,
            },
        },
        &target,
    );
    solarxy_host::composite_and_submit(
        &h.queue,
        &h.renderer,
        encoder,
        &h.surface_view,
        &solarxy_host::PaneComposite {
            index: 0,
            rect,
            look,
            inspection: InspectionMode::Shaded,
            is_uv_map: false,
            scene_present: true,
            outline: false,
        },
    );
    outcome
}

fn tracer(h: &Harness, samples: u32, chunk: u32) -> PathBackend {
    let mut backend = PathBackend::new(&h.device, &h.queue);
    backend.apply(&h.device, &h.queue, &sphere_delta());
    backend.set_sky(SKY_UP, SKY_DOWN);
    backend.set_settings(TraceSettings {
        samples,
        chunk,
        ..TraceSettings::default()
    });
    backend
}

/// The one a shell would hit first: does a finished render stay on screen.
#[test]
fn a_converged_pane_keeps_resolving_the_image_it_converged_to() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    let mut backend = tracer(&h, 3, 1);

    // Three chunks of one, which is three dispatches and two swaps.
    for i in 0..3 {
        let outcome = frame(&mut h, &mut backend, CompositeLook::default());
        if i < 2 {
            assert!(
                matches!(outcome, FrameOutcome::Converging { .. }),
                "a pane with samples left reported {outcome:?}"
            );
        } else {
            assert_eq!(outcome, FrameOutcome::Complete);
        }
    }
    let converged = read_surface(&h);

    // A fourth frame draws no samples at all and re-resolves what is already
    // there. If the ping-pong swapped on the wrong side of a dispatch this is
    // where it shows: the image goes black, or reverts by one chunk.
    let outcome = frame(&mut h, &mut backend, CompositeLook::default());
    assert_eq!(outcome, FrameOutcome::Complete);
    let re_resolved = read_surface(&h);
    assert_eq!(
        converged, re_resolved,
        "a converged pane re-resolved to a different image, which means the \
         accumulator handed back a slot the run did not write last"
    );

    // And it is a picture rather than a black frame, so the equality above is
    // not two empty buffers agreeing.
    assert!(
        converged.chunks_exact(4).any(|px| px[0] > 8),
        "the traced pane composited to black"
    );
}

#[test]
fn invalidate_drops_the_mean_the_pane_had_accumulated() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    let mut backend = tracer(&h, 4, 1);

    frame(&mut h, &mut backend, CompositeLook::default());
    frame(&mut h, &mut backend, CompositeLook::default());
    assert_eq!(backend.progress(0), (2, 4));

    // What a moved camera, an edited parameter or a cooked scene reaches the
    // accumulator as. It has no idea any of those exist, which is why the
    // contract carries this call.
    backend.invalidate();
    assert_eq!(backend.progress(0), (0, 4));

    // And the next frame starts a fresh run rather than reporting itself
    // already finished.
    let outcome = frame(&mut h, &mut backend, CompositeLook::default());
    assert_eq!(
        outcome,
        FrameOutcome::Converging {
            samples: 1,
            target_samples: 4
        }
    );
}

/// The architectural claim, stated as pixels.
///
/// A traced image goes through `CompositeState` and nothing else, so applying a
/// non-neutral look has to change it in exactly the way it changes anything
/// else that target holds. The comparison is against the *same* traced image
/// composited neutrally: if the look were being skipped or applied twice, these
/// would match, and they must not.
#[test]
fn a_traced_pane_inherits_the_camera_owned_look() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    let mut backend = tracer(&h, 2, 2);

    frame(&mut h, &mut backend, CompositeLook::default());
    let neutral = read_surface(&h);

    // Exposure alone, which is the one term whose direction is unambiguous:
    // half the light reaching the tone mapper cannot come out brighter.
    let darker = CompositeLook {
        exposure: 0.25,
        ..CompositeLook::default()
    };
    backend.invalidate();
    frame(&mut h, &mut backend, darker);
    let graded = read_surface(&h);

    assert_ne!(
        neutral, graded,
        "the look made no difference to a traced pane, so the composite is not \
         the one applying it"
    );
    let brighter_anywhere = neutral
        .chunks_exact(4)
        .zip(graded.chunks_exact(4))
        .any(|(n, g)| g[0] > n[0] + 1);
    assert!(
        !brighter_anywhere,
        "a quarter of the exposure made some pixel brighter"
    );

    // A grade the composite skips entirely when neutral, so this also pins the
    // gate that keeps neutral meaning bit-identical.
    let lifted = CompositeLook {
        lift: [0.25, 0.0, 0.0],
        ..CompositeLook::default()
    };
    backend.invalidate();
    frame(&mut h, &mut backend, lifted);
    let red = read_surface(&h);
    let reds_rose = red
        .chunks_exact(4)
        .zip(neutral.chunks_exact(4))
        .filter(|(r, n)| r[0] > n[0])
        .count();
    assert!(
        reds_rose > (WIDTH * HEIGHT / 2) as usize,
        "lifting the red channel raised it on only {reds_rose} pixels"
    );
}

/// The denoise toggle, from a shell's side of the contract.
///
/// The bit-identity half of the criterion is structural rather than
/// statistical: with the filter off the resolve is handed the accumulator's own
/// view, so what reaches the composite is the running mean and nothing else has
/// touched it. What is worth checking here is the other half, that the flag is
/// wired at all, because a toggle that silently does nothing looks exactly like
/// a filter that is very gentle.
#[test]
fn the_denoise_toggle_reaches_the_image() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    let mut backend = tracer(&h, 1, 1);

    frame(&mut h, &mut backend, CompositeLook::default());
    let plain = read_surface(&h);
    backend.invalidate();
    frame(&mut h, &mut backend, CompositeLook::default());
    let plain_again = read_surface(&h);
    assert_eq!(
        plain, plain_again,
        "two runs of the same seed with the filter off disagreed, so this test \
         cannot tell the filter apart from the noise"
    );

    backend.set_settings(TraceSettings {
        samples: 1,
        chunk: 1,
        denoise: true,
        ..TraceSettings::default()
    });
    frame(&mut h, &mut backend, CompositeLook::default());
    let filtered = read_surface(&h);
    assert_ne!(
        plain, filtered,
        "turning the filter on changed nothing at one sample per pixel"
    );
}
