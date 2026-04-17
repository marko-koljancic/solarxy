use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::camera::OPENGL_TO_WGPU_MATRIX;
use crate::cgi::light::LightsUniform;
use crate::cgi::model::Model;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowUniform {
    light_vp: [[f32; 4]; 4],
}

pub(crate) struct ShadowState {
    pub(crate) texture_view: wgpu::TextureView,
    pub(crate) pass_bind_group: wgpu::BindGroup,
    pub(crate) sample_bind_group: wgpu::BindGroup,
    uniform: ShadowUniform,
    uniform_buffer: wgpu::Buffer,
}

impl ShadowState {
    pub(crate) fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        lights_uniform: &LightsUniform,
        model: &Model,
        shadow_map_size: u32,
    ) -> Self {
        let shadow_map_size = shadow_map_size.clamp(512, 4096);
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow_texture"),
            size: wgpu::Extent3d {
                width: shadow_map_size,
                height: shadow_map_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let texture_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let key_pos = lights_uniform.lights[0].position;
        let light_vp = compute_light_vp(
            cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
            model.bounds.center(),
            model.bounds.diagonal() / 2.0,
        );
        let uniform = ShadowUniform {
            light_vp: light_vp.into(),
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Shadow Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let pass_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_pass_bind_group"),
            layout: &layouts.shadow_pass,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let sample_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_bind_group"),
            layout: &layouts.shadow_read,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });

        ShadowState {
            texture_view,
            pass_bind_group,
            sample_bind_group,
            uniform,
            uniform_buffer,
        }
    }

    pub(crate) fn update_light_vp(
        &mut self,
        queue: &wgpu::Queue,
        light_pos: cgmath::Point3<f32>,
        target: cgmath::Point3<f32>,
        extent: f32,
    ) {
        let light_vp = compute_light_vp(light_pos, target, extent);
        self.uniform.light_vp = light_vp.into();
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniform]),
        );
    }
}

pub(crate) fn compute_light_vp(
    light_pos: cgmath::Point3<f32>,
    target: cgmath::Point3<f32>,
    extent: f32,
) -> cgmath::Matrix4<f32> {
    use cgmath::MetricSpace;
    let dist = light_pos.distance(target);
    let near = (dist - extent * 2.0).max(0.1);
    let far = dist + extent * 4.0;
    let view = cgmath::Matrix4::look_at_rh(light_pos, target, cgmath::Vector3::unit_y());
    let s = extent * 1.5;
    let proj = cgmath::ortho(-s, s, -s, s, near, far);
    OPENGL_TO_WGPU_MATRIX * proj * view
}
