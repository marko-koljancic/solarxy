//! The linearly-transformed-cosine tables a rect-area light shades through.
//!
//! Two 64x64 `rgba16float` textures, baked by
//! `examples/gen_ltc_lut` and committed as `shaders/ltc_lut.rgba16f`. This
//! module only uploads them; everything about how they were produced, and
//! the argument for producing them rather than embedding someone else's,
//! lives in that example's doc comment.
//!
//! **What they are for.** Heitz, Dupuy, Hill and Neubelt, *Real-Time
//! Polygonal-Light Shading with Linearly Transformed Cosines* (SIGGRAPH
//! 2016). A point light needs a direction; a rectangle needs an integral
//! over its area, which has no closed form against a GGX lobe. The LTC
//! trick is to warp the lobe into a plain cosine with a matrix, because a
//! cosine integrated over a polygon *does* have a closed form. These tables
//! are that matrix, tabulated over roughness and view angle.
//!
//! **Indexing**, and the contract with `shader.wgsl`:
//!
//! - `u` is perceptual roughness, so `alpha = roughness^2`.
//! - `v` is `sqrt(1 - dot(N, V))`, which spends texels where the lobe
//!   changes fastest instead of spreading them evenly over an angle.
//!
//! Both are scaled and biased by half a texel at lookup so the ends of the
//! table are reachable under linear filtering.
//!
//! **Why `rgba16float`.** It is filterable in core WebGPU. `rgba32float` is
//! not without the `float32-filterable` feature, and this renderer holds
//! itself to core so the desktop and web paths stay the same code.

/// Table edge. The contract with `examples/gen_ltc_lut`.
pub const LUT_SIZE: u32 = 64;

/// Both tables, back to back: the transform table then the magnitude and
/// Fresnel table, each `LUT_SIZE * LUT_SIZE` RGBA half-float texels.
const LUT_BYTES: &[u8] = include_bytes!("shaders/ltc_lut.rgba16f");

/// Bytes in one table.
const TABLE_BYTES: usize = (LUT_SIZE * LUT_SIZE) as usize * 4 * 2;

const _: () = assert!(LUT_BYTES.len() == TABLE_BYTES * 2);

/// The two tables plus the sampler that reads them.
///
/// Owned beside [`crate::ibl::BrdfLut`] in the renderer and handed to the
/// light bind group, which is the only thing that binds them.
pub struct LtcLuts {
    /// `M^-1`, packed as `(m00, m20, m02, m22)` with `m11` normalized to 1.
    #[allow(dead_code)]
    pub transform: wgpu::Texture,
    pub transform_view: wgpu::TextureView,
    /// `(magnitude, fresnel, 0, 0)`.
    #[allow(dead_code)]
    pub magnitude: wgpu::Texture,
    pub magnitude_view: wgpu::TextureView,
    /// Linear, clamped. Clamping matters at the table edges: a wrapped
    /// lookup at grazing incidence would read the mirror row and put a
    /// hard seam across a rough surface.
    pub sampler: wgpu::Sampler,
}

impl LtcLuts {
    /// Uploads the committed tables.
    pub fn load(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let (first, second) = LUT_BYTES.split_at(TABLE_BYTES);
        let transform = upload(device, queue, "LTC Transform LUT", first);
        let magnitude = upload(device, queue, "LTC Magnitude LUT", second);
        let transform_view = transform.create_view(&wgpu::TextureViewDescriptor {
            label: Some("LTC Transform LUT View"),
            ..Default::default()
        });
        let magnitude_view = magnitude.create_view(&wgpu::TextureViewDescriptor {
            label: Some("LTC Magnitude LUT View"),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("LTC LUT Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            transform,
            transform_view,
            magnitude,
            magnitude_view,
            sampler,
        }
    }
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, data: &[u8]) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width: LUT_SIZE,
        height: LUT_SIZE,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(LUT_SIZE * 4 * 2),
            rows_per_image: Some(LUT_SIZE),
        },
        size,
    );
    texture
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blob is the shader's contract, so its shape is asserted here as
    /// well as at compile time: a table baked at another resolution would
    /// otherwise upload as garbage rather than fail.
    #[test]
    fn the_committed_blob_is_two_full_tables() {
        assert_eq!(LUT_BYTES.len(), TABLE_BYTES * 2);
        assert_eq!(TABLE_BYTES, 64 * 64 * 4 * 2);
    }

    /// Row 0 of the transform table is normal incidence, where the lobe is
    /// symmetric: the transform is diagonal, so both off-diagonal entries
    /// are zero and the first is exactly 1. If the table is ever written
    /// transposed, this is the cheapest place it shows.
    #[test]
    fn normal_incidence_is_symmetric() {
        let read = |texel: usize, channel: usize| -> f32 {
            let i = (texel * 4 + channel) * 2;
            f32::from(half::f16::from_le_bytes([LUT_BYTES[i], LUT_BYTES[i + 1]]))
        };
        for a in 0..LUT_SIZE as usize {
            assert!((read(a, 0) - 1.0).abs() < 1.0e-3, "m00 at roughness {a}");
            assert!(read(a, 1).abs() < 1.0e-3, "m20 at roughness {a}");
            assert!(read(a, 2).abs() < 1.0e-3, "m02 at roughness {a}");
        }
    }
}
