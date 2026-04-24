use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::ibl::{BrdfLut, IblState};
use solarxy_renderer::pipelines::Pipelines;

fn try_get_device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::SurfaceConfiguration)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("solarxy-renderer headless smoke"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::default(),
    }))
    .ok()?;

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width: 256,
        height: 256,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    Some((device, queue, config))
}

#[test]
fn renderer_components_construct_without_window() {
    let Some((device, queue, config)) = try_get_device() else {
        eprintln!("no wgpu adapter available — skipping headless smoke test");
        return;
    };

    let layouts = BindGroupLayouts::new(&device);

    let _pipelines = Pipelines::new(&device, &config, &layouts, 1);

    let ibl_fallback = IblState::fallback(&device, &queue);
    assert_eq!(
        ibl_fallback.irradiance_average.len(),
        3,
        "IBL fallback should emit a 3-channel L0 average"
    );

    let _brdf_lut = BrdfLut::generate(&device, &queue);
}
