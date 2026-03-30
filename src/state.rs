use crate::cgi::camera::{Camera, ProjectionMode, OPENGL_TO_WGPU_MATRIX};
use crate::cgi::camera_state::CameraState;
use crate::cgi::hud::HudRenderer;
use crate::cgi::light::{LightEntry, LightsUniform};
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

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowUniform {
    light_vp: [[f32; 4]; 4],
}

const BACKGROUND_COLOR: wgpu::Color = wgpu::Color {
    r: 0.4235,
    g: 0.4588,
    b: 0.4902,
    a: 1.0,
};

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

struct BindGroupLayouts {
    texture: wgpu::BindGroupLayout,
    camera: wgpu::BindGroupLayout,
    light: wgpu::BindGroupLayout,
    shadow_pass: wgpu::BindGroupLayout,
    shadow_read: wgpu::BindGroupLayout,
    grid: wgpu::BindGroupLayout,
    normals: wgpu::BindGroupLayout,
}

struct Pipelines {
    main: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    floor: wgpu::RenderPipeline,
    wireframe: wgpu::RenderPipeline,
    ghosted_fill: wgpu::RenderPipeline,
    ghosted_wire: wgpu::RenderPipeline,
    grid: wgpu::RenderPipeline,
    normals: wgpu::RenderPipeline,
}

struct ShadowState {
    texture_view: wgpu::TextureView,
    pass_bind_group: wgpu::BindGroup,
    sample_bind_group: wgpu::BindGroup,
    uniform: ShadowUniform,
    uniform_buffer: wgpu::Buffer,
}

struct VisualizationState {
    grid_mesh: model::Mesh,
    grid_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    grid_uniform_buf: wgpu::Buffer,
    floor_mesh: model::Mesh,
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
}

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    is_surface_configured: bool,
    depth_texture: texture::Texture,
    cam: CameraState,
    lights_uniform: LightsUniform,
    light_buffer: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    shadow: ShadowState,
    pipelines: Pipelines,
    vis: VisualizationState,
    model: Model,
    hud: HudRenderer,
    view_mode: ViewMode,
    prev_non_ghosted_mode: ViewMode,
    normals_mode: NormalsMode,
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
        let surface = instance.create_surface(window.clone())?;
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
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let depth_texture = texture::Texture::create_depth_texture(&device, &config, "depth_texture");
        let layouts = create_bind_group_layouts(&device);
        let (model, normals_geo, model_stats) =
            resources::load_model_any(&model_path, &device, &queue, &layouts.texture)?;

        let cam = CameraState::new(
            &device,
            &layouts.camera,
            &model.bounds,
            config.width as f32 / config.height as f32,
        );
        let instance_data = Instance {
            position: cgmath::Vector3::new(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::from_axis_angle(cgmath::Vector3::unit_z(), cgmath::Deg(0.0)),
        };
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(&[instance_data.to_raw()]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let lights_uniform = lights_from_camera(&cam.camera, &model.bounds);
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Light VB"),
            contents: bytemuck::cast_slice(&[lights_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layouts.light,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
        });
        let shadow = init_shadow_system(&device, &layouts, &lights_uniform, &model);
        let pipelines = init_pipelines(&device, &config, &layouts);
        let vis = init_visualization(&device, &layouts, &model, &normals_geo, &cam.buffer);

        let hud = HudRenderer::new(
            &device,
            surface_format,
            size.width,
            size.height,
            &model_stats,
            window.scale_factor(),
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            depth_texture,
            cam,
            lights_uniform,
            light_buffer,
            light_bind_group,
            instance_buffer,
            shadow,
            pipelines,
            vis,
            model,
            hud,
            view_mode: ViewMode::Shaded,
            prev_non_ghosted_mode: ViewMode::Shaded,
            normals_mode: NormalsMode::Off,
            window,
            model_path,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.cam.resize(width as f32 / height as f32);
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
        if !is_pressed {
            self.cam.handle_key(code, is_pressed);
            return;
        }
        match code {
            KeyCode::Escape => event_loop.exit(),
            KeyCode::KeyH => self.cam.reset_to_bounds(&self.model.bounds),
            KeyCode::KeyT => self.cam.reset_to_bounds_axis(
                &self.model.bounds,
                cgmath::Vector3::unit_y(),
                -cgmath::Vector3::unit_z(),
            ),
            KeyCode::KeyF => {
                self.cam
                    .reset_to_bounds_axis(&self.model.bounds, cgmath::Vector3::unit_z(), cgmath::Vector3::unit_y())
            }
            KeyCode::KeyL => self.cam.reset_to_bounds_axis(
                &self.model.bounds,
                -cgmath::Vector3::unit_x(),
                cgmath::Vector3::unit_y(),
            ),
            KeyCode::KeyR => {
                self.cam
                    .reset_to_bounds_axis(&self.model.bounds, cgmath::Vector3::unit_x(), cgmath::Vector3::unit_y())
            }
            KeyCode::KeyP => self.cam.set_projection(ProjectionMode::Perspective),
            KeyCode::KeyO => self.cam.set_projection(ProjectionMode::Orthographic),
            KeyCode::KeyW => {
                if self.view_mode != ViewMode::Ghosted {
                    self.view_mode = match self.view_mode {
                        ViewMode::Shaded => ViewMode::ShadedWireframe,
                        ViewMode::ShadedWireframe => ViewMode::WireframeOnly,
                        ViewMode::WireframeOnly => ViewMode::Shaded,
                        ViewMode::Ghosted => unreachable!(),
                    };
                }
            }
            KeyCode::KeyX => {
                if self.view_mode == ViewMode::Ghosted {
                    self.view_mode = self.prev_non_ghosted_mode;
                } else {
                    self.prev_non_ghosted_mode = self.view_mode;
                    self.view_mode = ViewMode::Ghosted;
                }
            }
            KeyCode::KeyS => self.view_mode = ViewMode::Shaded,
            KeyCode::KeyN => {
                self.normals_mode = match self.normals_mode {
                    NormalsMode::Off => NormalsMode::Face,
                    NormalsMode::Face => NormalsMode::Vertex,
                    NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                    NormalsMode::FaceAndVertex => NormalsMode::Off,
                };
            }
            _ => {
                self.cam.handle_key(code, is_pressed);
            }
        }
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        self.cam.handle_mouse_button(button, pressed);
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        self.cam.handle_mouse_move(x, y);
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        self.cam.handle_scroll(delta);
    }

    pub fn update(&mut self) {
        self.cam.update(&self.queue);

        self.lights_uniform = lights_from_camera(&self.cam.camera, &self.model.bounds);
        self.queue
            .write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(&[self.lights_uniform]));

        let key_pos = self.lights_uniform.lights[0].position;
        let light_vp = compute_light_vp(
            cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
            self.model.bounds.center(),
            self.model.bounds.diagonal() / 2.0,
        );
        self.shadow.uniform.light_vp = light_vp.into();
        self.queue.write_buffer(
            &self.shadow.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.shadow.uniform]),
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

        self.render_shadow_pass(&mut encoder);
        self.render_main_pass(&mut encoder, &view);
        self.hud.render(
            &self.device,
            &mut encoder,
            &view,
            &self.queue,
            self.config.width,
            self.config.height,
            &self.view_mode.to_string(),
            &self.cam.camera.projection.to_string(),
            &self.normals_mode.to_string(),
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    fn render_shadow_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shadow Pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow.texture_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines.shadow);
        pass.set_bind_group(0, &self.shadow.pass_bind_group, &[]);
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        use model::DrawMeshSimple;
        pass.draw_model_simple(&self.model, 0..1);
    }

    fn render_main_pass(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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

        use model::DrawMeshSimple;
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));

        match self.view_mode {
            ViewMode::Shaded | ViewMode::ShadedWireframe => {
                self.draw_shaded_model(&mut pass);
                self.draw_floor(&mut pass);
                if self.view_mode == ViewMode::ShadedWireframe {
                    pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                    pass.set_pipeline(&self.pipelines.wireframe);
                    pass.set_bind_group(0, &self.cam.bind_group, &[]);
                    pass.draw_model_simple(&self.model, 0..1);
                }
            }
            ViewMode::WireframeOnly => {
                pass.set_pipeline(&self.pipelines.wireframe);
                pass.set_bind_group(0, &self.cam.bind_group, &[]);
                pass.draw_model_simple(&self.model, 0..1);
            }
            ViewMode::Ghosted => {
                pass.set_pipeline(&self.pipelines.ghosted_fill);
                pass.set_bind_group(0, &self.cam.bind_group, &[]);
                pass.draw_model_simple(&self.model, 0..1);
                pass.set_pipeline(&self.pipelines.ghosted_wire);
                pass.draw_model_simple(&self.model, 0..1);
            }
        }

        pass.set_pipeline(&self.pipelines.grid);
        pass.set_bind_group(0, &self.vis.grid_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vis.grid_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(self.vis.grid_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.vis.grid_mesh.num_elements, 0, 0..1);
        self.draw_normals(&mut pass);
    }

    fn draw_shaded_model<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        use model::DrawModel;
        pass.set_pipeline(&self.pipelines.main);
        pass.set_bind_group(3, &self.shadow.sample_bind_group, &[]);
        pass.draw_model_instanced(&self.model, 0..1, &self.cam.bind_group, &self.light_bind_group);
    }

    fn draw_floor<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipelines.floor);
        pass.set_bind_group(0, &self.cam.bind_group, &[]);
        pass.set_bind_group(1, &self.shadow.sample_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vis.floor_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(self.vis.floor_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.vis.floor_mesh.num_elements, 0, 0..1);
    }

    fn draw_normals<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.normals_mode == NormalsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.normals);
        if matches!(self.normals_mode, NormalsMode::Face | NormalsMode::FaceAndVertex)
            && self.vis.face_normals_count > 0
        {
            pass.set_bind_group(0, &self.vis.face_normals_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vis.face_normals_buf.slice(..));
            pass.draw(0..self.vis.face_normals_count, 0..1);
        }
        if matches!(self.normals_mode, NormalsMode::Vertex | NormalsMode::FaceAndVertex)
            && self.vis.vertex_normals_count > 0
        {
            pass.set_bind_group(0, &self.vis.vertex_normals_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vis.vertex_normals_buf.slice(..));
            pass.draw(0..self.vis.vertex_normals_count, 0..1);
        }
    }
}

fn init_shadow_system(
    device: &wgpu::Device,
    layouts: &BindGroupLayouts,
    lights_uniform: &LightsUniform,
    model: &Model,
) -> ShadowState {
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
    let texture_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());

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
    let uniform = ShadowUniform {
        light_vp: light_vp.into(),
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Shadow Uniform Buffer"),
        contents: bytemuck::cast_slice(&[uniform]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let pass_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow_pass_bind_group"),
        layout: &layouts.shadow_pass,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let sample_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow_bind_group"),
        layout: &layouts.shadow_read,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&shadow_sampler),
            },
        ],
    });

    ShadowState {
        texture_view,
        pass_bind_group,
        sample_bind_group,
        uniform,
        uniform_buffer,
    }
}

fn init_pipelines(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration, layouts: &BindGroupLayouts) -> Pipelines {
    let shadow_pipeline = {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shadow Pipeline Layout"),
            bind_group_layouts: &[&layouts.shadow_pass],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shadow Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/shadow.wgsl").into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shadow Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &[model::ModelVertex::description(), InstanceRaw::description()],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
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
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };

    let main = {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rendering Pipeline Layout"),
            bind_group_layouts: &[&layouts.texture, &layouts.camera, &layouts.light, &layouts.shadow_read],
            push_constant_ranges: &[],
        });
        create_render_pipeline(
            device,
            &layout,
            config.format,
            Some(texture::Texture::DEPTH_FORMAT),
            &[model::ModelVertex::description(), InstanceRaw::description()],
            wgpu::ShaderModuleDescriptor {
                label: Some("Normal Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/shader.wgsl").into()),
            },
        )
    };

    let floor = {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Floor Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.shadow_read],
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
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };

    let ghosted_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Ghosted Pipeline Layout"),
        bind_group_layouts: &[&layouts.camera],
        push_constant_ranges: &[],
    });
    let ghosted_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Ghosted Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/ghosted.wgsl").into()),
    });

    let ghosted_fill = create_ghosted_pipeline(
        device,
        &ghosted_layout,
        &ghosted_shader,
        config.format,
        "fs_ghosted_fill",
        wgpu::PolygonMode::Fill,
        false,
        None,
    );
    let ghosted_wire = create_ghosted_pipeline(
        device,
        &ghosted_layout,
        &ghosted_shader,
        config.format,
        "fs_ghosted_wire",
        wgpu::PolygonMode::Line,
        false,
        None,
    );
    let wireframe = create_ghosted_pipeline(
        device,
        &ghosted_layout,
        &ghosted_shader,
        config.format,
        "fs_wireframe",
        wgpu::PolygonMode::Line,
        true,
        Some(wgpu::DepthBiasState {
            constant: -2,
            slope_scale: -2.0,
            clamp: 0.0,
        }),
    );

    let grid = {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grid Pipeline Layout"),
            bind_group_layouts: &[&layouts.grid],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Grid Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/grid.wgsl").into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Grid Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_grid"),
                buffers: &[model::LineVertex::description()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
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
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };

    let normals = {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Normals Pipeline Layout"),
            bind_group_layouts: &[&layouts.normals],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Normals Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cgi/shaders/normals.wgsl").into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Normals Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_normals"),
                buffers: &[model::LineVertex::description()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
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
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: texture::Texture::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };

    Pipelines {
        main,
        shadow: shadow_pipeline,
        floor,
        wireframe,
        ghosted_fill,
        ghosted_wire,
        grid,
        normals,
    }
}

fn init_visualization(
    device: &wgpu::Device,
    layouts: &BindGroupLayouts,
    model: &Model,
    normals_geo: &model::NormalsGeometry,
    camera_buffer: &wgpu::Buffer,
) -> VisualizationState {
    let floor_mesh = resources::create_floor_quad(device, &model.bounds);
    let (grid_mesh, cell_size) = resources::create_grid_quad(device, &model.bounds);

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
        layout: &layouts.grid,
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

    let (vertex_normals_buf, vertex_normals_count) =
        create_normals_buffer(device, &normals_geo.vertex_lines, "Vertex Normals Buffer");
    let (face_normals_buf, face_normals_count) =
        create_normals_buffer(device, &normals_geo.face_lines, "Face Normals Buffer");

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
        layout: &layouts.normals,
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
        layout: &layouts.normals,
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

    VisualizationState {
        grid_mesh,
        grid_bind_group,
        grid_uniform_buf,
        floor_mesh,
        vertex_normals_buf,
        face_normals_buf,
        vertex_normals_count,
        face_normals_count,
        face_normals_bind_group,
        vertex_normals_bind_group,
        face_normals_color_buf,
        vertex_normals_color_buf,
    }
}

fn create_normals_buffer(device: &wgpu::Device, lines: &[[f32; 3]], label: &str) -> (wgpu::Buffer, u32) {
    if lines.is_empty() {
        (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: &[0u8; 12],
                usage: wgpu::BufferUsages::VERTEX,
            }),
            0,
        )
    } else {
        (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(lines),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            lines.len() as u32,
        )
    }
}

fn create_bind_group_layouts(device: &wgpu::Device) -> BindGroupLayouts {
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
    BindGroupLayouts {
        texture,
        camera,
        light,
        shadow_pass,
        shadow_read,
        grid,
        normals,
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

#[allow(clippy::too_many_arguments)]
fn create_ghosted_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    fragment_entry: &str,
    polygon_mode: wgpu::PolygonMode,
    depth_write: bool,
    depth_bias: Option<wgpu::DepthBiasState>,
) -> wgpu::RenderPipeline {
    let cull_mode = if depth_write { Some(wgpu::Face::Back) } else { None };
    let blend = if depth_write {
        wgpu::BlendState {
            alpha: wgpu::BlendComponent::REPLACE,
            color: wgpu::BlendComponent::REPLACE,
        }
    } else {
        wgpu::BlendState::ALPHA_BLENDING
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_ghosted"),
            buffers: &[model::ModelVertex::description(), InstanceRaw::description()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode,
            polygon_mode,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: texture::Texture::DEPTH_FORMAT,
            depth_write_enabled: depth_write,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: depth_bias.unwrap_or_default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
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
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
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
