//! Startup wiring: creates the wgpu surface, device, queue, [`Renderer`],
//! egui state, and initial preferences. Called from [`crate::app::App::resumed`]
//! on first window creation.

use std::sync::Arc;

use wgpu::util::DeviceExt;
use winit::window::Window;

use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::bloom::BloomState;
use solarxy_renderer::composite::CompositeState;
use solarxy_renderer::ibl::{BrdfLut, IblState};
use solarxy_renderer::pipelines::Pipelines;
use solarxy_renderer::ssao::SsaoState;
use solarxy_renderer::texture::{self, SharedSamplers};

use super::*;

impl State {
    pub async fn new(
        window: Arc<Window>,
        model_path: Option<String>,
        preferences: Preferences,
        console_buffer: crate::console::LogBuffer,
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
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
            view_formats: vec![surface_format.remove_srgb_suffix()],
            desired_maximum_frame_latency: 2,
        };
        let msaa_sample_count = preferences.rendering.msaa_sample_count;
        let depth_texture = texture::Texture::create_depth_texture(
            &device,
            config.width,
            config.height,
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
        let layouts = Arc::new(BindGroupLayouts::new(&device));
        let pipelines = Pipelines::new(&device, &config, &layouts, msaa_sample_count);

        let mut gui = EguiRenderer::new(&device, surface_format, &window, console_buffer);
        gui.set_backend_info(backend_info.clone());

        let gradient_uniform = GradientUniform {
            top_color: [0.35, 0.41, 0.47, 1.0],
            bottom_color: [0.66, 0.70, 0.72, 1.0],
            uv_y_offset: 0.0,
            uv_y_scale: 1.0,
            _pad: [0.0; 2],
        };
        let gradient_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Gradient Uniform"),
            contents: bytemuck::bytes_of(&gradient_uniform),
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
        let wireframe_params_data = WireframeParams {
            color: wire_color,
            line_width: line_weight.width_px(),
            screen_width: size.width as f32,
            screen_height: size.height as f32,
            _pad: 0.0,
        };
        let wireframe_params_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Wireframe Params Uniform"),
                contents: bytemuck::bytes_of(&wireframe_params_data),
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

        let shared_samplers = SharedSamplers::new(&device);

        let checker_texture = texture::Texture::from_bytes(
            &device,
            &queue,
            include_bytes!("../../../../res/textures/uv-checker_1k.png"),
            "uv_checker_texture",
            false,
        )?;
        let uv_checker_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UßV Checker Bind Group"),
            layout: &layouts.uv_checker,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&checker_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&shared_samplers.linear_repeat),
                },
            ],
        });

        let bloom = BloomState::new(
            &device,
            &layouts,
            &hdr_resolve_view,
            shared_samplers.linear_clamp.clone(),
            config.width,
            config.height,
        );

        let composite = CompositeState::new(
            &device,
            &layouts,
            &hdr_resolve_view,
            &bloom.ping_view,
            &bloom.sampler,
            preferences.display.bloom_enabled,
            preferences.display.ssao_enabled,
            preferences.display.tone_mode,
            preferences.display.exposure,
        );

        let ssao = SsaoState::new(&device, &queue, &layouts, config.width, config.height);

        let uv_cam = solarxy_renderer::uv_camera::UvCameraState::new(&device, &layouts.camera);

        let yellow = [1.0, 0.85, 0.0];
        let boundary_verts: [solarxy_renderer::model::GizmoVertex; 8] = [
            solarxy_renderer::model::GizmoVertex {
                position: [0.0, 1.0, 0.0],
                color: yellow,
            },
            solarxy_renderer::model::GizmoVertex {
                position: [1.0, 1.0, 0.0],
                color: yellow,
            },
            solarxy_renderer::model::GizmoVertex {
                position: [1.0, 1.0, 0.0],
                color: yellow,
            },
            solarxy_renderer::model::GizmoVertex {
                position: [1.0, 0.0, 0.0],
                color: yellow,
            },
            solarxy_renderer::model::GizmoVertex {
                position: [1.0, 0.0, 0.0],
                color: yellow,
            },
            solarxy_renderer::model::GizmoVertex {
                position: [0.0, 0.0, 0.0],
                color: yellow,
            },
            solarxy_renderer::model::GizmoVertex {
                position: [0.0, 0.0, 0.0],
                color: yellow,
            },
            solarxy_renderer::model::GizmoVertex {
                position: [0.0, 1.0, 0.0],
                color: yellow,
            },
        ];
        let uv_boundary_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("UV Boundary Buffer"),
            contents: bytemuck::cast_slice(&boundary_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let (count_tex, count_view) =
            texture::create_overlap_count_texture(&device, config.width, config.height, false);
        let (stats_tex, stats_view) =
            texture::create_overlap_count_texture(&device, 512, 512, true);
        let overlap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("UV Overlap Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let overlap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UV Overlap Overlay Bind Group"),
            layout: &layouts.uv_overlap_read,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&count_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&overlap_sampler),
                },
            ],
        });

        let validation_colors = {
            use solarxy_renderer::validation::IssueCategory;
            let mut buffers = Vec::new();
            let mut bind_groups = Vec::new();
            for cat in IssueCategory::ALL {
                let color = cat.color();
                let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("Validation Color {:?}", cat)),
                    contents: bytemuck::cast_slice(&color),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("Validation Color BG {:?}", cat)),
                    layout: &layouts.validation_color,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    }],
                });
                buffers.push(buf);
                bind_groups.push(bg);
            }
            ValidationColorResources {
                bind_groups,
                buffers,
            }
        };

        let mut state = Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            renderer: Renderer {
                targets: RenderTargets {
                    depth_texture,
                    msaa_hdr_view,
                    _hdr_resolve_texture: hdr_resolve_texture,
                    hdr_resolve_view,
                },
                post: PostProcessing {
                    bloom,
                    bloom_enabled: preferences.display.bloom_enabled,
                    ssao,
                    ssao_enabled: preferences.display.ssao_enabled,
                    composite,
                    tone_mode: preferences.display.tone_mode,
                    exposure: preferences.display.exposure,
                },
                ibl_res: IblResources {
                    ibl,
                    ibl_fallback,
                    brdf_lut,
                    ibl_mode: preferences.display.ibl_mode,
                    last_active_ibl_mode: match preferences.display.ibl_mode {
                        IblMode::Off => IblMode::Full,
                        other => other,
                    },
                },
                wire: WireframeResources {
                    _gradient_buffer: gradient_buffer,
                    gradient_bind_group,
                    wireframe_params_buffer,
                    wireframe_params_bind_group,
                    _checker_texture: checker_texture,
                    uv_checker_bind_group,
                },
                layouts,
                pipelines,
                uv_cam,
                uv_boundary_buf,
                uv_overlap: UvOverlapResources {
                    count_texture: count_tex,
                    count_view,
                    overlay_bind_group: overlap_bind_group,
                    sampler: overlap_sampler,
                    stats_texture: stats_tex,
                    stats_view,
                    overlap_pct: None,
                    stats_dirty: false,
                    staging_buffer: None,
                    readback_pending: false,
                },
                validation_colors,
                shared_samplers,
                msaa_sample_count,
                target_width: size.width,
                target_height: size.height,
            },
            view: ViewState {
                pane_settings: {
                    let pds = PaneDisplaySettings {
                        view_mode: preferences.display.view_mode,
                        prev_non_ghosted_mode: ViewMode::Shaded,
                        ghosted_wireframe: false,
                        normals_mode: preferences.display.normals_mode,
                        background_mode,
                        uv_mode: preferences.display.uv_mode,
                        bounds_mode: BoundsMode::Off,
                        line_weight,
                        show_grid: preferences.display.grid_visible,
                        show_axis_gizmo: preferences.display.axis_gizmo_visible,
                        show_local_axes: preferences.display.local_axes_visible,
                        inspection_mode: preferences.display.inspection_mode,
                        material_override: MaterialOverride::None,
                        texel_density_target: preferences.display.texel_density_target,
                        pane_mode: PaneMode::Scene3D,
                        uv_bg: UvMapBackground::Dark,
                        uv_offset: [0.0, 0.0],
                        uv_zoom: 1.0,
                        show_uv_overlap: false,
                        show_validation: false,
                    };
                    [pds, pds]
                },
                display: DisplaySettings {
                    turntable_active: preferences.display.turntable_active,
                    turntable_rpm: preferences.display.turntable_rpm,
                    lights_locked: preferences.lighting.lock,
                    layout: ViewLayout::default(),
                    roughness_scale: 1.0,
                    metallic_scale: 1.0,
                },
                secondary_cam: None,
                active_pane: 0,
                cameras_linked: true,
            },
            gui,
            scene: None,
            input: InputState {
                cursor_pos: (0.0, 0.0),
                uv_last_mouse_pos: None,
                uv_left_pressed: false,
                uv_middle_pressed: false,
                modifiers: ModifiersState::empty(),
            },
            pending_load: None,
            pending_hdri: None,
            capture_requested: false,
            quit_requested: false,
            last_frame_time: Instant::now(),
            dt: 0.0,
            _backend_info: backend_info,
            preferences,
            window,
        };

        if let Some(path) = model_path {
            state.spawn_load(path);
        }

        Ok(state)
    }
}
