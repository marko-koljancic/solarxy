//! Overdraw inspection mode resources — `R16Float` count texture plus the
//! bind group the show pass uses to read it.
//!
//! The count pass (`overdraw_count.wgsl`) renders all scene geometry with
//! `depth_compare = Always`, no depth writes, and additive blending on the
//! R channel, so each fragment increments its target pixel by `1.0`. The
//! show pass (`overdraw_show.wgsl`) reads this count texture in a
//! fullscreen quad and maps to a 6-stop color ramp.
//!
//! Why `R16Float` rather than `R16Uint`: wgpu requires float color targets
//! for `BlendState::ADD`. Half-float has ~11 bits of usable mantissa,
//! covering overdraw counts up to ~2048 without precision loss — easily
//! enough for real-world models where the worst silhouette tends to land
//! in the dozens, not thousands.

use crate::bind_groups::BindGroupLayouts;

pub const COUNT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

pub struct OverdrawResources {
    pub count_texture: wgpu::Texture,
    pub count_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub show_bind_group: wgpu::BindGroup,
}

impl OverdrawResources {
    pub fn new(device: &wgpu::Device, layouts: &BindGroupLayouts, width: u32, height: u32) -> Self {
        let (count_texture, count_view) = create_count_texture(device, width, height);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Overdraw Count Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let show_bind_group = create_show_bind_group(
            device,
            &layouts.overdraw_show,
            &count_view,
            &sampler,
        );
        Self {
            count_texture,
            count_view,
            sampler,
            show_bind_group,
        }
    }

    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        width: u32,
        height: u32,
    ) {
        let (t, v) = create_count_texture(device, width, height);
        self.count_texture = t;
        self.count_view = v;
        self.show_bind_group =
            create_show_bind_group(device, &layouts.overdraw_show, &self.count_view, &self.sampler);
    }
}

fn create_count_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Overdraw Count"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COUNT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_show_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    count_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Overdraw Show Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(count_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
