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

use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ToneMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::scene::{CookedGeometry, CookedMesh, SceneDelta, SceneObjectId, SceneOp};
use solarxy_core::view_config::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout};
use solarxy_renderer::scene::BackgroundModeExt;
use std::sync::Arc;
use solarxy_renderer::backend::{FrameCtx, FrameOutcome, PaneContent, RenderBackend};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::{SceneEnvironment, placeholder_bounds};
use solarxy_renderer::frame::{Renderer, RendererInit};
use solarxy_renderer::panes::PaneRect;
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};
use solarxy_renderer::visualization::VisualizationState;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// The sky the tracer integrates against, and the only light in these scenes.
///
/// Bright above and dark below, and the gradient is load-bearing rather than
/// decorative: a *uniform* environment over a conserving surface returns the
/// same value every sample, so every partial mean would be bit-identical and a
/// test comparing two of them would prove nothing while passing.
const SKY_UP: [f32; 3] = [0.8, 0.85, 1.0];
const SKY_DOWN: [f32; 3] = [0.05, 0.04, 0.03];

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    env: SceneEnvironment,
    camera: CameraState,
    surface: wgpu::Texture,
    surface_view: wgpu::TextureView,
    format: wgpu::TextureFormat,
}

/// A headless renderer, sized to one pane.
///
/// `None` when the machine has no adapter, which is the same skip every other
/// GPU test in the workspace takes.
fn harness() -> Option<Harness> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    // Exactly what both shells ask for. More would prove something the browser
    // cannot run.
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("traced backend test device"),
        required_features: wgpu::Features::empty(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: WIDTH,
        height: HEIGHT,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    let background = BackgroundMode::GRADIENT.resolve(&[]);
    let (sky_top, sky_bottom) = background.sky_colors();
    let init = RendererInit {
        msaa_sample_count: 1,
        gradient_top: [0.35, 0.41, 0.47, 1.0],
        gradient_bottom: [0.66, 0.70, 0.72, 1.0],
        sky_top,
        sky_bottom,
        wireframe_color: background.wireframe_color(),
        wireframe_line_width: LineWeight::Medium.width_px(),
        // Off for both: a traced pane never runs ambient occlusion, and a bloom
        // that spread one pixel into its neighbours would make the constant
        // below stop being constant.
        bloom_enabled: false,
        ssao_enabled: false,
        tone_mode: ToneMode::AcesFilmic,
        exposure: 1.0,
        ibl_mode: IblMode::Full,
        uv_checker_png: include_bytes!("../../../res/textures/uv-checker_1k.png"),
    };
    let renderer = Renderer::new(&device, &queue, &config, &init).ok()?;

    let bounds = placeholder_bounds();
    let vis = VisualizationState::new_from_parts(
        &device,
        &renderer.layouts,
        &bounds,
        &[],
        None,
        background.grid_color(),
    );
    let env = SceneEnvironment::new(
        &device,
        &queue,
        &renderer.layouts,
        &bounds,
        1.0,
        &renderer.ibl_res.brdf_lut,
        &renderer.ibl_res.ltc,
        1024,
        vis,
    );
    let camera = CameraState::new(&device, &renderer.layouts.camera, &bounds, 1.0);

    let surface = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Traced Backend Surface"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let surface_view = surface.create_view(&wgpu::TextureViewDescriptor::default());

    Some(Harness {
        device,
        queue,
        renderer,
        env,
        camera,
        surface,
        surface_view,
        format,
    })
}

/// A plain shaded pane with the overlays off.
///
/// Spelled out rather than defaulted because neither struct has a `Default`,
/// deliberately: the two shells disagree about several of these and a default
/// would quietly pick one.
fn pane_settings() -> PaneDisplaySettings {
    PaneDisplaySettings {
        view_mode: ViewMode::Shaded,
        prev_non_ghosted_mode: ViewMode::Shaded,
        ghosted_wireframe: false,
        normals_mode: NormalsMode::Off,
        background_mode: BackgroundMode::GRADIENT,
        uv_mode: UvMode::Off,
        bounds_mode: BoundsMode::Off,
        line_weight: LineWeight::Medium,
        show_grid: false,
        show_axis_gizmo: false,
        show_local_axes: false,
        inspection_mode: InspectionMode::Shaded,
        material_override: MaterialOverride::None,
        texel_density_target: 1.0,
        pane_mode: PaneMode::Scene3D,
        uv_bg: UvMapBackground::Dark,
        uv_offset: [0.0, 0.0],
        uv_zoom: 1.0,
        show_uv_overlap: false,
        show_validation: false,
        turntable_active: false,
    }
}

fn display_settings() -> DisplaySettings {
    DisplaySettings {
        turntable_active: false,
        turntable_rpm: 6.0,
        lights_locked: false,
        layout: ViewLayout::Single,
        split_ratio: DisplaySettings::DEFAULT_SPLIT_RATIO,
        roughness_scale: 1.0,
        metallic_scale: 1.0,
        hdri_rotation: 0.0,
        hdri_intensity: solarxy_core::view_config::DEFAULT_HDRI_INTENSITY,
        point_size: solarxy_core::view_config::DEFAULT_POINT_SIZE,
    }
}

fn skip_or(h: Option<Harness>) -> Option<Harness> {
    if h.is_none() {
        assert!(
            std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
            "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
        );
        eprintln!("skipping: no GPU adapter available");
    }
    h
}

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

/// Read the composited surface back as RGBA8.
fn read_surface(h: &Harness) -> Vec<u8> {
    let mut encoder = h
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Traced Readback"),
        });
    let (buffer, padded) = solarxy_renderer::capture::encode_capture(
        &h.device,
        &mut encoder,
        &h.surface,
        (0, 0, WIDTH, HEIGHT),
    );
    h.queue.submit(Some(encoder.finish()));
    let pending = solarxy_renderer::capture::PendingCapture::arm(buffer, padded, WIDTH, HEIGHT);
    loop {
        match pending.poll(&h.device, h.format) {
            solarxy_renderer::capture::CapturePoll::Ready(pixels) => return pixels,
            solarxy_renderer::capture::CapturePoll::Pending => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            solarxy_renderer::capture::CapturePoll::Failed => panic!("surface readback failed"),
        }
    }
}

/// One rough sphere at the origin, delivered the way the engine delivers a
/// cooked scene.
///
/// The geometry is not decoration. Without it every sample of every pixel
/// returns the same constant sky, so a two-sample mean and a three-sample mean
/// are bit-identical and a test comparing them proves nothing. A rough surface
/// under a sky scatters differently every sample, which is what makes "did this
/// resolve the mean it just wrote" a question with an answer.
fn sphere_delta() -> SceneDelta {
    let sphere = solarxy_kernel::primitives::generate_sphere(1.0, 32, 16);
    let material = RawMaterialData {
        base_color_factor: [0.8, 0.75, 0.7, 1.0],
        // Rough, so the scatter spreads and the samples disagree.
        roughness_factor: 0.8,
        metallic_factor: 0.0,
        ..RawMaterialData::default()
    };
    let mesh = CookedMesh {
        name: sphere.name,
        positions: sphere.positions,
        normals: sphere.normals,
        tex_coords: sphere.tex_coords,
        indices: sphere.indices,
        material_index: Some(0),
        topology: sphere.topology,
        colors: None,
        instances: None,
    };
    SceneDelta {
        ops: vec![SceneOp::UpsertGeometry {
            id: SceneObjectId(1),
            geometry: Arc::new(CookedGeometry {
                meshes: vec![mesh],
                materials: vec![Arc::new(material)],
                bounds: placeholder_bounds(),
            }),
        }],
    }
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
