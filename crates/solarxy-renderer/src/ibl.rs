//! Image-based lighting: the irradiance + prefiltered specular cubemaps and
//! BRDF LUT used by the PBR shader.
//!
//! [`IblState`] construction: [`IblState::fallback`],
//! [`IblState::from_sky_colors`], and the HDRI family
//! ([`IblState::from_hdri`] on std-fs, [`IblState::from_hdr_bytes`] /
//! [`IblState::from_exr_bytes`] anywhere). Any IBL-derived CPU data (e.g.
//! the L0 ambient SH coefficient) must be computed in **all** constructors;
//! `solarxy-app/src/state/update.rs::rebuild_light_bind_group` is the
//! single chokepoint that pushes IBL-derived uniforms to the GPU.

#[cfg(feature = "std-fs")]
use std::path::Path;

use half::f16;

use crate::env_dist::EnvDistribution;
use crate::error::RendererError;
use crate::skybox::EquirectTexture;

pub struct BrdfLut {
    #[allow(dead_code)]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

pub struct IblState {
    #[allow(dead_code)]
    pub irradiance_texture: wgpu::Texture,
    pub irradiance_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    #[allow(dead_code)]
    pub prefiltered_texture: wgpu::Texture,
    pub prefiltered_view: wgpu::TextureView,
    pub prefiltered_sampler: wgpu::Sampler,
    pub irradiance_average: [f32; 3],
    /// Source equirect HDRI, retained only by [`IblState::from_hdri`] so
    /// `BackgroundMode::HdriSky` can render the HDRI as a visible sky.
    /// `None` for the procedural ([`IblState::fallback`] /
    /// [`IblState::from_sky_colors`]) constructors.
    pub equirect: Option<EquirectTexture>,
    /// The sampling distribution built over the same pixels, retained for the
    /// same reason `equirect` is: a consumer other than the light bind group
    /// needs the source, and rebuilding it means a second full pass over the
    /// largest asset in a scene.
    ///
    /// That consumer is the path tracer, which aims its escaping rays with it.
    /// Both HDRI routes already build one and dropped it here until the tracer
    /// had somewhere to read it from; the worker even ships it across the
    /// boundary inside [`PreparedHdri`] precisely so the main thread does not
    /// have to compute it. `None` alongside a `None` equirect, because a
    /// procedural sky has no image to distribute over.
    pub distribution: Option<EnvDistribution>,
}

const F16_MAX: f32 = 65504.0;

fn write_rgba16f(buf: &mut [u8], offset: usize, r: f32, g: f32, b: f32, a: f32) {
    let bytes = [
        f16::from_f32(r.clamp(0.0, F16_MAX)).to_ne_bytes(),
        f16::from_f32(g.clamp(0.0, F16_MAX)).to_ne_bytes(),
        f16::from_f32(b.clamp(0.0, F16_MAX)).to_ne_bytes(),
        f16::from_f32(a.clamp(0.0, F16_MAX)).to_ne_bytes(),
    ];
    buf[offset] = bytes[0][0];
    buf[offset + 1] = bytes[0][1];
    buf[offset + 2] = bytes[1][0];
    buf[offset + 3] = bytes[1][1];
    buf[offset + 4] = bytes[2][0];
    buf[offset + 5] = bytes[2][1];
    buf[offset + 6] = bytes[3][0];
    buf[offset + 7] = bytes[3][1];
}

fn write_rg16f(buf: &mut [u8], offset: usize, r: f32, g: f32) {
    let rb = f16::from_f32(r).to_ne_bytes();
    let gb = f16::from_f32(g).to_ne_bytes();
    buf[offset] = rb[0];
    buf[offset + 1] = rb[1];
    buf[offset + 2] = gb[0];
    buf[offset + 3] = gb[1];
}

const PI: f32 = std::f32::consts::PI;
const PREFILTERED_SIZE: u32 = 128;
const PREFILTERED_MIP_COUNT: u32 = 6;
const BRDF_LUT_SIZE: u32 = 512;
const BRDF_LUT_SAMPLES: u32 = 1024;

impl BrdfLut {
    pub fn generate(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = generate_brdf_lut(device, queue);
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL BRDF LUT View"),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL BRDF LUT Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            texture,
            view,
            sampler,
        }
    }

    pub fn fallback(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("IBL Fallback BRDF LUT"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut lut_pixel = [0u8; 4];
        write_rg16f(&mut lut_pixel, 0, 1.0, 0.0);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &lut_pixel,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL BRDF LUT View"),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL BRDF LUT Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            texture,
            view,
            sampler,
        }
    }
}

impl IblState {
    /// Solid-grey fallback IBL (`0.2, 0.2, 0.2`) — used when no HDRI is
    /// loaded and the user has not selected a sky-colour gradient. One of
    /// the three IBL constructors that **must all** compute the same
    /// CPU-side derived data (e.g. `irradiance_average`) so the
    /// `rebuild_light_bind_group` chokepoint sees a consistent shape.
    pub fn fallback(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mut pixel_bytes = [0u8; 8];
        write_rgba16f(&mut pixel_bytes, 0, 0.2, 0.2, 0.2, 1.0);

        let irradiance_texture = create_cubemap(device, "IBL Fallback Irradiance", 1, 1);
        for face in 0..6u32 {
            write_cubemap_face(queue, &irradiance_texture, face, 0, 1, &pixel_bytes);
        }

        let mut black_pixel = [0u8; 8];
        write_rgba16f(&mut black_pixel, 0, 0.0, 0.0, 0.0, 1.0);

        let prefiltered_texture = create_cubemap(device, "IBL Fallback Prefiltered", 1, 1);
        for face in 0..6u32 {
            write_cubemap_face(queue, &prefiltered_texture, face, 0, 1, &black_pixel);
        }

        Self::from_parts(
            device,
            irradiance_texture,
            prefiltered_texture,
            [0.2, 0.2, 0.2],
            None,
            None,
        )
    }

    /// Procedural IBL from a top/bottom sky-colour gradient. Generates the
    /// irradiance + prefiltered cubemaps on the GPU once. See [`Self::fallback`]
    /// for the CPU-side-derived-data invariant.
    pub fn from_sky_colors(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        top: [f32; 3],
        bottom: [f32; 3],
    ) -> Self {
        let irradiance_texture = generate_irradiance_sky(device, queue, top, bottom);
        let prefiltered_texture = generate_prefiltered_sky(device, queue, top, bottom);
        let irradiance_average = [
            f32::midpoint(top[0], bottom[0]),
            f32::midpoint(top[1], bottom[1]),
            f32::midpoint(top[2], bottom[2]),
        ];

        Self::from_parts(
            device,
            irradiance_texture,
            prefiltered_texture,
            irradiance_average,
            None,
            None,
        )
    }

    /// Builds an IBL from an HDRI image on disk (`.hdr` or `.exr`). Decodes
    /// the equirect, projects to a cubemap, and convolves into irradiance +
    /// prefiltered specular mips. See [`Self::fallback`] for the CPU-side-
    /// derived-data invariant the `rebuild_light_bind_group` chokepoint
    /// relies on.
    ///
    /// # Errors
    /// Returns `Err` if the image fails to decode or the file extension is
    /// not one of the supported HDRI formats.
    /// Build IBL state from an HDRI file on disk (`.hdr` or `.exr`).
    #[cfg(feature = "std-fs")]
    pub fn from_hdri(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &Path,
    ) -> Result<Self, RendererError> {
        let image = solarxy_formats::hdr::load_hdr_image(path)?;
        Ok(Self::from_hdr_image(device, queue, &image))
    }

    /// Build IBL state from in-memory Radiance `.hdr` bytes.
    pub fn from_hdr_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
    ) -> Result<Self, RendererError> {
        let image = solarxy_formats::hdr::decode_hdr_bytes(bytes)?;
        Ok(Self::from_hdr_image(device, queue, &image))
    }

    /// Build IBL state from in-memory `OpenEXR` bytes.
    pub fn from_exr_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
    ) -> Result<Self, RendererError> {
        let image = solarxy_formats::hdr::decode_exr_bytes(bytes)?;
        Ok(Self::from_hdr_image(device, queue, &image))
    }

    /// Shared HDRI-construction core: sanitize, convolve, prefilter, and
    /// assemble. Every HDRI entry point funnels through here so the
    /// IBL-derived CPU data stays consistent across constructors.
    ///
    /// Takes the image by reference because it arrives from the scene
    /// contract behind an `Arc`, shared with the engine, and cannot be
    /// mutated in place. This module's `sanitized` helper avoids copying
    /// it in the common case where there is nothing to sanitize.
    pub fn from_hdr_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &solarxy_formats::RawImageHdr,
    ) -> Self {
        let (width, height) = (image.width, image.height);
        let pixels = sanitized(&image.pixels);

        let irradiance_faces = convolve_equirect(width, height, rgb(&pixels));
        let irradiance_average = compute_irradiance_average(&irradiance_faces);
        // Built here as well as in `prepare`, so the inline path and the worker
        // path produce the same value rather than one of them producing none.
        let distribution = EnvDistribution::build(width, height, &pixels);
        Self::from_prepared(
            device,
            queue,
            &PreparedHdri {
                width,
                height,
                pixels: pixels.into_owned(),
                irradiance_faces,
                irradiance_average,
                distribution,
            },
        )
    }

    /// Finishes a [`PreparedHdri`] on the GPU: uploads the irradiance
    /// faces, runs the specular prefilter, and retains the source equirect
    /// for the skybox pass. The CPU-heavy stages (decode, sanitize,
    /// irradiance convolution) already ran in [`PreparedHdri::prepare`],
    /// off-thread on the web.
    pub fn from_prepared(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prepared: &PreparedHdri,
    ) -> Self {
        let irradiance_texture =
            irradiance_faces_to_texture(device, queue, &prepared.irradiance_faces);
        let prefiltered_texture = generate_prefiltered_equirect(
            device,
            queue,
            prepared.width,
            prepared.height,
            rgb(&prepared.pixels),
        );
        let equirect = EquirectTexture::from_hdr_pixels(
            device,
            queue,
            prepared.width,
            prepared.height,
            rgb(&prepared.pixels),
        );

        Self::from_parts(
            device,
            irradiance_texture,
            prefiltered_texture,
            prepared.irradiance_average,
            Some(equirect),
            Some(prepared.distribution.clone()),
        )
    }

    fn from_parts(
        device: &wgpu::Device,
        irradiance_texture: wgpu::Texture,
        prefiltered_texture: wgpu::Texture,
        irradiance_average: [f32; 3],
        equirect: Option<EquirectTexture>,
        distribution: Option<EnvDistribution>,
    ) -> Self {
        let irradiance_view = irradiance_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL Irradiance View"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL Irradiance Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let prefiltered_view = prefiltered_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL Prefiltered View"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });

        let prefiltered_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL Prefiltered Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            irradiance_texture,
            irradiance_view,
            sampler,
            prefiltered_texture,
            prefiltered_view,
            prefiltered_sampler,
            irradiance_average,
            equirect,
            distribution,
        }
    }
}

/// The GPU-free product of HDRI preparation: decoded and sanitized
/// equirect pixels plus the CPU-convolved irradiance faces and their
/// average (the expensive stages). On the web the import worker runs
/// [`PreparedHdri::prepare`] off-thread and ships the packed bytes to the
/// main thread, where [`IblState::from_prepared`] finishes on the GPU.
pub struct PreparedHdri {
    pub width: u32,
    pub height: u32,
    /// Sanitized linear-RGB equirect pixels, row-major, three floats per
    /// pixel: the same flat layout `solarxy_formats::RawImageHdr` decodes
    /// into, so nothing on this path copies to reshape it. This module's
    /// `rgb` helper views it as triples without copying.
    pub pixels: Vec<f32>,
    /// The 32x32 convolved irradiance cubemap faces.
    pub irradiance_faces: [Vec<[f32; 3]>; 6],
    pub irradiance_average: [f32; 3],
    /// The sampling distribution the path tracer draws directions from.
    ///
    /// It rides here rather than being built where it is used because of where
    /// the work has to happen: on the web the decode and the convolution run in
    /// the import worker so the main thread keeps its frame rate, and this is a
    /// third pass over the same pixels that would otherwise have to cross the
    /// boundary twice or block the renderer. Nothing else about the transfer
    /// changes, which is what "no new worker plumbing" means in the milestone.
    ///
    /// Empty for a black image, which is a scene state rather than a failure.
    pub distribution: EnvDistribution,
}

impl PreparedHdri {
    /// Decodes `.hdr` / `.exr` bytes and runs every CPU stage of the IBL
    /// build (sanitize, irradiance convolution, average). `format` is the
    /// lowercase extension without the dot.
    ///
    /// # Errors
    /// Returns `Err` if the bytes fail to decode or the format is not one
    /// of the supported HDRI formats.
    pub fn prepare(bytes: &[u8], format: &str) -> Result<Self, RendererError> {
        // An empty format sniffs the container magic (the `.slxy` reload
        // path only retains the content-addressed bytes); the dispatch and
        // the sniff both live in `solarxy_formats::hdr`.
        let image = solarxy_formats::hdr::decode_hdr_image_bytes(bytes, format)?;
        let (width, height) = (image.width, image.height);
        let mut pixels = image.pixels;
        sanitize_hdr_pixels(&mut pixels);
        let irradiance_faces = convolve_equirect(width, height, rgb(&pixels));
        let irradiance_average = compute_irradiance_average(&irradiance_faces);
        let distribution = EnvDistribution::build(width, height, &pixels);
        Ok(Self {
            width,
            height,
            pixels,
            irradiance_faces,
            irradiance_average,
            distribution,
        })
    }

    /// Packs into a little-endian byte blob for the worker boundary:
    /// `width, height`, the average, the six fixed-size irradiance faces,
    /// the equirect pixels, then the sampling distribution.
    ///
    /// The distribution goes last and is length-prefixed by its own dimensions,
    /// so a blob whose image is black carries two zeroes rather than an absent
    /// section, and the reader never has to guess whether one is present.
    #[must_use]
    pub fn pack(&self) -> Vec<u8> {
        let face_len: usize = self.irradiance_faces.iter().map(Vec::len).sum();
        let mut out = Vec::with_capacity(8 + 12 + face_len * 12 + self.pixels.len() * 4);
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        for c in self.irradiance_average {
            out.extend_from_slice(&c.to_le_bytes());
        }
        for face in &self.irradiance_faces {
            out.extend_from_slice(&(face.len() as u32).to_le_bytes());
            for px in face {
                for c in px {
                    out.extend_from_slice(&c.to_le_bytes());
                }
            }
        }
        // The pixel run is the same byte sequence a `[f32; 3]` buffer
        // produced: components in order, little-endian. Flattening the CPU
        // representation did not move the wire format.
        for c in &self.pixels {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&self.distribution.width().to_le_bytes());
        out.extend_from_slice(&self.distribution.height().to_le_bytes());
        out.extend_from_slice(&self.distribution.total_weight().to_le_bytes());
        for c in self.distribution.marginal() {
            out.extend_from_slice(&c.to_le_bytes());
        }
        for c in self.distribution.conditional() {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out
    }

    /// Reverses [`PreparedHdri::pack`].
    ///
    /// # Errors
    /// Returns `Err` on a truncated or malformed blob.
    pub fn unpack(bytes: &[u8]) -> Result<Self, RendererError> {
        let bad = || RendererError::Unsupported("malformed prepared-HDRI blob".to_string());
        let mut off = 0usize;
        let mut take = |n: usize| -> Result<&[u8], RendererError> {
            let end = off.checked_add(n).ok_or_else(bad)?;
            let slice = bytes.get(off..end).ok_or_else(bad)?;
            off = end;
            Ok(slice)
        };
        let read_u32 = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let read_f32 = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

        let width = read_u32(take(4)?);
        let height = read_u32(take(4)?);
        let mut irradiance_average = [0.0f32; 3];
        for c in &mut irradiance_average {
            *c = read_f32(take(4)?);
        }
        let mut faces: Vec<Vec<[f32; 3]>> = Vec::with_capacity(6);
        for _ in 0..6 {
            let len = read_u32(take(4)?) as usize;
            let data = take(len.checked_mul(12).ok_or_else(bad)?)?;
            let face = data
                .as_chunks::<12>()
                .0
                .iter()
                .map(|px| {
                    [
                        read_f32(&px[0..4]),
                        read_f32(&px[4..8]),
                        read_f32(&px[8..12]),
                    ]
                })
                .collect();
            faces.push(face);
        }
        let irradiance_faces: [Vec<[f32; 3]>; 6] = faces.try_into().map_err(|_| bad())?;

        let sample_count = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(3))
            .ok_or_else(bad)?;
        let data = take(sample_count.checked_mul(4).ok_or_else(bad)?)?;
        let pixels = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| read_f32(b))
            .collect();

        let dist_width = read_u32(take(4)?);
        let dist_height = read_u32(take(4)?);
        let total_weight = read_f32(take(4)?);
        let rows = dist_height as usize;
        let cells = (dist_width as usize).checked_mul(rows).ok_or_else(bad)?;
        let marginal: Vec<f32> = take(rows.checked_mul(4).ok_or_else(bad)?)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| read_f32(b))
            .collect();
        let conditional: Vec<f32> = take(cells.checked_mul(4).ok_or_else(bad)?)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| read_f32(b))
            .collect();
        let distribution = EnvDistribution::from_parts(
            dist_width,
            dist_height,
            marginal,
            conditional,
            total_weight,
        );

        Ok(Self {
            width,
            height,
            pixels,
            irradiance_faces,
            irradiance_average,
            distribution,
        })
    }
}

fn compute_irradiance_average(faces: &[Vec<[f32; 3]>; 6]) -> [f32; 3] {
    let mut sum = [0.0_f64; 3];
    let mut count: u32 = 0;
    for face in faces {
        for px in face {
            sum[0] += f64::from(px[0]);
            sum[1] += f64::from(px[1]);
            sum[2] += f64::from(px[2]);
            count += 1;
        }
    }
    let inv = 1.0 / f64::from(count.max(1));
    [
        (sum[0] * inv) as f32,
        (sum[1] * inv) as f32,
        (sum[2] * inv) as f32,
    ]
}

fn create_cubemap(device: &wgpu::Device, label: &str, size: u32, mip_levels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mip_levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_cubemap_face(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    face: u32,
    mip_level: u32,
    size: u32,
    data: &[u8],
) {
    let bytes_per_row = size * 8;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: face,
            },
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(size),
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
}

fn generate_irradiance_sky(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    top: [f32; 3],
    bottom: [f32; 3],
) -> wgpu::Texture {
    const SIZE: u32 = 32;
    const SAMPLES: u32 = 64;

    let texture = create_cubemap(device, "IBL Sky Irradiance Cubemap", SIZE, 1);
    let face_bytes = (SIZE * SIZE * 8) as usize;
    let mut face_data = vec![0u8; face_bytes];

    for face in 0..6u32 {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = (x as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let dir = normalize(face_direction(face, u, v));

                let (t, b, n) = build_tbn(dir);
                let mut acc = [0.0_f32; 3];

                for i in 0..SAMPLES {
                    let (u1, u2) = hammersley(i, SAMPLES);
                    let phi = 2.0 * PI * u1;
                    let cos_theta = u2.sqrt();
                    let sin_theta = (1.0 - u2).sqrt();

                    let local = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                    let world = [
                        t[0] * local[0] + b[0] * local[1] + n[0] * local[2],
                        t[1] * local[0] + b[1] * local[1] + n[1] * local[2],
                        t[2] * local[0] + b[2] * local[1] + n[2] * local[2],
                    ];

                    let blend = world[1] * 0.5 + 0.5;
                    acc[0] += lerp(bottom[0], top[0], blend);
                    acc[1] += lerp(bottom[1], top[1], blend);
                    acc[2] += lerp(bottom[2], top[2], blend);
                }

                let inv = 1.0 / SAMPLES as f32;
                let offset = ((y * SIZE + x) * 8) as usize;
                write_rgba16f(
                    &mut face_data,
                    offset,
                    acc[0] * inv,
                    acc[1] * inv,
                    acc[2] * inv,
                    1.0,
                );
            }
        }

        write_cubemap_face(queue, &texture, face, 0, SIZE, &face_data);
    }

    texture
}

fn irradiance_faces_to_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    faces: &[Vec<[f32; 3]>; 6],
) -> wgpu::Texture {
    const SIZE: u32 = 32;
    let texture = create_cubemap(device, "IBL HDRI Irradiance Cubemap", SIZE, 1);

    for (face_idx, face) in faces.iter().enumerate() {
        let mut data = vec![0u8; (SIZE * SIZE * 8) as usize];
        for (i, rgb) in face.iter().enumerate() {
            write_rgba16f(&mut data, i * 8, rgb[0], rgb[1], rgb[2], 1.0);
        }
        write_cubemap_face(queue, &texture, face_idx as u32, 0, SIZE, &data);
    }

    texture
}

fn generate_prefiltered_sky(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    top: [f32; 3],
    bottom: [f32; 3],
) -> wgpu::Texture {
    let texture = create_cubemap(
        device,
        "IBL Prefiltered Sky",
        PREFILTERED_SIZE,
        PREFILTERED_MIP_COUNT,
    );

    for mip in 0..PREFILTERED_MIP_COUNT {
        let roughness = mip as f32 / (PREFILTERED_MIP_COUNT - 1) as f32;
        let face_size = (PREFILTERED_SIZE >> mip).max(2);
        let sample_count = (128u32 >> mip).max(16);
        let face_bytes = (face_size * face_size * 8) as usize;
        let mut face_data = vec![0u8; face_bytes];

        for face in 0..6u32 {
            for y in 0..face_size {
                for x in 0..face_size {
                    let u = (x as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let v = (y as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let n = normalize(face_direction(face, u, v));
                    let (t, b, nn) = build_tbn(n);

                    let mut acc = [0.0_f32; 3];
                    let mut total_weight = 0.0_f32;

                    for i in 0..sample_count {
                        let xi = hammersley(i, sample_count);
                        let h_local = importance_sample_ggx(xi, roughness);
                        let h = [
                            t[0] * h_local[0] + b[0] * h_local[1] + nn[0] * h_local[2],
                            t[1] * h_local[0] + b[1] * h_local[1] + nn[1] * h_local[2],
                            t[2] * h_local[0] + b[2] * h_local[1] + nn[2] * h_local[2],
                        ];
                        let n_dot_h = dot(n, h).max(0.0);

                        let l = [
                            2.0 * n_dot_h * h[0] - n[0],
                            2.0 * n_dot_h * h[1] - n[1],
                            2.0 * n_dot_h * h[2] - n[2],
                        ];
                        let n_dot_l = dot(n, l);
                        if n_dot_l > 0.0 {
                            let blend = l[1] * 0.5 + 0.5;
                            acc[0] += lerp(bottom[0], top[0], blend) * n_dot_l;
                            acc[1] += lerp(bottom[1], top[1], blend) * n_dot_l;
                            acc[2] += lerp(bottom[2], top[2], blend) * n_dot_l;
                            total_weight += n_dot_l;
                        }
                    }

                    if total_weight > 0.0 {
                        let inv = 1.0 / total_weight;
                        acc[0] *= inv;
                        acc[1] *= inv;
                        acc[2] *= inv;
                    }

                    let offset = ((y * face_size + x) * 8) as usize;
                    write_rgba16f(&mut face_data, offset, acc[0], acc[1], acc[2], 1.0);
                }
            }

            write_cubemap_face(queue, &texture, face, mip, face_size, &face_data);
        }
    }

    texture
}

fn generate_prefiltered_equirect(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    pixels: &[[f32; 3]],
) -> wgpu::Texture {
    let texture = create_cubemap(
        device,
        "IBL Prefiltered HDRI",
        PREFILTERED_SIZE,
        PREFILTERED_MIP_COUNT,
    );

    for mip in 0..PREFILTERED_MIP_COUNT {
        let roughness = mip as f32 / (PREFILTERED_MIP_COUNT - 1) as f32;
        let face_size = (PREFILTERED_SIZE >> mip).max(2);
        let sample_count = (512u32 >> mip).max(16);
        let face_bytes = (face_size * face_size * 8) as usize;
        let mut face_data = vec![0u8; face_bytes];

        for face in 0..6u32 {
            for y in 0..face_size {
                for x in 0..face_size {
                    let u = (x as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let v = (y as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let n = normalize(face_direction(face, u, v));
                    let (t, b, nn) = build_tbn(n);

                    let mut acc = [0.0_f32; 3];
                    let mut total_weight = 0.0_f32;

                    for i in 0..sample_count {
                        let xi = hammersley(i, sample_count);
                        let h_local = importance_sample_ggx(xi, roughness);
                        let h = [
                            t[0] * h_local[0] + b[0] * h_local[1] + nn[0] * h_local[2],
                            t[1] * h_local[0] + b[1] * h_local[1] + nn[1] * h_local[2],
                            t[2] * h_local[0] + b[2] * h_local[1] + nn[2] * h_local[2],
                        ];
                        let n_dot_h = dot(n, h).max(0.0);
                        let l = [
                            2.0 * n_dot_h * h[0] - n[0],
                            2.0 * n_dot_h * h[1] - n[1],
                            2.0 * n_dot_h * h[2] - n[2],
                        ];
                        let n_dot_l = dot(n, l);
                        if n_dot_l > 0.0 {
                            let sample = sample_equirect(width, height, pixels, l);
                            acc[0] += sample[0] * n_dot_l;
                            acc[1] += sample[1] * n_dot_l;
                            acc[2] += sample[2] * n_dot_l;
                            total_weight += n_dot_l;
                        }
                    }

                    if total_weight > 0.0 {
                        let inv = 1.0 / total_weight;
                        acc[0] *= inv;
                        acc[1] *= inv;
                        acc[2] *= inv;
                    }

                    let offset = ((y * face_size + x) * 8) as usize;
                    write_rgba16f(&mut face_data, offset, acc[0], acc[1], acc[2], 1.0);
                }
            }

            write_cubemap_face(queue, &texture, face, mip, face_size, &face_data);
        }
    }

    texture
}

fn generate_brdf_lut(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("IBL BRDF LUT"),
        size: wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let bytes_per_row = BRDF_LUT_SIZE * 4;
    let total_bytes = (BRDF_LUT_SIZE * BRDF_LUT_SIZE * 4) as usize;
    let mut data = vec![0u8; total_bytes];

    for y in 0..BRDF_LUT_SIZE {
        for x in 0..BRDF_LUT_SIZE {
            let n_dot_v = (x as f32 + 0.5) / BRDF_LUT_SIZE as f32;
            let roughness = (y as f32 + 0.5) / BRDF_LUT_SIZE as f32;

            let (scale, bias) = integrate_brdf(n_dot_v.max(0.001), roughness.max(0.001));

            let offset = ((y * BRDF_LUT_SIZE + x) * 4) as usize;
            write_rg16f(&mut data, offset, scale, bias);
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
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(BRDF_LUT_SIZE),
        },
        wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
    );

    texture
}

fn integrate_brdf(n_dot_v: f32, roughness: f32) -> (f32, f32) {
    let sin_v = (1.0 - n_dot_v * n_dot_v).max(0.0).sqrt();
    let v = [sin_v, 0.0, n_dot_v];

    let mut scale = 0.0_f32;
    let mut bias = 0.0_f32;

    for i in 0..BRDF_LUT_SAMPLES {
        let xi = hammersley(i, BRDF_LUT_SAMPLES);
        let h = importance_sample_ggx(xi, roughness);

        let v_dot_h = (v[0] * h[0] + v[1] * h[1] + v[2] * h[2]).max(0.0);
        let l = [
            2.0 * v_dot_h * h[0] - v[0],
            2.0 * v_dot_h * h[1] - v[1],
            2.0 * v_dot_h * h[2] - v[2],
        ];

        let n_dot_l = l[2].max(0.0);
        let n_dot_h = h[2].max(0.0);

        if n_dot_l > 0.0 {
            let g = g_smith_ibl(n_dot_v, n_dot_l, roughness);
            let g_vis = (g * v_dot_h) / (n_dot_h * n_dot_v).max(0.001);
            let fc = (1.0 - v_dot_h).max(0.0).powi(5);

            scale += g_vis * (1.0 - fc);
            bias += g_vis * fc;
        }
    }

    let inv = 1.0 / BRDF_LUT_SAMPLES as f32;
    (scale * inv, bias * inv)
}

fn importance_sample_ggx(xi: (f32, f32), roughness: f32) -> [f32; 3] {
    let a = roughness * roughness;
    let phi = 2.0 * PI * xi.0;
    let cos_theta = ((1.0 - xi.1) / (1.0 + (a * a - 1.0) * xi.1)).sqrt();
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta]
}

fn g_schlick_ibl(n_dot_v: f32, roughness: f32) -> f32 {
    let k = (roughness * roughness) / 2.0;
    n_dot_v / (n_dot_v * (1.0 - k) + k)
}

fn g_smith_ibl(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    g_schlick_ibl(n_dot_v, roughness) * g_schlick_ibl(n_dot_l, roughness)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-10 {
        return [0.0, 1.0, 0.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

fn face_direction(face: u32, u: f32, v: f32) -> [f32; 3] {
    match face {
        0 => [1.0, -v, -u],
        1 => [-1.0, -v, u],
        2 => [u, 1.0, v],
        3 => [u, -1.0, -v],
        4 => [u, -v, 1.0],
        _ => [-u, -v, -1.0],
    }
}

fn build_tbn(n: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let up = if n[1].abs() < 0.999 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    (t, b, n)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (i as f32 / n as f32, radical_inverse_vdc(i))
}

fn radical_inverse_vdc(mut bits: u32) -> f32 {
    bits = bits.rotate_right(16);
    bits = ((bits & 0x55555555) << 1) | ((bits & 0xAAAAAAAA) >> 1);
    bits = ((bits & 0x33333333) << 2) | ((bits & 0xCCCCCCCC) >> 2);
    bits = ((bits & 0x0F0F0F0F) << 4) | ((bits & 0xF0F0F0F0) >> 4);
    bits = ((bits & 0x00FF00FF) << 8) | ((bits & 0xFF00FF00) >> 8);
    bits as f32 * 2.328_306_4e-10
}

/// View a flat three-floats-per-pixel buffer as RGB triples.
///
/// `[f32; 3]` has the same alignment as `f32`, so this is a reinterpret
/// rather than a copy. Truncating to a whole number of triples first makes
/// the cast infallible, which keeps this off the panic path even though
/// every producer already upholds the invariant.
fn rgb(pixels: &[f32]) -> &[[f32; 3]] {
    let usable = pixels.len() - pixels.len() % 3;
    bytemuck::cast_slice(&pixels[..usable])
}

/// Replace non-finite samples with `0` and clamp negative radiance to `0`.
/// HDRIs in the wild are not always clean (sun disks, sloppy exporters),
/// and the IBL convolution + f16 skybox upload downstream assume finite,
/// non-negative input — an `Inf` here becomes an f16 infinity that bloom
/// amplifies into sparkle.
fn sanitize_hdr_pixels(pixels: &mut [f32]) {
    for c in pixels {
        *c = clean_sample(*c);
    }
}

/// The single per-sample rule both sanitize paths apply, so the in-place
/// one and the borrowed one cannot drift.
fn clean_sample(c: f32) -> f32 {
    if c.is_finite() { c.max(0.0) } else { 0.0 }
}

/// Borrowed-input sanitize: returns the input untouched when it is already
/// clean, and a corrected copy only when it is not.
///
/// An HDRI arriving through the scene contract is shared behind an `Arc`
/// with the engine, so it cannot be fixed in place. Copying every one
/// unconditionally would mean a second full-resolution allocation per
/// install (about 100 MB for a 4K equirect) to fix samples that almost
/// never need fixing. Scanning first is one cheap read pass, and the clean
/// case (nearly all of them) then costs nothing at all.
fn sanitized(pixels: &[f32]) -> std::borrow::Cow<'_, [f32]> {
    if pixels.iter().all(|c| c.is_finite() && *c >= 0.0) {
        std::borrow::Cow::Borrowed(pixels)
    } else {
        std::borrow::Cow::Owned(pixels.iter().map(|c| clean_sample(*c)).collect())
    }
}

fn sample_equirect(width: u32, height: u32, pixels: &[[f32; 3]], dir: [f32; 3]) -> [f32; 3] {
    let theta = dir[1].clamp(-1.0, 1.0).acos();
    let phi = dir[2].atan2(dir[0]);
    let u = (phi + PI) / (2.0 * PI);
    let v = theta / PI;
    let px = ((u * width as f32) as u32).min(width - 1);
    let py = ((v * height as f32) as u32).min(height - 1);
    pixels[(py * width + px) as usize]
}

fn convolve_equirect(width: u32, height: u32, pixels: &[[f32; 3]]) -> [Vec<[f32; 3]>; 6] {
    const SIZE: u32 = 32;
    const SAMPLES: u32 = 256;

    std::array::from_fn(|face| {
        let mut face_data = Vec::with_capacity((SIZE * SIZE) as usize);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = (x as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let dir = normalize(face_direction(face as u32, u, v));

                let (t, b, n) = build_tbn(dir);
                let mut acc = [0.0_f32; 3];

                for i in 0..SAMPLES {
                    let (u1, u2) = hammersley(i, SAMPLES);
                    let phi = 2.0 * PI * u1;
                    let cos_theta = u2.sqrt();
                    let sin_theta = (1.0 - u2).sqrt();

                    let local = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                    let world = normalize([
                        t[0] * local[0] + b[0] * local[1] + n[0] * local[2],
                        t[1] * local[0] + b[1] * local[1] + n[1] * local[2],
                        t[2] * local[0] + b[2] * local[1] + n[2] * local[2],
                    ]);

                    let sample = sample_equirect(width, height, pixels, world);
                    acc[0] += sample[0];
                    acc[1] += sample[1];
                    acc[2] += sample[2];
                }

                let inv = 1.0 / SAMPLES as f32;
                face_data.push([acc[0] * inv, acc[1] * inv, acc[2] * inv]);
            }
        }
        face_data
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_hdr_pixels_drops_non_finite_and_negative() {
        let mut pixels = [f32::INFINITY, -1.0, 2.0, f32::NAN, 0.5, f32::NEG_INFINITY];
        sanitize_hdr_pixels(&mut pixels);
        let expect = [0.0_f32, 0.0, 2.0, 0.0, 0.5, 0.0];
        for (got, want) in pixels.iter().zip(expect.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn rgb_views_a_flat_buffer_without_copying() {
        let flat = [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        let triples = rgb(&flat);
        assert_eq!(triples, &[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(triples.as_ptr().cast::<f32>(), flat.as_ptr());
    }

    #[test]
    fn rgb_truncates_a_partial_trailing_pixel_rather_than_panicking() {
        // No producer in the tree can emit this, but the cast would panic
        // on it, and a panic in the lighting path is worse than a dropped
        // trailing sample.
        assert_eq!(rgb(&[1.0, 2.0, 3.0, 4.0]), &[[1.0, 2.0, 3.0]]);
        assert!(rgb(&[1.0, 2.0]).is_empty());
        assert!(rgb(&[]).is_empty());
    }
}

#[cfg(test)]
// Bitwise float equality is the point of these tests: the worker path must
// reproduce the inline path exactly, and the codec must be lossless.
#[allow(clippy::float_cmp)]
mod prepared_tests {
    use super::*;

    /// A tiny synthetic 4x2 Radiance HDR file (RLE-free), enough to
    /// exercise decode + convolve deterministically.
    fn tiny_hdr() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n");
        out.extend_from_slice(b"-Y 2 +X 4\n");
        // 8 flat RGBE pixels (r=128, g=64, b=32, e=128 -> mid grey-ish).
        for _ in 0..8 {
            out.extend_from_slice(&[128, 64, 32, 128]);
        }
        out
    }

    #[test]
    fn prepare_matches_the_inline_hdr_path_stage_for_stage() {
        // The worker path (prepare) must produce exactly the CPU data the
        // inline `from_hdr_bytes` path derives, so a web-prepared IBL is
        // bitwise-identical to a desktop-loaded one.
        let bytes = tiny_hdr();
        let prepared = PreparedHdri::prepare(&bytes, "hdr").expect("prepare");

        let image = solarxy_formats::hdr::decode_hdr_bytes(&bytes).expect("decode");
        let (w, h) = (image.width, image.height);
        let mut pixels = image.pixels;
        sanitize_hdr_pixels(&mut pixels);
        let faces = convolve_equirect(w, h, rgb(&pixels));
        let avg = compute_irradiance_average(&faces);

        assert_eq!((prepared.width, prepared.height), (w, h));
        assert_eq!(prepared.pixels, pixels);
        assert_eq!(prepared.irradiance_faces, faces);
        assert_eq!(prepared.irradiance_average, avg);
    }

    #[test]
    fn prepared_hdri_round_trips_through_the_pack_codec() {
        let prepared = PreparedHdri::prepare(&tiny_hdr(), "hdr").expect("prepare");
        let back = PreparedHdri::unpack(&prepared.pack()).expect("unpack");
        assert_eq!(back.width, prepared.width);
        assert_eq!(back.height, prepared.height);
        assert_eq!(back.pixels, prepared.pixels);
        assert_eq!(back.irradiance_faces, prepared.irradiance_faces);
        assert_eq!(back.irradiance_average, prepared.irradiance_average);
        // The sampling distribution rides the same blob, and it is the half a
        // reader is most likely to add a section for and forget to read back:
        // an environment whose tables arrived empty falls silently back to
        // uniform sampling, which converges to the right picture slowly enough
        // that it reads as the tracer being slow rather than as a broken
        // transfer.
        assert_eq!(back.distribution, prepared.distribution);
        assert!(!back.distribution.is_empty());
    }

    #[test]
    fn unpack_rejects_truncated_blobs() {
        let prepared = PreparedHdri::prepare(&tiny_hdr(), "hdr").expect("prepare");
        let packed = prepared.pack();
        assert!(PreparedHdri::unpack(&packed[..packed.len() - 4]).is_err());
        assert!(PreparedHdri::unpack(&[1, 2, 3]).is_err());
    }

    #[test]
    fn prepare_rejects_unknown_formats() {
        assert!(PreparedHdri::prepare(&[0u8; 16], "png").is_err());
    }
}
