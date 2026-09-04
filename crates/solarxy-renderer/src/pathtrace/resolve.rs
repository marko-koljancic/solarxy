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
    /// The transparent render's variant: the same copy, with the coverage
    /// counts and the drawn-sample total bound beside the mean so the alpha
    /// it writes is a real matte rather than the constant one. A second
    /// pipeline rather than a mode on the first, so the opaque path's
    /// pipeline, layout and arithmetic are untouched objects rather than a
    /// branch believed inert.
    matte_layout: wgpu::BindGroupLayout,
    matte_pipeline: wgpu::RenderPipeline,
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

        let matte_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_resolve_matte_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // The kernel's coverage counts, read-only here: the fragment
                // stage divides them by the drawn total, it never writes them.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let matte_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Pathtrace Resolve Matte Pipeline Layout"),
                bind_group_layouts: &[&matte_layout],
                push_constant_ranges: &[],
            });
        let matte_pipeline = crate::pipeline_builder::PipelineBuilder::new(
            device,
            "Pathtrace Resolve Matte Pipeline",
            &matte_pipeline_layout,
            &shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_resolve_matte")
        .color_format(Texture::HDR_FORMAT)
        .no_blend()
        .no_depth()
        .build();
        Self {
            layout,
            pipeline,
            matte_layout,
            matte_pipeline,
        }
    }

    /// Encodes the copy from `source` into the `viewport`-sized corner of
    /// `target`.
    ///
    /// Clears rather than loads, because everything the target held is
    /// about to be overwritten or belongs to a pane that has already been
    /// composited this frame: panes render sequentially through the shared
    /// target, so a whole-target clear here is the same discipline every
    /// raster pass already follows.
    ///
    /// The viewport is the pane's size in texels, which can be smaller
    /// than the target (a split layout sizes the shared target to its
    /// largest pane) and larger than the source (the preview's resolution
    /// scale). Under a viewport the quad's normalized coordinate becomes
    /// pane-relative, which is what lets one fragment shader serve the
    /// exact copy, the upscale, and the sub-target pane at once.
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
        viewport: (u32, u32),
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Resolve Bind Group"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        Self::encode_pass(encoder, &self.pipeline, &bind_group, target, viewport);
    }

    /// The transparent render's resolve: the same copy, writing the coverage
    /// fraction as alpha instead of the constant one.
    ///
    /// `coverage` is the target's own count buffer and `drawn` a uniform
    /// holding how many samples the run has integrated so far. The buffer
    /// lives on the pane rather than on this pass, because a queue write lands
    /// before the whole submission: one shared buffer written per pane would
    /// hand every pane the last pane's total.
    pub fn encode_matte(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        coverage: &wgpu::Buffer,
        drawn: &wgpu::Buffer,
        target: &wgpu::TextureView,
        viewport: (u32, u32),
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Resolve Matte Bind Group"),
            layout: &self.matte_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: coverage.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: drawn.as_entire_binding(),
                },
            ],
        });
        Self::encode_pass(encoder, &self.matte_pipeline, &bind_group, target, viewport);
    }

    /// The pass both resolves share: clear, viewport, one triangle.
    fn encode_pass(
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        target: &wgpu::TextureView,
        viewport: (u32, u32),
    ) {
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
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        #[allow(clippy::cast_precision_loss)]
        pass.set_viewport(
            0.0,
            0.0,
            viewport.0.max(1) as f32,
            viewport.1.max(1) as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(0, 0, viewport.0.max(1), viewport.1.max(1));
        pass.draw(0..3, 0..1);
    }
}
