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

/// Rays per row of the dispatch grid. The kernel's workgroup shape is shared
/// with the real one, so a linear corpus is walked as a 2D grid.
const CORPUS_WIDTH: u32 = 64;

/// Taps per row of the atlas probe's dispatch grid, for the same reason.
const TAP_WIDTH: u32 = 64;

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
