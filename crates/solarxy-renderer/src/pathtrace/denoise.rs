//! The edge-aware denoiser: the a-trous wavelet filter and the scratch it
//! ping-pongs through.
//!
//! # Its own bind groups, and why that is not a contradiction
//!
//! The tracer's binding budget is spent: seven of core WebGPU's eight storage
//! buffers, four of four storage textures, the whole sampled group. None of
//! that constrains this, because a limit is per pipeline and this is a
//! different pipeline. It binds three storage textures and one uniform and has
//! room it does not use.
//!
//! # Where it sits
//!
//! Between the accumulator and the resolve. It reads the running mean and the
//! auxiliary channels, writes a filtered copy into scratch of its own, and the
//! resolve takes that instead. It never writes the accumulator: the next
//! dispatch has to fold its samples into the *unfiltered* mean, and an
//! accumulator that had been filtered in place would compound its own bias
//! every chunk until the image was a smear of its first frame.

use bytemuck::{Pod, Zeroable};

use crate::pathtrace::{TraceTarget, WORKGROUP_SIZE};

/// The filter, composed over the auxiliary packing it shares with the kernel
/// that writes the guides.
const DENOISE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/aov.wgsl"),
    include_str!("../shaders/pathtrace/denoise.wgsl"),
);

/// Wavelet levels.
///
/// Five, which is the top of the three-to-five range the design ratified. Each
/// doubles the stride, so five reach a 33-pixel support from 25 taps a level.
/// Fewer leaves low-sample noise at the coarse scales, where it reads as
/// blotches rather than grain and is the more objectionable of the two.
pub const DENOISE_LEVELS: u32 = 5;

/// How the filter is steered.
///
/// Defaults measured against a one-sample frame of two sharply different
/// materials under a graded sky, scored two ways at once: error against a
/// 512-sample reference, and how much of the material step survives. Both
/// numbers are in `tests/pathtrace_denoise.rs`, which is also where the sweep
/// that produced them lives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DenoiseSettings {
    /// How far apart two radiances may be before a tap is discounted, at one
    /// sample and the finest level. Scaled down by the level and by the square
    /// root of the sample count inside the kernel.
    pub sigma_color: f32,
    /// The exponent on the cosine between two normals. Larger is narrower.
    pub normal_power: f32,
    /// How far apart two base colours may be before a tap is discounted.
    pub sigma_albedo: f32,
    /// How fast the colour tolerance tightens as the support widens: the
    /// tolerance is divided by this raised to the level.
    pub level_falloff: f32,
}

impl Default for DenoiseSettings {
    fn default() -> Self {
        Self {
            // Measured rather than picked. On a one-sample frame of two
            // sharply different materials, restricted to the pixels that found
            // a surface: 0.3 leaves 76% of the error, 0.6 leaves 55%, 1.2
            // leaves 33%, and everything past it is within three points of
            // 30% while being progressively more willing to average across a
            // colour difference that was real. This is the smallest tolerance
            // that reaches the plateau.
            sigma_color: 1.2,
            // About ten degrees of agreement, which keeps a sphere's shading
            // gradient intact while rejecting the far side of a crease.
            normal_power: 128.0,
            // Tight. An albedo step is a material boundary, and the whole
            // reason the guide is here is that blurring across one is the
            // artifact a viewer notices first.
            sigma_albedo: 0.08,
            // The paper's, and the measurement agrees the choice is nearly
            // free here: 1.0 and 2.0 differ by two points of error and one of
            // edge separation. Halving per level is what keeps the coarse
            // levels from averaging across a boundary the fine ones held.
            level_falloff: 2.0,
        }
    }
}

/// Per-dispatch uniforms. One level of the filter.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct DenoiseParams {
    pub resolution: [u32; 2],
    pub stride: u32,
    pub level: u32,
    pub samples: u32,
    pub sigma_color: f32,
    pub normal_power: f32,
    pub sigma_albedo: f32,
    pub level_falloff: f32,
    pub _pad: u32,
}

// Forty bytes with one pad word. `resolution` is a `vec2u` and aligns the
// struct to eight; the eight real members occupy thirty-six, which rounds up to
// forty. WGSL does that rounding whether or not the pad is named, so naming it
// is what keeps the two sides the same size; `tests/uniform_layout.rs` is what
// checks that they are.
const _: () = assert!(std::mem::size_of::<DenoiseParams>() == 40);

/// The a-trous filter and the two textures it ping-pongs through.
pub struct Denoiser {
    pipeline: wgpu::ComputePipeline,
    images: wgpu::BindGroupLayout,
    /// **One uniform buffer per level, not one reused across them.**
    ///
    /// `Queue::write_buffer` is not interleaved with the encoder's commands: a
    /// submission applies every queued write first and then runs the command
    /// buffers. Writing one buffer between five encoded dispatches therefore
    /// gives all five the *last* level's stride, which produces a plausible
    /// filtered image that is not the a-trous transform of anything. Five
    /// buffers is the correction, and it costs a hundred and sixty bytes.
    params: Vec<wgpu::Buffer>,
    params_groups: Vec<wgpu::BindGroup>,
    /// Scratch, allocated on first use and reallocated when the pane resizes.
    scratch: Option<Scratch>,
    settings: DenoiseSettings,
}

/// Which scratch slot the last level writes, and so where the result is.
const FINAL_SLOT: usize = ((DENOISE_LEVELS - 1) % 2) as usize;

struct Scratch {
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    width: u32,
    height: u32,
}

impl Denoiser {
    /// Builds the pipeline. Allocates no scratch: a backend with the filter
    /// switched off never pays for one.
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Denoise Shader"),
            source: wgpu::ShaderSource::Wgsl(DENOISE_KERNEL.into()),
        });
        let storage =
            |binding: u32, access: wgpu::StorageTextureAccess| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access,
                    format: wgpu::TextureFormat::Rgba32Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            };
        let images = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_denoise_images_layout"),
            entries: &[
                storage(0, wgpu::StorageTextureAccess::ReadOnly),
                storage(1, wgpu::StorageTextureAccess::WriteOnly),
                storage(2, wgpu::StorageTextureAccess::ReadOnly),
            ],
        });
        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_denoise_params_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let params: Vec<wgpu::Buffer> = (0..DENOISE_LEVELS)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Pathtrace Denoise Params"),
                    size: std::mem::size_of::<DenoiseParams>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let params_groups: Vec<wgpu::BindGroup> = params
            .iter()
            .map(|buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Pathtrace Denoise Params Group"),
                    layout: &params_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                })
            })
            .collect();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Denoise Pipeline Layout"),
            bind_group_layouts: &[&images, &params_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Pathtrace Denoise Pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("denoise_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        Self {
            pipeline,
            images,
            params,
            params_groups,
            scratch: None,
            settings: DenoiseSettings::default(),
        }
    }

    pub fn set_settings(&mut self, settings: DenoiseSettings) {
        self.settings = settings;
    }

    #[must_use]
    pub fn settings(&self) -> DenoiseSettings {
        self.settings
    }

    /// Filters `target`'s current mean and returns the view holding the result.
    ///
    /// The returned view is scratch this owns, not the accumulator: see the
    /// module documentation for why filtering in place would compound.
    ///
    /// `samples` is how many the mean averages, which is what tells the filter
    /// how much noise is left to hide.
    ///
    /// # Panics
    ///
    /// Never. The `expect` below cannot fire: the scratch is allocated on the
    /// line above it.
    pub fn encode<'a>(
        &'a mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &TraceTarget,
        samples: u32,
    ) -> &'a wgpu::TextureView {
        let (width, height) = (target.width(), target.height());
        self.ensure_scratch(device, width, height);
        let scratch = self
            .scratch
            .as_ref()
            .expect("scratch was just allocated for this size");

        let aux = target.auxiliary_view();
        for level in 0..DENOISE_LEVELS {
            let index = level as usize;
            let slot = index % 2;
            // Level zero reads the accumulator; every level after it reads what
            // the level before wrote. Resolved per level rather than carried in
            // a variable, so the returned view below borrows the scratch alone
            // and does not have to unify two lifetimes the loop cannot relate.
            let source: &wgpu::TextureView = if level == 0 {
                target.color_view()
            } else {
                &scratch.views[(index - 1) % 2]
            };
            let params = DenoiseParams {
                resolution: [width, height],
                stride: 1 << level,
                level,
                samples: samples.max(1),
                sigma_color: self.settings.sigma_color,
                normal_power: self.settings.normal_power,
                sigma_albedo: self.settings.sigma_albedo,
                level_falloff: self.settings.level_falloff,
                _pad: 0,
            };
            queue.write_buffer(&self.params[index], 0, bytemuck::bytes_of(&params));
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Pathtrace Denoise Images"),
                layout: &self.images,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&scratch.views[slot]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(aux),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pathtrace Denoise Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.set_bind_group(1, &self.params_groups[index], &[]);
            pass.dispatch_workgroups(
                width.div_ceil(WORKGROUP_SIZE),
                height.div_ceil(WORKGROUP_SIZE),
                1,
            );
        }
        &scratch.views[FINAL_SLOT]
    }

    fn ensure_scratch(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self
            .scratch
            .as_ref()
            .is_some_and(|s| s.width == width && s.height == height)
        {
            return;
        }
        let make = |label: &str| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                // `TEXTURE_BINDING` because the resolve samples whichever of
                // these the last level wrote; `COPY_SRC` so a test can read one
                // back the way it reads the accumulator.
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let textures = [make("Pathtrace Denoise A"), make("Pathtrace Denoise B")];
        let views = [
            textures[0].create_view(&wgpu::TextureViewDescriptor::default()),
            textures[1].create_view(&wgpu::TextureViewDescriptor::default()),
        ];
        self.scratch = Some(Scratch {
            textures,
            views,
            width,
            height,
        });
    }

    /// The texture holding the filtered result of the last [`Denoiser::encode`],
    /// for a caller that copies rather than samples.
    #[must_use]
    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.scratch.as_ref().map(|s| &s.textures[FINAL_SLOT])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_levels_reach_a_thirty_three_pixel_support() {
        // Each level's kernel spans two taps either side at its stride, so the
        // radius is twice the sum of the strides. This is the number the level
        // count is chosen for, and it moves if anyone changes either.
        let radius: u32 = (0..DENOISE_LEVELS).map(|l| 2 * (1 << l)).sum();
        assert_eq!(radius, 62);
        assert_eq!(DENOISE_LEVELS, 5);
    }

    #[test]
    fn the_albedo_guide_is_tighter_than_the_colour_one() {
        // The ordering is the design, not a coincidence. Colour is noisy and
        // has to tolerate disagreement; albedo arrives free of noise, so a
        // difference in it is a real material boundary and blurring across one
        // is the artifact this filter exists to avoid.
        let s = DenoiseSettings::default();
        assert!(s.sigma_albedo < s.sigma_color);
        assert!(s.normal_power >= 64.0);
    }
}
