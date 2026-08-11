//! The UV overlap statistic, and the one property it has to have.
//!
//! The percentage describes a layout: how much of it is covered more than
//! once. It is not a property of how far a reader has zoomed into the pane
//! looking at it, and for a while it was, because the pass that measured it
//! shared a camera buffer with the pass that drew the pane. Both writes landed
//! before either pass ran, so the last one won and the statistic followed the
//! gesture.
//!
//! This drives the shared pane body the way a shell does and reads the number
//! back at two zooms.

mod common;

use std::sync::Arc;

use common::{Harness, WIDTH, display_settings, harness, pane_settings, skip_or};
use solarxy_core::geometry::{MeshTopology, RawMaterialData};
use solarxy_core::preferences::{BackgroundMode, InspectionMode, PaneMode};
use solarxy_core::scene::{CookedGeometry, CookedMesh, SceneDelta, SceneObjectId, SceneOp};
use solarxy_host::RasterBackend;
use solarxy_renderer::backend::{FrameCtx, PaneContent, RenderBackend, UvSource};
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::placeholder_bounds;
use solarxy_renderer::panes::PaneRect;

/// A layout that overlaps itself, because one that does not tests nothing.
///
/// The bundled sphere's unwrap reports zero percent, and a statistic compared
/// against itself at two zooms while reading zero at both is two zeroes
/// agreeing. So this is built to have an answer strictly between the two
/// extremes: two quads stacked on the same UV rectangle, and a third elsewhere
/// with nothing over it.
///
/// The two rectangles are placed off centre and at different sizes on purpose.
/// The pane camera zooms about the middle of the unit square, so a layout
/// arranged symmetrically around that point could survive being measured
/// through the wrong camera by accident.
fn overlapping_uv_delta() -> SceneDelta {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut tex_coords: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let mut quad = |u0: f32, v0: f32, u1: f32, v1: f32, z: f32| {
        let base = u32::try_from(positions.len()).expect("small mesh");
        for (u, v) in [(u0, v0), (u1, v0), (u1, v1), (u0, v1)] {
            // The UV pass draws in layout space, so the positions only have to
            // exist and to bound something.
            positions.push([u * 2.0 - 1.0, v * 2.0 - 1.0, z]);
            normals.push([0.0, 0.0, 1.0]);
            tex_coords.push([u, v]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    // Stacked, so every texel under them is covered twice.
    quad(0.08, 0.12, 0.48, 0.52, 0.0);
    quad(0.08, 0.12, 0.48, 0.52, 0.1);
    // And one with nothing over it, so the answer is not simply a hundred.
    quad(0.60, 0.66, 0.90, 0.96, 0.0);

    SceneDelta {
        ops: vec![SceneOp::UpsertGeometry {
            id: SceneObjectId(1),
            geometry: Arc::new(CookedGeometry {
                meshes: vec![CookedMesh {
                    name: "overlapping".into(),
                    positions: Arc::new(positions),
                    normals: Some(Arc::new(normals)),
                    tex_coords: Some(Arc::new(tex_coords)),
                    indices: Arc::new(indices),
                    material_index: Some(0),
                    topology: MeshTopology::Triangles,
                    colors: None,
                    instances: None,
                }],
                materials: vec![Arc::new(RawMaterialData::default())],
                bounds: placeholder_bounds(),
            }),
        }],
    }
}

/// Long enough that a readback landing slowly is not a failure, short enough
/// that one that never lands does not hang a suite. A duration rather than an
/// iteration count, for the reason `still_render` states at length.
const READBACK_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// Renders one UV pane at `zoom` and returns the statistic it reports.
fn overlap_at(h: &mut Harness, backend: &mut RasterBackend, zoom: f32) -> f32 {
    let mut pds = pane_settings();
    pds.pane_mode = PaneMode::UvMap;
    pds.show_uv_overlap = true;
    pds.uv_zoom = zoom;
    let display = display_settings();
    let background = BackgroundMode::GRADIENT.resolve(&[]);
    let bounds = placeholder_bounds();
    let rect = PaneRect {
        x: 0.0,
        y: 0.0,
        width: WIDTH as f32,
        height: WIDTH as f32,
    };

    // What a shell sets when the layout it is measuring may have changed.
    h.renderer.uv_overlap.stats_dirty = true;
    h.renderer.uv_overlap.overlap_pct = None;

    let mut encoder = h
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("UV Pane Encoder"),
        });
    let target = h.renderer.targets.hdr_resolve_view.clone();
    backend.encode(
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
            look: CompositeLook::default(),
            scene_present: true,
            outline: false,
            window: None,
            content: PaneContent::Uv {
                source: UvSource::Scene { preferred: None },
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
            look: CompositeLook::default(),
            inspection: InspectionMode::Shaded,
            is_uv_map: true,
            scene_present: true,
            outline: false,
        },
    );

    let started = std::time::Instant::now();
    loop {
        assert!(
            started.elapsed() < READBACK_BUDGET,
            "the overlap readback never landed"
        );
        if h.renderer.uv_overlap.poll_readback(&h.device) {
            break;
        }
        std::thread::yield_now();
    }
    h.renderer
        .uv_overlap
        .overlap_pct
        .expect("a readback that reported an update carries a percentage")
}

/// The invariant, and the whole reason the statistics pass has a camera of its
/// own: zooming the pane is a reading gesture, not a measurement.
#[test]
fn the_overlap_statistic_does_not_move_when_the_pane_zooms() {
    let Some(mut h) = skip_or(harness()) else {
        return;
    };
    let mut backend = RasterBackend::new(Arc::clone(&h.renderer.layouts));
    backend.apply(&h.device, &h.queue, &overlapping_uv_delta());

    let square_on = overlap_at(&mut h, &mut backend, 1.0);
    let zoomed_in = overlap_at(&mut h, &mut backend, 4.0);
    let zoomed_out = overlap_at(&mut h, &mut backend, 0.25);
    eprintln!("overlap at zoom 1 {square_on:.3}%, at 4 {zoomed_in:.3}%, at 0.25 {zoomed_out:.3}%");

    // Without this the rest is two zeroes agreeing, which is what the sphere
    // this started with reported.
    assert!(
        square_on > 1.0 && square_on < 99.0,
        "the fixture is supposed to overlap itself partly, and reads {square_on:.3}%"
    );

    assert_eq!(
        (square_on, zoomed_in),
        (square_on, square_on),
        "zooming in changed the statistic, so it is being measured through the \
         pane's camera rather than through the layout's own"
    );
    assert_eq!(
        (square_on, zoomed_out),
        (square_on, square_on),
        "zooming out changed the statistic"
    );
}
