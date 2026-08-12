//! The shared GPU harness for this crate's integration tests.
//!
//! A headless renderer sized to one pane, a scene to put in front of it, and
//! the readback that turns a composited surface back into bytes. Two test
//! binaries drive it: the backend contract's, and the still render's.
//!
//! Each integration test binary compiles this separately, so anything only one
//! of them uses reads as dead code in the other.
#![allow(dead_code)]

use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ToneMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::geometry::RawMaterialData;
use solarxy_core::scene::{CookedGeometry, CookedMesh, SceneDelta, SceneObjectId, SceneOp};
use solarxy_core::view_config::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout};
use solarxy_renderer::scene::BackgroundModeExt;
use std::sync::Arc;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::environment::{SceneEnvironment, placeholder_bounds};
use solarxy_renderer::frame::{Renderer, RendererInit};
use solarxy_renderer::visualization::VisualizationState;

pub const WIDTH: u32 = 64;
pub const HEIGHT: u32 = 64;

/// The sky the tracer integrates against, and the only light in these scenes.
///
/// Bright above and dark below, and the gradient is load-bearing rather than
/// decorative: a *uniform* environment over a conserving surface returns the
/// same value every sample, so every partial mean would be bit-identical and a
/// test comparing two of them would prove nothing while passing.
pub const SKY_UP: [f32; 3] = [0.8, 0.85, 1.0];
pub const SKY_DOWN: [f32; 3] = [0.05, 0.04, 0.03];

pub struct Harness {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub renderer: Renderer,
    pub env: SceneEnvironment,
    pub camera: CameraState,
    pub surface: wgpu::Texture,
    pub surface_view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
}

/// A headless renderer, sized to one pane.
///
/// `None` when the machine has no adapter, which is the same skip every other
/// GPU test in the workspace takes.
pub fn harness() -> Option<Harness> {
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
        // Four, not one. The raster pass chain resolves a multisampled colour
        // attachment into the shared high-dynamic-range target, and a resolve
        // whose source is not multisampled is a validation error rather than a
        // slightly different picture. The path tracer never touches that
        // attachment, which is why a harness that only drove it was fine at
        // one sample and the still render's raster arm was not.
        msaa_sample_count: 4,
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
        uv_checker_png: include_bytes!("../../../../res/textures/uv-checker_1k.png"),
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
pub fn pane_settings() -> PaneDisplaySettings {
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

pub fn display_settings() -> DisplaySettings {
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

pub fn skip_or(h: Option<Harness>) -> Option<Harness> {
    if h.is_none() {
        assert!(
            std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
            "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter"
        );
        eprintln!("skipping: no GPU adapter available");
    }
    h
}

/// Read the composited surface back as RGBA8.
pub fn read_surface(h: &Harness) -> Vec<u8> {
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
pub fn sphere_delta() -> SceneDelta {
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
