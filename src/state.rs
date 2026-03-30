use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::camera::{Camera, ProjectionMode};
use crate::cgi::camera_state::CameraState;
use crate::cgi::hud::HudRenderer;
use crate::cgi::light::{LightEntry, LightsUniform};
use crate::cgi::model::{self, Model};
use crate::cgi::pipelines::{Instance, Pipelines};
use crate::cgi::resources::{self, ModelStats};
use crate::cgi::shadow::ShadowState;
use crate::cgi::visualization::VisualizationState;
use crate::cgi::texture;
use cgmath::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
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

#[derive(Clone, Copy, PartialEq)]
enum BackgroundPreset {
    BlueGray,
    DarkGray,
    StudioGray,
    White,
    Black,
}

impl BackgroundPreset {
    fn color(self) -> wgpu::Color {
        match self {
            Self::BlueGray => wgpu::Color {
                r: 0.4235,
                g: 0.4588,
                b: 0.4902,
                a: 1.0,
            },
            Self::DarkGray => wgpu::Color {
                r: 0.12,
                g: 0.12,
                b: 0.12,
                a: 1.0,
            },
            Self::StudioGray => wgpu::Color {
                r: 0.45,
                g: 0.45,
                b: 0.45,
                a: 1.0,
            },
            Self::White => wgpu::Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            Self::Black => wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        }
    }

    fn next(self) -> Self {
        match self {
            Self::BlueGray => Self::DarkGray,
            Self::DarkGray => Self::StudioGray,
            Self::StudioGray => Self::White,
            Self::White => Self::Black,
            Self::Black => Self::BlueGray,
        }
    }
}

impl std::fmt::Display for BackgroundPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlueGray => write!(f, "Blue-gray"),
            Self::DarkGray => write!(f, "Dark"),
            Self::StudioGray => write!(f, "Studio"),
            Self::White => write!(f, "White"),
            Self::Black => write!(f, "Black"),
        }
    }
}

struct ModelScene {
    model: Model,
    cam: CameraState,
    lights_uniform: LightsUniform,
    light_buffer: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    shadow: ShadowState,
    vis: VisualizationState,
    #[allow(dead_code)]
    model_path: String,
    stats: ModelStats,
}

impl ModelScene {
    fn new(
        model_path: String,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        config: &wgpu::SurfaceConfiguration,
    ) -> anyhow::Result<Self> {
        let (model, normals_geo, stats) = resources::load_model_any(&model_path, device, queue, &layouts.texture)?;

        let cam = CameraState::new(
            device,
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

        let shadow = ShadowState::new(device, layouts, &lights_uniform, &model);
        let vis = VisualizationState::new(device, layouts, &model, &normals_geo, &cam.buffer);

        Ok(ModelScene {
            model,
            cam,
            lights_uniform,
            light_buffer,
            light_bind_group,
            instance_buffer,
            shadow,
            vis,
            model_path,
            stats,
        })
    }
}

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    is_surface_configured: bool,
    depth_texture: texture::Texture,
    layouts: BindGroupLayouts,
    pipelines: Pipelines,
    hud: HudRenderer,
    scene: Option<ModelScene>,
    view_mode: ViewMode,
    prev_non_ghosted_mode: ViewMode,
    normals_mode: NormalsMode,
    background: BackgroundPreset,
    capture_requested: bool,
    last_frame_time: Instant,
    dt: f32,
    pub window: Arc<Window>,
}

impl State {
    pub async fn new(window: Arc<Window>, model_path: Option<String>) -> anyhow::Result<Self> {
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
        let pipelines = Pipelines::new(&device, &config, &layouts);

        let (scene, stats) = match model_path {
            Some(path) => {
                let s = ModelScene::new(path, &device, &queue, &layouts, &config)?;
                let stats = ModelStats {
                    polys: s.stats.polys,
                    tris: s.stats.tris,
                    verts: s.stats.verts,
                };
                (Some(s), Some(stats))
            }
            None => (None, None),
        };

        let hud = HudRenderer::new(
            &device,
            surface_format,
            size.width,
            size.height,
            stats.as_ref(),
            window.scale_factor(),
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            depth_texture,
            layouts,
            pipelines,
            hud,
            scene,
            view_mode: ViewMode::Shaded,
            prev_non_ghosted_mode: ViewMode::Shaded,
            normals_mode: NormalsMode::Off,
            background: BackgroundPreset::BlueGray,
            capture_requested: false,
            last_frame_time: Instant::now(),
            dt: 0.0,
            window,
        })
    }

    pub fn handle_dropped_file(&mut self, path: PathBuf) {
        if !resources::is_supported_model_extension(&path) {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("none");
            self.hud
                .set_toast(&format!("Unsupported format: .{}", ext), [0.6, 0.0, 0.0, 1.0]);
            return;
        }

        let model_path = match path.canonicalize() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(e) => {
                self.hud
                    .set_toast(&format!("Invalid path: {}", e), [0.6, 0.0, 0.0, 1.0]);
                return;
            }
        };

        match ModelScene::new(model_path, &self.device, &self.queue, &self.layouts, &self.config) {
            Ok(new_scene) => {
                self.hud.update_stats(Some(&new_scene.stats));
                self.scene = Some(new_scene);
                self.view_mode = ViewMode::Shaded;
                self.prev_non_ghosted_mode = ViewMode::Shaded;
                self.normals_mode = NormalsMode::Off;
            }
            Err(e) => {
                self.hud
                    .set_toast(&format!("Failed to load: {}", e), [0.6, 0.0, 0.0, 1.0]);
            }
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.depth_texture = texture::Texture::create_depth_texture(&self.device, &self.config, "depth_texture");
            self.is_surface_configured = true;
            self.hud.resize(width, height, &self.queue);
            self.hud.set_scale_factor(self.window.scale_factor());
            if let Some(scene) = &mut self.scene {
                scene.cam.resize(width as f32 / height as f32);
            }
        }
    }

    pub fn toggle_hints(&mut self) {
        self.hud.toggle_hints();
    }

    pub fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        if !is_pressed {
            if let Some(scene) = &mut self.scene {
                scene.cam.handle_key(code, is_pressed);
            }
            return;
        }
        match code {
            KeyCode::Escape => event_loop.exit(),
            KeyCode::KeyH => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.reset_to_bounds(&scene.model.bounds);
                }
            }
            KeyCode::KeyT => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.reset_to_bounds_axis(
                        &scene.model.bounds,
                        cgmath::Vector3::unit_y(),
                        -cgmath::Vector3::unit_z(),
                    );
                }
            }
            KeyCode::KeyF => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.reset_to_bounds_axis(
                        &scene.model.bounds,
                        cgmath::Vector3::unit_z(),
                        cgmath::Vector3::unit_y(),
                    );
                }
            }
            KeyCode::KeyL => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.reset_to_bounds_axis(
                        &scene.model.bounds,
                        -cgmath::Vector3::unit_x(),
                        cgmath::Vector3::unit_y(),
                    );
                }
            }
            KeyCode::KeyR => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.reset_to_bounds_axis(
                        &scene.model.bounds,
                        cgmath::Vector3::unit_x(),
                        cgmath::Vector3::unit_y(),
                    );
                }
            }
            KeyCode::KeyP => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.set_projection(ProjectionMode::Perspective);
                }
            }
            KeyCode::KeyO => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.set_projection(ProjectionMode::Orthographic);
                }
            }
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
            KeyCode::KeyC => {
                if self.scene.is_some() {
                    self.capture_requested = true;
                }
            }
            KeyCode::KeyB => {
                self.background = self.background.next();
            }
            KeyCode::KeyN => {
                self.normals_mode = match self.normals_mode {
                    NormalsMode::Off => NormalsMode::Face,
                    NormalsMode::Face => NormalsMode::Vertex,
                    NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                    NormalsMode::FaceAndVertex => NormalsMode::Off,
                };
            }
            _ => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.handle_key(code, is_pressed);
                }
            }
        }
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if let Some(scene) = &mut self.scene {
            scene.cam.handle_mouse_button(button, pressed);
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        if let Some(scene) = &mut self.scene {
            scene.cam.handle_mouse_move(x, y);
        }
    }

    pub fn handle_scroll(&mut self, delta: f32) {
        if let Some(scene) = &mut self.scene {
            scene.cam.handle_scroll(delta);
        }
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        self.dt = (now - self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        if let Some(scene) = &mut self.scene {
            scene.cam.update(&self.queue, self.dt);

            scene.lights_uniform = lights_from_camera(&scene.cam.camera, &scene.model.bounds);
            self.queue
                .write_buffer(&scene.light_buffer, 0, bytemuck::cast_slice(&[scene.lights_uniform]));

            let key_pos = scene.lights_uniform.lights[0].position;
            scene.shadow.update_light_vp(
                &self.queue,
                cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
                scene.model.bounds.center(),
                scene.model.bounds.diagonal() / 2.0,
            );
        }
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        self.window.request_redraw();
        if !self.is_surface_configured {
            return Ok(());
        }

        let frame_ms = self.dt * 1000.0;

        self.hud.clear_expired_toast();

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        let has_model = self.scene.is_some();

        if let Some(scene) = &self.scene {
            self.render_shadow_pass(&mut encoder, scene);
            self.render_main_pass(&mut encoder, &view, scene);
        } else {
            self.render_empty_pass(&mut encoder, &view);
        }

        let (projection_str, normals_str) = if let Some(scene) = &self.scene {
            (scene.cam.camera.projection.to_string(), self.normals_mode.to_string())
        } else {
            (String::new(), String::new())
        };

        self.hud.render(
            &self.device,
            &mut encoder,
            &view,
            &self.queue,
            self.config.width,
            self.config.height,
            &self.view_mode.to_string(),
            &projection_str,
            &normals_str,
            &self.background.to_string(),
            self.background.color(),
            frame_ms,
            has_model,
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

    fn render_empty_pass(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Empty Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.background.color()),
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
    }

    fn render_shadow_pass(&self, encoder: &mut wgpu::CommandEncoder, scene: &ModelScene) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shadow Pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &scene.shadow.texture_view,
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
        pass.set_bind_group(0, &scene.shadow.pass_bind_group, &[]);
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
        use model::DrawMeshSimple;
        pass.draw_model_simple(&scene.model, 0..1);
    }

    fn render_main_pass(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, scene: &ModelScene) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.background.color()),
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
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));

        match self.view_mode {
            ViewMode::Shaded | ViewMode::ShadedWireframe => {
                self.draw_shaded_model(&mut pass, scene);
                self.draw_floor(&mut pass, scene);
                if self.view_mode == ViewMode::ShadedWireframe {
                    pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
                    pass.set_pipeline(&self.pipelines.wireframe);
                    pass.set_bind_group(0, &scene.cam.bind_group, &[]);
                    pass.draw_model_simple(&scene.model, 0..1);
                }
            }
            ViewMode::WireframeOnly => {
                pass.set_pipeline(&self.pipelines.wireframe);
                pass.set_bind_group(0, &scene.cam.bind_group, &[]);
                pass.draw_model_simple(&scene.model, 0..1);
            }
            ViewMode::Ghosted => {
                pass.set_pipeline(&self.pipelines.ghosted_fill);
                pass.set_bind_group(0, &scene.cam.bind_group, &[]);
                pass.draw_model_simple(&scene.model, 0..1);
                pass.set_pipeline(&self.pipelines.ghosted_wire);
                pass.draw_model_simple(&scene.model, 0..1);
            }
        }

        pass.set_pipeline(&self.pipelines.grid);
        pass.set_bind_group(0, &scene.vis.grid_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.grid_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(scene.vis.grid_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..scene.vis.grid_mesh.num_elements, 0, 0..1);
        self.draw_normals(&mut pass, scene);
    }

    fn draw_shaded_model<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        use model::DrawModel;
        pass.set_pipeline(&self.pipelines.main);
        pass.set_bind_group(3, &scene.shadow.sample_bind_group, &[]);
        pass.draw_model_instanced(&scene.model, 0..1, &scene.cam.bind_group, &scene.light_bind_group);
    }

    fn draw_floor<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        pass.set_pipeline(&self.pipelines.floor);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_bind_group(1, &scene.shadow.sample_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.floor_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(scene.vis.floor_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..scene.vis.floor_mesh.num_elements, 0, 0..1);
    }

    fn draw_normals<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if self.normals_mode == NormalsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.normals);
        if matches!(self.normals_mode, NormalsMode::Face | NormalsMode::FaceAndVertex)
            && scene.vis.face_normals_count > 0
        {
            pass.set_bind_group(0, &scene.vis.face_normals_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.face_normals_buf.slice(..));
            pass.draw(0..scene.vis.face_normals_count, 0..1);
        }
        if matches!(self.normals_mode, NormalsMode::Vertex | NormalsMode::FaceAndVertex)
            && scene.vis.vertex_normals_count > 0
        {
            pass.set_bind_group(0, &scene.vis.vertex_normals_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.vertex_normals_buf.slice(..));
            pass.draw(0..scene.vis.vertex_normals_count, 0..1);
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
