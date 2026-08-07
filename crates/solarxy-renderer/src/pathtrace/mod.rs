//! The path tracer: the renderer's compute path.
//!
//! What is here is the foundation rather than the tracer: the compute
//! pipelines, the scene buffers the kernel binds, and a debug kernel that runs
//! camera rays through the two-level traversal and writes what it found. The
//! shading, lighting, accumulation and job orchestration arrive on top of it.
//!
//! # Shader composition
//!
//! WGSL has no include mechanism and nothing else in this crate composes
//! shaders: a pass either owns one file or shares a module with another entry
//! point. Neither works here. The traversal has to be one text shared by every
//! kernel that walks the scene *and* by the test that pins it against the CPU
//! twin, and inlining it into each would be several copies of the code least
//! able to tolerate drift.
//!
//! So the kernels are assembled from fragments with `concat!`, which costs
//! nothing, adds no dependency, and keeps one source per concern on disk. The
//! traversal fragment is public as [`TRAVERSE_SOURCE`] so the parity test
//! compiles its own entry point over the exact bytes that ship.

pub mod arena;
pub mod probe;
pub mod scene;

use bytemuck::{Pod, Zeroable};

use crate::bind_groups::PathtraceLayouts;
use arena::TraceArena;

/// The traversal fragment: scene bindings, intersection primitives, and the
/// closest-hit and any-hit walks. No entry point.
pub const TRAVERSE_SOURCE: &str = include_str!("../shaders/pathtrace/traverse.wgsl");

/// The debug kernel, composed over the traversal.
const TRACE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/trace.wgsl"),
);

/// Invocations per workgroup edge. 64 per workgroup, comfortably inside core
/// WebGPU's 256, and matched by `@workgroup_size(8, 8, 1)` in every kernel;
/// `pathtrace_shader_source.rs` is what keeps the two from drifting.
pub const WORKGROUP_SIZE: u32 = 8;

/// Which readout the debug kernel writes.
///
/// Selected by pipeline-overridable constant, so the three are three
/// specializations of one source rather than three sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugChannel {
    /// World-space shading normal, faced toward the viewer, mapped to `0..1`.
    Normal,
    /// World-space distance, unnormalized, in all three channels.
    Depth,
    /// A stable colour per instance.
    Instance,
}

impl DebugChannel {
    /// Every channel, so a caller can build all three pipelines by iterating.
    pub const ALL: [Self; 3] = [Self::Normal, Self::Depth, Self::Instance];

    fn index(self) -> usize {
        match self {
            Self::Normal => 0,
            Self::Depth => 1,
            Self::Instance => 2,
        }
    }
}

/// Per-dispatch uniforms.
///
/// Tiling is a dispatch offset rather than a scissor rect, which is what makes
/// a long render pace itself: the kernel is chunked purely by varying these
/// between dispatches, and no queue state has to stay consistent across them.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct TraceParams {
    /// Where this dispatch's tile sits in the image, in pixels.
    pub tile_offset: [u32; 2],
    /// The tile's size, which is what the dispatch is sized against.
    pub tile_size: [u32; 2],
    /// The whole image.
    pub resolution: [u32; 2],
}

const _: () = assert!(std::mem::size_of::<TraceParams>() == 24);

/// The compute pipelines and the group-2 placeholder.
pub struct PathTracer {
    debug: [wgpu::ComputePipeline; 3],
    /// The sampled group, empty until the atlas and environment exist. A
    /// pipeline layout is indexed by group number, so the gap has to be
    /// occupied by something rather than skipped.
    sampled: wgpu::BindGroup,
}

impl PathTracer {
    /// Builds every pipeline. Fails only if the device rejects the module,
    /// which is what the browser check is looking for.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Debug Shader"),
            source: wgpu::ShaderSource::Wgsl(TRACE_KERNEL.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Pipeline Layout"),
            bind_group_layouts: &[
                &layouts.scene,
                &layouts.target,
                &layouts.sampled,
                &layouts.params,
            ],
            push_constant_ranges: &[],
        });

        let debug = DebugChannel::ALL.map(|channel| {
            let value = channel.index() as f64;
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Pathtrace Debug Pipeline"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("trace_debug"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("DEBUG_CHANNEL", value)],
                    zero_initialize_workgroup_memory: false,
                },
                cache: None,
            })
        });

        let sampled = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Sampled Bind Group"),
            layout: &layouts.sampled,
            entries: &[],
        });

        Self { debug, sampled }
    }

    /// Encodes one tile of one channel.
    ///
    /// The dispatch is rounded up to whole workgroups and the kernel bounds-
    /// checks twice, because a tile at the image edge is a partial one.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        channel: DebugChannel,
        scene: &TraceScene,
        target: &TraceTarget,
        uniforms: &TraceUniforms,
        tile: [u32; 2],
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Pathtrace Debug Pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.debug[channel.index()]);
        pass.set_bind_group(0, &scene.bind_group, &[]);
        pass.set_bind_group(1, &target.bind_group, &[]);
        pass.set_bind_group(2, &self.sampled, &[]);
        pass.set_bind_group(3, &uniforms.bind_group, &[]);
        pass.dispatch_workgroups(
            tile[0].div_ceil(WORKGROUP_SIZE),
            tile[1].div_ceil(WORKGROUP_SIZE),
            1,
        );
    }
}

/// The scene arena on the GPU, plus the bind group over it.
///
/// Every buffer grows and is rewritten in place while it fits, on the policy
/// [`crate::scene_objects`] set for the raster path: a repack that fits is a
/// `queue.write_buffer`, and one that does not reallocates with headroom. The
/// bind group is rebuilt only when a buffer was actually recreated, because
/// that is the only thing that invalidates it.
pub struct TraceScene {
    nodes: GrowBuffer,
    prim_indices: GrowBuffer,
    vertex_pos: GrowBuffer,
    vertex_attr: GrowBuffer,
    instances: GrowBuffer,
    bind_group: wgpu::BindGroup,
    instance_count: u32,
}

impl TraceScene {
    /// An empty scene, ready to grow.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts) -> Self {
        let nodes = GrowBuffer::new::<solarxy_bvh::BvhNode>(device, "Pathtrace Nodes");
        let prim_indices = GrowBuffer::new::<u32>(device, "Pathtrace Primitive Indices");
        let vertex_pos = GrowBuffer::new::<[f32; 4]>(device, "Pathtrace Vertex Positions");
        let vertex_attr =
            GrowBuffer::new::<arena::VertexAttr>(device, "Pathtrace Vertex Attributes");
        let instances = GrowBuffer::new::<arena::Instance>(device, "Pathtrace Instances");
        let bind_group = Self::bind(
            device,
            layouts,
            [&nodes, &prim_indices, &vertex_pos, &vertex_attr, &instances],
        );
        Self {
            nodes,
            prim_indices,
            vertex_pos,
            vertex_attr,
            instances,
            bind_group,
            instance_count: 0,
        }
    }

    /// Brings the GPU buffers up to date with a packed arena.
    pub fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &PathtraceLayouts,
        arena: &TraceArena,
    ) {
        // Five bindings and then one disjunction, rather than a folded `|=`,
        // so that a later reader cannot "simplify" this into short-circuiting
        // and leave four buffers holding the previous scene.
        let a = self.nodes.sync(device, queue, arena.nodes());
        let b = self.prim_indices.sync(device, queue, arena.prim_indices());
        let c = self.vertex_pos.sync(device, queue, arena.vertex_pos());
        let d = self.vertex_attr.sync(device, queue, arena.vertex_attr());
        let e = self.instances.sync(device, queue, arena.instances());
        if a || b || c || d || e {
            self.bind_group = Self::bind(
                device,
                layouts,
                [
                    &self.nodes,
                    &self.prim_indices,
                    &self.vertex_pos,
                    &self.vertex_attr,
                    &self.instances,
                ],
            );
        }
        self.instance_count = u32::try_from(arena.instances().len()).unwrap_or(u32::MAX);
    }

    /// [`TraceScene::new`] followed by [`TraceScene::sync`], for a caller that
    /// uploads a scene once and never changes it.
    #[must_use]
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &PathtraceLayouts,
        arena: &TraceArena,
    ) -> Self {
        let mut scene = Self::new(device, layouts);
        scene.sync(device, queue, layouts, arena);
        scene
    }

    fn bind(
        device: &wgpu::Device,
        layouts: &PathtraceLayouts,
        buffers: [&GrowBuffer; 5],
    ) -> wgpu::BindGroup {
        // Binding 6 rather than 4: the scene group reserves 4, 5 and 7 by
        // number for the materials, the lights, and the escape hatch.
        let bindings = [0u32, 1, 2, 3, 6];
        let entries: Vec<wgpu::BindGroupEntry<'_>> = bindings
            .iter()
            .zip(buffers)
            .map(|(&binding, buffer)| wgpu::BindGroupEntry {
                binding,
                resource: buffer.buffer.as_entire_binding(),
            })
            .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Scene Bind Group"),
            layout: &layouts.scene,
            entries: &entries,
        })
    }

    /// How many placements the kernel will walk.
    #[must_use]
    pub fn instance_count(&self) -> u32 {
        self.instance_count
    }

    /// The scene group, for a pipeline that binds it at group 0.
    #[must_use]
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// A `Rgba32Float` image the kernel writes, and the bind group over it.
pub struct TraceTarget {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl TraceTarget {
    /// Allocates a target of `width` by `height`.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Pathtrace Debug Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Target Bind Group"),
            layout: &layouts.target,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        Self {
            texture,
            bind_group,
            width,
            height,
        }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Encodes a full-image copy into a mappable buffer.
    ///
    /// Submit the encoder, then drive [`FloatReadback::poll`], which arms the
    /// map on its first call and never blocks: WebGPU has no blocking wait, and
    /// a blocking wait on the desktop is a frame hitch.
    #[must_use]
    pub fn encode_readback(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> FloatReadback {
        let (buffer, padded) = crate::capture::encode_capture(
            device,
            encoder,
            &self.texture,
            (0, 0, self.width, self.height),
        );
        FloatReadback {
            buffer,
            padded_row_bytes: padded,
            width: self.width,
            height: self.height,
            receiver: None,
        }
    }
}

/// One in-flight readback of a float target.
///
/// Separate from `capture::PendingCapture`, which resolves to unpadded RGBA8
/// bytes and swizzles BGRA. Those steps are correct for a screenshot and wrong
/// for a target whose texels are four floats, so only the encode half is
/// shared.
pub struct FloatReadback {
    buffer: wgpu::Buffer,
    padded_row_bytes: u32,
    width: u32,
    height: u32,
    receiver: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
}

/// The state of a polled float readback.
pub enum ReadbackPoll {
    /// Not resolved yet; poll again next frame.
    Pending,
    /// The map failed; the readback is abandoned.
    Failed,
    /// Tightly-packed RGBA float texels, row padding stripped.
    Ready(Vec<f32>),
}

impl FloatReadback {
    /// Pumps the device without blocking and checks the map.
    ///
    /// Arms the map on the first call, which is deliberately a frame after the
    /// copy was submitted.
    pub fn poll(&mut self, device: &wgpu::Device) -> ReadbackPoll {
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
            return ReadbackPoll::Failed;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => return ReadbackPoll::Pending,
            Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                tracing::error!("pathtrace readback map failed");
                return ReadbackPoll::Failed;
            }
        }

        let data = self.buffer.slice(..).get_mapped_range();
        let row_floats = self.width as usize * 4;
        let mut out = Vec::with_capacity(row_floats * self.height as usize);
        for row in 0..self.height {
            let start = (row * self.padded_row_bytes) as usize;
            let end = start + row_floats * 4;
            out.extend_from_slice(bytemuck::cast_slice::<u8, f32>(&data[start..end]));
        }
        drop(data);
        self.buffer.unmap();
        ReadbackPoll::Ready(out)
    }
}

/// The camera and per-dispatch uniforms, and the bind group over them.
pub struct TraceUniforms {
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl TraceUniforms {
    /// Binds an existing camera uniform buffer beside a fresh params buffer.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts, camera: &wgpu::Buffer) -> Self {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Params"),
            size: std::mem::size_of::<TraceParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Params Bind Group"),
            layout: &layouts.params,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        Self { params, bind_group }
    }

    /// Writes the tile this dispatch covers.
    pub fn write(&self, queue: &wgpu::Queue, params: &TraceParams) {
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(params));
    }
}

/// One growable read-only storage buffer, never shorter than one element.
///
/// An empty scene is a state the renderer reaches in the ordinary course of
/// editing rather than a bug, and a zero-sized binding is invalid. The floor
/// has to be one whole element rather than a fixed number of bytes: wgpu
/// checks a runtime-sized array's binding against the element stride, so an
/// `array<Instance>` backed by sixteen bytes is rejected at dispatch with a
/// message about a size the shader expects. That is a different rule from the
/// four-byte copy alignment `scene_objects::create_with_capacity` floors at,
/// which is why the two are not one function despite looking alike.
///
/// A zeroed element is unreachable in every buffer. The node array is never
/// genuinely empty, because the builder always emits a root and an empty root
/// is a leaf holding no primitives, so nothing descends into the padding.
///
/// There is no capacity field beside the buffer: `wgpu::Buffer::size()` is the
/// authority and cannot drift from the allocation it describes.
struct GrowBuffer {
    buffer: wgpu::Buffer,
    label: &'static str,
    /// One element's bytes, which is both the empty-scene floor and what a
    /// capacity must never round below.
    stride: u64,
}

impl GrowBuffer {
    fn new<T: Pod>(device: &wgpu::Device, label: &'static str) -> Self {
        let stride = std::mem::size_of::<T>() as u64;
        Self {
            buffer: Self::allocate(device, label, stride),
            label,
            stride,
        }
    }

    /// Writes `data`, reallocating with headroom when it does not fit.
    /// Returns whether the buffer was recreated, which is exactly when the
    /// bind group over it has gone stale.
    fn sync<T: Pod>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[T]) -> bool {
        debug_assert_eq!(std::mem::size_of::<T>() as u64, self.stride);
        let empty = [T::zeroed()];
        let bytes: &[u8] = if data.is_empty() {
            bytemuck::cast_slice(&empty)
        } else {
            bytemuck::cast_slice(data)
        };
        let needed = bytes.len() as u64;
        if needed <= self.buffer.size() {
            queue.write_buffer(&self.buffer, 0, bytes);
            return false;
        }
        self.buffer = Self::allocate(
            device,
            self.label,
            crate::scene_objects::with_headroom(needed),
        );
        queue.write_buffer(&self.buffer, 0, bytes);
        true
    }

    fn allocate(device: &wgpu::Device, label: &'static str, bytes: u64) -> wgpu::Buffer {
        // The round-up is not decoration. `prim_indices` has a four-byte
        // stride, and 1.5x of twelve bytes is eighteen, which is not a legal
        // buffer size.
        let size = bytes
            .max(1)
            .div_ceil(wgpu::COPY_BUFFER_ALIGNMENT)
            .saturating_mul(wgpu::COPY_BUFFER_ALIGNMENT);
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
}
