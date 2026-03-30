use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::camera::{Camera, ProjectionMode};
use crate::cgi::camera_state::CameraState;
use crate::cgi::hud::HudRenderer;
use crate::cgi::light::{LightEntry, LightsUniform};
use crate::cgi::model::{self, Model};
use crate::cgi::pipelines::{Instance, Pipelines};
use crate::cgi::shadow::ShadowState;
use crate::cgi::visualization::VisualizationState;
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

const BACKGROUND_COLOR: wgpu::Color = wgpu::Color {
    r: 0.4235,
    g: 0.4588,
    b: 0.4902,
    a: 1.0,
};

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
    capture_requested: bool,
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let depth_texture = texture::Texture::create_depth_texture(&device, &config, "depth_texture");
        let layouts = BindGroupLayouts::new(&device);
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
        let shadow = ShadowState::new(&device, &layouts, &lights_uniform, &model);
        let pipelines = Pipelines::new(&device, &config, &layouts);
        let vis = VisualizationState::new(&device, &layouts, &model, &normals_geo, &cam.buffer);

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
            capture_requested: false,
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
            KeyCode::KeyC => self.capture_requested = true,
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
        self.shadow.update_light_vp(
            &self.queue,
            cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
            self.model.bounds.center(),
            self.model.bounds.diagonal() / 2.0,
        );
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        self.window.request_redraw();
        if !self.is_surface_configured {
            return Ok(());
        }

        self.hud.clear_expired_message();

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

        let capture_buffer = if self.capture_requested {
            self.capture_requested = false;
            Some(self.encode_capture(&output.texture, &mut encoder))
        } else {
            None
        };

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some((buffer, padded_row_bytes, width, height)) = capture_buffer {
            self.save_capture(buffer, padded_row_bytes, width, height);
        }

        output.present();
        Ok(())
    }

    fn encode_capture(
        &self,
        texture: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) -> (wgpu::Buffer, u32, u32, u32) {
        let width = self.config.width;
        let height = self.config.height;
        let bytes_per_pixel = 4u32;
        let unpadded_row_bytes = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = unpadded_row_bytes.div_ceil(align) * align;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Capture Staging Buffer"),
            size: (padded_row_bytes * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        (buffer, padded_row_bytes, width, height)
    }

    fn save_capture(&mut self, buffer: wgpu::Buffer, padded_row_bytes: u32, width: u32, height: u32) {
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });

        if rx.recv().unwrap().is_err() {
            eprintln!("Failed to map capture buffer");
            return;
        }

        let data = slice.get_mapped_range();
        let bytes_per_pixel = 4u32;
        let unpadded_row_bytes = width * bytes_per_pixel;

        let mut pixels = Vec::with_capacity((unpadded_row_bytes * height) as usize);
        for row in 0..height {
            let start = (row * padded_row_bytes) as usize;
            let end = start + unpadded_row_bytes as usize;
            pixels.extend_from_slice(&data[start..end]);
        }
        drop(data);
        buffer.unmap();

        let needs_swizzle = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        if needs_swizzle {
            for chunk in pixels.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }
        }

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("solarxy_{}.png", timestamp);

        let img = image::RgbaImage::from_raw(width, height, pixels).expect("Failed to create image from pixel data");
        if let Err(e) = img.save(&filename) {
            eprintln!("Failed to save screenshot: {}", e);
        } else {
            self.hud.set_capture_message(filename);
        }
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
