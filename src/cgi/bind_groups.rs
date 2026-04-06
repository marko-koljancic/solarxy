pub(crate) struct BindGroupLayouts {
    pub(crate) texture: wgpu::BindGroupLayout,
    pub(crate) camera: wgpu::BindGroupLayout,
    pub(crate) light: wgpu::BindGroupLayout,
    pub(crate) shadow_pass: wgpu::BindGroupLayout,
    pub(crate) shadow_read: wgpu::BindGroupLayout,
    pub(crate) grid: wgpu::BindGroupLayout,
    pub(crate) normals: wgpu::BindGroupLayout,
    pub(crate) background: wgpu::BindGroupLayout,
    pub(crate) uv_checker: wgpu::BindGroupLayout,
    pub(crate) bloom_texture: wgpu::BindGroupLayout,
    pub(crate) bloom_params: wgpu::BindGroupLayout,
    pub(crate) composite: wgpu::BindGroupLayout,
    pub(crate) composite_params: wgpu::BindGroupLayout,
    pub(crate) edge_geometry: wgpu::BindGroupLayout,
    pub(crate) wireframe_params: wgpu::BindGroupLayout,
    pub(crate) ssao: wgpu::BindGroupLayout,
    pub(crate) ssao_blur: wgpu::BindGroupLayout,
    pub(crate) ssao_read: wgpu::BindGroupLayout,
    pub(crate) uv_overlap_read: wgpu::BindGroupLayout,
}

impl BindGroupLayouts {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
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
        let grid = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid_bind_group_layout"),
            entries: &[
                bgl_uniform_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                bgl_uniform_entry(1, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let normals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("normals_bind_group_layout"),
            entries: &[
                bgl_uniform_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
                bgl_uniform_entry(1, wgpu::ShaderStages::FRAGMENT),
            ],
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
                bgl_uniform_entry(5, wgpu::ShaderStages::FRAGMENT),
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
                bgl_uniform_entry(3, wgpu::ShaderStages::FRAGMENT),
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
        BindGroupLayouts {
            texture,
            camera,
            light,
            shadow_pass,
            shadow_read,
            grid,
            normals,
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

fn bgl_sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
