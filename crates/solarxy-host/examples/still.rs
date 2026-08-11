//! The still render, driven natively: the endurance run, and a way to look at
//! the result.
//!
//! The desktop shell has no Render menu entry yet -- that is stage eight's --
//! but the driver is shared, and the claim that a still survives being run over
//! and over is about the driver rather than about a menu. So this drives the
//! same job the browser drives, with the same tiling and the same readbacks,
//! and reports what it found.
//!
//! ```text
//! still endurance [--jobs N] [--width W] [--height H] [--samples S] [--engine raster|traced]
//! still once --out image.png [--width W] [--height H] [--samples S] [--engine ...]
//! ```

use std::sync::Arc;

use anyhow::Context;
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
use solarxy_renderer::backend::RenderBackend;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::{SceneEnvironment, placeholder_bounds};
use solarxy_renderer::frame::{Renderer, RendererInit};
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};
use solarxy_renderer::scene::BackgroundModeExt;
use solarxy_renderer::visualization::VisualizationState;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("endurance") => endurance(&args[1..]),
        Some("once") => once(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  still endurance [--jobs N] [--width W] [--height H] [--samples S] [--engine raster|traced]\n  still once --out <path.png> [--width W] [--height H] [--samples S] [--engine raster|traced]"
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
    // Exactly what both shells ask for: an endurance run at more than the
    // shipped limits would prove nothing about the shipped app.
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

    loop {
        let Some(tile) = job.current() else {
            break;
        };
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
            // A shell is paced by its display and never spins here. This has
            // nothing pacing it, so it yields rather than polling a readback as
            // fast as the processor will let it.
            StillStep::Working => std::thread::yield_now(),
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
