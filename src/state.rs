use crate::cgi::camera::{
    camera_from_bounds, camera_from_bounds_axis, Camera, CameraController, CameraUniform, ProjectionMode,
};
use crate::cgi::hud::HudRenderer;
use crate::cgi::light::{LightsUniform, LightEntry};
use crate::cgi::model::Model;
use crate::cgi::model::{self, Vertex};
use crate::cgi::{resources, texture};
use cgmath::prelude::*;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::{event::MouseButton, event_loop::ActiveEventLoop, keyboard::KeyCode, window::Window};

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Shaded,
    ShadedWireframe,
    WireframeOnly,
    Ghosted,
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewMode::Shaded => write!(f, "Shaded"),
            ViewMode::ShadedWireframe => write!(f, "Shaded+Wire"),
            ViewMode::WireframeOnly => write!(f, "Wireframe"),
            ViewMode::Ghosted => write!(f, "Ghosted"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum NormalsMode {
    Off,
    Face,
    Vertex,
    FaceAndVertex,
}

impl std::fmt::Display for NormalsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NormalsMode::Off => write!(f, "Off"),
            NormalsMode::Face => write!(f, "Face"),
            NormalsMode::Vertex => write!(f, "Vertex"),
            NormalsMode::FaceAndVertex => write!(f, "Face+Vertex"),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct NormalsColor {
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniform {
    cell_size: f32,
    _pad: [f32; 3],
}

const BACKGROUND_COLOR: wgpu::Color = wgpu::Color {
    r: 0.4235,
    g: 0.4588,
    b: 0.4902,
    a: 1.0,
};

#[rustfmt::skip]
const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::from_cols(
    cgmath::Vector4::new(1.0, 0.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 1.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 1.0),
);

struct Instance {
    position: cgmath::Vector3<f32>,
    rotation: cgmath::Quaternion<f32>,
}

impl Instance {
    fn to_raw(&self) -> InstanceRaw {
        let model = cgmath::Matrix4::from_translation(self.position) * cgmath::Matrix4::from(self.rotation);
        InstanceRaw {
            model: model.into(),
            normal: cgmath::Matrix3::from(self.rotation).into(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    normal: [[f32; 3]; 3],
}

impl model::Vertex for InstanceRaw {
    fn description() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 16]>() as wgpu::BufferAddress,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 19]>() as wgpu::BufferAddress,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 22]>() as wgpu::BufferAddress,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowUniform {
    light_vp: [[f32; 4]; 4],
}

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    is_surface_configured: bool,
    render_pipeline: wgpu::RenderPipeline,
    diffuse_bind_group: wgpu::BindGroup,
    camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_controller: CameraController,
    camera_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    depth_texture: texture::Texture,
    lights_uniform: LightsUniform,
    light_buffer: wgpu::Buffer,
    model: Model,
    light_bind_group: wgpu::BindGroup,
    shadow_texture_view: wgpu::TextureView,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_pass_bind_group: wgpu::BindGroup,
    shadow_bind_group: wgpu::BindGroup,
    shadow_uniform: ShadowUniform,
    shadow_uniform_buffer: wgpu::Buffer,
    floor_mesh: model::Mesh,
    floor_pipeline: wgpu::RenderPipeline,
    wireframe_pipeline: wgpu::RenderPipeline,
    ghosted_fill_pipeline: wgpu::RenderPipeline,
    ghosted_wire_pipeline: wgpu::RenderPipeline,
    view_mode: ViewMode,
    prev_non_ghosted_mode: ViewMode,
    grid_mesh: model::Mesh,
    grid_pipeline: wgpu::RenderPipeline,
    grid_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    grid_uniform_buf: wgpu::Buffer,
    normals_mode: NormalsMode,
    normals_pipeline: wgpu::RenderPipeline,
    vertex_normals_buf: wgpu::Buffer,
    face_normals_buf: wgpu::Buffer,
    vertex_normals_count: u32,
    face_normals_count: u32,
    face_normals_bind_group: wgpu::BindGroup,
    vertex_normals_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    face_normals_color_buf: wgpu::Buffer,
    #[allow(dead_code)]
    vertex_normals_color_buf: wgpu::Buffer,
    hud: HudRenderer,
    pub window: Arc<Window>,
    pub model_path: String,
}

impl State {
    pub async fn new(window: Arc<Window>, model_path: String) -> anyhow::Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::POLYGON_MODE_LINE,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::default()
                },
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_formats = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_formats,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let texture_binding_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("texture_binding_group_layout"),
        });

        let (model, normals_geo) =
            resources::load_model_any(&model_path, &device, &queue, &texture_binding_group_layout)
                .await
                .unwrap();

        let diffuse_texture = &model.materials[0].diffuse_texture;

        let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &texture_binding_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
            label: Some("diffuse_bind_group"),
        });

        let depth_texture = texture::Texture::create_depth_texture(&device, &config, "depth_texture");

        let camera = camera_from_bounds(&model.bounds, config.width as f32 / config.height as f32);

        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("camera_binding_group_layout"),
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: Some("camera_bind_group"),
        });

        let camera_controller = CameraController::new(0.2);

        let instance = Instance {
            position: cgmath::Vector3 { x: 0.0, y: 0.0, z: 0.0 },
            rotation: cgmath::Quaternion::from_axis_angle(cgmath::Vector3::unit_z(), cgmath::Deg(0.0)),
        };

        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(&[instance.to_raw()]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let lights_uniform = lights_from_camera(&camera, &model.bounds);

        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Light VB"),
            contents: bytemuck::cast_slice(&[lights_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let light_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("light_bind_group_layout"),
        });

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
            label: None,
        });

        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow_texture"),
            size: wgpu::Extent3d {
                width: 2048,
                height: 2048,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_texture_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let key_pos = lights_uniform.lights[0].position;
        let light_vp = compute_light_vp(
            cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
            model.bounds.center(),
            model.bounds.diagonal() / 2.0,
        );
        let shadow_uniform = ShadowUniform {
            light_vp: light_vp.into(),
        };
        let shadow_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Shadow Uniform Buffer"),
            contents: bytemuck::cast_slice(&[shadow_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let shadow_pass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_pass_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let shadow_read_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_read_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
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

        let shadow_pass_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_pass_bind_group"),
            layout: &shadow_pass_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: shadow_uniform_buffer.as_entire_binding(),
            }],
        });

        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_bind_group"),
            layout: &shadow_read_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: shadow_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });

        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shadow Pipeline Layout"),
            bind_group_layouts: &[&shadow_pass_layout],
            push_constant_ranges: &[],
        });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shadow Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/shadow.wgsl").into()),
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shadow Pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_shadow"),
                buffers: &[model::ModelVertex::description(), InstanceRaw::description()],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rendering Pipeline Layout"),
            bind_group_layouts: &[
                &texture_binding_group_layout,
                &camera_bind_group_layout,
                &light_bind_group_layout,
                &shadow_read_layout,
            ],
            push_constant_ranges: &[],
        });

        let render_pipeline = {
            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Normal Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/shader.wgsl").into()),
            };
            create_render_pipeline(
                &device,
                &render_pipeline_layout,
                config.format,
                Some(texture::Texture::DEPTH_FORMAT),
                &[model::ModelVertex::description(), InstanceRaw::description()],
                shader,
            )
        };

        let floor_mesh = resources::create_floor_quad(&device, &model.bounds);

        let floor_pipeline = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Floor Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &shadow_read_layout],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Floor Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/floor.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Floor Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_floor"),
                    buffers: &[model::ModelVertex::description()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_floor"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };

        let ghosted_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Ghosted Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });
        let ghosted_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ghosted Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/ghosted.wgsl").into()),
        });
        let ghosted_fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Ghosted Fill Pipeline"),
            layout: Some(&ghosted_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ghosted_shader,
                entry_point: Some("vs_ghosted"),
                buffers: &[model::ModelVertex::description(), InstanceRaw::description()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &ghosted_shader,
                entry_point: Some("fs_ghosted_fill"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        let ghosted_wire_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Ghosted Wire Pipeline"),
            layout: Some(&ghosted_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ghosted_shader,
                entry_point: Some("vs_ghosted"),
                buffers: &[model::ModelVertex::description(), InstanceRaw::description()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &ghosted_shader,
                entry_point: Some("fs_ghosted_wire"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Line,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        let wireframe_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Wireframe Pipeline"),
            layout: Some(&ghosted_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ghosted_shader,
                entry_point: Some("vs_ghosted"),
                buffers: &[model::ModelVertex::description(), InstanceRaw::description()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &ghosted_shader,
                entry_point: Some("fs_wireframe"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        alpha: wgpu::BlendComponent::REPLACE,
                        color: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Line,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: -2,
                    slope_scale: -2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        let grid_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Grid Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let (grid_mesh, cell_size) = resources::create_grid_quad(&device, &model.bounds);
        let grid_uniform = GridUniform {
            cell_size,
            _pad: [0.0; 3],
        };
        let grid_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Grid Uniform Buffer"),
            contents: bytemuck::cast_slice(&[grid_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let grid_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Grid Bind Group"),
            layout: &grid_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: grid_uniform_buf.as_entire_binding(),
                },
            ],
        });
        let grid_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grid Pipeline Layout"),
            bind_group_layouts: &[&grid_bind_group_layout],
            push_constant_ranges: &[],
        });
        let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Grid Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/grid.wgsl").into()),
        });
        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Grid Pipeline"),
            layout: Some(&grid_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &grid_shader,
                entry_point: Some("vs_grid"),
                buffers: &[model::LineVertex::description()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &grid_shader,
                entry_point: Some("fs_grid"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        let normals_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Normals Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let normals_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Normals Pipeline Layout"),
            bind_group_layouts: &[&normals_bind_group_layout],
            push_constant_ranges: &[],
        });
        let normals_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Normals Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/normals.wgsl").into()),
        });
        let normals_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Normals Pipeline"),
            layout: Some(&normals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &normals_shader,
                entry_point: Some("vs_normals"),
                buffers: &[model::LineVertex::description()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &normals_shader,
                entry_point: Some("fs_normals"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        alpha: wgpu::BlendComponent::REPLACE,
                        color: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        let (vertex_normals_buf, vertex_normals_count) = if normals_geo.vertex_lines.is_empty() {
            (
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Vertex Normals Buffer"),
                    contents: &[0u8; 12],
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                0u32,
            )
        } else {
            let count = normals_geo.vertex_lines.len() as u32;
            (
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Vertex Normals Buffer"),
                    contents: bytemuck::cast_slice(&normals_geo.vertex_lines),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                count,
            )
        };
        let (face_normals_buf, face_normals_count) = if normals_geo.face_lines.is_empty() {
            (
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Face Normals Buffer"),
                    contents: &[0u8; 12],
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                0u32,
            )
        } else {
            let count = normals_geo.face_lines.len() as u32;
            (
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Face Normals Buffer"),
                    contents: bytemuck::cast_slice(&normals_geo.face_lines),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                count,
            )
        };

        let face_normals_color_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Face Normals Color Buffer"),
            contents: bytemuck::cast_slice(&[NormalsColor {
                color: [0.2, 0.85, 0.2, 1.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let vertex_normals_color_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Normals Color Buffer"),
            contents: bytemuck::cast_slice(&[NormalsColor {
                color: [0.25, 0.55, 1.0, 1.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let face_normals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Face Normals Bind Group"),
            layout: &normals_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: face_normals_color_buf.as_entire_binding(),
                },
            ],
        });
        let vertex_normals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Vertex Normals Bind Group"),
            layout: &normals_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: vertex_normals_color_buf.as_entire_binding(),
                },
            ],
        });

        let hud = HudRenderer::new(
            &device,
            surface_formats,
            size.width,
            size.height,
            &model_path,
            window.scale_factor(),
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            render_pipeline,
            diffuse_bind_group,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            camera_controller,
            instance_buffer,
            lights_uniform,
            light_buffer,
            depth_texture,
            model,
            light_bind_group,
            shadow_texture_view,
            shadow_pipeline,
            shadow_pass_bind_group,
            shadow_bind_group,
            shadow_uniform,
            shadow_uniform_buffer,
            floor_mesh,
            floor_pipeline,
            wireframe_pipeline,
            ghosted_fill_pipeline,
            ghosted_wire_pipeline,
            view_mode: ViewMode::Shaded,
            prev_non_ghosted_mode: ViewMode::Shaded,
            grid_mesh,
            grid_pipeline,
            grid_bind_group,
            grid_uniform_buf,
            normals_mode: NormalsMode::Off,
            normals_pipeline,
            vertex_normals_buf,
            face_normals_buf,
            vertex_normals_count,
            face_normals_count,
            face_normals_bind_group,
            vertex_normals_bind_group,
            face_normals_color_buf,
            vertex_normals_color_buf,
            hud,
            window,
            model_path,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.camera.aspect = width as f32 / height as f32;
            self.surface.configure(&self.device, &self.config);
            self.depth_texture = texture::Texture::create_depth_texture(&self.device, &self.config, "depth_texture");
            self.is_surface_configured = true;
            self.hud.resize(width, height, &self.queue);
            self.hud.set_scale_factor(self.window.scale_factor());
        }
    }

    pub fn toggle_hints(&mut self) {
        self.hud.toggle_hints();
    }

    pub fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if code == KeyCode::Escape && is_pressed {
            event_loop.exit();
        } else if code == KeyCode::KeyH && is_pressed {
            let aspect = self.camera.aspect;
            self.camera = camera_from_bounds(&self.model.bounds, aspect);
            self.camera_controller = CameraController::new(0.2);
        } else if code == KeyCode::KeyT && is_pressed {
            let aspect = self.camera.aspect;
            self.camera = camera_from_bounds_axis(
                &self.model.bounds,
                aspect,
                cgmath::Vector3::unit_y(),
                -cgmath::Vector3::unit_z(),
            );
            self.camera_controller = CameraController::new(0.2);
        } else if code == KeyCode::KeyF && is_pressed {
            let aspect = self.camera.aspect;
            self.camera = camera_from_bounds_axis(
                &self.model.bounds,
                aspect,
                cgmath::Vector3::unit_z(),
                cgmath::Vector3::unit_y(),
            );
            self.camera_controller = CameraController::new(0.2);
        } else if code == KeyCode::KeyL && is_pressed {
            let aspect = self.camera.aspect;
            self.camera = camera_from_bounds_axis(
                &self.model.bounds,
                aspect,
                -cgmath::Vector3::unit_x(),
                cgmath::Vector3::unit_y(),
            );
            self.camera_controller = CameraController::new(0.2);
        } else if code == KeyCode::KeyR && is_pressed {
            let aspect = self.camera.aspect;
            self.camera = camera_from_bounds_axis(
                &self.model.bounds,
                aspect,
                cgmath::Vector3::unit_x(),
                cgmath::Vector3::unit_y(),
            );
            self.camera_controller = CameraController::new(0.2);
        } else if code == KeyCode::KeyP && is_pressed {
            self.camera.projection = ProjectionMode::Perspective;
        } else if code == KeyCode::KeyO && is_pressed {
            if self.camera.projection != ProjectionMode::Orthographic {
                use cgmath::InnerSpace;
                let dist = (self.camera.target - self.camera.eye).magnitude();
                self.camera.ortho_scale = dist * (self.camera.fovy / 2.0).to_radians().tan();
                self.camera.projection = ProjectionMode::Orthographic;
            }
        } else if code == KeyCode::KeyW && is_pressed {
            if self.view_mode != ViewMode::Ghosted {
                self.view_mode = match self.view_mode {
                    ViewMode::Shaded => ViewMode::ShadedWireframe,
                    ViewMode::ShadedWireframe => ViewMode::WireframeOnly,
                    ViewMode::WireframeOnly => ViewMode::Shaded,
                    ViewMode::Ghosted => unreachable!(),
                };
            }
        } else if code == KeyCode::KeyX && is_pressed {
            if self.view_mode == ViewMode::Ghosted {
                self.view_mode = self.prev_non_ghosted_mode;
            } else {
                self.prev_non_ghosted_mode = self.view_mode;
                self.view_mode = ViewMode::Ghosted;
            }
        } else if code == KeyCode::KeyS && is_pressed {
            self.view_mode = ViewMode::Shaded;
        } else if code == KeyCode::KeyN && is_pressed {
            self.normals_mode = match self.normals_mode {
                NormalsMode::Off => NormalsMode::Face,
                NormalsMode::Face => NormalsMode::Vertex,
                NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                NormalsMode::FaceAndVertex => NormalsMode::Off,
            };
        } else {
            self.camera_controller.handle_key(code, is_pressed);
        }
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        self.camera_controller.handle_mouse_button(button, pressed);
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        self.camera_controller.handle_mouse_move(x, y);
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        self.camera_controller.handle_scroll(delta);
    }

    pub fn update(&mut self) {
        self.camera_controller.update_camera(&mut self.camera);
        self.camera_uniform.update_view_proj(&self.camera);
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));

        self.lights_uniform = lights_from_camera(&self.camera, &self.model.bounds);
        self.queue
            .write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(&[self.lights_uniform]));

        let key_pos = self.lights_uniform.lights[0].position;
        let light_vp = compute_light_vp(
            cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
            self.model.bounds.center(),
            self.model.bounds.diagonal() / 2.0,
        );
        self.shadow_uniform.light_vp = light_vp.into();
        self.queue.write_buffer(
            &self.shadow_uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.shadow_uniform]),
        );
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        self.window.request_redraw();
        if !self.is_surface_configured {
            return Ok(());
        }

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            let mut shadow_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shadow Pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_texture_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            shadow_pass.set_pipeline(&self.shadow_pipeline);
            shadow_pass.set_bind_group(0, &self.shadow_pass_bind_group, &[]);
            shadow_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            use model::DrawShadow;
            shadow_pass.draw_model_shadow_instanced(&self.model, 0..1);
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(BACKGROUND_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            use model::DrawModel;
            use model::DrawViewMode;

            render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));

            match self.view_mode {
                ViewMode::Shaded => {
                    render_pass.set_pipeline(&self.render_pipeline);
                    render_pass.set_bind_group(3, &self.shadow_bind_group, &[]);
                    render_pass.draw_model_instanced(
                        &self.model,
                        0..1,
                        &self.camera_bind_group,
                        &self.light_bind_group,
                    );
                    render_pass.set_pipeline(&self.floor_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.set_bind_group(1, &self.shadow_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.floor_mesh.vertex_buffer.slice(..));
                    render_pass.set_index_buffer(self.floor_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..self.floor_mesh.num_elements, 0, 0..1);
                }
                ViewMode::ShadedWireframe => {
                    render_pass.set_pipeline(&self.render_pipeline);
                    render_pass.set_bind_group(3, &self.shadow_bind_group, &[]);
                    render_pass.draw_model_instanced(
                        &self.model,
                        0..1,
                        &self.camera_bind_group,
                        &self.light_bind_group,
                    );
                    render_pass.set_pipeline(&self.floor_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.set_bind_group(1, &self.shadow_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.floor_mesh.vertex_buffer.slice(..));
                    render_pass.set_index_buffer(self.floor_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..self.floor_mesh.num_elements, 0, 0..1);
                    render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                    render_pass.set_pipeline(&self.wireframe_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.draw_model_view_mode(&self.model, 0..1);
                }
                ViewMode::WireframeOnly => {
                    render_pass.set_pipeline(&self.wireframe_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.draw_model_view_mode(&self.model, 0..1);
                }
                ViewMode::Ghosted => {
                    render_pass.set_pipeline(&self.ghosted_fill_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.draw_model_view_mode(&self.model, 0..1);
                    render_pass.set_pipeline(&self.ghosted_wire_pipeline);
                    render_pass.draw_model_view_mode(&self.model, 0..1);
                }
            }

            render_pass.set_pipeline(&self.grid_pipeline);
            render_pass.set_bind_group(0, &self.grid_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.grid_mesh.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.grid_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.grid_mesh.num_elements, 0, 0..1);

            if self.normals_mode != NormalsMode::Off {
                render_pass.set_pipeline(&self.normals_pipeline);
                if matches!(self.normals_mode, NormalsMode::Face | NormalsMode::FaceAndVertex)
                    && self.face_normals_count > 0
                {
                    render_pass.set_bind_group(0, &self.face_normals_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.face_normals_buf.slice(..));
                    render_pass.draw(0..self.face_normals_count, 0..1);
                }
                if matches!(self.normals_mode, NormalsMode::Vertex | NormalsMode::FaceAndVertex)
                    && self.vertex_normals_count > 0
                {
                    render_pass.set_bind_group(0, &self.vertex_normals_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.vertex_normals_buf.slice(..));
                    render_pass.draw(0..self.vertex_normals_count, 0..1);
                }
            }
        }

        self.hud.render(
            &self.device,
            &mut encoder,
            &view,
            &self.queue,
            self.config.width,
            self.config.height,
            &self.view_mode.to_string(),
            &self.camera.projection.to_string(),
            &self.normals_mode.to_string(),
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

fn lights_from_camera(camera: &Camera, bounds: &model::AABB) -> LightsUniform {
    use cgmath::InnerSpace;

    let target = camera.target;
    let radius = (camera.eye - camera.target).magnitude() * 2.0;

    let forward = (camera.target - camera.eye).normalize();
    let right = forward.cross(camera.up).normalize();
    let up = right.cross(forward);

    let key_dir = (right * -0.5 + up * 0.8 + (-forward) * 0.5).normalize();
    let fill_dir = (right * 1.0 + up * 0.5 + (-forward) * 0.5).normalize();
    let rim_dir = (right * 0.0 + up * 0.5 + (forward) * 1.5).normalize();

    let key = target + key_dir * radius;
    let fill = target + fill_dir * radius;
    let rim = target + rim_dir * radius;

    LightsUniform {
        lights: [
            LightEntry {
                position: [key.x, key.y, key.z],
                _pad0: 0.0,
                color: [1.0, 0.98, 0.95],
                intensity: 2.0,
            },
            LightEntry {
                position: [fill.x, fill.y, fill.z],
                _pad0: 0.0,
                color: [0.90, 0.93, 1.00],
                intensity: 1.0,
            },
            LightEntry {
                position: [rim.x, rim.y, rim.z],
                _pad0: 0.0,
                color: [1.0, 1.00, 1.00],
                intensity: 0.8,
            },
        ],
        sphere_scale: bounds.diagonal() * 0.04,
        _pad1: [0.0; 3],
    }
}

fn compute_light_vp(light_pos: cgmath::Point3<f32>, target: cgmath::Point3<f32>, extent: f32) -> cgmath::Matrix4<f32> {
    use cgmath::MetricSpace;
    let dist = light_pos.distance(target);
    let near = (dist - extent * 2.0).max(0.1);
    let far = dist + extent * 4.0;
    let view = cgmath::Matrix4::look_at_rh(light_pos, target, cgmath::Vector3::unit_y());
    let s = extent * 1.5;
    let proj = cgmath::ortho(-s, s, -s, s, near, far);
    OPENGL_TO_WGPU_MATRIX * proj * view
}

fn create_render_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    vertex_layouts: &[wgpu::VertexBufferLayout],
    shader: wgpu::ShaderModuleDescriptor,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(shader);

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: vertex_layouts,
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState {
                    alpha: wgpu::BlendComponent::REPLACE,
                    color: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}
