//! Final composite pass: tone-mapping HDR onto the swapchain, plus the
//! per-pane viewport/scissor rectangle that splits the surface in F2/F3.

use crate::bind_groups::BindGroupLayouts;
use crate::bloom::BLOOM_STRENGTH;
use crate::pipelines::Pipelines;
use crate::ssao::SsaoState;
use solarxy_core::preferences::ToneMode;
use wgpu::util::DeviceExt;

const SSAO_STRENGTH: f32 = 0.8;

pub struct CompositeState {
    params_buffer: wgpu::Buffer,
    params_bind_group: wgpu::BindGroup,
    bind_group: wgpu::BindGroup,
}

impl CompositeState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        hdr_resolve_view: &wgpu::TextureView,
        bloom_ping_view: &wgpu::TextureView,
        bloom_sampler: &wgpu::Sampler,
        bloom_enabled: bool,
        ssao_enabled: bool,
        tone_mode: ToneMode,
        exposure: f32,
    ) -> Self {
        let params_data = build_params(bloom_enabled, ssao_enabled, tone_mode, exposure);
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Composite Params Uniform"),
            contents: bytemuck::bytes_of(&params_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Composite Params Bind Group"),
            layout: &layouts.composite_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
        });
        let bind_group = create_bind_group(
            device,
            &layouts.composite,
            hdr_resolve_view,
            bloom_ping_view,
            bloom_sampler,
        );

        Self {
            params_buffer,
            params_bind_group,
            bind_group,
        }
    }

    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        hdr_resolve_view: &wgpu::TextureView,
        bloom_ping_view: &wgpu::TextureView,
        bloom_sampler: &wgpu::Sampler,
    ) {
        self.bind_group = create_bind_group(
            device,
            &layouts.composite,
            hdr_resolve_view,
            bloom_ping_view,
            bloom_sampler,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &Pipelines,
        view: &wgpu::TextureView,
        ssao_enabled: bool,
        ssao: &SsaoState,
        viewport: Option<[f32; 4]>,
        clear: bool,
    ) {
        let load = if clear {
            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
        } else {
            wgpu::LoadOp::Load
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Composite Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        if let Some([x, y, w, h]) = viewport {
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
        }
        pass.set_pipeline(&pipelines.post.composite);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &self.params_bind_group, &[]);
        if ssao_enabled {
            pass.set_bind_group(2, &ssao.read_bind_group, &[]);
        } else {
            pass.set_bind_group(2, &ssao.read_off_bind_group, &[]);
        }
        pass.draw(0..3, 0..1);
    }

    pub fn write_params(
        &self,
        queue: &wgpu::Queue,
        bloom_enabled: bool,
        ssao_enabled: bool,
        tone_mode: ToneMode,
        exposure: f32,
    ) {
        let params = build_params(bloom_enabled, ssao_enabled, tone_mode, exposure);
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeParams {
    bloom_strength: f32,
    bloom_enabled: u32,
    ssao_enabled: u32,
    ssao_strength: f32,
    tone_mode: u32,
    exposure: f32,
}

fn build_params(
    bloom_enabled: bool,
    ssao_enabled: bool,
    tone_mode: ToneMode,
    exposure: f32,
) -> CompositeParams {
    CompositeParams {
        bloom_strength: BLOOM_STRENGTH,
        bloom_enabled: u32::from(bloom_enabled),
        ssao_enabled: u32::from(ssao_enabled),
        ssao_strength: SSAO_STRENGTH,
        tone_mode: tone_mode.as_u32(),
        exposure,
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    scene_view: &wgpu::TextureView,
    bloom_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Composite Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(bloom_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
