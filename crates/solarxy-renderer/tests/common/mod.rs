//! Shared setup for the renderer's GPU integration tests.
//!
//! `headless_smoke.rs` and `scene_objects.rs` each predate this module and
//! carry their own copy of the device setup; they are left alone rather than
//! churned, and anything new comes here.

// Each integration test binary compiles this module separately, so anything
// only one of them uses reads as dead code in the others.
#![allow(dead_code)]

use solarxy_renderer::bind_groups::{BindGroupLayouts, PathtraceLayouts};

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub layouts: BindGroupLayouts,
    /// Built separately from the registry: the tracer's scene group binds
    /// six compute-stage storage buffers, which core WebGPU allows and
    /// downlevel limits do not, so a non-tracing consumer must not pay for it.
    pub pathtrace: PathtraceLayouts,
}

/// A device at the core WebGPU floor both shells guarantee, or `None`.
///
/// `Features::empty()` and `Limits::default()` are not a convenience here: the
/// milestone's whole shape depends on the tracer fitting inside core WebGPU, so
/// a test that quietly asked for more would prove something the browser cannot
/// run. The shells raise two buffer size limits off the adapter where it offers
/// more, which is a capacity they may be granted rather than one anything here
/// may depend on.
fn gpu() -> Option<Gpu> {
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
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("pathtrace test device"),
        required_features: wgpu::Features::empty(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    let layouts = BindGroupLayouts::new(&device);
    let pathtrace = PathtraceLayouts::new(&device);
    Some(Gpu {
        device,
        queue,
        layouts,
        pathtrace,
    })
}

/// A GPU, or `None` after noting the skip.
///
/// The skip is right on a developer machine without an adapter and wrong in
/// CI, so `SOLARXY_REQUIRE_GPU=1` turns a missing adapter into a failure. CI
/// sets it on the runners that have one; a silent skip is what let the whole
/// renderer suite go unrun for the life of the project.
pub fn gpu_or_skip() -> Option<Gpu> {
    match gpu() {
        Some(g) => Some(g),
        None => {
            assert!(
                std::env::var("SOLARXY_REQUIRE_GPU").as_deref() != Ok("1"),
                "SOLARXY_REQUIRE_GPU=1 but no usable GPU adapter. This runner is \
                 supposed to have one; a silent skip is what let this suite go \
                 unrun for the whole project."
            );
            eprintln!("skipping: no GPU adapter available");
            None
        }
    }
}
