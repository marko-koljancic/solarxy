pub(crate) struct BindGroupLayouts {
    pub(crate) texture: wgpu::BindGroupLayout,
    pub(crate) camera: wgpu::BindGroupLayout,
    pub(crate) light: wgpu::BindGroupLayout,
    pub(crate) shadow_pass: wgpu::BindGroupLayout,
    pub(crate) shadow_read: wgpu::BindGroupLayout,
    pub(crate) grid: wgpu::BindGroupLayout,
    pub(crate) normals: wgpu::BindGroupLayout,
    pub(crate) background: wgpu::BindGroupLayout,
    pub(crate) wireframe_color: wgpu::BindGroupLayout,
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
            entries: &[bgl_uniform_entry(
                0,
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            )],
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
        let wireframe_color = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wireframe_color_bind_group_layout"),
            entries: &[bgl_uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
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
            wireframe_color,
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
