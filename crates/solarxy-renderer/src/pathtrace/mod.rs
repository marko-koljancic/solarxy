//! The path tracer: the renderer's compute path.
//!
//! What is here is the foundation rather than the tracer: the compute
//! pipelines, the scene buffers the kernel binds, the texture atlas it samples,
//! and a debug kernel that runs camera rays through the two-level traversal and
//! writes what it found. The shading, lighting, accumulation and job
//! orchestration arrive on top of it.
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
pub mod atlas;
pub mod backend;
pub mod denoise;
pub mod depth;
pub mod environment;
pub mod light;
pub mod material;
pub mod probe;
pub mod resolve;
pub mod scene;

use bytemuck::{Pod, Zeroable};

use crate::bind_groups::PathtraceLayouts;
use arena::TraceArena;

/// The traversal fragment: scene bindings, intersection primitives, and the
/// closest-hit and any-hit walks. No entry point.
pub const TRAVERSE_SOURCE: &str = include_str!("../shaders/pathtrace/traverse.wgsl");

/// The atlas fragment: the sampled group's bindings and `sample_atlas`. No
/// entry point, like the traversal.
pub const ATLAS_SOURCE: &str = include_str!("../shaders/pathtrace/atlas.wgsl");

/// The material fragment: the record's texture taps resolved against its
/// factors. No entry point, and composed after both the traversal, which
/// declares the record, and the atlas, which it samples through.
pub const MATERIAL_SOURCE: &str = include_str!("../shaders/pathtrace/material.wgsl");

/// The sampler fragment: the generator, stratification, and `sample_sphere`. No
/// entry point and no bindings, so it is a base like the traversal and the
/// atlas.
pub const RAND_SOURCE: &str = include_str!("../shaders/pathtrace/rand.wgsl");

/// The camera fragment: the per-dispatch uniforms and the ray through a pixel.
/// No entry point, and composed after the traversal, whose `Ray` it returns,
/// and after the sampler, whose constants its aperture reads.
///
/// Extracted from the debug kernel when a second kernel needed a camera ray.
/// Composing the two kernels together would have worked and would have dragged a
/// second entry point into every kernel that wanted one.
pub const CAMERA_SOURCE: &str = include_str!("../shaders/pathtrace/camera.wgsl");

/// The debug kernel, composed over the traversal, the atlas, the material and
/// the camera.
///
/// It shades nothing. The atlas and the material ride along so that the
/// fragments the shading stage will call are compiled by both WGSL front ends
/// from the day they are written, rather than first meeting the browser's
/// several stages later. The sampler rides along because the camera fragment
/// reads its constants: this kernel draws a pixel centre and no random number
/// at all, and still cannot be built without it.
const TRACE_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/atlas.wgsl"),
    include_str!("../shaders/pathtrace/material.wgsl"),
    include_str!("../shaders/pathtrace/rand.wgsl"),
    include_str!("../shaders/pathtrace/camera.wgsl"),
    include_str!("../shaders/pathtrace/trace.wgsl"),
);

/// The path kernel: camera rays, the material response, next-event estimation
/// with multiple importance sampling, and an environment. Composed over
/// everything.
///
/// It grew from the furnace kernel rather than replacing it, which is why the
/// white furnace test still drives it: both environment colours the same and no
/// lights in the scene is the furnace configuration, and the stage-four albedo
/// table reproducing through this loop is the evidence that adding a second
/// density did not disturb the first.
///
/// Each dispatch draws a slice of one run's samples and folds them into the
/// ping-ponged running mean, so a long render is paced by chunking. The
/// auxiliary packing composes first, because the denoiser reads the guides this
/// writes and the two halves have to be one text.
const PATH_KERNEL: &str = concat!(
    include_str!("../shaders/pathtrace/aov.wgsl"),
    include_str!("../shaders/pathtrace/traverse.wgsl"),
    include_str!("../shaders/pathtrace/atlas.wgsl"),
    include_str!("../shaders/pathtrace/material.wgsl"),
    include_str!("../shaders/pathtrace/rand.wgsl"),
    include_str!("../shaders/pathtrace/bsdf.wgsl"),
    include_str!("../shaders/pathtrace/environment.wgsl"),
    include_str!("../shaders/pathtrace/light.wgsl"),
    include_str!("../shaders/pathtrace/camera.wgsl"),
    include_str!("../shaders/pathtrace/path.wgsl"),
);

/// Steps per octahedral component in the auxiliary target's packed normal.
/// Mirrors `OCT_STEPS` in `aov.wgsl`.
const OCT_STEPS: f32 = 4096.0;

/// Decodes a world normal from the auxiliary target's alpha lane.
///
/// The other half of `pack_octahedral` in `aov.wgsl`, which packs two
/// twelve-bit octahedral components arithmetically into one float rather than
/// reinterpreting bits: an integer below 2^24 is exact in an `f32` on every
/// platform, where an arbitrary bit pattern read as a float may be a denormal
/// the hardware is free to flush.
///
/// Returns a unit vector. A lane written by a pixel that found no surface
/// decodes to `+Z`, which is what the kernel writes there.
#[must_use]
pub fn unpack_aov_normal(packed: f32) -> [f32; 3] {
    let combined = packed.max(0.0);
    let qx = (combined / OCT_STEPS).floor();
    let qy = combined - qx * OCT_STEPS;
    let to_signed = |q: f32| (q / (OCT_STEPS - 1.0)) * 2.0 - 1.0;
    let (ex, ey) = (to_signed(qx), to_signed(qy));

    let z = 1.0 - ex.abs() - ey.abs();
    let (mut x, mut y) = (ex, ey);
    if z < 0.0 {
        // The inverse of the fold: the lower hemisphere was mapped out across
        // the octahedron's edges, so it comes back the same way.
        let sx = if ex >= 0.0 { 1.0 } else { -1.0 };
        let sy = if ey >= 0.0 { 1.0 } else { -1.0 };
        x = (1.0 - ey.abs()) * sx;
        y = (1.0 - ex.abs()) * sy;
    }
    let len = (x * x + y * y + z * z).sqrt();
    if len <= 0.0 {
        return [0.0, 0.0, 1.0];
    }
    [x / len, y / len, z / len]
}

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
    /// How many scattering events a path may have, counted for every scatter.
    pub bounces: u32,
    /// How many of those may additionally be transmissive.
    ///
    /// A separate budget rather than the reference's trick of handing a bounce
    /// back on a transmissive hit, which keeps glass from eating a whole path at
    /// the cost of making the bounce count mean two things.
    pub transmissive_bounces: u32,
    /// Samples per pixel the **run** converges to, and the count the stratified
    /// sampler divides its domain into. Zero or one turns stratification off.
    ///
    /// The whole run rather than one dispatch, which is what lets a chunked
    /// render stratify globally: every dispatch draws a disjoint slice of one
    /// domain rather than re-drawing its own. See [`TraceParams::chunk`].
    pub samples: u32,
    /// Decorrelates one dispatch from the next. A fixed value is what makes a
    /// render reproducible.
    pub seed: u32,
    /// How many entries of the light array next-event estimation may pick
    /// from.
    ///
    /// Here rather than read from `arrayLength(&lights)`, which WGSL does
    /// offer, because the buffer never shrinks below one whole record and an
    /// empty scene's padding record would otherwise be a light. It is also
    /// what makes the tracer's light ceiling nothing at all: the raster path's
    /// eight is the size of a uniform, and this is a count.
    pub light_count: u32,
    /// The aperture's radius in world units, already resolved out of the
    /// camera's f-number. Zero is a pinhole and costs nothing: the sampling is
    /// branched away rather than multiplied by zero.
    pub aperture_radius: f32,
    /// How far in front of the camera is sharp, in world units. Resolved, so
    /// the "focus on the target" default has already become a distance.
    pub focus_distance: f32,
    /// Aperture blades. Zero, one and two are a circular opening; three or
    /// more is a polygon.
    pub aperture_blades: u32,
    /// How many of [`TraceParams::samples`] this dispatch draws, starting at
    /// [`TraceParams::sample_base`].
    ///
    /// **Zero means all of them**, which is what a one-shot dispatch wants and
    /// is what every caller written before the accumulator existed gets from
    /// [`Default`]. A chunked render sets it to the slice it can afford between
    /// two frames.
    pub chunk: u32,
    /// How many samples the accumulator's read slot already averages, and the
    /// global index of this dispatch's first sample.
    ///
    /// **Zero means the read slot holds nothing**, so the kernel does not read
    /// it. That is what makes the first dispatch of a run correct without a
    /// zero-fill pass, and what makes a one-shot dispatch byte-identical to
    /// what it produced before there was an accumulator.
    pub sample_base: u32,
    /// The luminance one sample's indirect contribution may reach before it is
    /// scaled back to it. Zero or less turns the clamp off.
    ///
    /// Bounds the single enormous sample a 32-bit accumulator tolerates and a
    /// viewer does not: one path in ten thousand finding a small bright source
    /// through a near-specular scatter leaves a pixel that never averages away.
    /// It bites only what the path found **after** its first scatter, so a
    /// directly-viewed emitter or environment keeps its authored brightness;
    /// see `path.wgsl`.
    pub firefly_clamp: f32,
    /// Pads the struct to its own alignment. See below. Never read; build one
    /// of these through [`Default`] rather than naming this.
    pub _pad: u32,
}

// Seventy-two bytes with one padding word. Every member aligns to eight or
// four, so the struct aligns to eight, and the sixty-eight bytes the members
// occupy round up to the next multiple of it. WGSL does that rounding whether
// or not the pad is written, so writing it is what keeps the Rust size equal to
// the shader's; `tests/uniform_layout.rs` is what checks that it does.
//
// A `vec3f` or `vec4f` appended here would raise the alignment to sixteen and
// need more, which is why the environment is its own uniform instead.
const _: () = assert!(std::mem::size_of::<TraceParams>() == 72);

/// The compute pipelines.
pub struct PathTracer {
    debug: [wgpu::ComputePipeline; 3],
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

        Self { debug }
    }

    /// Encodes one tile of one channel.
    ///
    /// The dispatch is rounded up to whole workgroups and the kernel bounds-
    /// checks twice, because a tile at the image edge is a partial one.
    ///
    /// The atlas is a parameter rather than a field for the same reason the
    /// scene is: it belongs to what is being rendered, not to the pipelines
    /// that render it, and an untextured scene binds the null atlas rather than
    /// skipping the group.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        channel: DebugChannel,
        scene: &TraceScene,
        atlas: &TraceAtlas,
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
        pass.set_bind_group(1, target.bind_group(), &[]);
        pass.set_bind_group(2, &atlas.bind_group, &[]);
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
    materials: GrowBuffer,
    lights: GrowBuffer,
    bind_group: wgpu::BindGroup,
    instance_count: u32,
    light_count: u32,
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
        let materials = GrowBuffer::new::<material::TracedMaterial>(device, "Pathtrace Materials");
        let lights = GrowBuffer::new::<light::TracedLight>(device, "Pathtrace Lights");
        let bind_group = Self::bind(
            device,
            layouts,
            [
                &nodes,
                &prim_indices,
                &vertex_pos,
                &vertex_attr,
                &materials,
                &lights,
                &instances,
            ],
        );
        Self {
            nodes,
            prim_indices,
            vertex_pos,
            vertex_attr,
            instances,
            materials,
            lights,
            bind_group,
            instance_count: 0,
            light_count: 0,
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
        // Seven bindings and then one disjunction, rather than a folded `|=`,
        // so that a later reader cannot "simplify" this into short-circuiting
        // and leave six buffers holding the previous scene.
        let a = self.nodes.sync(device, queue, arena.nodes());
        let b = self.prim_indices.sync(device, queue, arena.prim_indices());
        let c = self.vertex_pos.sync(device, queue, arena.vertex_pos());
        let d = self.vertex_attr.sync(device, queue, arena.vertex_attr());
        let e = self.instances.sync(device, queue, arena.instances());
        let f = self.materials.sync(device, queue, arena.materials());
        let g = self.lights.sync(device, queue, arena.lights());
        if a || b || c || d || e || f || g {
            self.bind_group = Self::bind(
                device,
                layouts,
                [
                    &self.nodes,
                    &self.prim_indices,
                    &self.vertex_pos,
                    &self.vertex_attr,
                    &self.materials,
                    &self.lights,
                    &self.instances,
                ],
            );
        }
        self.instance_count = u32::try_from(arena.instances().len()).unwrap_or(u32::MAX);
        // The count, not the buffer's length: an empty pool still uploads one
        // zeroed record, because a zero-sized binding is invalid, and that
        // record is a black light at the origin rather than an absent one.
        self.light_count = u32::try_from(arena.lights().len()).unwrap_or(u32::MAX);
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
        buffers: [&GrowBuffer; 7],
    ) -> wgpu::BindGroup {
        // Not contiguous, and not in buffer order: the instances are binding 6
        // because 4 is the material pool and 5 the lights, which is why the
        // array runs materials, lights, instances at the end. Binding 7 is the
        // escape hatch a ninth logical array would otherwise force and stays
        // unspent.
        let bindings = [0u32, 1, 2, 3, 4, 5, 6];
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

    /// How many lights next-event estimation may pick from.
    ///
    /// The number the caller writes into [`TraceParams::light_count`], and the
    /// only thing that keeps the estimator off the padding record an empty
    /// pool leaves in the buffer.
    #[must_use]
    pub fn light_count(&self) -> u32 {
        self.light_count
    }

    /// The scene group, for a pipeline that binds it at group 0.
    #[must_use]
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// The atlas texture on the GPU, and the **whole sampled group** over it.
///
/// The arrangement is decided by [`atlas::AtlasPlan`], which is wgpu-free; what
/// is here is the allocation, the upload, and the two samplers the descriptor's
/// filter bit chooses between.
///
/// It owns the environment's four bindings as well, and that is not a
/// conflation: group 2 is one bind group, a bind group has to belong to
/// something, and the alternative of a third type owning the group with both
/// halves feeding it costs every call site a parameter to buy a name. A scene
/// with no HDRI holds the null environment, which is a real set of textures for
/// the same reason the null atlas is.
///
/// A resync rewrites every entry rather than diffing. Textures change when a
/// material changes, which is rare beside the geometry churn the arena absorbs,
/// and a partial upload would have to reason about a rectangle that moved
/// between two arrangements. When that becomes the bottleneck, the honest fix
/// is a stable-placement packer, not a diff over an unstable one.
pub struct TraceAtlas {
    #[allow(unused)]
    texture: wgpu::Texture,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    environment: environment::TraceEnvironment,
    bind_group: wgpu::BindGroup,
    page: u32,
    layers: u32,
}

impl TraceAtlas {
    /// The null atlas: one transparent texel in one layer.
    ///
    /// Not a nicety. A pipeline layout is satisfied by a bind group or by
    /// nothing, and most scenes carry no textures at all, so the empty case has
    /// to be a real texture. Nothing samples it, because every descriptor over
    /// an empty plan carries [`atlas::TEXTURE_UNUSED`].
    #[must_use]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layouts: &PathtraceLayouts) -> Self {
        // Nearest is declared non-filtering and linear filtering, matching the
        // layout: the platform derives a sampler's filtering from its own
        // filters, so the two cannot be created from one descriptor.
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Pathtrace Atlas Nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Pathtrace Atlas Linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let environment = environment::TraceEnvironment::null(device, queue);
        let (texture, bind_group) =
            Self::allocate(device, layouts, &nearest, &linear, &environment, 1, 1);
        Self {
            texture,
            nearest,
            linear,
            environment,
            bind_group,
            page: 1,
            layers: 1,
        }
    }

    /// Installs a prepared environment and rebuilds the group over it.
    ///
    /// Separate from the atlas sync because the two change on different
    /// occasions: an environment arrives when an HDRI is dropped or an
    /// environment node cooks, and the atlas is repacked when a material's
    /// textures move. Rebuilding the group is the whole cost either way, since
    /// a bind group cannot be edited in place.
    pub fn set_environment(
        &mut self,
        device: &wgpu::Device,
        layouts: &PathtraceLayouts,
        environment: environment::TraceEnvironment,
    ) {
        self.environment = environment;
        self.bind_group = Self::rebind(
            device,
            layouts,
            &self.texture,
            &self.nearest,
            &self.linear,
            &self.environment,
        );
    }

    /// What the kernel needs to know about the environment: its distribution's
    /// size, zero when there is nothing to sample, and the density's
    /// denominator.
    #[must_use]
    pub fn environment(&self) -> &environment::TraceEnvironment {
        &self.environment
    }

    /// Brings the atlas up to date with a plan and the images it arranged.
    ///
    /// `textures` may hold entries the plan dropped, and the plan may name
    /// entries `textures` no longer carries; both are skipped, because a plan
    /// and a texture list that disagree describe a scene mid-edit rather than a
    /// bug.
    pub fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &PathtraceLayouts,
        plan: &atlas::AtlasPlan,
        textures: &[atlas::AtlasTexture],
    ) {
        if plan.page() != self.page || plan.layers() != self.layers {
            let (texture, bind_group) = Self::allocate(
                device,
                layouts,
                &self.nearest,
                &self.linear,
                &self.environment,
                plan.page(),
                plan.layers(),
            );
            self.texture = texture;
            self.bind_group = bind_group;
            self.page = plan.page();
            self.layers = plan.layers();
        }

        let by_key: std::collections::HashMap<_, _> =
            textures.iter().map(|t| (t.key, &t.image)).collect();
        for entry in plan.entries() {
            let Some(image) = by_key.get(&entry.key) else {
                continue;
            };
            let block = atlas::compose(entry, image);
            let width = entry.width + 2 * atlas::GUARD;
            let height = entry.height + 2 * atlas::GUARD;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: entry.x - atlas::GUARD,
                        y: entry.y - atlas::GUARD,
                        z: entry.layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &block,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * atlas::BYTES_PER_TEXEL),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn allocate(
        device: &wgpu::Device,
        layouts: &PathtraceLayouts,
        nearest: &wgpu::Sampler,
        linear: &wgpu::Sampler,
        environment: &environment::TraceEnvironment,
        page: u32,
        layers: u32,
    ) -> (wgpu::Texture, wgpu::BindGroup) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Pathtrace Atlas"),
            size: wgpu::Extent3d {
                width: page,
                height: page,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Unorm rather than sRGB throughout, so one page holds a base
            // colour map beside a normal map; the transfer function is a
            // per-texture flag the kernel applies.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bind_group = Self::rebind(device, layouts, &texture, nearest, linear, environment);
        (texture, bind_group)
    }

    /// The sampled group: the atlas at 0 to 2 and the environment at 3 to 6.
    fn rebind(
        device: &wgpu::Device,
        layouts: &PathtraceLayouts,
        texture: &wgpu::Texture,
        nearest: &wgpu::Sampler,
        linear: &wgpu::Sampler,
        environment: &environment::TraceEnvironment,
    ) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(nearest),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(linear),
            },
        ];
        entries.extend(environment.entries());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Sampled Bind Group"),
            layout: &layouts.sampled,
            entries: &entries,
        })
    }

    /// The page edge in texels.
    #[must_use]
    pub fn page(&self) -> u32 {
        self.page
    }

    /// Allocated array layers.
    #[must_use]
    pub fn layers(&self) -> u32 {
        self.layers
    }

    /// The sampled group, for a pipeline that binds it at group 2.
    #[must_use]
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// The accumulator: two `Rgba32Float` colour images, two auxiliary images, and
/// a bind group for each direction of the ping-pong.
///
/// # Why two of each
///
/// A running mean has to read what it is averaging against, and a
/// `Rgba32Float` storage texture cannot be read and written by the same
/// dispatch: WebGPU grants read-write storage access to `r32uint`, `r32sint`
/// and `r32float` only. So each channel is a pair, one written while the other
/// is read, and [`TraceTarget::swap`] exchanges their roles between dispatches.
/// That spends all four of core WebGPU's storage textures, which is why there
/// is no third auxiliary channel and no depth lane.
///
/// # Which one holds the answer
///
/// [`TraceTarget::swap`] runs **between** dispatches rather than after one, so
/// the slot a dispatch wrote is still the write slot when it finishes. Every
/// reader here -- the two readbacks and the resolve -- therefore reads the
/// write slot, and a caller that never accumulates (the debug readout, every
/// probe) never swaps and sees a single fixed target.
///
/// Nothing clears these. A dispatch with no samples behind it ignores the read
/// side entirely, which is what makes the first dispatch of a run correct
/// without a zero-fill pass.
pub struct TraceTarget {
    /// Colour, indexed by slot.
    color: [wgpu::Texture; 2],
    /// Albedo in `rgb` with the octahedral world normal in `a`, indexed by
    /// the same slot as the colour it describes.
    auxiliary: [wgpu::Texture; 2],
    /// `groups[i]` writes slot `i` and reads slot `i ^ 1`.
    groups: [wgpu::BindGroup; 2],
    /// Held so the resolve and the denoiser can bind the colour without
    /// creating a view per frame.
    color_views: [wgpu::TextureView; 2],
    /// The same for the guides the denoiser reads.
    aux_views: [wgpu::TextureView; 2],
    /// The slot the next dispatch writes, and the slot every reader reads.
    write: usize,
    width: u32,
    height: u32,
}

impl TraceTarget {
    /// Allocates a target of `width` by `height`.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts, width: u32, height: u32) -> Self {
        let storage = |label: &str| {
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
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        // The auxiliary target: what the surface looked like, as opposed to
        // what it returned. One texture per slot rather than two, because the
        // albedo takes three lanes and the normal is folded into the fourth,
        // octahedrally, which costs a little precision on a vector that only
        // has to steer an edge-stopping filter.
        let color = [storage("Pathtrace Colour A"), storage("Pathtrace Colour B")];
        let auxiliary = [
            storage("Pathtrace Auxiliary A"),
            storage("Pathtrace Auxiliary B"),
        ];
        let views = |t: &[wgpu::Texture; 2]| {
            [
                t[0].create_view(&wgpu::TextureViewDescriptor::default()),
                t[1].create_view(&wgpu::TextureViewDescriptor::default()),
            ]
        };
        let color_views = views(&color);
        let aux_views = views(&auxiliary);
        let group = |slot: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Pathtrace Target Bind Group"),
                layout: &layouts.target,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&color_views[slot]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&aux_views[slot]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&color_views[slot ^ 1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&aux_views[slot ^ 1]),
                    },
                ],
            })
        };
        let groups = [group(0), group(1)];
        Self {
            color,
            auxiliary,
            groups,
            color_views,
            aux_views,
            write: 0,
            width,
            height,
        }
    }

    /// Exchanges the read and write slots.
    ///
    /// Call **before** a dispatch that has history to average against, never
    /// after one. Every reader here reads the write slot, so a swap that
    /// follows the last dispatch of a run hands back the image from the
    /// dispatch before it, or from no dispatch at all if there was only one.
    /// The first dispatch of a run does not swap: it writes one slot and, with
    /// a sample base of zero, never reads the other.
    pub fn swap(&mut self) {
        self.write ^= 1;
    }

    /// The colour a reader outside this module binds: the slot the last
    /// dispatch wrote.
    #[must_use]
    pub fn color_texture(&self) -> &wgpu::Texture {
        &self.color[self.write]
    }

    /// The auxiliary channels beside [`TraceTarget::color_texture`].
    #[must_use]
    pub fn auxiliary_texture(&self) -> &wgpu::Texture {
        &self.auxiliary[self.write]
    }

    /// The accumulation group, for a pipeline that binds it at group 1.
    #[must_use]
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.groups[self.write]
    }

    /// A view of [`TraceTarget::color_texture`], for a pass that samples the
    /// running mean rather than copying it.
    #[must_use]
    pub fn color_view(&self) -> &wgpu::TextureView {
        &self.color_views[self.write]
    }

    /// A view of [`TraceTarget::auxiliary_texture`], for the denoiser that
    /// steers by it.
    #[must_use]
    pub fn auxiliary_view(&self) -> &wgpu::TextureView {
        &self.aux_views[self.write]
    }

    /// Encodes a full-image copy of the auxiliary target, the same way
    /// [`TraceTarget::encode_readback`] does for the colour.
    ///
    /// Albedo in `rgb`; the world normal is octahedrally packed into `a` and
    /// comes back through [`unpack_aov_normal`].
    #[must_use]
    pub fn encode_auxiliary_readback(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> FloatReadback {
        let (buffer, padded) = crate::capture::encode_capture(
            device,
            encoder,
            self.auxiliary_texture(),
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
            self.color_texture(),
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

    /// The camera-and-parameters group, for a kernel encoded outside this
    /// module.
    #[must_use]
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// [`EnvParams::sampling`]: draw in proportion to how bright the environment is.
pub const ENV_SAMPLING_IMPORTANCE: u32 = 0;
/// [`EnvParams::sampling`]: draw uniformly over the sphere. The baseline the
/// importance-sampled variance is measured against, and nothing else.
pub const ENV_SAMPLING_UNIFORM: u32 = 1;

/// The environment the kernel integrates against.
///
/// Two colours blended by the world up axis, which is a stand-in for the sampled
/// environment that arrives with importance sampling and its two lookup
/// textures. Equal colours are the white furnace configuration; different ones
/// make curved geometry legible, which a genuinely uniform environment does not,
/// since a conserving surface under one is invisible.
///
/// Its own uniform rather than four more floats on [`TraceParams`], because this
/// is the part that gets replaced wholesale and a field documented offset by
/// offset in the shipped per-dispatch struct would have to be deleted rather than
/// superseded.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct EnvParams {
    /// Radiance looking up, in `rgb`. `w` unused. Read only when there is no
    /// image.
    pub env_up: [f32; 4],
    /// Radiance looking down.
    pub env_down: [f32; 4],
    /// The distribution's dimensions. **Zero means there is no image**, and it
    /// is the one flag the kernel's environment branches on.
    pub size: [u32; 2],
    /// The sum of every weight, which is the density's denominator.
    pub total_weight: f32,
    /// A plain multiplier on the environment's contribution, as authored.
    pub intensity: f32,
    /// Yaw about the world up axis as `[cos, sin]`.
    ///
    /// Precomputed rather than taken as an angle, because the kernel needs both
    /// at every lookup and neither changes within a dispatch. It is also what
    /// keeps the traced environment and the raster skybox in step: both apply
    /// the same two numbers the same way round, and a rotation applied one way
    /// here and the other there is a scene lit from the opposite side of itself.
    pub rotation: [f32; 2],
    /// Which strategy `env_sample` uses: 0 draws in proportion to brightness,
    /// 1 draws uniformly over the sphere.
    ///
    /// It exists to be **measured**, not configured, and nothing offers it to a
    /// user. Both are unbiased and converge to the same image, so the only thing
    /// that separates them is variance, and a claim that importance sampling
    /// converges faster is worth exactly as much as the measurement behind it.
    /// Keeping the baseline in the shipped kernel is what makes that
    /// measurement repeatable rather than a number recorded once.
    ///
    /// A uniform branch rather than a pipeline-overridable constant, unlike the
    /// estimator selector: this is one `if` inside one function, where that one
    /// restructures the whole bounce loop.
    pub sampling: u32,
    pub _pad: u32,
}

const _: () = assert!(std::mem::size_of::<EnvParams>() == 64);

impl Default for EnvParams {
    /// A black constant environment: no image, and nothing to sample.
    ///
    /// Hand-written rather than derived because `intensity` has to default to
    /// one. A zeroed intensity would make every environment silently absent,
    /// including one whose image and tables were uploaded correctly, and the
    /// symptom would be an unlit scene rather than an error.
    fn default() -> Self {
        Self {
            env_up: [0.0; 4],
            env_down: [0.0; 4],
            size: [0, 0],
            total_weight: 0.0,
            intensity: 1.0,
            rotation: [1.0, 0.0],
            sampling: ENV_SAMPLING_IMPORTANCE,
            _pad: 0,
        }
    }
}

impl EnvParams {
    /// The constant environment: two colours blended by the world up axis.
    ///
    /// Equal colours are the white furnace configuration, where a surface lit by
    /// a uniform environment must return exactly what reached it. Different ones
    /// make curved geometry legible, which a genuinely uniform environment does
    /// not, since a conserving surface under one is invisible.
    #[must_use]
    pub fn constant(up: [f32; 3], down: [f32; 3]) -> Self {
        Self {
            env_up: [up[0], up[1], up[2], 0.0],
            env_down: [down[0], down[1], down[2], 0.0],
            ..Self::default()
        }
    }

    /// The uploaded environment: its distribution's size and denominator, plus
    /// the rotation and intensity the environment node authored.
    #[must_use]
    pub fn image(
        environment: &environment::TraceEnvironment,
        rotation: f32,
        intensity: f32,
    ) -> Self {
        Self {
            size: environment.size(),
            total_weight: environment.total_weight(),
            intensity,
            rotation: [rotation.cos(), rotation.sin()],
            ..Self::default()
        }
    }
}

/// The path kernel's group-3 uniforms: camera, per-dispatch, environment.
///
/// A layout of its own rather than an entry added to [`PathtraceLayouts::params`],
/// because the debug kernel binds two uniforms and must not be made to bind three
/// for an environment it does not read. Built here the way each probe builds its
/// own io layout, and for the same reason.
pub struct PathUniforms {
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    environment: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl PathUniforms {
    /// Binds an existing camera uniform buffer beside fresh params and environment
    /// buffers.
    #[must_use]
    pub fn new(device: &wgpu::Device, camera: &wgpu::Buffer) -> Self {
        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_path_params_bind_group_layout"),
            entries: &[uniform(0), uniform(1), uniform(2)],
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Path Params"),
            size: std::mem::size_of::<TraceParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let environment = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pathtrace Environment"),
            size: std::mem::size_of::<EnvParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Pathtrace Path Params Bind Group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: environment.as_entire_binding(),
                },
            ],
        });
        Self {
            layout,
            params,
            environment,
            bind_group,
        }
    }

    /// The layout, which the pipeline needs before the bind group exists.
    #[must_use]
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Writes the tile this dispatch covers and the environment it integrates
    /// against.
    pub fn write(&self, queue: &wgpu::Queue, params: &TraceParams, environment: &EnvParams) {
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(params));
        queue.write_buffer(&self.environment, 0, bytemuck::bytes_of(environment));
    }
}

/// The path-tracing pipeline: the one that renders an image.
///
/// Separate from [`PathTracer`] rather than a fourth channel on it, because it
/// binds a different group-3 layout and answers a different question: that one
/// is a readout of what the traversal returned, and this one is light transport.
/// Nothing in either shell reaches it yet, which is what has kept the compute
/// path unable to move a golden capture.
pub struct PathKernel {
    pipelines: [wgpu::ComputePipeline; 3],
}

/// Which estimator a dispatch runs.
///
/// This exists to be **tested**, not to be configured, and nothing ships a way
/// to choose it. Multiple importance sampling is a partition of unity across
/// the two techniques, so all three converge to the same image and differ only
/// in variance. That equality is the sharpest available check that the
/// material's one-sample mixture density is the right quantity to weight a
/// light's density against, and a disagreement between these three is a
/// disagreement between the two densities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathEstimator {
    /// Both techniques, weighted by the power heuristic. What a render uses.
    #[default]
    Mis,
    /// Scattering alone. Correct, and hopeless at finding a small light.
    Scatter,
    /// Connections alone. Correct, and blind to anything a connection cannot
    /// reach.
    NextEvent,
}

impl PathEstimator {
    /// Every mode, so a caller can build or compare all three by iterating.
    pub const ALL: [Self; 3] = [Self::Mis, Self::Scatter, Self::NextEvent];

    fn index(self) -> usize {
        match self {
            Self::Mis => 0,
            Self::Scatter => 1,
            Self::NextEvent => 2,
        }
    }
}

impl PathKernel {
    /// Builds every pipeline. Like the probes, this is the call a browser check
    /// is watching: it is where a WGSL front end either accepts the whole
    /// composition or does not.
    #[must_use]
    pub fn new(device: &wgpu::Device, layouts: &PathtraceLayouts, uniforms: &PathUniforms) -> Self {
        Self::build(device, layouts, uniforms, 0.0)
    }

    /// The kernel with the lobe-weight split skewed, for the unbiasedness guard.
    ///
    /// The weights are a sampling density and appear only in the selection step
    /// and the mixture density, so a scene rendered at two different splits has
    /// to converge to the same image. Nothing ships a way to set this; it exists
    /// to be tested, exactly as the estimator override does.
    #[must_use]
    pub fn with_lobe_bias(
        device: &wgpu::Device,
        layouts: &PathtraceLayouts,
        uniforms: &PathUniforms,
        bias: f32,
    ) -> Self {
        Self::build(device, layouts, uniforms, f64::from(bias))
    }

    fn build(
        device: &wgpu::Device,
        layouts: &PathtraceLayouts,
        uniforms: &PathUniforms,
        bias: f64,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pathtrace Path Shader"),
            source: wgpu::ShaderSource::Wgsl(PATH_KERNEL.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pathtrace Path Pipeline Layout"),
            bind_group_layouts: &[
                &layouts.scene,
                &layouts.target,
                &layouts.sampled,
                uniforms.layout(),
            ],
            push_constant_ranges: &[],
        });
        let pipelines = PathEstimator::ALL.map(|estimator| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Pathtrace Path Pipeline"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("path_main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[
                        ("PATH_ESTIMATOR", estimator.index() as f64),
                        ("BSDF_TRANSMISSION_BIAS", bias),
                    ],
                    zero_initialize_workgroup_memory: false,
                },
                cache: None,
            })
        });
        Self { pipelines }
    }

    /// Encodes one tile's dispatch.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        estimator: PathEstimator,
        scene: &TraceScene,
        atlas: &TraceAtlas,
        target: &TraceTarget,
        uniforms: &PathUniforms,
        tile: [u32; 2],
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Pathtrace Path Pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines[estimator.index()]);
        pass.set_bind_group(0, scene.bind_group(), &[]);
        pass.set_bind_group(1, target.bind_group(), &[]);
        pass.set_bind_group(2, atlas.bind_group(), &[]);
        pass.set_bind_group(3, &uniforms.bind_group, &[]);
        pass.dispatch_workgroups(
            tile[0].div_ceil(WORKGROUP_SIZE),
            tile[1].div_ceil(WORKGROUP_SIZE),
            1,
        );
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
