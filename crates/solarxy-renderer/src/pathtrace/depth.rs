//! The depth pass: a single primary ray per pixel, and the distance along the
//! camera's axis to whatever it found.
//!
//! # Why it is not part of the accumulator
//!
//! Because a depth is not a quantity whose mean is the answer. A pixel on a
//! silhouette sees one surface in some samples and another in the rest, and
//! averaging two distances places a surface in the gap between them where there
//! is nothing at all. Every other channel the accumulator carries is an
//! estimate of an integral; this is a measurement.
//!
//! # What it costs when nobody asks
//!
//! Nothing. The pipeline is built the first time a depth pass is encoded, so a
//! session that only ever renders colour never compiles it, and the target is
//! allocated by whoever wants the answer.

use crate::bind_groups::PathtraceLayouts;
use crate::pathtrace::{TraceScene, TraceUniforms, WORKGROUP_SIZE};

/// The kernel: the traversal, the sampler the camera fragment reads its
/// constants from, the camera, and one entry point.
///
/// The sampler rides along without a random number ever being drawn, for the
/// same reason the debug kernel carries it: `camera.wgsl` reads `PI` from it,
/// and a kernel that cannot be built is worse than one carrying a few unused
/// functions the compiler will drop.
const DEPTH_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/rand.wgsl"),
    include_str!("../shaders/pathtrace/camera.wgsl"),
    include_str!("../shaders/pathtrace/depth.wgsl"),
);

/// What a ray that found nothing writes. Mirrors `DEPTH_MISS` in `depth.wgsl`.
///
/// Large and finite: a compositor divides by depth and tests against it, and an
/// infinity turns each of those into a value that is not a number several steps
/// later and somewhere else.
pub const DEPTH_MISS: f32 = 1e30;

/// Where a depth pass writes, and the bind group over it.
pub struct DepthTarget {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl DepthTarget {
    /// Allocates a `width` by `height` single-channel float target.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Pathtrace Depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Depth Bind Group"),
            layout: &layouts.depth,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        Self {
            texture,
            bind_group,
            width,
            height,
        }
    }

    /// The texture, for a caller copying it out.
    #[must_use]
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    #[must_use]
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }
}

/// The compute pipeline, and the empty group the layout needs in the middle.
pub struct DepthPass {
    pipeline: wgpu::ComputePipeline,
    /// Bound at group 2 and read by nothing.
    ///
    /// A pipeline layout is an array rather than a map, so a kernel that uses
    /// groups 0, 1 and 3 has to declare something at 2. The alternative was to
    /// declare the sampled group there and bind the atlas, which would make a
    /// depth pass need a texture atlas to answer a question about geometry.
    filler: wgpu::BindGroup,
}

impl DepthPass {
    /// Builds the pipeline. Fails only if the device rejects the module.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Depth Shader"),
            source: wgpu::ShaderSource::Wgsl(DEPTH_KERNEL.into()),
        });
        let empty = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_depth_filler_bind_group_layout"),
            entries: &[],
        });
        let filler = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Depth Filler"),
            layout: &empty,
            entries: &[],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Depth Pipeline Layout"),
            bind_group_layouts: &[&layouts.scene, &layouts.depth, &empty, &layouts.params],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Pathtrace Depth Pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("depth_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        Self { pipeline, filler }
    }

    /// Encodes one tile of the depth pass.
    ///
    /// The caller has already written `uniforms` with the tile this covers and
    /// an aperture radius of zero; the dispatch is sized by `tile`, which is
    /// the target's own size for an untiled render.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &TraceScene,
        target: &DepthTarget,
        uniforms: &TraceUniforms,
        tile: [u32; 2],
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Pathtrace Depth Pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, scene.bind_group(), &[]);
        pass.set_bind_group(1, &target.bind_group, &[]);
        pass.set_bind_group(2, &self.filler, &[]);
        pass.set_bind_group(3, uniforms.bind_group(), &[]);
        pass.dispatch_workgroups(
            tile[0].div_ceil(WORKGROUP_SIZE),
            tile[1].div_ceil(WORKGROUP_SIZE),
            1,
        );
    }
}
