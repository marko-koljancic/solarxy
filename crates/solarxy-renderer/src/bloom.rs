//! Bloom post-processing: brightness extraction + separable Gaussian blur
//! (horizontal + vertical) over a half-resolution bloom mip chain.

use crate::bind_groups::BindGroupLayouts;
use crate::pipelines::Pipelines;
use crate::texture;

const BLOOM_THRESHOLD: f32 = 0.8;
pub const BLOOM_STRENGTH: f32 = 0.8;

pub struct BloomState {
    _ping_texture: wgpu::Texture,
    pub ping_view: wgpu::TextureView,
    _pong_texture: wgpu::Texture,
    pong_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    params_buffer: wgpu::Buffer,
    params_bind_group: wgpu::BindGroup,
    extract_bind_group: wgpu::BindGroup,
    blur_h_bind_group: wgpu::BindGroup,
    blur_v_bind_group: wgpu::BindGroup,
}

impl BloomState {
    pub fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        hdr_resolve_view: &wgpu::TextureView,
        sampler: wgpu::Sampler,
        width: u32,
        height: u32,
    ) -> Self {
        let (ping_texture, ping_view) =
            texture::create_bloom_texture(device, width, height, "Bloom Ping");
        let (pong_texture, pong_view) =
            texture::create_bloom_texture(device, width, height, "Bloom Pong");

        let bloom_params_data: [f32; 4] = [BLOOM_THRESHOLD, BLOOM_STRENGTH, 0.0, 0.0];
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Params Uniform"),
            contents: bytemuck::cast_slice(&bloom_params_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Params Bind Group"),
            layout: &layouts.bloom_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
        });

        let extract_bind_group =
            create_sample_bind_group(device, &layouts.bloom_texture, hdr_resolve_view, &sampler);
        let blur_h_bind_group =
            create_sample_bind_group(device, &layouts.bloom_texture, &ping_view, &sampler);
        let blur_v_bind_group =
            create_sample_bind_group(device, &layouts.bloom_texture, &pong_view, &sampler);

        Self {
            _ping_texture: ping_texture,
            ping_view,
            _pong_texture: pong_texture,
            pong_view,
            sampler,
            params_buffer,
            params_bind_group,
            extract_bind_group,
            blur_h_bind_group,
            blur_v_bind_group,
        }
    }

    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        hdr_resolve_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        let (ping_texture, ping_view) =
            texture::create_bloom_texture(device, width, height, "Bloom Ping");
        let (pong_texture, pong_view) =
            texture::create_bloom_texture(device, width, height, "Bloom Pong");

        self.extract_bind_group = create_sample_bind_group(
            device,
            &layouts.bloom_texture,
            hdr_resolve_view,
            &self.sampler,
        );
        self.blur_h_bind_group =
            create_sample_bind_group(device, &layouts.bloom_texture, &ping_view, &self.sampler);
        self.blur_v_bind_group =
            create_sample_bind_group(device, &layouts.bloom_texture, &pong_view, &self.sampler);

        self._ping_texture = ping_texture;
        self.ping_view = ping_view;
        self._pong_texture = pong_texture;
        self.pong_view = pong_view;
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &Pipelines,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        let full_texel: [f32; 4] = [
            BLOOM_THRESHOLD,
            BLOOM_STRENGTH,
            1.0 / width.max(1) as f32,
            1.0 / height.max(1) as f32,
        ];
        queue.write_buffer(&self.params_buffer, 0, bytemuck::cast_slice(&full_texel));

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Extract Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ping_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines.post.bloom_extract);
            pass.set_bind_group(0, &self.extract_bind_group, &[]);
            pass.set_bind_group(1, &self.params_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        let half_w = (width / 2).max(1) as f32;
        let half_h = (height / 2).max(1) as f32;
        let half_texel: [f32; 4] = [BLOOM_THRESHOLD, BLOOM_STRENGTH, 1.0 / half_w, 1.0 / half_h];
        queue.write_buffer(&self.params_buffer, 0, bytemuck::cast_slice(&half_texel));

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Blur H Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.pong_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines.post.bloom_blur_h);
            pass.set_bind_group(0, &self.blur_h_bind_group, &[]);
            pass.set_bind_group(1, &self.params_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Blur V Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ping_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines.post.bloom_blur_v);
            pass.set_bind_group(0, &self.blur_v_bind_group, &[]);
            pass.set_bind_group(1, &self.params_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn create_sample_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Bloom Sample Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

use wgpu::util::DeviceExt;
