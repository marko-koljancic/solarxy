//! The environment on the GPU: the image a ray finds when it leaves the scene,
//! and the two tables the kernel searches to aim at the bright parts of it.
//!
//! # Why it lives in the sampled group beside the atlas
//!
//! Bindings 3 to 6 of that group were reserved by number two stages ago for
//! exactly this, so nothing here renumbers anything. What it does mean is that
//! the sampled group is one bind group with two owners, and a bind group has to
//! belong to something: [`super::TraceAtlas`] owns it, holds a null environment
//! until a real one arrives, and rebuilds when one does. The alternative, a
//! third type owning the group and both halves feeding it, buys nothing here and
//! costs every call site a parameter.
//!
//! # The formats are not a preference
//!
//! The equirect is `Rgba16Float`, which is what the skybox already uses for the
//! same image, because the kernel samples it with filtering and core WebGPU does
//! not offer filtering on `Rgba32Float` without a feature this renderer holds
//! itself away from.
//!
//! The two tables are `R32Float` and are read with `textureLoad`, never through
//! a sampler. That is the format's own constraint turned into an advantage: an
//! unfilterable texture is exactly right for a table that must be searched
//! rather than interpolated, it keeps full float precision where half float
//! would lose the tail of a cumulative distribution near one, and it spends no
//! sampler.

use crate::env_dist::EnvDistribution;

/// The environment's textures and the sampler over its image.
pub struct TraceEnvironment {
    /// `None` when the image is shared rather than owned. A view keeps its
    /// texture alive on its own, so the owned handle exists only for the
    /// upload path that created one.
    #[allow(unused)]
    equirect: Option<wgpu::Texture>,
    equirect_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    #[allow(unused)]
    marginal: wgpu::Texture,
    marginal_view: wgpu::TextureView,
    #[allow(unused)]
    conditional: wgpu::Texture,
    conditional_view: wgpu::TextureView,
    width: u32,
    height: u32,
    total_weight: f32,
}

impl TraceEnvironment {
    /// The null environment: one black texel and two one-entry tables.
    ///
    /// Not a nicety, for the same reason the null atlas is not. A pipeline
    /// layout is satisfied by a bind group or by nothing, and a scene with no
    /// HDRI is the ordinary case, so the empty state has to be a real set of
    /// textures. Nothing samples them, because the kernel reads the size from
    /// its uniform and falls back to the constant environment when it is zero.
    #[must_use]
    pub fn null(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self::build(device, queue, 1, 1, &[0u8; 8], &[1.0], &[1.0], 0.0, 0, 0)
    }

    /// Uploads a prepared environment: its sanitized equirect pixels and the
    /// distribution built over them.
    ///
    /// `pixels` is three floats per pixel, row-major, which is the layout
    /// `PreparedHdri` carries. An empty distribution produces the null
    /// environment, so a black HDRI is indistinguishable from no HDRI, which is
    /// the honest answer: neither has anything to sample.
    #[must_use]
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[f32],
        distribution: &EnvDistribution,
    ) -> Self {
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(3);
        if distribution.is_empty() || width == 0 || height == 0 || pixels.len() < expected {
            return Self::null(device, queue);
        }
        // Half float, one texel at a time, because the source is three
        // components and the texture is four: the alpha lane is written opaque
        // rather than left at whatever the allocation held.
        let mut texels = vec![0u8; (width as usize) * (height as usize) * 8];
        for (i, px) in pixels.chunks_exact(3).enumerate() {
            let out = i * 8;
            for (c, value) in px.iter().enumerate() {
                let bytes = half::f16::from_f32(value.clamp(0.0, 65504.0)).to_le_bytes();
                texels[out + c * 2] = bytes[0];
                texels[out + c * 2 + 1] = bytes[1];
            }
            let one = half::f16::from_f32(1.0).to_le_bytes();
            texels[out + 6] = one[0];
            texels[out + 7] = one[1];
        }
        Self::build(
            device,
            queue,
            width,
            height,
            &texels,
            distribution.marginal(),
            distribution.conditional(),
            distribution.total_weight(),
            distribution.width(),
            distribution.height(),
        )
    }

    /// Shares an equirect already on the GPU instead of uploading a second
    /// copy of it, and builds only the two tables over it.
    ///
    /// The image the raster path retains for the sky pass and the image the
    /// kernel walks are the same image in the same format, so the honest
    /// relationship between them is one texture with two readers rather than
    /// two textures with one each. A view holds its texture alive, so the
    /// borrow is safe without either side knowing about the other; what the
    /// two do *not* share is the sampler, because this one reads NEAREST for
    /// the reason spelled out below and the sky pass reads filtered.
    ///
    /// The caller is trusted for the format, which must be the `Rgba16Float`
    /// both sides already use. A mismatch is a bind-group error at creation
    /// rather than a wrong picture.
    #[must_use]
    pub fn from_shared_equirect(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        equirect_view: &wgpu::TextureView,
        distribution: &EnvDistribution,
    ) -> Self {
        if distribution.is_empty() {
            return Self::null(device, queue);
        }
        Self::assemble(
            device,
            queue,
            None,
            equirect_view.clone(),
            distribution.marginal(),
            distribution.conditional(),
            distribution.total_weight(),
            distribution.width(),
            distribution.height(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        texels: &[u8],
        marginal_data: &[f32],
        conditional_data: &[f32],
        total_weight: f32,
        dist_width: u32,
        dist_height: u32,
    ) -> Self {
        let equirect = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Pathtrace Environment"),
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
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &equirect,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            texels,
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

        let equirect_view = equirect.create_view(&wgpu::TextureViewDescriptor::default());
        Self::assemble(
            device,
            queue,
            Some(equirect),
            equirect_view,
            marginal_data,
            conditional_data,
            total_weight,
            dist_width,
            dist_height,
        )
    }

    /// Everything both constructors share: the two tables, the sampler, and
    /// the record. Split so the uploading path and the sharing path cannot
    /// drift on the sampler, which is where the environment's load-bearing
    /// decision lives.
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        equirect: Option<wgpu::Texture>,
        equirect_view: wgpu::TextureView,
        marginal_data: &[f32],
        conditional_data: &[f32],
        total_weight: f32,
        dist_width: u32,
        dist_height: u32,
    ) -> Self {
        // The tables. The marginal is one row of `dist_height` entries and the
        // conditional is `dist_height` rows of `dist_width`, so both are 2D
        // textures and the kernel indexes them with `textureLoad`.
        let (marg_w, marg_h) = (dist_height.max(1), 1);
        let marginal = Self::table(
            device,
            queue,
            "Pathtrace Environment Marginal",
            marg_w,
            marg_h,
            marginal_data,
        );
        let (cond_w, cond_h) = (dist_width.max(1), dist_height.max(1));
        let conditional = Self::table(
            device,
            queue,
            "Pathtrace Environment Conditional",
            cond_w,
            cond_h,
            conditional_data,
        );

        let marginal_view = marginal.create_view(&wgpu::TextureViewDescriptor::default());
        let conditional_view = conditional.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Pathtrace Environment Sampler"),
            // Repeat across the seam and clamp at the poles, which is what an
            // equirectangular image means: longitude wraps and latitude does
            // not. Getting the second one wrong puts the bottom of the sky at
            // the top of it in a band one texel wide.
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            // Nearest, and this is the environment's load-bearing decision.
            //
            // The sampling distribution is piecewise constant over texels: it
            // picks a texel in proportion to that texel's own brightness and
            // reports a density derived from it. A *filtered* radiance would
            // then describe a different environment from the one the density
            // describes, and the two disagree exactly where it matters most.
            // A single bright sun read bilinearly returns about half its value
            // averaged over its own texel and spills the rest into neighbours
            // the distribution considers dim, so the estimator converges to the
            // right answer by finding rare enormous samples in the spill, which
            // is the variance importance sampling was meant to remove.
            //
            // What this gives up is a smooth background at low resolution: a
            // ray that escapes to the camera reads one texel. For an authored
            // HDRI at two thousand pixels across against a render at two
            // thousand pixels wide, that is about one texel per pixel and costs
            // nothing.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            equirect,
            equirect_view,
            sampler,
            marginal,
            marginal_view,
            conditional,
            conditional_view,
            width: dist_width,
            height: dist_height,
            total_weight,
        }
    }

    fn table(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        width: u32,
        height: u32,
        data: &[f32],
    ) -> wgpu::Texture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // A short slice is padded rather than refused: the null environment
        // hands one entry for a one-texel table, and the allocation is what the
        // write has to fill.
        let needed = (width as usize) * (height as usize);
        let mut padded;
        let source = if data.len() >= needed {
            &data[..needed]
        } else {
            padded = vec![1.0f32; needed];
            padded[..data.len()].copy_from_slice(data);
            &padded[..]
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(source),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        texture
    }

    /// The distribution's dimensions, which are zero when there is nothing to
    /// sample. The kernel reads these from its uniform to decide whether to use
    /// the image or the constant fallback.
    #[must_use]
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// The sum of every weight, which is the density's denominator.
    #[must_use]
    pub fn total_weight(&self) -> f32 {
        self.total_weight
    }

    /// The four entries this contributes to the sampled group, at the numbers
    /// reserved for them.
    pub(super) fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 4] {
        [
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&self.equirect_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&self.marginal_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&self.conditional_view),
            },
        ]
    }
}
