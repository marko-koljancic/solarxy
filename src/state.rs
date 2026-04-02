use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::camera::Camera;
use crate::cgi::camera_state::CameraState;
use crate::cgi::hud::HudRenderer;
use crate::cgi::ibl::{BrdfLut, IblState};
use crate::cgi::light::{LightEntry, LightsUniform};
use crate::cgi::model::{self, Model};
use crate::cgi::pipelines::{Instance, Pipelines};
use crate::cgi::resources::{self, ModelStats};
use crate::cgi::shadow::ShadowState;
use crate::cgi::ssao::SsaoState;
use crate::cgi::visualization::VisualizationState;
use crate::cgi::texture;
use crate::preferences::{
    self, BackgroundMode, IblMode, LineWeight, NormalsMode, Preferences, ProjectionMode, ToneMode,
    UvMode, ViewMode,
};
use cgmath::prelude::*;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Instant;
use wgpu::util::DeviceExt;
use winit::{
    event::MouseButton,
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, ModifiersState},
    window::Window,
};

#[derive(Clone, Copy, PartialEq)]
enum BoundsMode {
    Off,
    WholeModel,
    PerMesh,
}

impl std::fmt::Display for BoundsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoundsMode::Off => write!(f, "Off"),
            BoundsMode::WholeModel => write!(f, "Model"),
            BoundsMode::PerMesh => write!(f, "Per Mesh"),
        }
    }
}

impl BackgroundMode {
    fn clear_color(self) -> wgpu::Color {
        match self {
            Self::White => wgpu::Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            Self::Gradient => wgpu::Color {
                r: 0.165,
                g: 0.165,
                b: 0.180,
                a: 1.0,
            },
            Self::DarkGray => wgpu::Color {
                r: 0.12,
                g: 0.12,
                b: 0.12,
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

    fn wireframe_color(self) -> [f32; 4] {
        let c = self.clear_color();
        let luminance = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        if luminance < 0.3 {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    }

    fn sky_colors(self) -> ([f32; 3], [f32; 3]) {
        match self {
            Self::White => ([1.0, 1.0, 1.0], [0.85, 0.85, 0.85]),
            Self::Gradient => ([0.66, 0.70, 0.72], [0.35, 0.41, 0.47]),
            Self::DarkGray => ([0.30, 0.32, 0.35], [0.15, 0.14, 0.13]),
            Self::Black => ([0.20, 0.22, 0.25], [0.08, 0.07, 0.06]),
        }
    }

    fn grid_color(self) -> [f32; 3] {
        let c = self.clear_color();
        let lum = (0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b) as f32;
        if lum < 0.3 {
            let v = (lum + 0.15).min(1.0);
            [v, v, v]
        } else {
            let v = (lum - 0.20).max(0.0);
            [v, v, v]
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

fn create_light_bind_group(
    device: &wgpu::Device,
    layouts: &BindGroupLayouts,
    light_buffer: &wgpu::Buffer,
    ibl: &IblState,
    brdf_lut: &BrdfLut,
) -> wgpu::BindGroup {
    create_light_bind_group_selective(device, layouts, light_buffer, ibl, ibl, brdf_lut)
}

fn create_light_bind_group_selective(
    device: &wgpu::Device,
    layouts: &BindGroupLayouts,
    light_buffer: &wgpu::Buffer,
    diffuse_src: &IblState,
    specular_src: &IblState,
    brdf_lut: &BrdfLut,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("light_bind_group"),
        layout: &layouts.light,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&diffuse_src.irradiance_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&diffuse_src.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&specular_src.prefiltered_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&specular_src.prefiltered_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&brdf_lut.view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(&brdf_lut.sampler),
            },
        ],
    })
}

impl ModelScene {
    fn new(
        model_path: String,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        config: &wgpu::SurfaceConfiguration,
        initial_grid_color: [f32; 3],
        brdf_lut: &BrdfLut,
    ) -> anyhow::Result<Self> {
        let (model, normals_geo, stats) = resources::load_model_any(
            &model_path,
            device,
            queue,
            &layouts.texture,
            &layouts.edge_geometry,
        )?;

        let cam = CameraState::new(
            device,
            &layouts.camera,
            &model.bounds,
            config.width as f32 / config.height as f32,
        );

        let instance_data = Instance {
            position: cgmath::Vector3::new(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::from_axis_angle(
                cgmath::Vector3::unit_z(),
                cgmath::Deg(0.0),
            ),
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
        let placeholder_ibl = IblState::fallback(device, queue);
        let light_bind_group =
            create_light_bind_group(device, layouts, &light_buffer, &placeholder_ibl, brdf_lut);

        let shadow = ShadowState::new(device, layouts, &lights_uniform, &model);
        let vis = VisualizationState::new(
            device,
            layouts,
            &model,
            &normals_geo,
            &cam.buffer,
            initial_grid_color,
        );

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

struct PendingLoad {
    receiver: mpsc::Receiver<anyhow::Result<ModelScene>>,
    filename: String,
    path: String,
}

const TURNTABLE_SPEED: f32 = std::f32::consts::PI / 6.0;
const BLOOM_THRESHOLD: f32 = 0.8;
const BLOOM_STRENGTH: f32 = 0.8;

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    is_surface_configured: bool,
    depth_texture: texture::Texture,
    msaa_hdr_view: wgpu::TextureView,
    #[allow(unused)]
    hdr_resolve_texture: wgpu::Texture,
    hdr_resolve_view: wgpu::TextureView,
    #[allow(unused)]
    bloom_ping_texture: wgpu::Texture,
    bloom_ping_view: wgpu::TextureView,
    #[allow(unused)]
    bloom_pong_texture: wgpu::Texture,
    bloom_pong_view: wgpu::TextureView,
    bloom_sampler: wgpu::Sampler,
    bloom_params_buffer: wgpu::Buffer,
    bloom_params_bind_group: wgpu::BindGroup,
    bloom_extract_bind_group: wgpu::BindGroup,
    bloom_blur_h_bind_group: wgpu::BindGroup,
    bloom_blur_v_bind_group: wgpu::BindGroup,
    composite_params_buffer: wgpu::Buffer,
    composite_params_bind_group: wgpu::BindGroup,
    composite_bind_group: wgpu::BindGroup,
    bloom_enabled: bool,
    ssao: SsaoState,
    ssao_enabled: bool,
    tone_mode: ToneMode,
    ibl: IblState,
    ibl_fallback: IblState,
    brdf_lut: BrdfLut,
    ibl_mode: IblMode,
    last_active_ibl_mode: IblMode,
    layouts: Arc<BindGroupLayouts>,
    pipelines: Pipelines,
    hud: HudRenderer,
    scene: Option<ModelScene>,
    pending_load: Option<PendingLoad>,
    view_mode: ViewMode,
    prev_non_ghosted_mode: ViewMode,
    ghosted_wireframe: bool,
    normals_mode: NormalsMode,
    background_mode: BackgroundMode,
    _gradient_buffer: wgpu::Buffer,
    gradient_bind_group: wgpu::BindGroup,
    line_weight: LineWeight,
    wireframe_params_buffer: wgpu::Buffer,
    wireframe_params_bind_group: wgpu::BindGroup,
    uv_mode: UvMode,
    bounds_mode: BoundsMode,
    _checker_texture: texture::Texture,
    uv_checker_bind_group: wgpu::BindGroup,
    capture_requested: bool,
    turntable_active: bool,
    show_grid: bool,
    lights_locked: bool,
    show_axis_gizmo: bool,
    modifiers: ModifiersState,
    last_frame_time: Instant,
    dt: f32,
    #[allow(unused)]
    backend_info: String,
    preferences: Preferences,
    msaa_sample_count: u32,
    pub window: Arc<Window>,
}

impl State {
    pub async fn new(
        window: Arc<Window>,
        model_path: Option<String>,
        preferences: Preferences,
    ) -> anyhow::Result<Self> {
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
        let adapter_info = adapter.get_info();
        let backend_info = if adapter_info.driver_info.is_empty() {
            format!("{:?} \u{2014} {}", adapter_info.backend, adapter_info.name)
        } else {
            format!(
                "{:?} \u{2014} {} \u{2014} {}",
                adapter_info.backend, adapter_info.name, adapter_info.driver_info
            )
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
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
        let msaa_sample_count = preferences.rendering.msaa_sample_count;
        let depth_texture = texture::Texture::create_depth_texture(
            &device,
            &config,
            "depth_texture",
            msaa_sample_count,
        );
        let msaa_hdr_view = texture::create_msaa_hdr_texture(
            &device,
            config.width,
            config.height,
            msaa_sample_count,
        );
        let (hdr_resolve_texture, hdr_resolve_view) =
            texture::create_hdr_resolve_texture(&device, config.width, config.height);
        let (bloom_ping_texture, bloom_ping_view) =
            texture::create_bloom_texture(&device, config.width, config.height, "Bloom Ping");
        let (bloom_pong_texture, bloom_pong_view) =
            texture::create_bloom_texture(&device, config.width, config.height, "Bloom Pong");

        let bloom_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Bloom Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let layouts = Arc::new(BindGroupLayouts::new(&device));
        let pipelines = Pipelines::new(&device, &config, &layouts, msaa_sample_count);

        let mut hud = HudRenderer::new(
            &device,
            surface_format,
            size.width,
            size.height,
            None,
            window.scale_factor(),
        );
        hud.set_backend_info(backend_info.clone());

        let gradient_data: [f32; 8] = [0.35, 0.41, 0.47, 1.0, 0.66, 0.70, 0.72, 1.0];
        let gradient_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Gradient Uniform"),
            contents: bytemuck::cast_slice(&gradient_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let gradient_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gradient Bind Group"),
            layout: &layouts.background,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: gradient_buffer.as_entire_binding(),
            }],
        });

        let background_mode = preferences.display.background;

        let brdf_lut = BrdfLut::generate(&device, &queue);
        let (ibl_top, ibl_bottom) = background_mode.sky_colors();
        let ibl = IblState::from_sky_colors(&device, &queue, ibl_top, ibl_bottom);
        let ibl_fallback = IblState::fallback(&device, &queue);

        let wire_color = background_mode.wireframe_color();

        let line_weight = preferences.rendering.wireframe_line_weight;
        let wireframe_params_data: [f32; 8] = [
            wire_color[0],
            wire_color[1],
            wire_color[2],
            wire_color[3],
            line_weight.width_px(),
            size.width as f32,
            size.height as f32,
            0.0,
        ];
        let wireframe_params_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Wireframe Params Uniform"),
                contents: bytemuck::cast_slice(&wireframe_params_data),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let wireframe_params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Wireframe Params Bind Group"),
            layout: &layouts.wireframe_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wireframe_params_buffer.as_entire_binding(),
            }],
        });

        let checker_texture = texture::Texture::from_bytes(
            &device,
            &queue,
            include_bytes!("../res/textures/uv-checker_1k.png"),
            "uv_checker_texture",
            false,
        )?;
        let checker_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("UV Checker Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uv_checker_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UV Checker Bind Group"),
            layout: &layouts.uv_checker,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&checker_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&checker_sampler),
                },
            ],
        });

        let bloom_params_data: [f32; 4] = [BLOOM_THRESHOLD, BLOOM_STRENGTH, 0.0, 0.0];
        let bloom_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom Params Uniform"),
            contents: bytemuck::cast_slice(&bloom_params_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bloom_params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom Params Bind Group"),
            layout: &layouts.bloom_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: bloom_params_buffer.as_entire_binding(),
            }],
        });

        let bloom_extract_bind_group = create_bloom_sample_bind_group(
            &device,
            &layouts.bloom_texture,
            &hdr_resolve_view,
            &bloom_sampler,
        );
        let bloom_blur_h_bind_group = create_bloom_sample_bind_group(
            &device,
            &layouts.bloom_texture,
            &bloom_ping_view,
            &bloom_sampler,
        );
        let bloom_blur_v_bind_group = create_bloom_sample_bind_group(
            &device,
            &layouts.bloom_texture,
            &bloom_pong_view,
            &bloom_sampler,
        );

        let composite_params_data: [u8; 20] = build_composite_params(
            preferences.display.bloom_enabled,
            preferences.display.ssao_enabled,
            preferences.display.tone_mode,
        );
        let composite_params_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Composite Params Uniform"),
                contents: &composite_params_data,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let composite_params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Composite Params Bind Group"),
            layout: &layouts.composite_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: composite_params_buffer.as_entire_binding(),
            }],
        });

        let composite_bind_group = create_composite_bind_group(
            &device,
            &layouts.composite,
            &hdr_resolve_view,
            &bloom_ping_view,
            &bloom_sampler,
        );

        let dummy_camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Dummy Camera Buffer for SSAO"),
            contents: &[0u8; 288],
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let ssao = SsaoState::new(
            &device,
            &queue,
            &layouts,
            &dummy_camera_buffer,
            config.width,
            config.height,
        );

        let mut state = Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            depth_texture,
            msaa_hdr_view,
            hdr_resolve_texture,
            hdr_resolve_view,
            bloom_ping_texture,
            bloom_ping_view,
            bloom_pong_texture,
            bloom_pong_view,
            bloom_sampler,
            bloom_params_buffer,
            bloom_params_bind_group,
            bloom_extract_bind_group,
            bloom_blur_h_bind_group,
            bloom_blur_v_bind_group,
            composite_params_buffer,
            composite_params_bind_group,
            composite_bind_group,
            bloom_enabled: preferences.display.bloom_enabled,
            ssao,
            ssao_enabled: preferences.display.ssao_enabled,
            tone_mode: preferences.display.tone_mode,
            ibl,
            ibl_fallback,
            brdf_lut,
            ibl_mode: preferences.display.ibl_mode,
            last_active_ibl_mode: match preferences.display.ibl_mode {
                IblMode::Off => IblMode::Full,
                other => other,
            },
            layouts,
            pipelines,
            hud,
            scene: None,
            pending_load: None,
            view_mode: preferences.display.view_mode,
            prev_non_ghosted_mode: ViewMode::Shaded,
            ghosted_wireframe: false,
            normals_mode: preferences.display.normals_mode,
            background_mode,
            _gradient_buffer: gradient_buffer,
            gradient_bind_group,
            line_weight,
            wireframe_params_buffer,
            wireframe_params_bind_group,
            uv_mode: preferences.display.uv_mode,
            bounds_mode: BoundsMode::Off,
            _checker_texture: checker_texture,
            uv_checker_bind_group,
            capture_requested: false,
            turntable_active: preferences.display.turntable_active,
            show_grid: preferences.display.grid_visible,
            lights_locked: preferences.lighting.lock,
            show_axis_gizmo: preferences.display.axis_gizmo_visible,
            modifiers: ModifiersState::empty(),
            last_frame_time: Instant::now(),
            dt: 0.0,
            backend_info,
            preferences,
            msaa_sample_count,
            window,
        };

        if let Some(path) = model_path {
            state.spawn_load(path);
        }

        Ok(state)
    }

    pub fn handle_dropped_file(&mut self, path: PathBuf) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("hdr") || ext.eq_ignore_ascii_case("exr") {
                match IblState::from_hdri(&self.device, &self.queue, &path) {
                    Ok(new_ibl) => {
                        self.ibl = new_ibl;
                        self.ibl_mode = IblMode::Full;
                        self.last_active_ibl_mode = IblMode::Full;
                        self.rebuild_light_bind_group();
                        self.hud.set_toast("HDRI loaded", [0.0, 0.4, 0.0, 1.0]);
                    }
                    Err(e) => {
                        self.hud
                            .set_toast(&format!("HDRI error: {}", e), [0.6, 0.0, 0.0, 1.0]);
                    }
                }
                return;
            }
        }

        if !resources::is_supported_model_extension(&path) {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("none");
            self.hud.set_toast(
                &format!("Unsupported format: .{}", ext),
                [0.6, 0.0, 0.0, 1.0],
            );
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

        self.spawn_load(model_path);
    }

    fn active_ibl(&self) -> &IblState {
        match self.ibl_mode {
            IblMode::Off => &self.ibl_fallback,
            IblMode::Diffuse | IblMode::Full => &self.ibl,
        }
    }

    fn rebuild_light_bind_group(&mut self) {
        if let Some(scene) = &mut self.scene {
            scene.light_bind_group = match self.ibl_mode {
                IblMode::Off => create_light_bind_group(
                    &self.device,
                    &self.layouts,
                    &scene.light_buffer,
                    &self.ibl_fallback,
                    &self.brdf_lut,
                ),
                IblMode::Diffuse => create_light_bind_group_selective(
                    &self.device,
                    &self.layouts,
                    &scene.light_buffer,
                    &self.ibl,
                    &self.ibl_fallback,
                    &self.brdf_lut,
                ),
                IblMode::Full => create_light_bind_group(
                    &self.device,
                    &self.layouts,
                    &scene.light_buffer,
                    &self.ibl,
                    &self.brdf_lut,
                ),
            };
        }
    }

    fn update_wireframe_params(&self) {
        let color = self.background_mode.wireframe_color();
        let data: [f32; 8] = [
            color[0],
            color[1],
            color[2],
            color[3],
            self.line_weight.width_px(),
            self.config.width as f32,
            self.config.height as f32,
            0.0,
        ];
        self.queue.write_buffer(
            &self.wireframe_params_buffer,
            0,
            bytemuck::cast_slice(&data),
        );
    }

    fn spawn_load(&mut self, model_path: String) {
        let filename = std::path::Path::new(&model_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&model_path)
            .to_string();

        self.hud
            .set_loading_message(&format!("Loading {}...", filename));

        let device = self.device.clone();
        let queue = self.queue.clone();
        let layouts = Arc::clone(&self.layouts);
        let config = self.config.clone();
        let initial_grid_color = self.background_mode.grid_color();
        let path = model_path.clone();

        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let placeholder_brdf = BrdfLut::fallback(&device, &queue);
            let result = ModelScene::new(
                model_path,
                &device,
                &queue,
                &layouts,
                &config,
                initial_grid_color,
                &placeholder_brdf,
            );
            let _ = tx.send(result);
        });

        self.pending_load = Some(PendingLoad {
            receiver: rx,
            filename,
            path,
        });
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.depth_texture = texture::Texture::create_depth_texture(
                &self.device,
                &self.config,
                "depth_texture",
                self.msaa_sample_count,
            );
            self.msaa_hdr_view = texture::create_msaa_hdr_texture(
                &self.device,
                width,
                height,
                self.msaa_sample_count,
            );
            let (hdr_tex, hdr_view) =
                texture::create_hdr_resolve_texture(&self.device, width, height);
            self.hdr_resolve_texture = hdr_tex;
            self.hdr_resolve_view = hdr_view;
            let (ping_tex, ping_view) =
                texture::create_bloom_texture(&self.device, width, height, "Bloom Ping");
            self.bloom_ping_texture = ping_tex;
            self.bloom_ping_view = ping_view;
            let (pong_tex, pong_view) =
                texture::create_bloom_texture(&self.device, width, height, "Bloom Pong");
            self.bloom_pong_texture = pong_tex;
            self.bloom_pong_view = pong_view;
            self.bloom_extract_bind_group = create_bloom_sample_bind_group(
                &self.device,
                &self.layouts.bloom_texture,
                &self.hdr_resolve_view,
                &self.bloom_sampler,
            );
            self.bloom_blur_h_bind_group = create_bloom_sample_bind_group(
                &self.device,
                &self.layouts.bloom_texture,
                &self.bloom_ping_view,
                &self.bloom_sampler,
            );
            self.bloom_blur_v_bind_group = create_bloom_sample_bind_group(
                &self.device,
                &self.layouts.bloom_texture,
                &self.bloom_pong_view,
                &self.bloom_sampler,
            );
            self.composite_bind_group = create_composite_bind_group(
                &self.device,
                &self.layouts.composite,
                &self.hdr_resolve_view,
                &self.bloom_ping_view,
                &self.bloom_sampler,
            );
            if let Some(scene) = &self.scene {
                self.ssao.resize(
                    &self.device,
                    &self.layouts,
                    &scene.cam.buffer,
                    width,
                    height,
                );
            } else {
                let dummy_buf = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Dummy Camera Buffer for SSAO resize"),
                        contents: &[0u8; 288],
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                self.ssao
                    .resize(&self.device, &self.layouts, &dummy_buf, width, height);
            }
            self.is_surface_configured = true;
            self.hud.resize(width, height, &self.queue);
            self.hud.set_scale_factor(self.window.scale_factor());
            self.update_wireframe_params();
            if let Some(scene) = &mut self.scene {
                scene.cam.resize(width as f32 / height as f32);
            }
        }
    }

    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
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
                if self.modifiers.shift_key() {
                    self.tone_mode = self.tone_mode.next();
                    self.write_composite_params();
                    let msg = format!("Tone: {}", self.tone_mode);
                    self.hud.set_toast(&msg, [0.0, 0.4, 0.0, 1.0]);
                } else if let Some(scene) = &mut self.scene {
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
                if self.modifiers.shift_key() {
                    self.lights_locked = !self.lights_locked;
                    let msg = if self.lights_locked {
                        "Lights locked"
                    } else {
                        "Lights unlocked"
                    };
                    self.hud.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
                } else if let Some(scene) = &mut self.scene {
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
                if self.modifiers.shift_key() {
                    self.line_weight = self.line_weight.next();
                    self.update_wireframe_params();
                    self.hud.set_toast(
                        &format!("Line Weight: {}", self.line_weight),
                        [0.0, 0.4, 0.0, 1.0],
                    );
                } else if self.view_mode == ViewMode::Ghosted {
                    self.ghosted_wireframe = !self.ghosted_wireframe;
                } else {
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
                    self.ghosted_wireframe = matches!(
                        self.view_mode,
                        ViewMode::ShadedWireframe | ViewMode::WireframeOnly
                    );
                    self.view_mode = ViewMode::Ghosted;
                }
            }
            KeyCode::KeyS => {
                if self.modifiers.shift_key() {
                    self.save_preferences();
                } else {
                    self.view_mode = ViewMode::Shaded;
                }
            }
            KeyCode::KeyC => {
                if self.scene.is_some() {
                    self.capture_requested = true;
                }
            }
            KeyCode::KeyA => {
                if self.modifiers.shift_key() {
                    self.ssao_enabled = !self.ssao_enabled;
                    self.write_composite_params();
                    let msg = if self.ssao_enabled {
                        "SSAO: On"
                    } else {
                        "SSAO: Off"
                    };
                    self.hud.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
                } else {
                    self.show_axis_gizmo = !self.show_axis_gizmo;
                }
            }
            KeyCode::KeyG => self.show_grid = !self.show_grid,
            KeyCode::KeyI => {
                if self.modifiers.shift_key() {
                    if self.ibl_mode != IblMode::Off {
                        self.ibl_mode = match self.ibl_mode {
                            IblMode::Diffuse => IblMode::Full,
                            IblMode::Full => IblMode::Diffuse,
                            IblMode::Off => unreachable!(),
                        };
                        self.last_active_ibl_mode = self.ibl_mode;
                    }
                } else if self.ibl_mode == IblMode::Off {
                    self.ibl_mode = self.last_active_ibl_mode;
                } else {
                    self.last_active_ibl_mode = self.ibl_mode;
                    self.ibl_mode = IblMode::Off;
                }
                self.rebuild_light_bind_group();
                let msg = match self.ibl_mode {
                    IblMode::Off => "IBL: Off",
                    IblMode::Diffuse => "IBL: Diffuse",
                    IblMode::Full => "IBL: Full",
                };
                self.hud.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
            }
            KeyCode::KeyB => {
                if self.modifiers.shift_key() {
                    let is_multi = self
                        .scene
                        .as_ref()
                        .is_some_and(|s| s.model.meshes.len() > 1);
                    self.bounds_mode = match self.bounds_mode {
                        BoundsMode::Off => BoundsMode::WholeModel,
                        BoundsMode::WholeModel if is_multi => BoundsMode::PerMesh,
                        BoundsMode::WholeModel | BoundsMode::PerMesh => BoundsMode::Off,
                    };
                    let msg = match self.bounds_mode {
                        BoundsMode::Off => "Bounds: Off",
                        BoundsMode::WholeModel => "Bounds: Whole Model",
                        BoundsMode::PerMesh => "Bounds: Per Mesh",
                    };
                    self.hud.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
                } else {
                    self.background_mode = self.background_mode.next();
                    self.update_wireframe_params();
                    self.update_grid_color();
                    let (top, bottom) = self.background_mode.sky_colors();
                    self.ibl = IblState::from_sky_colors(&self.device, &self.queue, top, bottom);
                    self.rebuild_light_bind_group();
                }
            }
            KeyCode::KeyM => {
                if self.modifiers.shift_key() {
                    self.bloom_enabled = !self.bloom_enabled;
                    self.write_composite_params();
                    let msg = if self.bloom_enabled {
                        "Bloom: On"
                    } else {
                        "Bloom: Off"
                    };
                    self.hud.set_toast(msg, [0.0, 0.4, 0.0, 1.0]);
                }
            }
            KeyCode::KeyN => {
                self.normals_mode = match self.normals_mode {
                    NormalsMode::Off => NormalsMode::Face,
                    NormalsMode::Face => NormalsMode::Vertex,
                    NormalsMode::Vertex => NormalsMode::FaceAndVertex,
                    NormalsMode::FaceAndVertex => NormalsMode::Off,
                };
            }
            KeyCode::KeyV => self.turntable_active = !self.turntable_active,
            KeyCode::KeyU => {
                self.uv_mode = match self.uv_mode {
                    UvMode::Off => UvMode::Gradient,
                    UvMode::Gradient => UvMode::Checker,
                    UvMode::Checker => UvMode::Off,
                };
            }
            _ => {
                if let Some(scene) = &mut self.scene {
                    scene.cam.handle_key(code, is_pressed);
                }
            }
        }
    }

    fn save_preferences(&mut self) {
        self.preferences.display.background = self.background_mode;
        self.preferences.display.view_mode = self.view_mode;
        self.preferences.display.normals_mode = self.normals_mode;
        self.preferences.display.grid_visible = self.show_grid;
        self.preferences.display.axis_gizmo_visible = self.show_axis_gizmo;
        self.preferences.display.bloom_enabled = self.bloom_enabled;
        self.preferences.display.ssao_enabled = self.ssao_enabled;
        self.preferences.display.uv_mode = self.uv_mode;
        self.preferences.display.turntable_active = self.turntable_active;
        if let Some(scene) = &self.scene {
            self.preferences.display.projection_mode = scene.cam.camera.projection;
        }
        self.preferences.rendering.wireframe_line_weight = self.line_weight;
        self.preferences.lighting.lock = self.lights_locked;
        self.preferences.display.ibl_mode = self.ibl_mode;
        self.preferences.display.tone_mode = self.tone_mode;

        match preferences::save(&self.preferences) {
            Ok(()) => self
                .hud
                .set_toast("Preferences saved", [0.0, 0.4, 0.0, 1.0]),
            Err(e) => self
                .hud
                .set_toast(&format!("Save failed: {}", e), [0.6, 0.0, 0.0, 1.0]),
        }
    }

    fn update_grid_color(&self) {
        if let Some(scene) = &self.scene {
            let color = self.background_mode.grid_color();
            self.queue
                .write_buffer(&scene.vis.grid_uniform_buf, 4, bytemuck::cast_slice(&color));
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
        let poll = self.pending_load.as_ref().map(|p| p.receiver.try_recv());
        match poll {
            Some(Ok(Ok(mut new_scene))) => {
                let pending = self.pending_load.take().unwrap();
                let active_ibl = self.active_ibl();
                new_scene.light_bind_group = create_light_bind_group(
                    &self.device,
                    &self.layouts,
                    &new_scene.light_buffer,
                    active_ibl,
                    &self.brdf_lut,
                );
                self.hud.update_stats(Some(&new_scene.stats));
                self.hud
                    .update_model_info(&pending.filename, new_scene.model.meshes.len());
                self.hud.clear_loading_message();
                self.window
                    .set_title(&format!("Solarxy \u{2014} {}", pending.filename));
                preferences::add_recent_file(&mut self.preferences, &pending.path);
                self.scene = Some(new_scene);
                if let Some(scene) = &self.scene {
                    self.ssao
                        .rebuild_bind_groups(&self.device, &self.layouts, &scene.cam.buffer);
                }
                if let Some(scene) = &mut self.scene {
                    scene
                        .cam
                        .resize(self.config.width as f32 / self.config.height as f32);
                    scene
                        .cam
                        .set_projection(self.preferences.display.projection_mode);
                }
                self.view_mode = self.preferences.display.view_mode;
                self.prev_non_ghosted_mode = ViewMode::Shaded;
                self.ghosted_wireframe = false;
                self.normals_mode = self.preferences.display.normals_mode;
                self.uv_mode = self.preferences.display.uv_mode;
                self.turntable_active = self.preferences.display.turntable_active;
            }
            Some(Ok(Err(e))) => {
                self.pending_load.take();
                self.hud.clear_loading_message();
                self.hud
                    .set_toast(&format!("Failed to load: {}", e), [0.6, 0.0, 0.0, 1.0]);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.pending_load.take();
                self.hud.clear_loading_message();
                self.hud
                    .set_toast("Loading thread crashed", [0.6, 0.0, 0.0, 1.0]);
            }
            _ => {}
        }

        let now = Instant::now();
        self.dt = (now - self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        if let Some(scene) = &mut self.scene {
            if self.turntable_active && !scene.cam.is_orbiting() {
                scene.cam.inject_orbit_yaw(TURNTABLE_SPEED * self.dt);
            }
            scene.cam.update(&self.queue, self.dt);

            if !self.lights_locked {
                scene.lights_uniform = lights_from_camera(&scene.cam.camera, &scene.model.bounds);
                self.queue.write_buffer(
                    &scene.light_buffer,
                    0,
                    bytemuck::cast_slice(&[scene.lights_uniform]),
                );

                let key_pos = scene.lights_uniform.lights[0].position;
                scene.shadow.update_light_vp(
                    &self.queue,
                    cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
                    scene.model.bounds.center(),
                    scene.model.bounds.diagonal() / 2.0,
                );
            }
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
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let has_model = self.scene.is_some();

        if let Some(scene) = &self.scene {
            self.render_shadow_pass(&mut encoder, scene);
            if self.ssao_enabled {
                self.render_gbuffer_pass(&mut encoder, scene);
            }
            self.render_main_pass(&mut encoder, scene);
        } else {
            self.render_empty_pass(&mut encoder);
        }

        if self.ssao_enabled {
            self.render_ssao_passes(&mut encoder);
        }

        if self.bloom_enabled {
            self.render_bloom_passes(&mut encoder);
        }
        self.render_composite_pass(&mut encoder, &view);

        let (projection_str, normals_str) = if let Some(scene) = &self.scene {
            (
                scene.cam.camera.projection.to_string(),
                self.normals_mode.to_string(),
            )
        } else {
            (String::new(), String::new())
        };

        let mode_str = if self.uv_mode != UvMode::Off {
            format!("{} [UV: {}]", self.view_mode, self.uv_mode)
        } else if self.view_mode == ViewMode::Ghosted && self.ghosted_wireframe {
            "Ghosted+Wire".to_string()
        } else {
            self.view_mode.to_string()
        };

        let bounds_info = match self.bounds_mode {
            BoundsMode::Off => String::new(),
            BoundsMode::WholeModel => {
                if let Some(scene) = &self.scene {
                    let s = scene.model.bounds.size();
                    format!(
                        "Extents: {:.3} \u{00d7} {:.3} \u{00d7} {:.3}",
                        s.x, s.y, s.z
                    )
                } else {
                    String::new()
                }
            }
            BoundsMode::PerMesh => {
                if let Some(scene) = &self.scene {
                    let mut lines = Vec::new();
                    for (i, mesh) in scene.model.meshes.iter().enumerate() {
                        let s = scene.model.mesh_bounds[i].size();
                        let name = if mesh.name.len() > 20 {
                            format!("{}\u{2026}", &mesh.name[..19])
                        } else {
                            mesh.name.clone()
                        };
                        lines.push(format!(
                            "{}: {:.3} \u{00d7} {:.3} \u{00d7} {:.3}",
                            name, s.x, s.y, s.z
                        ));
                    }
                    lines.join("\n")
                } else {
                    String::new()
                }
            }
        };

        self.hud.render(
            &self.device,
            &mut encoder,
            &view,
            &self.queue,
            self.config.width,
            self.config.height,
            &mode_str,
            &projection_str,
            &normals_str,
            &self.background_mode.to_string(),
            self.background_mode.clear_color(),
            frame_ms,
            has_model,
            self.show_grid,
            self.lights_locked,
            self.show_axis_gizmo,
            &self.bounds_mode.to_string(),
            &bounds_info,
            &self.line_weight.to_string(),
            &self.ibl_mode.to_string(),
            self.ssao_enabled,
            &self.tone_mode.to_string(),
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

    fn save_capture(
        &mut self,
        buffer: wgpu::Buffer,
        padded_row_bytes: u32,
        width: u32,
        height: u32,
    ) {
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

        let img = image::RgbaImage::from_raw(width, height, pixels)
            .expect("Failed to create image from pixel data");
        if let Err(e) = img.save(&filename) {
            eprintln!("Failed to save screenshot: {}", e);
        } else {
            self.hud.set_capture_message(filename);
        }
    }

    fn draw_background_gradient<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipelines.background);
        pass.set_bind_group(0, &self.gradient_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn render_empty_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Empty Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.msaa_hdr_view,
                resolve_target: Some(&self.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.background_mode.clear_color()),
                    store: wgpu::StoreOp::Discard,
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
        if self.background_mode == BackgroundMode::Gradient {
            self.draw_background_gradient(&mut pass);
        }
    }

    fn render_bloom_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        let full_texel: [f32; 4] = [
            BLOOM_THRESHOLD,
            BLOOM_STRENGTH,
            1.0 / self.config.width.max(1) as f32,
            1.0 / self.config.height.max(1) as f32,
        ];
        self.queue.write_buffer(
            &self.bloom_params_buffer,
            0,
            bytemuck::cast_slice(&full_texel),
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Extract Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.bloom_ping_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.bloom_extract);
            pass.set_bind_group(0, &self.bloom_extract_bind_group, &[]);
            pass.set_bind_group(1, &self.bloom_params_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let half_w = (self.config.width / 2).max(1) as f32;
        let half_h = (self.config.height / 2).max(1) as f32;
        let half_texel: [f32; 4] = [BLOOM_THRESHOLD, BLOOM_STRENGTH, 1.0 / half_w, 1.0 / half_h];
        self.queue.write_buffer(
            &self.bloom_params_buffer,
            0,
            bytemuck::cast_slice(&half_texel),
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Blur H Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.bloom_pong_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.bloom_blur_h);
            pass.set_bind_group(0, &self.bloom_blur_h_bind_group, &[]);
            pass.set_bind_group(1, &self.bloom_params_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom Blur V Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.bloom_ping_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.bloom_blur_v);
            pass.set_bind_group(0, &self.bloom_blur_v_bind_group, &[]);
            pass.set_bind_group(1, &self.bloom_params_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn render_composite_pass(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Composite Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines.composite);
        pass.set_bind_group(0, &self.composite_bind_group, &[]);
        pass.set_bind_group(1, &self.composite_params_bind_group, &[]);
        if self.ssao_enabled {
            pass.set_bind_group(2, &self.ssao.read_bind_group, &[]);
        } else {
            pass.set_bind_group(2, &self.ssao.read_off_bind_group, &[]);
        }
        pass.draw(0..3, 0..1);
    }

    fn write_composite_params(&self) {
        let buf = build_composite_params(self.bloom_enabled, self.ssao_enabled, self.tone_mode);
        self.queue
            .write_buffer(&self.composite_params_buffer, 0, &buf);
    }

    fn render_gbuffer_pass(&self, encoder: &mut wgpu::CommandEncoder, scene: &ModelScene) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("G-Buffer Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.ssao.gbuffer_normal_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.5,
                        g: 0.5,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.ssao.gbuffer_depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines.gbuffer);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
        for mesh in &scene.model.meshes {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode == 2 {
                continue;
            }
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
        }
    }

    fn render_ssao_passes(&self, encoder: &mut wgpu::CommandEncoder) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ssao.ssao_raw_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.ssao);
            pass.set_bind_group(0, &self.ssao.ssao_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO Blur H Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ssao.ssao_blur_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.ssao_blur_h);
            pass.set_bind_group(0, &self.ssao.blur_h_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO Blur V Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ssao.ssao_output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.ssao_blur_v);
            pass.set_bind_group(0, &self.ssao.blur_v_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
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
        for mesh in &scene.model.meshes {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode == 2 {
                continue;
            }
            pass.set_bind_group(1, &material.bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
        }
    }

    fn render_main_pass(&self, encoder: &mut wgpu::CommandEncoder, scene: &ModelScene) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.msaa_hdr_view,
                resolve_target: Some(&self.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.background_mode.clear_color()),
                    store: wgpu::StoreOp::Discard,
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

        if self.background_mode == BackgroundMode::Gradient {
            self.draw_background_gradient(&mut pass);
        }

        use model::DrawMeshSimple;
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));

        if self.uv_mode != UvMode::Off {
            pass.set_bind_group(0, &scene.cam.bind_group, &[]);
            if !scene.model.has_uvs {
                pass.set_pipeline(&self.pipelines.uv_no_uvs);
            } else {
                match self.uv_mode {
                    UvMode::Gradient => {
                        pass.set_pipeline(&self.pipelines.uv_gradient);
                    }
                    UvMode::Checker => {
                        pass.set_pipeline(&self.pipelines.uv_checker);
                        pass.set_bind_group(1, &self.uv_checker_bind_group, &[]);
                    }
                    UvMode::Off => unreachable!(),
                }
            }
            pass.draw_model_simple(&scene.model, 0..1);

            match self.view_mode {
                ViewMode::Shaded => {}
                ViewMode::ShadedWireframe | ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire);
                }
                ViewMode::Ghosted => {
                    if self.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            scene,
                            &self.pipelines.edge_wire_ghosted,
                        );
                    }
                }
            }
        } else {
            match self.view_mode {
                ViewMode::Shaded | ViewMode::ShadedWireframe => {
                    self.draw_opaque_meshes(&mut pass, scene);
                    self.draw_floor(&mut pass, scene);
                    if self.view_mode == ViewMode::ShadedWireframe {
                        self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire);
                    }
                    self.draw_blend_meshes(&mut pass, scene);
                }
                ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire);
                }
                ViewMode::Ghosted => {
                    pass.set_pipeline(&self.pipelines.ghosted_fill);
                    pass.set_bind_group(0, &scene.cam.bind_group, &[]);
                    pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
                    pass.draw_model_simple(&scene.model, 0..1);
                    if self.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            scene,
                            &self.pipelines.edge_wire_ghosted,
                        );
                    }
                }
            }
        }

        if self.show_grid {
            pass.set_pipeline(&self.pipelines.grid);
            pass.set_bind_group(0, &scene.vis.grid_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.grid_mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(
                scene.vis.grid_mesh.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..scene.vis.grid_mesh.num_elements, 0, 0..1);
        }
        self.draw_normals(&mut pass, scene);
        self.draw_axes(&mut pass, scene);
        self.draw_bounds(&mut pass, scene);
    }

    fn draw_opaque_meshes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        use model::DrawModel;
        pass.set_pipeline(&self.pipelines.main);
        pass.set_bind_group(3, &scene.shadow.sample_bind_group, &[]);
        for mesh in &scene.model.meshes {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode == 2 {
                continue;
            }
            pass.draw_mesh_instanced(
                mesh,
                material,
                0..1,
                &scene.cam.bind_group,
                &scene.light_bind_group,
            );
        }
    }

    fn draw_blend_meshes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        use model::DrawModel;

        let forward = (scene.cam.camera.target - scene.cam.camera.eye).normalize();
        let eye = scene.cam.camera.eye;

        let mut blend_list: Vec<(usize, f32)> = Vec::new();
        for (i, mesh) in scene.model.meshes.iter().enumerate() {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode != 2 {
                continue;
            }
            let center = scene.model.mesh_bounds[i].center();
            let to_center = center - eye;
            let depth = to_center.dot(forward);
            blend_list.push((i, depth));
        }

        if blend_list.is_empty() {
            return;
        }

        blend_list
            .sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        pass.set_pipeline(&self.pipelines.alpha_blend);
        pass.set_bind_group(3, &scene.shadow.sample_bind_group, &[]);
        for (idx, _) in &blend_list {
            let mesh = &scene.model.meshes[*idx];
            let material = &scene.model.materials[mesh.material];
            pass.draw_mesh_instanced(
                mesh,
                material,
                0..1,
                &scene.cam.bind_group,
                &scene.light_bind_group,
            );
        }
    }

    fn draw_edge_wireframe<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        pipeline: &'a wgpu::RenderPipeline,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_bind_group(1, &self.wireframe_params_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.instance_buffer.slice(..));
        for mesh in &scene.model.meshes {
            if let Some(edge) = &mesh.edge_data {
                pass.set_bind_group(2, &edge.bind_group, &[]);
                pass.draw(0..edge.num_edges * 6, 0..1);
            }
        }
    }

    fn draw_floor<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        pass.set_pipeline(&self.pipelines.floor);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_bind_group(1, &scene.shadow.sample_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.floor_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(
            scene.vis.floor_mesh.index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..scene.vis.floor_mesh.num_elements, 0, 0..1);
    }

    fn draw_axes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if !self.show_axis_gizmo {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.axes_vertex_buf.slice(..));
        pass.draw(0..6, 0..1);
    }

    fn draw_bounds<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if self.bounds_mode == BoundsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        match self.bounds_mode {
            BoundsMode::Off => unreachable!(),
            BoundsMode::WholeModel => {
                pass.set_vertex_buffer(0, scene.vis.bounds_whole_buf.slice(..));
                pass.draw(0..scene.vis.bounds_whole_count, 0..1);
            }
            BoundsMode::PerMesh => {
                if scene.vis.bounds_per_mesh_count > 0 {
                    pass.set_vertex_buffer(0, scene.vis.bounds_per_mesh_buf.slice(..));
                    pass.draw(0..scene.vis.bounds_per_mesh_count, 0..1);
                }
            }
        }
    }

    fn draw_normals<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if self.normals_mode == NormalsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.normals);
        if matches!(
            self.normals_mode,
            NormalsMode::Face | NormalsMode::FaceAndVertex
        ) && scene.vis.face_normals_count > 0
        {
            pass.set_bind_group(0, &scene.vis.face_normals_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.face_normals_buf.slice(..));
            pass.draw(0..scene.vis.face_normals_count, 0..1);
        }
        if matches!(
            self.normals_mode,
            NormalsMode::Vertex | NormalsMode::FaceAndVertex
        ) && scene.vis.vertex_normals_count > 0
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

fn create_bloom_sample_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Bloom Sample Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

const SSAO_STRENGTH: f32 = 0.8;

fn build_composite_params(
    bloom_enabled: bool,
    ssao_enabled: bool,
    tone_mode: ToneMode,
) -> [u8; 20] {
    let mut buf = [0u8; 20];
    buf[0..4].copy_from_slice(&BLOOM_STRENGTH.to_le_bytes());
    let bloom_flag: u32 = if bloom_enabled { 1 } else { 0 };
    buf[4..8].copy_from_slice(&bloom_flag.to_le_bytes());
    let ssao_flag: u32 = if ssao_enabled { 1 } else { 0 };
    buf[8..12].copy_from_slice(&ssao_flag.to_le_bytes());
    buf[12..16].copy_from_slice(&SSAO_STRENGTH.to_le_bytes());
    buf[16..20].copy_from_slice(&tone_mode.as_u32().to_le_bytes());
    buf
}

fn create_composite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    scene_view: &wgpu::TextureView,
    bloom_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Composite Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(bloom_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
