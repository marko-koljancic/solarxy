use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::bloom::BLOOM_STRENGTH;
use crate::cgi::pipelines::Pipelines;
use crate::cgi::ssao::SsaoState;
use crate::preferences::ToneMode;
use wgpu::util::DeviceExt;

const SSAO_STRENGTH: f32 = 0.8;

pub(crate) struct CompositeState {
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
            contents: &params_data,
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
        }
        pass.set_pipeline(&pipelines.composite);
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
        let buf = build_params(bloom_enabled, ssao_enabled, tone_mode, exposure);
        queue.write_buffer(&self.params_buffer, 0, &buf);
    }
}

fn build_params(
    bloom_enabled: bool,
    ssao_enabled: bool,
    tone_mode: ToneMode,
    exposure: f32,
) -> [u8; 24] {
    let mut buf = [0u8; 24];
    buf[0..4].copy_from_slice(&BLOOM_STRENGTH.to_le_bytes());
    let bloom_flag: u32 = u32::from(bloom_enabled);
    buf[4..8].copy_from_slice(&bloom_flag.to_le_bytes());
    let ssao_flag: u32 = u32::from(ssao_enabled);
    buf[8..12].copy_from_slice(&ssao_flag.to_le_bytes());
    buf[12..16].copy_from_slice(&SSAO_STRENGTH.to_le_bytes());
    buf[16..20].copy_from_slice(&tone_mode.as_u32().to_le_bytes());
    buf[20..24].copy_from_slice(&exposure.to_le_bytes());
    buf
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
