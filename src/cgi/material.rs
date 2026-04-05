use wgpu::util::DeviceExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaMode {
    #[default]
    Opaque = 0,
    Mask = 1,
    Blend = 2,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaterialUniform {
    pub roughness_factor: f32,
    pub metallic_factor: f32,
    pub ao_strength: f32,
    pub alpha_cutoff: f32,
    pub emissive: [f32; 3],
    pub alpha_mode: u32,
}

const _: () = assert!(std::mem::size_of::<MaterialUniform>() == 32);

impl Default for MaterialUniform {
    fn default() -> Self {
        Self {
            roughness_factor: 0.7,
            metallic_factor: 0.0,
            ao_strength: 1.0,
            alpha_cutoff: 0.5,
            emissive: [0.0, 0.0, 0.0],
            alpha_mode: 0,
        }
    }
}

#[allow(dead_code)]
pub struct Material {
    pub name: String,
    pub diffuse_texture: super::texture::Texture,
    pub normal_texture: super::texture::Texture,
    pub orm_texture: super::texture::Texture,
    pub emissive_texture: super::texture::Texture,
    pub uniform: MaterialUniform,
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl Material {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        name: &str,
        diffuse_texture: super::texture::Texture,
        normal_texture: super::texture::Texture,
        orm_texture: super::texture::Texture,
        emissive_texture: super::texture::Texture,
        uniform: MaterialUniform,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{name}_material_uniform")),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&normal_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&normal_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&orm_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&orm_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&emissive_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&emissive_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
            label: Some(name),
        });

        Self {
            name: name.to_string(),
            diffuse_texture,
            normal_texture,
            orm_texture,
            emissive_texture,
            uniform,
            uniform_buffer,
            bind_group,
        }
    }
}
