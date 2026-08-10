//! The traced image's handoff into the shared post chain.
//!
//! # Why this is three dozen lines and not a subsystem
//!
//! [`crate::composite::CompositeState::render`] already takes an arbitrary view
//! and applies the whole look: exposure, both grading slots, the tone map, the
//! grade, the bloom add and the ambient-occlusion multiply, with the selection
//! rim blitted after it. It was written for the rasterizer's resolve target and
//! it does not know or care what wrote it.
//!
//! So the tracer does not grow a look of its own. It writes the same target,
//! and the two render paths share one finishing chain **by construction**
//! rather than by two implementations being kept in step. That is why the
//! backend contract's `encode` takes a target view at all, and why this pass is
//! the entire cost of the arrangement.
//!
//! # Why the pipeline lives here rather than in `Pipelines`
//!
//! Everything in [`crate::pipelines::Pipelines`] is built at startup, for every
//! consumer, including the ones with no tracer. This one is built beside
//! [`crate::bind_groups::PathtraceLayouts`], which is itself built only when a
//! tracer exists, so a shell that never traces never compiles it.

use crate::texture::Texture;

/// The fullscreen pass that copies the accumulator's running mean into a linear
/// high-dynamic-range view.
pub struct TraceResolve {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl TraceResolve {
    /// Builds the pipeline. Its colour format is the shared high-dynamic-range
    /// one, which is what makes the target interchangeable with the
    /// rasterizer's.
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Resolve Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/pathtrace_resolve.wgsl").into(),
            ),
        });
        // Non-filterable, and stated rather than worked around: `rgba32float`
        // is unfilterable without an optional feature, and this pass resamples
        // nothing, so the honest declaration and the useful one are the same
        // one. The same reasoning the environment's lookup tables are declared
        // under.
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_resolve_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Resolve Pipeline Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = crate::pipeline_builder::PipelineBuilder::new(
            device,
            "Pathtrace Resolve Pipeline",
            &pipeline_layout,
            &shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_resolve")
        .color_format(Texture::HDR_FORMAT)
        .no_blend()
        .no_depth()
        .build();
        Self { layout, pipeline }
    }

    /// Encodes the copy from `source` into `target`.
    ///
    /// Clears rather than loads, because it writes every texel of the target it
    /// is given and a load would only cost a read of pixels about to be
    /// overwritten.
    ///
    /// The bind group is built per call rather than cached. A resolve happens
    /// once per pane per frame, against a source that can be reallocated by a
    /// resize and swapped by every dispatch, and caching it would mean tracking
    /// both. One descriptor per frame is not the cost worth carrying that for.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Resolve Bind Group"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Pathtrace Resolve Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
