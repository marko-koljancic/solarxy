//! HDRI skybox resources: the source equirectangular texture retained
//! after IBL convolution, plus the bind group the skybox pass samples.
//!
//! [`crate::ibl::IblState::from_hdri`] keeps an [`EquirectTexture`] beside
//! the convolved cubemaps so `BackgroundMode::HdriSky` can render the HDRI
//! as a visible backdrop. `frame.rs` runs the skybox pass; the rotation
//! yaw rides in `CameraUniform::hdri_rotation`, shared with the IBL
//! cubemap lookups in `shader.wgsl` so sky and lighting stay in sync.

use half::f16;

/// The source equirectangular HDRI, kept as a 2D `Rgba16Float` texture for
/// the skybox pass — roughly 64 MB at 4K, accepted for RC2.
pub struct EquirectTexture {
    #[allow(dead_code)]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl EquirectTexture {
    /// Upload decoded equirect HDR pixels (`[r, g, b]` linear floats, row
    /// major) into a half-float 2D texture. `pixels.len()` is expected to
    /// equal `width * height`; a shorter slice leaves the tail black.
    pub fn from_hdr_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[[f32; 3]],
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Skybox Equirect HDRI"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texel_count = (width as usize) * (height as usize);
        let mut data = vec![0u8; texel_count * 8];
        for (i, px) in pixels.iter().take(texel_count).enumerate() {
            let o = i * 8;
            let rgba = [
                f16::from_f32(px[0]),
                f16::from_f32(px[1]),
                f16::from_f32(px[2]),
                f16::from_f32(1.0),
            ];
            for (c, half) in rgba.iter().enumerate() {
                let b = half.to_ne_bytes();
                data[o + c * 2] = b[0];
                data[o + c * 2 + 1] = b[1];
            }
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("Skybox Equirect View"),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Skybox Equirect Sampler"),
            // Wrap horizontally so the HDRI yaw seam is clean; clamp
            // vertically at the poles.
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
        }
    }
}

/// Build the skybox pass bind group: the equirect texture + its sampler.
pub fn create_skybox_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    equirect: &EquirectTexture,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("skybox_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&equirect.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&equirect.sampler),
            },
        ],
    })
}
