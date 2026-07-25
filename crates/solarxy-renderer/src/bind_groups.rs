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
