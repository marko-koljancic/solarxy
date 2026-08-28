//! The shader probes: drive a corpus through a kernel and read the answers
//! back.
//!
//! These exist because a shader cannot be unit tested. Two questions live here,
//! and both are only answerable on a device.
//!
//! [`TraversalProbe`] pins the WGSL traversal to the Rust one. `solarxy_bvh`
//! writes the traversal twice and pins its Rust half to
//! `solarxy_core::raycast`; this is the remaining link in that chain, and the
//! only one that needs a GPU.
//!
//! [`AtlasProbe`] pins the packing arithmetic to the shader that reads it. Its
//! real subject is the guard ring: whether a bilinear tap at the extreme edge
//! of a sub-rectangle stays inside its own border or reaches the neighbour is a
//! question about hardware interpolation, and nothing on the CPU can answer it.
//!
//! [`MaterialProbe`] pins the two declarations of the material record to each
//! other. Its subject is field order, which no automatic check reaches: the size
//! guard measures a total and the Rust offset assertions measure one side, so two
//! transposed sixteen-byte blocks satisfy both and shade wrong.
//!
//! [`RandProbe`] pins the sampler's decorrelation. Its subject is the pixel,
//! which every other probe holds fixed: whether two pixels draw different point
//! sets is invisible to any instrument that varies only the sample index, and
//! the failure it exists for looked, in an image, like a faint stationary
//! swirl across every smooth gradient.
//!
//! [`BsdfProbe`] pins the lobes to themselves. Its two subjects are whether the
//! density a sampler reports describes the directions it actually produces, which
//! only a histogram over many samples can answer, and whether the throughput
//! integrates to something a surface could reflect, which is the furnace question.
//! Neither is visible from Rust, and in an image both look like noise or like a
//! surface that is slightly too bright, which is to say like nothing at all until
//! someone compares two renderers.
//!
//! They live in the library rather than in the tests that use them because the
//! browser needs to run the same checks: the desktop's WGSL front end and the
//! browser's are different implementations of the same specification, and the
//! codebase has already lost time to one accepting what the other rejects.
//! Nothing in the shipped shells reaches either, so both are stripped from the
//! artifact they build.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::{TraceAtlas, TraceScene, WORKGROUP_SIZE};
use crate::bind_groups::PathtraceLayouts;

/// The kernel, composed over the traversal that ships.
const PARITY_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/parity.wgsl"),
);

/// The atlas kernel, composed over the atlas fragment that ships.
const ATLAS_PROBE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/atlas.wgsl"),
    include_str!("../shaders/pathtrace/atlas_probe.wgsl"),
);

/// The sampler kernel, composed over the sampler fragment that ships.
const RAND_PROBE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/rand.wgsl"),
    include_str!("../shaders/pathtrace/rand_probe.wgsl"),
);

/// The material kernel, composed over all three fragments that ship. It binds
/// the real scene group, so it reads the pool through binding 4 rather than
/// through one of its own.
const MATERIAL_PROBE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/atlas.wgsl"),
    include_str!("../shaders/pathtrace/material.wgsl"),
    include_str!("../shaders/pathtrace/material_probe.wgsl"),
);

/// The BSDF kernel, composed over everything the material response reads: the
/// traversal for the record, the atlas and the material fragment for the taps, the
/// sampler for the generator, and the lobes themselves.
const BSDF_PROBE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/atlas.wgsl"),
    include_str!("../shaders/pathtrace/material.wgsl"),
    include_str!("../shaders/pathtrace/rand.wgsl"),
    include_str!("../shaders/pathtrace/bsdf.wgsl"),
    include_str!("../shaders/pathtrace/bsdf_probe.wgsl"),
);

/// The light kernel, composed over everything light sampling reads. The
/// environment rides along because the light fragment's estimator calls into it,
/// and the estimator is not what this probe drives: the two functions it does
/// call, `sample_light` and `intersect_lights`, read only the light array, which
/// is why this needs no camera and no environment uniform bound.
const LIGHT_PROBE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/atlas.wgsl"),
    include_str!("../shaders/pathtrace/material.wgsl"),
    include_str!("../shaders/pathtrace/rand.wgsl"),
    include_str!("../shaders/pathtrace/bsdf.wgsl"),
    include_str!("../shaders/pathtrace/environment.wgsl"),
    include_str!("../shaders/pathtrace/light.wgsl"),
    include_str!("../shaders/pathtrace/light_probe.wgsl"),
);

/// Rays per row of the dispatch grid. The kernel's workgroup shape is shared
/// with the real one, so a linear corpus is walked as a 2D grid.
const CORPUS_WIDTH: u32 = 64;

/// Taps per row of the atlas probe's dispatch grid, for the same reason.
const TAP_WIDTH: u32 = 64;

/// How many `vec4` the material probe writes per tap: the record's nine blocks
/// then the three the resolved surface takes. The kernel is given this value, so
/// a change here is a change there.
pub const MATERIAL_RESULT_WIDTH: usize = 12;

/// How many `vec4` the BSDF probe writes per tap: the sampled or evaluated
/// direction with its density, the throughput with the lobe that produced it, and
/// the selection distribution. Both modes write the same three, so the host reads
/// one shape and the two are comparable.
pub const BSDF_RESULT_WIDTH: usize = 3;

/// How many `vec4` the light probe writes per tap: the direction with its
/// density, and the radiance with the distance. Both modes write the same two,
/// so the host reads one shape and the two are comparable.
pub const LIGHT_RESULT_WIDTH: usize = 2;

/// Bit 0 of [`CorpusHit::flags`]: the closest-hit walk found something.
pub const HIT_CLOSEST: u32 = 1 << 0;
/// Bit 1 of [`CorpusHit::flags`]: the any-hit walk found something.
pub const HIT_ANY: u32 = 1 << 1;

/// One corpus ray, as the kernel reads it.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct CorpusRay {
    /// World-space origin in `xyz`; `w` unused.
    pub origin: [f32; 4],
    /// World-space direction in `xyz`, unit length; `w` unused.
    pub direction: [f32; 4],
}

/// One hit record, as the kernel writes it.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct CorpusHit {
    pub t: f32,
    pub instance: u32,
    pub prim: u32,
    /// [`HIT_CLOSEST`] and [`HIT_ANY`].
    pub flags: u32,
    /// Barycentric `[w, u, v]` in `xyz`; `w` unused.
    pub bary: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<CorpusRay>() == 32);
const _: () = assert!(std::mem::size_of::<CorpusHit>() == 32);

impl CorpusHit {
    /// Whether the closest-hit walk found anything.
    #[must_use]
    pub fn hit(&self) -> bool {
        self.flags & HIT_CLOSEST != 0
    }

    /// Whether the any-hit walk found anything.
    ///
    /// A genuinely different traversal, which orders no children and returns
    /// early, answering the same question.
    #[must_use]
    pub fn occluded(&self) -> bool {
        self.flags & HIT_ANY != 0
    }
}

/// The parity pipeline and its own bind-group layout.
pub struct TraversalProbe {
    pipeline: wgpu::ComputePipeline,
    io_layout: wgpu::BindGroupLayout,
}

impl TraversalProbe {
    /// Builds the pipeline. This is the call the browser check is watching:
    /// a WGSL front end that rejects the traversal fails here.
    #[must_use]
    pub fn new(device: &wgpu::Device, scene_layout: &wgpu::BindGroupLayout) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Parity Shader"),
            source: wgpu::ShaderSource::Wgsl(PARITY_KERNEL.into()),
        });
        let io_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_parity_io_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Parity Pipeline Layout"),
            bind_group_layouts: &[scene_layout, &io_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Pathtrace Parity Pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("parity"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("CORPUS_WIDTH", f64::from(CORPUS_WIDTH))],
                zero_initialize_workgroup_memory: false,
            },
            cache: None,
        });
        Self {
            pipeline,
            io_layout,
        }
    }

    /// Encodes and submits one corpus, returning a readback to poll.
    ///
    /// Submitting here rather than taking an encoder keeps the caller from
    /// having to know that the map must be armed after the submit.
    #[must_use]
    pub fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &TraceScene,
        rays: &[CorpusRay],
    ) -> HitReadback {
        let count = rays.len().max(1);
        // An empty corpus is a caller mistake rather than a scene state, but a
        // zero-sized binding is invalid, so it becomes one ray that hits
        // nothing instead of a validation error.
        let placeholder = [CorpusRay::default()];
        let ray_bytes: &[u8] = if rays.is_empty() {
            bytemuck::cast_slice(&placeholder)
        } else {
            bytemuck::cast_slice(rays)
        };
        let ray_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Pathtrace Corpus Rays"),
            contents: ray_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let hit_bytes = (count * std::mem::size_of::<CorpusHit>()) as u64;
        let hit_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Corpus Hits"),
            size: hit_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Corpus Hits Readback"),
            size: hit_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let io = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Parity IO Bind Group"),
            layout: &self.io_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ray_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: hit_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pathtrace Parity Encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pathtrace Parity Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, scene.bind_group(), &[]);
            pass.set_bind_group(1, &io, &[]);
            let rows = (count as u32).div_ceil(CORPUS_WIDTH);
            pass.dispatch_workgroups(
                CORPUS_WIDTH / WORKGROUP_SIZE,
                rows.div_ceil(WORKGROUP_SIZE),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&hit_buffer, 0, &staging, 0, hit_bytes);
        queue.submit(Some(encoder.finish()));

        HitReadback {
            buffer: staging,
            count,
            receiver: None,
        }
    }
}

/// One in-flight corpus readback.
pub struct HitReadback {
    buffer: wgpu::Buffer,
    count: usize,
    receiver: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
}

/// The state of a polled corpus readback.
pub enum HitPoll {
    /// Not resolved yet; poll again.
    Pending,
    /// The map failed; the readback is abandoned.
    Failed,
    /// One record per ray.
    Ready(Vec<CorpusHit>),
}

impl HitReadback {
    /// Pumps the device without blocking and checks the map.
    ///
    /// Never `PollType::Wait`: WebGPU has no blocking wait, so a probe that
    /// blocked would work on the desktop and hang in the browser, which is the
    /// one place it most needs to run.
    pub fn poll(&mut self, device: &wgpu::Device) -> HitPoll {
        if self.receiver.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            self.receiver = Some(rx);
        }
        let _ = device.poll(wgpu::PollType::Poll);

        let Some(rx) = self.receiver.as_ref() else {
            return HitPoll::Failed;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => return HitPoll::Pending,
            Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                tracing::error!("pathtrace parity readback map failed");
                return HitPoll::Failed;
            }
        }

        let data = self.buffer.slice(..).get_mapped_range();
        let hits: Vec<CorpusHit> =
            bytemuck::cast_slice::<u8, CorpusHit>(&data)[..self.count].into();
        drop(data);
        self.buffer.unmap();
        HitPoll::Ready(hits)
    }
}

/// One atlas sample the probe asks for.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct AtlasTap {
    /// The sub-rectangle, page-normalized, exactly as
    /// [`super::atlas::AtlasPlan::rect`] produced it.
    pub rect: [f32; 4],
    /// The texture coordinate, before wrapping.
    pub uv: [f32; 2],
    /// The packed descriptor.
    pub desc: u32,
    pub _pad: u32,
}

const _: () = assert!(std::mem::size_of::<AtlasTap>() == 32);

/// The atlas sampling pipeline and its own bind-group layouts.
pub struct AtlasProbe {
    pipeline: wgpu::ComputePipeline,
    io_layout: wgpu::BindGroupLayout,
    /// Group 1, empty. The sampled group is group 2 by design and a pipeline
    /// layout is indexed by group number, so the gap is occupied rather than
    /// closed: renumbering it here would mean the probe sampled through
    /// different bindings than the kernel does, which is the one thing it must
    /// not do.
    gap: wgpu::BindGroup,
}

impl AtlasProbe {
    /// Builds the pipeline. Like the traversal probe, this is the call a
    /// browser check is watching.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Atlas Probe Shader"),
            source: wgpu::ShaderSource::Wgsl(ATLAS_PROBE_KERNEL.into()),
        });
        let io_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_atlas_probe_io_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let gap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_atlas_probe_gap_bind_group_layout"),
            entries: &[],
        });
        let gap = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Atlas Probe Gap Bind Group"),
            layout: &gap_layout,
            entries: &[],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Atlas Probe Pipeline Layout"),
            bind_group_layouts: &[&io_layout, &gap_layout, &layouts.sampled],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Pathtrace Atlas Probe Pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("atlas_probe"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("TAP_WIDTH", f64::from(TAP_WIDTH))],
                zero_initialize_workgroup_memory: false,
            },
            cache: None,
        });
        Self {
            pipeline,
            io_layout,
            gap,
        }
    }

    /// Encodes and submits one batch of taps, returning a readback to poll.
    #[must_use]
    pub fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &TraceAtlas,
        taps: &[AtlasTap],
    ) -> ColorReadback {
        let count = taps.len().max(1);
        // An empty batch is a caller mistake rather than a scene state, but a
        // zero-sized binding is invalid, so it becomes one tap of nothing.
        let placeholder = [AtlasTap::default()];
        let tap_bytes: &[u8] = if taps.is_empty() {
            bytemuck::cast_slice(&placeholder)
        } else {
            bytemuck::cast_slice(taps)
        };
        let tap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Pathtrace Atlas Taps"),
            contents: tap_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let result_bytes = (count * 16) as u64;
        let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Atlas Results"),
            size: result_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Atlas Results Readback"),
            size: result_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let io = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Atlas Probe IO Bind Group"),
            layout: &self.io_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: tap_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: result_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pathtrace Atlas Probe Encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pathtrace Atlas Probe Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &io, &[]);
            pass.set_bind_group(1, &self.gap, &[]);
            pass.set_bind_group(2, atlas.bind_group(), &[]);
            let rows = (count as u32).div_ceil(TAP_WIDTH);
            pass.dispatch_workgroups(TAP_WIDTH / WORKGROUP_SIZE, rows.div_ceil(WORKGROUP_SIZE), 1);
        }
        encoder.copy_buffer_to_buffer(&result_buffer, 0, &staging, 0, result_bytes);
        queue.submit(Some(encoder.finish()));

        ColorReadback {
            buffer: staging,
            count,
            receiver: None,
        }
    }
}

/// One in-flight batch of atlas samples.
pub struct ColorReadback {
    buffer: wgpu::Buffer,
    count: usize,
    receiver: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
}

/// The state of a polled atlas readback.
pub enum ColorPoll {
    /// Not resolved yet; poll again.
    Pending,
    /// The map failed; the readback is abandoned.
    Failed,
    /// One linear RGBA sample per tap.
    Ready(Vec<[f32; 4]>),
}

impl ColorReadback {
    /// Pumps the device without blocking and checks the map.
    pub fn poll(&mut self, device: &wgpu::Device) -> ColorPoll {
        if self.receiver.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            self.receiver = Some(rx);
        }
        let _ = device.poll(wgpu::PollType::Poll);

        let Some(rx) = self.receiver.as_ref() else {
            return ColorPoll::Failed;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => return ColorPoll::Pending,
            Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                tracing::error!("pathtrace atlas readback map failed");
                return ColorPoll::Failed;
            }
        }

        let data = self.buffer.slice(..).get_mapped_range();
        let colors: Vec<[f32; 4]> =
            bytemuck::cast_slice::<u8, [f32; 4]>(&data)[..self.count].into();
        drop(data);
        self.buffer.unmap();
        ColorPoll::Ready(colors)
    }
}

/// One stratified draw the probe asks for.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct RandTap {
    /// The pixel the generator is seeded for. This is the field the probe
    /// exists to vary: every other probe holds it at the origin.
    pub pixel: [u32; 2],
    /// Which sample of the sequence to draw.
    pub sample_index: u32,
    /// The total sample count, which is what turns stratification on.
    pub strata: u32,
    /// The per-render seed.
    pub seed: u32,
    /// The dimension label, as the kernel's `RNG_DIM_*` values.
    pub dim: u32,
    /// Nonzero draws the scalar path, zero the pair path; both rotate per
    /// pixel and both need the instrument.
    pub scalar: u32,
    pub _pad: u32,
}

const _: () = assert!(std::mem::size_of::<RandTap>() == 32);

/// The sampler pipeline and its own io layout.
///
/// What it answers is whether two pixels draw different point sets, which no
/// other probe can: they all fix the pixel and vary the sample index, the
/// right shape for comparing a density to its sampler and a shape that is
/// structurally blind to correlation across the image. It binds nothing but
/// its own io, because the sampler is self-contained arithmetic; a wrong
/// answer here is the sampler's own.
pub struct RandProbe {
    pipeline: wgpu::ComputePipeline,
    io_layout: wgpu::BindGroupLayout,
}

impl RandProbe {
    /// Builds the pipeline. Like the other probes, this is the call a browser
    /// check is watching.
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Rand Probe Shader"),
            source: wgpu::ShaderSource::Wgsl(RAND_PROBE_KERNEL.into()),
        });
        let io_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_rand_probe_io_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Rand Probe Pipeline Layout"),
            bind_group_layouts: &[&io_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Pathtrace Rand Probe Pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("rand_probe"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("TAP_WIDTH", f64::from(TAP_WIDTH))],
                zero_initialize_workgroup_memory: false,
            },
            cache: None,
        });
        Self {
            pipeline,
            io_layout,
        }
    }

    /// Encodes and submits one batch of taps, returning a readback to poll.
    /// Each result carries the stratified pair in its first two lanes.
    #[must_use]
    pub fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        taps: &[RandTap],
    ) -> ColorReadback {
        let count = taps.len().max(1);
        // An empty batch is a caller mistake rather than a scene state, but a
        // zero-sized binding is invalid, so it becomes one tap of nothing.
        let placeholder = [RandTap::default()];
        let tap_bytes: &[u8] = if taps.is_empty() {
            bytemuck::cast_slice(&placeholder)
        } else {
            bytemuck::cast_slice(taps)
        };
        let tap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Pathtrace Rand Taps"),
            contents: tap_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let result_bytes = (count * 16) as u64;
        let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Rand Results"),
            size: result_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Rand Results Readback"),
            size: result_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let io = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Rand Probe IO Bind Group"),
            layout: &self.io_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: tap_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: result_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pathtrace Rand Probe Encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pathtrace Rand Probe Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &io, &[]);
            let rows = (count as u32).div_ceil(TAP_WIDTH);
            pass.dispatch_workgroups(TAP_WIDTH / WORKGROUP_SIZE, rows.div_ceil(WORKGROUP_SIZE), 1);
        }
        encoder.copy_buffer_to_buffer(&result_buffer, 0, &staging, 0, result_bytes);
        queue.submit(Some(encoder.finish()));

        ColorReadback {
            buffer: staging,
            count,
            receiver: None,
        }
    }
}

/// One material the probe asks about.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct MaterialTap {
    /// The vertex uv set: uv0 in `xy`, uv1 in `zw`, exactly as `VertexAttr`
    /// carries it.
    pub uv: [f32; 4],
    /// Index into the material pool.
    pub material: u32,
    pub _pad: [u32; 3],
}

const _: () = assert!(std::mem::size_of::<MaterialTap>() == 32);

/// The material readout pipeline and its own io layout.
///
/// What it answers is whether the two sides of [`super::material::TracedMaterial`]
/// agree about field order. Nothing else can: the size guard measures the total
/// and the Rust offset assertions measure one side, so a transposition of two
/// sixteen-byte blocks passes both and shades wrong.
///
/// It binds the **real** scene group, so the pool is read through binding 4 with
/// the layout the kernel uses rather than through a layout the test invented.
pub struct MaterialProbe {
    pipeline: wgpu::ComputePipeline,
    io_layout: wgpu::BindGroupLayout,
}

impl MaterialProbe {
    /// Builds the pipeline. Like the other two, this is the call a browser check
    /// is watching.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Material Probe Shader"),
            source: wgpu::ShaderSource::Wgsl(MATERIAL_PROBE_KERNEL.into()),
        });
        let io_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_material_probe_io_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        // Group 1 rather than a gap: the probe's io takes the accumulation
        // group's number, which no probe binds, so the scene and the atlas keep
        // theirs and are bound through the layouts the kernel uses.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Material Probe Pipeline Layout"),
            bind_group_layouts: &[&layouts.scene, &io_layout, &layouts.sampled],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Pathtrace Material Probe Pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("material_probe"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[
                    ("MATERIAL_TAP_WIDTH", f64::from(TAP_WIDTH)),
                    ("MATERIAL_RESULT_WIDTH", MATERIAL_RESULT_WIDTH as f64),
                ],
                zero_initialize_workgroup_memory: false,
            },
            cache: None,
        });
        Self {
            pipeline,
            io_layout,
        }
    }

    /// Encodes and submits one batch, returning a readback to poll.
    ///
    /// The readback holds [`MATERIAL_RESULT_WIDTH`] entries per tap, in the
    /// kernel's write order.
    #[must_use]
    pub fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &TraceScene,
        atlas: &TraceAtlas,
        taps: &[MaterialTap],
    ) -> ColorReadback {
        let count = taps.len().max(1);
        // An empty batch is a caller mistake rather than a scene state, but a
        // zero-sized binding is invalid, so it becomes one tap of material zero.
        let placeholder = [MaterialTap::default()];
        let tap_bytes: &[u8] = if taps.is_empty() {
            bytemuck::cast_slice(&placeholder)
        } else {
            bytemuck::cast_slice(taps)
        };
        let tap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Pathtrace Material Taps"),
            contents: tap_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let entries = count * MATERIAL_RESULT_WIDTH;
        let result_bytes = (entries * 16) as u64;
        let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Material Results"),
            size: result_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Material Results Readback"),
            size: result_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let io = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Material Probe IO Bind Group"),
            layout: &self.io_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: tap_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: result_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pathtrace Material Probe Encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pathtrace Material Probe Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, scene.bind_group(), &[]);
            pass.set_bind_group(1, &io, &[]);
            pass.set_bind_group(2, atlas.bind_group(), &[]);
            let rows = (count as u32).div_ceil(TAP_WIDTH);
            pass.dispatch_workgroups(TAP_WIDTH / WORKGROUP_SIZE, rows.div_ceil(WORKGROUP_SIZE), 1);
        }
        encoder.copy_buffer_to_buffer(&result_buffer, 0, &staging, 0, result_bytes);
        queue.submit(Some(encoder.finish()));

        ColorReadback {
            buffer: staging,
            count: entries,
            receiver: None,
        }
    }
}

/// Which half of the BSDF one dispatch exercises.
///
/// Two pipelines from one module, specialized by overridable constant, the same
/// arrangement [`super::DebugChannel`] uses: the dead half folds away at pipeline
/// creation rather than costing a uniform branch per invocation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BsdfProbeMode {
    /// Draw a direction and report it with its density and throughput.
    Sample,
    /// Take a direction and report the density and throughput for it.
    Evaluate,
}

impl BsdfProbeMode {
    /// Both, so a caller can build the pair without naming them.
    pub const ALL: [Self; 2] = [Self::Sample, Self::Evaluate];

    fn index(self) -> usize {
        match self {
            Self::Sample => 0,
            Self::Evaluate => 1,
        }
    }
}

/// One question the BSDF probe asks.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct BsdfTap {
    /// Outgoing direction, **tangent space**, z up. `w` unused.
    ///
    /// The probe fixes the surface's frame, so a direction handed in this way and
    /// one handed back round-trip exactly through an orthonormal rotation. That is
    /// what lets a test compare a sampled direction against a density it computes
    /// in the same space.
    pub wo: [f32; 4],
    /// Incident direction, tangent space. Read in
    /// [`BsdfProbeMode::Evaluate`] only.
    pub wi: [f32; 4],
    /// Index into the material pool.
    pub material: u32,
    /// Which sample of [`Self::strata`] this tap draws.
    pub sample_index: u32,
    /// How many samples the batch holds, which is what the stratified sampler
    /// divides its domain into. Zero or one asks for white noise.
    pub strata: u32,
    /// Fixed across a batch, so every sample in it shares one stratified sequence
    /// and only [`Self::sample_index`] moves. Varying this per tap instead would
    /// give every sample its own scramble and stratify nothing.
    pub seed: u32,
}

const _: () = assert!(std::mem::size_of::<BsdfTap>() == 48);

/// The material-response pipelines and their own io layout.
///
/// What they answer is whether the lobes agree with themselves: whether the
/// density `bsdf_sample` returns describes the directions it actually produces,
/// and whether the throughput integrates to something a surface could reflect.
/// Neither is visible from Rust, and neither is visible in an image until it is
/// visible as noise or as a surface that is too bright.
///
/// It binds the **real** scene and sampled groups, so the record is read through
/// binding 4 with the layout the kernel uses rather than one a test invented.
pub struct BsdfProbe {
    pipelines: [wgpu::ComputePipeline; 2],
    io_layout: wgpu::BindGroupLayout,
}

impl BsdfProbe {
    /// Builds both pipelines. Like the other probes, this is the call a browser
    /// check is watching: it is where the WGSL front end either accepts the lobes
    /// or does not.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace BSDF Probe Shader"),
            source: wgpu::ShaderSource::Wgsl(BSDF_PROBE_KERNEL.into()),
        });
        let io_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_bsdf_probe_io_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        // Group 1, like the material probe: the io takes the accumulation group's
        // number, which no probe binds, so the scene and the atlas keep theirs.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace BSDF Probe Pipeline Layout"),
            bind_group_layouts: &[&layouts.scene, &io_layout, &layouts.sampled],
            push_constant_ranges: &[],
        });
        let pipelines = BsdfProbeMode::ALL.map(|mode| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Pathtrace BSDF Probe Pipeline"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("bsdf_probe"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[
                        ("BSDF_TAP_WIDTH", f64::from(TAP_WIDTH)),
                        ("BSDF_RESULT_WIDTH", BSDF_RESULT_WIDTH as f64),
                        ("BSDF_PROBE_MODE", mode.index() as f64),
                    ],
                    zero_initialize_workgroup_memory: false,
                },
                cache: None,
            })
        });
        Self {
            pipelines,
            io_layout,
        }
    }

    /// Encodes and submits one batch, returning a readback to poll.
    ///
    /// The readback holds [`BSDF_RESULT_WIDTH`] entries per tap, in the kernel's
    /// write order.
    #[must_use]
    pub fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mode: BsdfProbeMode,
        scene: &TraceScene,
        atlas: &TraceAtlas,
        taps: &[BsdfTap],
    ) -> ColorReadback {
        let count = taps.len().max(1);
        // An empty batch is a caller mistake rather than a scene state, but a
        // zero-sized binding is invalid, so it becomes one tap of material zero.
        let placeholder = [BsdfTap::default()];
        let tap_bytes: &[u8] = if taps.is_empty() {
            bytemuck::cast_slice(&placeholder)
        } else {
            bytemuck::cast_slice(taps)
        };
        let tap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Pathtrace BSDF Taps"),
            contents: tap_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let entries = count * BSDF_RESULT_WIDTH;
        let result_bytes = (entries * 16) as u64;
        let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace BSDF Results"),
            size: result_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace BSDF Results Readback"),
            size: result_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let io = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace BSDF Probe IO Bind Group"),
            layout: &self.io_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: tap_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: result_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pathtrace BSDF Probe Encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pathtrace BSDF Probe Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines[mode.index()]);
            pass.set_bind_group(0, scene.bind_group(), &[]);
            pass.set_bind_group(1, &io, &[]);
            pass.set_bind_group(2, atlas.bind_group(), &[]);
            let rows = (count as u32).div_ceil(TAP_WIDTH);
            pass.dispatch_workgroups(TAP_WIDTH / WORKGROUP_SIZE, rows.div_ceil(WORKGROUP_SIZE), 1);
        }
        encoder.copy_buffer_to_buffer(&result_buffer, 0, &staging, 0, result_bytes);
        queue.submit(Some(encoder.finish()));

        ColorReadback {
            buffer: staging,
            count: entries,
            receiver: None,
        }
    }
}

/// Which half of light sampling one dispatch exercises.
///
/// Two pipelines from one module, specialized by overridable constant, the same
/// arrangement [`BsdfProbeMode`] uses.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LightProbeMode {
    /// Draw a connection to a light and report its direction and density.
    Sample,
    /// Take a direction and report the density of the light it lands on.
    Intersect,
}

impl LightProbeMode {
    /// Both, so a caller can build the pair without naming them.
    pub const ALL: [Self; 2] = [Self::Sample, Self::Intersect];

    fn index(self) -> usize {
        match self {
            Self::Sample => 0,
            Self::Intersect => 1,
        }
    }
}

/// One question the light probe asks.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct LightTap {
    /// The shading point a connection starts from. `w` unused.
    pub origin: [f32; 4],
    /// A direction to test, in [`LightProbeMode::Intersect`] only.
    pub direction: [f32; 4],
    /// Index into the light pool.
    pub light: u32,
    /// Which sample of [`Self::strata`] this tap draws.
    pub sample_index: u32,
    /// How many samples the batch holds, which is what the stratified sampler
    /// divides its domain into. Zero or one asks for white noise.
    pub strata: u32,
    /// Fixed across a batch, so every sample in it shares one stratified
    /// sequence and only [`Self::sample_index`] moves.
    pub seed: u32,
}

const _: () = assert!(std::mem::size_of::<LightTap>() == 48);

/// The light-sampling pipelines and their own io layout.
///
/// What they answer is whether a light's density describes the directions its
/// sampler produces. Get that wrong and every image is still plausible: it
/// converges, it has no artefacts, and every surface facing that light is the
/// wrong brightness by a constant. The instrument is the same as the BSDF's, a
/// histogram against an independently written density, and the independence here
/// is real rather than nominal: the sampler's density comes from the point it
/// chose on the rectangle, and the intersection's comes from where a ray landed
/// on it.
///
/// It binds the **real** scene group, so the light array is read through binding
/// 5 with the layout the kernel uses rather than one a test invented.
pub struct LightProbe {
    pipelines: [wgpu::ComputePipeline; 2],
    io_layout: wgpu::BindGroupLayout,
}

impl LightProbe {
    /// Builds both pipelines.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Light Probe Shader"),
            source: wgpu::ShaderSource::Wgsl(LIGHT_PROBE_KERNEL.into()),
        });
        let io_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_light_probe_io_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Light Probe Pipeline Layout"),
            bind_group_layouts: &[&layouts.scene, &io_layout, &layouts.sampled],
            push_constant_ranges: &[],
        });
        let pipelines = LightProbeMode::ALL.map(|mode| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Pathtrace Light Probe Pipeline"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("light_probe"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[
                        ("LIGHT_TAP_WIDTH", f64::from(TAP_WIDTH)),
                        ("LIGHT_RESULT_WIDTH", LIGHT_RESULT_WIDTH as f64),
                        ("LIGHT_PROBE_MODE", mode.index() as f64),
                    ],
                    zero_initialize_workgroup_memory: false,
                },
                cache: None,
            })
        });
        Self {
            pipelines,
            io_layout,
        }
    }

    /// Encodes and submits one batch, returning a readback to poll.
    ///
    /// The readback holds [`LIGHT_RESULT_WIDTH`] entries per tap, in the
    /// kernel's write order.
    #[must_use]
    pub fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mode: LightProbeMode,
        scene: &TraceScene,
        atlas: &TraceAtlas,
        taps: &[LightTap],
    ) -> ColorReadback {
        let count = taps.len().max(1);
        let placeholder = [LightTap::default()];
        let tap_bytes: &[u8] = if taps.is_empty() {
            bytemuck::cast_slice(&placeholder)
        } else {
            bytemuck::cast_slice(taps)
        };
        let tap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Pathtrace Light Taps"),
            contents: tap_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let entries = count * LIGHT_RESULT_WIDTH;
        let result_bytes = (entries * 16) as u64;
        let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Light Results"),
            size: result_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Light Results Readback"),
            size: result_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let io = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Light Probe IO Bind Group"),
            layout: &self.io_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: tap_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: result_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pathtrace Light Probe Encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pathtrace Light Probe Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines[mode.index()]);
            pass.set_bind_group(0, scene.bind_group(), &[]);
            pass.set_bind_group(1, &io, &[]);
            pass.set_bind_group(2, atlas.bind_group(), &[]);
            let rows = (count as u32).div_ceil(TAP_WIDTH);
            pass.dispatch_workgroups(TAP_WIDTH / WORKGROUP_SIZE, rows.div_ceil(WORKGROUP_SIZE), 1);
        }
        encoder.copy_buffer_to_buffer(&result_buffer, 0, &staging, 0, result_bytes);
        queue.submit(Some(encoder.finish()));

        ColorReadback {
            buffer: staging,
            count: entries,
            receiver: None,
        }
    }
}
