//! [`BindGroupLayouts`]: the single source of truth for every wgpu bind
//! group layout used by the renderer's pipelines. All uniform entries use
//! `min_binding_size: None`, so growing a uniform is layout-invisible.

pub struct BindGroupLayouts {
    pub texture: wgpu::BindGroupLayout,
    pub camera: wgpu::BindGroupLayout,
    pub light: wgpu::BindGroupLayout,
    pub shadow_pass: wgpu::BindGroupLayout,
    pub shadow_read: wgpu::BindGroupLayout,
    pub grid_params: wgpu::BindGroupLayout,
    pub normals_params: wgpu::BindGroupLayout,
    pub background: wgpu::BindGroupLayout,
    pub uv_checker: wgpu::BindGroupLayout,
    pub bloom_texture: wgpu::BindGroupLayout,
    pub bloom_params: wgpu::BindGroupLayout,
    pub composite: wgpu::BindGroupLayout,
    pub composite_params: wgpu::BindGroupLayout,
    pub edge_geometry: wgpu::BindGroupLayout,
    pub wireframe_params: wgpu::BindGroupLayout,
    pub ssao: wgpu::BindGroupLayout,
    pub ssao_blur: wgpu::BindGroupLayout,
    pub ssao_read: wgpu::BindGroupLayout,
    pub uv_overlap_read: wgpu::BindGroupLayout,
    pub validation_color: wgpu::BindGroupLayout,
    pub overdraw_show: wgpu::BindGroupLayout,
    pub outline_texture: wgpu::BindGroupLayout,
    pub outline_params: wgpu::BindGroupLayout,
    pub skybox: wgpu::BindGroupLayout,
    /// The GPU label channel: params uniform, label + glyph storage, the
    /// SDF atlas and its sampler.
    pub labels: wgpu::BindGroupLayout,
}

impl BindGroupLayouts {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_binding_group_layout"),
            entries: &[
                bgl_texture_entry(0),
                bgl_sampler_entry(1),
                bgl_texture_entry(2),
                bgl_sampler_entry(3),
                bgl_texture_entry(4),
                bgl_sampler_entry(5),
                bgl_texture_entry(6),
                bgl_sampler_entry(7),
                bgl_uniform_entry(8, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let camera = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera_binding_group_layout"),
            entries: &[bgl_uniform_entry(
                0,
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            )],
        });
        let light = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light_bind_group_layout"),
            entries: &[
                bgl_uniform_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                bgl_sampler_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                bgl_sampler_entry(4),
                bgl_texture_entry(5),
                bgl_sampler_entry(6),
                // The LTC tables, and the one sampler both are read with.
                // They live on the light layout rather than a group of
                // their own because they are only ever read by the
                // rect-area arm of the light loop, and adding a bind group
                // would touch every pipeline that has nothing to do with
                // area lights.
                bgl_texture_entry(7),
                bgl_texture_entry(8),
                bgl_sampler_entry(9),
            ],
        });
        let shadow_pass = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_pass_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let shadow_read = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_read_layout"),
            entries: &[
                bgl_uniform_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Depth,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let grid_params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid_params_bind_group_layout"),
            // Vertex reads `plane` (to place the quad in its world plane) and
            // fragment reads cell_size/color/plane, so both stages need it.
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let normals_params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("normals_params_bind_group_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let background = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("background_bind_group_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let uv_checker = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uv_checker_bind_group_layout"),
            entries: &[bgl_texture_entry(0), bgl_sampler_entry(1)],
        });
        let bloom_texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom_texture_bind_group_layout"),
            entries: &[bgl_texture_entry(0), bgl_sampler_entry(1)],
        });
        let bloom_params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom_params_bind_group_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let composite = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite_bind_group_layout"),
            entries: &[
                bgl_texture_entry(0),
                bgl_texture_entry(1),
                bgl_sampler_entry(2),
                // The two colour-grading LUT slots and the sampler both are
                // read with. They join this group rather than taking one of
                // their own because the composite pipeline is the only
                // consumer, and a fourth group would be a pipeline-layout
                // change for a pass nothing else shares.
                bgl_texture_entry_3d(3),
                bgl_texture_entry_3d(4),
                bgl_sampler_entry(5),
            ],
        });
        let composite_params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite_params_bind_group_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let edge_geometry = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edge_geometry_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let wireframe_params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wireframe_params_bind_group_layout"),
            entries: &[bgl_uniform_entry(
                0,
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            )],
        });
        let ssao = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Depth,
                    },
                    count: None,
                },
                bgl_texture_entry(1),
                bgl_texture_entry(2),
                bgl_sampler_entry(3),
                bgl_uniform_entry(4, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let ssao_blur = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao_blur_bind_group_layout"),
            entries: &[
                bgl_texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Depth,
                    },
                    count: None,
                },
                bgl_sampler_entry(2),
            ],
        });
        let ssao_read = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao_read_bind_group_layout"),
            entries: &[bgl_texture_entry(0), bgl_sampler_entry(1)],
        });
        let uv_overlap_read = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uv_overlap_read_bind_group_layout"),
            entries: &[bgl_texture_entry(0), bgl_sampler_entry(1)],
        });
        let validation_color = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("validation_color_bind_group_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let overdraw_show = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overdraw_show_bind_group_layout"),
            entries: &[bgl_texture_entry(0), bgl_sampler_entry(1)],
        });
        let skybox = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skybox_bind_group_layout"),
            entries: &[bgl_texture_entry(0), bgl_sampler_entry(1)],
        });
        let labels = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("labels_bind_group_layout"),
            entries: &[
                // Params: the vertex stage lays out quads from the px
                // metrics, the fragment stage colors them.
                bgl_uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                bgl_storage_entry(1),
                bgl_storage_entry(2),
                bgl_texture_entry(3),
                bgl_sampler_entry(4),
            ],
        });
        // Selection-outline jump flood: the source texture is
        // read with textureLoad only (Rg32Float is non-filterable), and
        // one uniform layout serves the per-step and blit params.
        let outline_texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline_texture_bind_group_layout"),
            entries: &[bgl_texture_entry_unfilterable(0)],
        });
        let outline_params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline_params_bind_group_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });

        BindGroupLayouts {
            texture,
            camera,
            light,
            shadow_pass,
            shadow_read,
            grid_params,
            normals_params,
            background,
            uv_checker,
            bloom_texture,
            bloom_params,
            composite,
            composite_params,
            edge_geometry,
            wireframe_params,
            ssao,
            ssao_blur,
            ssao_read,
            uv_overlap_read,
            validation_color,
            overdraw_show,
            outline_texture,
            outline_params,
            skybox,
            labels,
        }
    }
}

/// The path tracer's four bind group layouts.
///
/// Declared here with every other layout, so this file stays the single source
/// of truth, but built separately and only when a tracer exists. That is not
/// tidiness: the scene group binds six storage buffers in the compute stage,
/// which core WebGPU allows (eight per stage) and
/// `Limits::downlevel_defaults()` does not (four). Building them inside
/// [`BindGroupLayouts::new`] would impose the tracer's limits on every consumer
/// of the registry, including the desktop shell, the golden harness, and any
/// headless tool that never traces a ray. It is not a hypothetical: it broke
/// the renderer's own smoke suite, which requests downlevel limits deliberately.
///
/// The numbering across the four groups is a budget rather than a convention.
/// Core WebGPU grants four bind groups, eight storage buffers and four storage
/// textures per stage, and the tracer's final shape spends all of them, so a
/// ninth logical array is a design error rather than a refactor.
pub struct PathtraceLayouts {
    /// Group 0: the scene arena's storage buffers.
    pub scene: wgpu::BindGroupLayout,
    /// Group 1: the storage textures the kernel writes, plus the coverage
    /// count buffer a transparent render's matte accumulates in.
    pub target: wgpu::BindGroupLayout,
    /// Group 2: sampled textures. The atlas and its two samplers; the
    /// environment's equirect and lookup textures are reserved by number.
    pub sampled: wgpu::BindGroupLayout,
    /// Group 3: the camera and per-dispatch uniforms.
    pub params: wgpu::BindGroupLayout,
    /// Group 1 **for the depth pass only**: one single-channel storage
    /// texture.
    ///
    /// The same number the accumulator takes in every other kernel, which is
    /// free there because that pass binds no accumulator: depth cannot be
    /// averaged and so is not one of the channels the accumulator holds. See
    /// `pathtrace::depth`.
    pub depth: wgpu::BindGroupLayout,
}

impl PathtraceLayouts {
    /// Builds all four. Requires a device with core WebGPU limits.
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        // Seven of core WebGPU's eight compute-stage storage buffers, which is
        // the whole budget the design set out to spend: 4 is the material pool
        // and 5 the lights, both arriving with their consumers rather than as
        // placeholders. Binding 7 stays unnumbered here, but the *count* it was
        // reserving is now spent: the coverage buffer on the target group below
        // is the stage's eighth storage buffer. A ninth logical array can no
        // longer simply take a slot; it would have to move coverage into a
        // replay kernel of its own (a primary-only kernel with its own
        // `r32float` read-write texture, one of four in that stage), which is
        // the recorded fallback and the price of the matte counting itself
        // inside the path kernel rather than beside it.
        let scene = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_scene_bind_group_layout"),
            entries: &[
                bgl_compute_storage_entry(0),
                bgl_compute_storage_entry(1),
                bgl_compute_storage_entry(2),
                bgl_compute_storage_entry(3),
                bgl_compute_storage_entry(4),
                bgl_compute_storage_entry(5),
                bgl_compute_storage_entry(6),
            ],
        });
        // The accumulation group: the colour the kernel returns and the
        // auxiliary channels that describe the surface it came from, each with
        // a read side to average against.
        //
        // **Four of four, and there is no spare.** Core WebGPU grants four
        // storage textures per stage and read-write on `Rgba32Float` is not
        // portable -- it grants that for `r32uint`, `r32sint` and `r32float`
        // only -- so the accumulator ping-pongs rather than accumulating in
        // place, and a ping-pong needs a read side for each of the two written
        // channels. That is the whole budget. A third auxiliary channel does
        // not fit, which is why the world normal is folded into the albedo's
        // alpha lane rather than taking a texture of its own.
        //
        // The write sides keep bindings 0 and 1, which they had before the
        // read sides arrived, so every kernel that only writes -- the debug
        // readout, every probe -- compiles unchanged. A pipeline may leave a
        // layout entry unused; a bind *group* may not, which is why
        // `TraceTarget` allocates both pairs whether or not anything reads
        // them.
        //
        // Binding 4 is a storage *buffer*, not a fifth texture: the coverage
        // count behind a transparent render's matte. It could not be a texture
        // -- the four above are the stage's whole texture budget -- and it
        // needs no ping-pong, because `u32` is one of the formats read-write
        // access is portable for, buffer or texture alike. It is the compute
        // stage's eighth and last storage buffer beside the scene group's
        // seven, which is a deliberate spend of the reserved slot; the comment
        // on the scene group says what a ninth array would now cost.
        let target = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_target_bind_group_layout"),
            entries: &[
                bgl_storage_texture_entry(0, wgpu::StorageTextureAccess::WriteOnly),
                bgl_storage_texture_entry(1, wgpu::StorageTextureAccess::WriteOnly),
                bgl_storage_texture_entry(2, wgpu::StorageTextureAccess::ReadOnly),
                bgl_storage_texture_entry(3, wgpu::StorageTextureAccess::ReadOnly),
                bgl_compute_storage_rw_entry(4),
            ],
        });
        // The sampled group: the atlas and its two samplers at 0 to 2, and the
        // environment at 3 to 6, which are the numbers reserved for it two
        // stages before it arrived. Nothing renumbered when it did.
        //
        // Two atlas samplers rather than one because a texture descriptor
        // carries a filter bit and WGSL cannot index a sampler: the kernel
        // branches, and both have to be bound for either branch to be legal.
        // Their binding types differ because the platform ties them to the
        // sampler's own filters -- an all-nearest sampler is a non-filtering
        // sampler, and declaring it `Filtering` is rejected at bind group
        // creation.
        //
        // The environment's two tables are declared **non-filterable** and are
        // read with `textureLoad`. That is not a limitation worked around: a
        // cumulative distribution is searched rather than interpolated, and
        // `R32Float` is unfilterable on core WebGPU anyway, so the honest
        // declaration and the useful one are the same one.
        //
        // Its image is non-filterable too, and that is a decision rather than a
        // constraint: the sampling distribution is piecewise constant over
        // texels, so a *filtered* radiance would describe a different
        // environment from the one the density describes. See
        // `pathtrace::environment`.
        let sampled = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_sampled_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                bgl_compute_texture_entry(3, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                bgl_compute_texture_entry(5, false),
                bgl_compute_texture_entry(6, false),
            ],
        });
        let params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_params_bind_group_layout"),
            entries: &[
                bgl_uniform_entry(0, wgpu::ShaderStages::COMPUTE),
                bgl_uniform_entry(1, wgpu::ShaderStages::COMPUTE),
            ],
        });
        // One texture, single channel, and it is `R32Float` rather than the
        // accumulator's four-channel format because a distance is one number
        // and a compositing package reads it as one.
        let depth = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_depth_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::R32Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            }],
        });
        Self {
            scene,
            target,
            sampled,
            params,
            depth,
        }
    }
}

fn bgl_uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A compute-stage read-only storage buffer entry.
///
/// Separate from [`bgl_storage_entry`] rather than taking a visibility, because
/// every raster storage binding is vertex-stage and every tracer one is
/// compute-stage; a shared parameter would be a parameter with two callers and
/// one value each.
/// A compute-stage sampled 2D float texture entry.
///
/// `filterable` is explicit at every call site because it is the thing the
/// platform constrains and the thing a reader most wants stated: the
/// environment's image is filtered and its two distribution tables are not,
/// and declaring a table filterable would be rejected against the `R32Float`
/// it is bound to.
fn bgl_compute_texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable },
        },
        count: None,
    }
}

fn bgl_compute_storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A compute-stage read-write storage buffer entry.
///
/// The accumulating kind: the coverage count loads what a chunk already wrote
/// and adds to it inside one dispatch's own pixel, which a read-only entry
/// cannot express and a second buffer plus a ping-pong would over-express.
fn bgl_compute_storage_rw_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A compute-stage `Rgba32Float` 2D storage texture entry.
///
/// The access mode is explicit at every call site because it is the thing the
/// platform constrains: `ReadWrite` is not available for this format on core
/// WebGPU, and native wgpu only rejects it at bind *group* creation, so a probe
/// that stops at the layout gets a false all-clear.
fn bgl_storage_texture_entry(
    binding: u32,
    access: wgpu::StorageTextureAccess,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access,
            format: wgpu::TextureFormat::Rgba32Float,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

/// A vertex-stage read-only storage buffer entry (the label channel's
/// instance and glyph streams; the edge-geometry layout spells out the
/// same shape inline).
fn bgl_storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bgl_texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
        },
        count: None,
    }
}

/// A filterable 3D float texture entry, for the colour-grading LUTs.
/// Filterable is the whole point (a 33-cubed table is interpolated between
/// entries, not stepped), which is what pins the LUT format to
/// `Rgba16Float`: `Rgba32Float` needs the `float32-filterable` feature and
/// this renderer holds itself to core WebGPU. See `crate::lut`.
fn bgl_texture_entry_3d(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D3,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
        },
        count: None,
    }
}

/// A non-filterable float texture entry (`Rg32Float` sources read with
/// textureLoad; filterable textures also satisfy it).
fn bgl_texture_entry_unfilterable(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
        },
        count: None,
    }
}

fn bgl_sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
