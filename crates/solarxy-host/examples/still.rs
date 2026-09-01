//! The still render, driven natively: the endurance run, a way to look at the
//! result, and the performance probe.
//!
//! Both shells have a render entry now, but the driver is shared, and the claim
//! that a still survives being run over and over is about the driver rather
//! than about a menu. So this drives the same job the shells drive, with the
//! same tiling and the same readbacks, and reports what it found.
//!
//! The probe is the same argument applied to performance. The milestone asks
//! for a set of figures at every stage exit, taken on the reference machine and
//! never in CI, where a runner's software rasterizer would describe the runner.
//! Taking them through the shipped job rather than through a rig written for
//! the occasion is what makes them mean anything a year from now.
//!
//! **Traversal throughput is deliberately not here.** It lives in
//! `solarxy-renderer/tests/pathtrace_perf.rs`, which measures the coherent and
//! the incoherent case and prints the hierarchy build beside them. A second
//! instrument answering the same question is a second answer to disagree with.
//!
//! ```text
//! still endurance [--jobs N] [--width W] [--height H] [--samples S] [--engine raster|traced]
//! still once --out image.png [--width W] [--height H] [--samples S] [--engine ...]
//! still probe [--width W] [--height H] [--samples S] [--segments N]
//! ```

use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use solarxy_bvh::{Bvh, corpus};
use solarxy_core::AABB;
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ToneMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::scene::{CookedGeometry, CookedMesh, SceneDelta, SceneObjectId, SceneOp};
use solarxy_core::view_config::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout};
use solarxy_host::still::{StillEngine, StillRenderJob, StillSpec, StillStep, TILE_BUDGET_PIXELS};
use solarxy_host::{RasterBackend, StillCtx};
use solarxy_renderer::backend::{FrameCtx, FrameOutcome, PaneContent, RenderBackend};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::{SceneEnvironment, placeholder_bounds};
use solarxy_renderer::frame::{Renderer, RendererInit};
use solarxy_renderer::panes::PaneRect;
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};
use solarxy_renderer::scene::BackgroundModeExt;
use solarxy_renderer::visualization::VisualizationState;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("endurance") => endurance(&args[1..]),
        Some("once") => once(&args[1..]),
        Some("probe") => probe(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  still endurance [--jobs N] [--width W] [--height H] [--samples S] [--engine raster|traced]\n  still once --out <path.png> [--width W] [--height H] [--samples S] [--engine raster|traced]\n  still probe [--width W] [--height H] [--samples S] [--segments N]"
            );
            Ok(())
        }
    }
}

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn arg_u32(args: &[String], key: &str, default: u32) -> u32 {
    arg(args, key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn engine_of(args: &[String]) -> StillEngine {
    match arg(args, "--engine").as_deref() {
        Some("traced" | "pathTraced") => StillEngine::PathTraced,
        _ => StillEngine::Raster,
    }
}

struct Host {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    env: SceneEnvironment,
    camera: CameraState,
    format: wgpu::TextureFormat,
}

fn host() -> anyhow::Result<Host> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    // The core WebGPU floor, which is what the shells guarantee everywhere
    // even though they now raise two buffer size limits off the adapter: an
    // endurance run at more than the guaranteed floor would prove nothing
    // about the machines the app has to hold up on.
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("still endurance"),
        required_features: wgpu::Features::empty(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))?;

    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: 1024,
        height: 1024,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    let background = BackgroundMode::GRADIENT.resolve(&[]);
    let (sky_top, sky_bottom) = background.sky_colors();
    let init = RendererInit {
        msaa_sample_count: 4,
        gradient_top: [0.35, 0.41, 0.47, 1.0],
        gradient_bottom: [0.66, 0.70, 0.72, 1.0],
        sky_top,
        sky_bottom,
        wireframe_color: background.wireframe_color(),
        wireframe_line_width: LineWeight::Medium.width_px(),
        bloom_enabled: false,
        ssao_enabled: false,
        tone_mode: ToneMode::AcesFilmic,
        exposure: 1.0,
        ibl_mode: IblMode::Full,
        uv_checker_png: include_bytes!("../../../res/textures/uv-checker_1k.png"),
    };
    let renderer = Renderer::new(&device, &queue, &config, &init)
        .map_err(|e| anyhow::anyhow!("Renderer::new: {e}"))?;

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
    Ok(Host {
        device,
        queue,
        renderer,
        env,
        camera,
        format,
    })
}

fn scene() -> SceneDelta {
    let sphere = solarxy_kernel::primitives::generate_sphere(1.0, 48, 24);
    let material = RawMaterialData {
        base_color_factor: [0.8, 0.75, 0.7, 1.0],
        roughness_factor: 0.6,
        metallic_factor: 0.0,
        ..RawMaterialData::default()
    };
    SceneDelta {
        ops: vec![SceneOp::UpsertGeometry {
            id: SceneObjectId(1),
            geometry: Arc::new(CookedGeometry {
                meshes: vec![CookedMesh {
                    name: sphere.name,
                    positions: sphere.positions,
                    normals: sphere.normals,
                    tex_coords: sphere.tex_coords,
                    indices: sphere.indices,
                    material_index: Some(0),
                    topology: sphere.topology,
                    colors: None,
                    instances: None,
                }],
                materials: vec![Arc::new(material)],
                bounds: placeholder_bounds(),
            }),
        }],
    }
}

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
        pane_engine: solarxy_core::view_config::PaneEngine::Raster,
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

/// Runs one job to completion and returns the assembled RGBA8 image.
fn run_one(
    h: &mut Host,
    backend: &mut dyn RenderBackend,
    spec: StillSpec,
) -> anyhow::Result<Vec<u8>> {
    let pds = pane_settings();
    let display = display_settings();
    let background = BackgroundMode::GRADIENT.resolve(&[]);
    let bounds: AABB = placeholder_bounds();
    let format = h.format;

    let mut job = StillRenderJob::new(spec);
    let spec = job.spec();
    let mut image = vec![0u8; (spec.width as usize) * (spec.height as usize) * 4];
    let row = spec.width as usize * 4;

    while let Some(tile) = job.current() {
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
                now_ms: 0,
            };
            job.advance(&mut ctx, backend)
        };
        match step {
            // A shell is paced by its display and never spins here. This has
            // nothing pacing it, so it yields rather than polling a readback as
            // fast as the processor will let it.
            // Unreachable: this driver asks for no previews. It renders to a
            // file and to a stopwatch, and neither is watching.
            StillStep::Working | StillStep::Preview => std::thread::yield_now(),
            StillStep::Tile => {
                while let Some(t) = job.take_tile() {
                    for y in 0..t.rect.height as usize {
                        let dst = (t.rect.y as usize + y) * row + t.rect.x as usize * 4;
                        let src = y * t.rect.width as usize * 4;
                        image[dst..dst + t.rect.width as usize * 4]
                            .copy_from_slice(&t.pixels[src..src + t.rect.width as usize * 4]);
                    }
                }
            }
            StillStep::Done => break,
            StillStep::Failed => anyhow::bail!("a tile readback failed"),
        }
    }
    Ok(image)
}

fn backend_for(h: &Host, engine: StillEngine, samples: u32) -> Box<dyn RenderBackend> {
    match engine {
        StillEngine::Raster => {
            let mut b = RasterBackend::new(Arc::clone(&h.renderer.layouts));
            b.apply(&h.device, &h.queue, &scene());
            Box::new(b)
        }
        StillEngine::PathTraced => {
            let mut b = PathBackend::new(&h.device, &h.queue);
            b.apply(&h.device, &h.queue, &scene());
            b.set_sky([0.8, 0.85, 1.0], [0.05, 0.04, 0.03]);
            b.set_settings(TraceSettings {
                samples,
                // The browser paces at one sample per animation frame; native
                // has no frame to pace against, so a larger chunk is the same
                // work in fewer submissions.
                chunk: 8.min(samples.max(1)),
                ..TraceSettings::default()
            });
            Box::new(b)
        }
    }
}

/// Thirty consecutive jobs, or however many were asked for.
///
/// The point is device survival rather than convergence, so the default sample
/// count is low: what loses a device is the allocation churn, the readbacks and
/// the sheer number of submissions, and those are per tile rather than per
/// sample. One full-quality run is a separate invocation.
fn endurance(args: &[String]) -> anyhow::Result<()> {
    let jobs = arg_u32(args, "--jobs", 30);
    let width = arg_u32(args, "--width", 4096);
    let height = arg_u32(args, "--height", 2304);
    let samples = arg_u32(args, "--samples", 4);
    let engine = engine_of(args);

    let mut h = host().context("no usable adapter")?;
    let mut backend = backend_for(&h, engine, samples);
    let spec = StillSpec {
        width,
        height,
        engine,
        samples,
        screen_space_post: false,
        tile_budget: TILE_BUDGET_PIXELS,
        readback: solarxy_host::still::StillReadback::Display8,
        // The endurance run measures device survival, not output: an auxiliary
        // copy per tile would measure a copy.
        aux: false,
        depth: false,
        preview_interval_ms: 0,
    };
    let tiles = StillRenderJob::new(spec).plan().len();
    println!(
        "STILL endurance: {jobs} jobs of {width}x{height} at {samples} spp, {tiles} tiles each"
    );

    let started = std::time::Instant::now();
    for i in 0..jobs {
        let at = std::time::Instant::now();
        let image = run_one(&mut h, backend.as_mut(), spec)?;
        anyhow::ensure!(
            image.iter().any(|b| *b != 0),
            "job {i} assembled an entirely black image"
        );
        println!(
            "STILL job {:>3} ok in {:.2}s",
            i + 1,
            at.elapsed().as_secs_f64()
        );
    }
    println!(
        "STILL endurance complete: {jobs} jobs, {:.1}s total, no device loss",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// One job, written to a PNG.
fn once(args: &[String]) -> anyhow::Result<()> {
    let out = arg(args, "--out").context("--out <path.png> is required")?;
    let width = arg_u32(args, "--width", 4096);
    let height = arg_u32(args, "--height", 2304);
    let samples = arg_u32(args, "--samples", 256);
    let engine = engine_of(args);

    let mut h = host().context("no usable adapter")?;
    let mut backend = backend_for(&h, engine, samples);
    let spec = StillSpec {
        width,
        height,
        engine,
        samples,
        screen_space_post: false,
        tile_budget: TILE_BUDGET_PIXELS,
        readback: solarxy_host::still::StillReadback::Display8,
        // The endurance run measures device survival, not output: an auxiliary
        // copy per tile would measure a copy.
        aux: false,
        depth: false,
        preview_interval_ms: 0,
    };
    let tiles = StillRenderJob::new(spec).plan().len();
    println!("STILL {width}x{height} at {samples} spp in {tiles} tiles");
    let at = std::time::Instant::now();
    let image = run_one(&mut h, backend.as_mut(), spec)?;
    println!("STILL rendered in {:.1}s", at.elapsed().as_secs_f64());

    image::RgbaImage::from_raw(width, height, image)
        .context("the assembled buffer does not match the requested size")?
        .save(&out)
        .with_context(|| format!("writing {out}"))?;
    println!("STILL wrote {out}");
    Ok(())
}

/// Timed runs per figure, after one untimed one.
///
/// The untimed run is not politeness. Shader specialization and first-touch
/// buffer residency land on whichever run goes first, and reporting that run
/// would show up as thermal drift with the sign inverted.
const PROBE_RUNS: usize = 3;

/// The performance probe: the figures the milestone asks for at a stage exit,
/// measured through the shipped code.
///
/// Read every number against the machine it was taken on and against the same
/// number from an earlier stage, never in absolute terms. The reference machine
/// is fanless, so a figure taken while something else is compiling describes
/// the compile.
fn probe(args: &[String]) -> anyhow::Result<()> {
    let width = arg_u32(args, "--width", 1920);
    let height = arg_u32(args, "--height", 1080);
    let samples = arg_u32(args, "--samples", 32);
    let segments = arg_u32(args, "--segments", 1000);

    println!("PROBE best of {PROBE_RUNS} timed runs after one untimed run");
    probe_build();

    let mut h = host().context("no usable adapter")?;
    let delta = probe_scene(segments);
    let triangles: usize = delta
        .ops
        .iter()
        .map(|op| match op {
            SceneOp::UpsertGeometry { geometry, .. } => {
                geometry.meshes.iter().map(|m| m.indices.len() / 3).sum()
            }
            _ => 0,
        })
        .sum();
    println!("PROBE scene: a sphere on a ground slab, {triangles} triangles");

    let mut backend = PathBackend::new(&h.device, &h.queue);
    backend.set_sky([0.8, 0.85, 1.0], [0.05, 0.04, 0.03]);
    let at = Instant::now();
    backend.apply(&h.device, &h.queue, &delta);
    println!(
        "PROBE ingest: {:.1} ms to hierarchies, arena and upload",
        at.elapsed().as_secs_f64() * 1000.0
    );

    probe_still(&mut h, &mut backend, width, height, samples)?;
    probe_preview(&mut h, &mut backend, width, height)?;
    Ok(())
}

/// The hierarchy build, by triangle count.
///
/// The corpus the parity tests and the throughput test both draw from, so a
/// build figure here and a build figure there describe the same geometry.
fn probe_build() {
    for (width, height) in [(125u32, 62u32), (250, 125), (500, 250), (1000, 500)] {
        let (positions, indices) = corpus::sphere(width, height);
        let triangles = indices.len() / 3;

        let warm = Bvh::build_triangles(&positions, &indices);
        let mut best = f64::INFINITY;
        for _ in 0..PROBE_RUNS {
            let at = Instant::now();
            let bvh = Bvh::build_triangles(&positions, &indices);
            best = best.min(at.elapsed().as_secs_f64());
            // The builder is pure and its result is dropped, which is exactly
            // the shape an optimizer is allowed to delete outright.
            std::hint::black_box(&bvh);
        }
        let stats = warm.stats();
        println!(
            "PROBE build {triangles:>9} triangles: {:>8.1} ms, depth {}, {} nodes",
            best * 1000.0,
            stats.max_depth,
            stats.node_count
        );
    }
}

/// Seconds per sample, through the job a shell drives.
///
/// The figure covers the whole job rather than the kernel alone: the tile plan,
/// the readback and the assembly are what a person waits through, and a
/// per-sample number that excluded them would describe a render nobody runs.
fn probe_still(
    h: &mut Host,
    backend: &mut PathBackend,
    width: u32,
    height: u32,
    samples: u32,
) -> anyhow::Result<()> {
    let samples = samples.max(1);
    backend.set_settings(TraceSettings {
        samples,
        chunk: 8.min(samples),
        ..TraceSettings::default()
    });
    let spec = StillSpec {
        width,
        height,
        engine: StillEngine::PathTraced,
        samples,
        screen_space_post: false,
        tile_budget: TILE_BUDGET_PIXELS,
        readback: solarxy_host::still::StillReadback::Display8,
        aux: false,
        depth: false,
        preview_interval_ms: 0,
    };
    let tiles = StillRenderJob::new(spec).plan().len();

    let at = Instant::now();
    let image = run_one(h, backend, spec)?;
    let secs = at.elapsed().as_secs_f64();
    anyhow::ensure!(
        image.iter().any(|b| *b != 0),
        "the probe still assembled an entirely black image"
    );
    println!(
        "PROBE still {width}x{height} at {samples} spp in {tiles} tile(s): {secs:.1} s, {:.3} s per sample",
        secs / f64::from(samples)
    );
    Ok(())
}

/// One denoised sample at half resolution scale, which is the interactive
/// preview's frame.
///
/// The native lower bound on the browser's budget rather than the budget
/// itself: only a browser can answer what a browser costs. It measures the
/// dispatch, the filter and the resolve, and not the shared composite, which
/// is what the gate this compares against measured.
fn probe_preview(
    h: &mut Host,
    backend: &mut PathBackend,
    width: u32,
    height: u32,
) -> anyhow::Result<()> {
    backend.set_settings(TraceSettings {
        samples: 1,
        chunk: 1,
        denoise: true,
        resolution_scale: 0.5,
        ..TraceSettings::default()
    });
    h.renderer.resize_targets(&h.device, width, height);
    h.camera.resize(width as f32 / (height.max(1)) as f32);
    // A shell writes the camera's uniform once a frame and the kernel reads it
    // through a bind group, so a frame driven from here owes the same write.
    // Zero delta: there is no transition to advance.
    h.camera.update(&h.queue, 0.0);

    let mut best = f64::INFINITY;
    for run in 0..=PROBE_RUNS {
        // Every timed frame starts from nothing, which is what a preview pays
        // on the frame after a camera move. Without it the pane is already at
        // its one sample and the encode returns without dispatching, so the
        // figure would be the cost of asking.
        backend.invalidate();
        let at = Instant::now();
        let outcome = probe_frame(h, backend, width, height);
        let secs = at.elapsed().as_secs_f64();
        anyhow::ensure!(
            outcome == FrameOutcome::Complete,
            "a one-sample preview frame did not finish in one encode"
        );
        if run > 0 {
            best = best.min(secs);
        }
    }
    println!(
        "PROBE preview 1 spp denoised at scale 0.5 on a {width}x{height} pane: {:.2} ms",
        best * 1000.0
    );
    Ok(())
}

/// One pane encoded and submitted, the way a shell's frame loop does it.
fn probe_frame(h: &mut Host, backend: &mut PathBackend, width: u32, height: u32) -> FrameOutcome {
    let pds = pane_settings();
    let display = display_settings();
    let background = BackgroundMode::GRADIENT.resolve(&[]);
    let bounds: AABB = placeholder_bounds();
    let cam_data = h.camera.camera;
    let rect = PaneRect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let mut encoder = h
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Probe Preview Encoder"),
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
            look: CompositeLook::default(),
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
    h.queue.submit(Some(encoder.finish()));
    // The wall clock around a submit is the measurement, so this blocks where
    // a shell would poll once and get on with something else.
    let _ = h.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    outcome
}

/// The probe's scene: a sphere at the requested tessellation on a ground slab.
///
/// The slab is load-bearing rather than decorative. Without it nearly every ray
/// that leaves the sphere escapes to the sky on its first bounce, and the
/// figures describe how fast the hierarchy can be missed. The throughput test
/// puts a floor under its scene for the same reason, so the two sets of numbers
/// describe comparable work.
fn probe_scene(segments: u32) -> SceneDelta {
    let segments = segments.max(4);
    let sphere = solarxy_kernel::primitives::generate_sphere(1.0, segments, segments / 2);
    let slab = solarxy_kernel::primitives::generate_box(20.0, 0.4, 20.0, 1, 1, 1);
    // Dropped under the sphere. The generator centers on the origin and the
    // positions are shared, so the offset builds its own.
    let dropped: Vec<[f32; 3]> = slab
        .positions
        .iter()
        .map(|p| [p[0], p[1] - 1.4, p[2]])
        .collect();

    let material = RawMaterialData {
        base_color_factor: [0.8, 0.75, 0.7, 1.0],
        roughness_factor: 0.6,
        metallic_factor: 0.0,
        ..RawMaterialData::default()
    };
    SceneDelta {
        ops: vec![SceneOp::UpsertGeometry {
            id: SceneObjectId(1),
            geometry: Arc::new(CookedGeometry {
                meshes: vec![
                    CookedMesh {
                        name: sphere.name,
                        positions: sphere.positions,
                        normals: sphere.normals,
                        tex_coords: sphere.tex_coords,
                        indices: sphere.indices,
                        material_index: Some(0),
                        topology: sphere.topology,
                        colors: None,
                        instances: None,
                    },
                    CookedMesh {
                        name: slab.name,
                        positions: Arc::new(dropped),
                        normals: slab.normals,
                        tex_coords: slab.tex_coords,
                        indices: slab.indices,
                        material_index: Some(0),
                        topology: slab.topology,
                        colors: None,
                        instances: None,
                    },
                ],
                materials: vec![Arc::new(material)],
                bounds: placeholder_bounds(),
            }),
        }],
    }
}
