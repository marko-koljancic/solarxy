use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::ibl::{BrdfLut, IblState};
use solarxy_renderer::pipelines::Pipelines;

/// Fails the test instead of skipping it when `SOLARXY_REQUIRE_GPU=1`.
///
/// These tests skip themselves when no adapter is present, which is right on a
/// developer machine without a GPU and wrong in CI: the whole GPU suite skipped
/// silently on every run for the life of the project, so zero pixels were ever
/// verified. CI sets this on the runners that do have an adapter, turning "no
/// GPU" from an invisible skip into a red build.
fn no_adapter(reason: &str) {
    assert!(
        std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
        "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter: {reason}. \
         This runner is supposed to have one; a silent skip here is what let the \
         GPU suite go unrun for the whole project."
    );
    eprintln!("no wgpu adapter available ({reason}) — skipping");
}

fn try_get_device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::SurfaceConfiguration)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        // Honour `WGPU_BACKEND` so a backend can be pinned (and so the
        // require-GPU gate can be exercised by pointing at an absent backend).
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        ..Default::default()
    });

    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) {
        Ok(a) => a,
        Err(e) => {
            no_adapter(&format!("request_adapter failed: {e}"));
            return None;
        }
    };

    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("solarxy-renderer headless smoke"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::default(),
        })) {
            Ok(dq) => dq,
            Err(e) => {
                no_adapter(&format!("request_device failed: {e}"));
                return None;
            }
        };

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
