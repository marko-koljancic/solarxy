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
/// tidiness: the scene group binds five storage buffers in the compute stage,
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
    /// Group 1: the storage textures the kernel writes.
    pub target: wgpu::BindGroupLayout,
    /// Group 2: sampled textures. The atlas and its two samplers; the
    /// environment's equirect and lookup textures are reserved by number.
    pub sampled: wgpu::BindGroupLayout,
    /// Group 3: the camera and per-dispatch uniforms.
    pub params: wgpu::BindGroupLayout,
}

impl PathtraceLayouts {
    /// Builds all four. Requires a device with core WebGPU limits.
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        // Bindings 4 and 5 of the scene group are materials and lights, and 7
        // is the ninth-array escape hatch. They arrive with their consumers
        // rather than as placeholders, so the numbering is deliberately not
        // contiguous and a later stage does not renegotiate it.
        let scene = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_scene_bind_group_layout"),
            entries: &[
                bgl_compute_storage_entry(0),
                bgl_compute_storage_entry(1),
                bgl_compute_storage_entry(2),
                bgl_compute_storage_entry(3),
                bgl_compute_storage_entry(6),
            ],
        });
        // The accumulation group. It ends as four ping-ponged `Rgba32Float`
        // storage textures, colour and auxiliary; the tracer writes one debug
        // target until the accumulator arrives. Read-write on this format is
        // not portable -- core WebGPU grants it for `r32uint`, `r32sint` and
        // `r32float` only -- which is why the pair ping-pongs rather than
        // accumulating in place, and why this group has no headroom.
        let target = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_target_bind_group_layout"),
            entries: &[bgl_storage_texture_entry(
                0,
                wgpu::StorageTextureAccess::WriteOnly,
            )],
        });
        // The sampled group. Bindings 3 to 6 are reserved by number for the
        // environment (its equirect, its sampler, and the two CDF textures the
        // importance sampler looks up), so nothing renumbers when they arrive.
        //
        // Two samplers rather than one because a texture descriptor carries a
        // filter bit and WGSL cannot index a sampler: the kernel branches, and
        // both have to be bound for either branch to be legal. Their binding
        // types differ because the platform ties them to the sampler's own
        // filters -- an all-nearest sampler is a non-filtering sampler, and
        // declaring it `Filtering` is rejected at bind group creation.
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
            ],
        });
        let params = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_params_bind_group_layout"),
            entries: &[
                bgl_uniform_entry(0, wgpu::ShaderStages::COMPUTE),
                bgl_uniform_entry(1, wgpu::ShaderStages::COMPUTE),
            ],
        });
        Self {
            scene,
            target,
            sampled,
            params,
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
