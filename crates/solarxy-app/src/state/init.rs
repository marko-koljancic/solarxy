//! Startup wiring: creates the wgpu surface, device, queue, [`Renderer`],
//! egui state, and initial preferences. Called from [`crate::app::App::resumed`]
//! on first window creation.

use std::sync::Arc;

use winit::window::Window;

use solarxy_renderer::frame::RendererInit;

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

        let mut gui = EguiRenderer::new(&device, surface_format, &window, console_buffer);
        gui.set_backend_info(backend_info.clone());
        gui.apply_theme_choice(preferences.ui.theme);
        gui.status_bar_visible = preferences.ui.status_bar_visible;
        if let Some(json) = preferences.dock.last_layout_json.as_deref() {
            gui.apply_layout_json(json);
        }
        gui.set_has_saved_layout(preferences.dock.saved_layout_json.is_some());

        let background_mode = preferences.display.background;
        let background = background_mode.resolve(&preferences.view.custom_backgrounds);
        let (ibl_top, ibl_bottom) = background.sky_colors();
        let line_weight = preferences.rendering.wireframe_line_weight;

        let renderer_init = RendererInit {
            msaa_sample_count,
            gradient_top: [0.35, 0.41, 0.47, 1.0],
            gradient_bottom: [0.66, 0.70, 0.72, 1.0],
            sky_top: ibl_top,
            sky_bottom: ibl_bottom,
            wireframe_color: background.wireframe_color(),
            wireframe_line_width: line_weight.width_px(),
            bloom_enabled: preferences.display.bloom_enabled,
            ssao_enabled: preferences.display.ssao_enabled,
            tone_mode: preferences.display.tone_mode,
            exposure: preferences.display.exposure,
            ibl_mode: preferences.display.ibl_mode,
            uv_checker_png: include_bytes!("../../../../res/textures/uv-checker_1k.png"),
        };
        let renderer = Renderer::new(&device, &queue, &config, &renderer_init)?;
        // Built before the renderer moves into `State`: the backend keeps its
        // own handle on the layouts so it can upload without being handed the
        // renderer back.
        let raster = solarxy_host::RasterBackend::new(std::sync::Arc::clone(&renderer.layouts));

        // The viewport renders before any model is chosen, so the scene
        // environment exists from startup, fitted to a placeholder box.
        let env_bounds = solarxy_renderer::environment::placeholder_bounds();
        let env = super::update::build_bounds_env(
            &device,
            &queue,
            &renderer,
            &env_bounds,
            background.grid_color(),
            preferences.rendering.shadow_map_size,
        );

        let mut state = Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            renderer,
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
                        turntable_active: false,
                    };
                    [pds; 4]
                },
                display: DisplaySettings {
                    point_size: solarxy_core::view_config::DEFAULT_POINT_SIZE,
                    turntable_active: preferences.display.turntable_active,
                    turntable_rpm: preferences.display.turntable_rpm,
                    lights_locked: preferences.lighting.lock,
                    layout: ViewLayout::default(),
                    split_ratio: DisplaySettings::DEFAULT_SPLIT_RATIO,
                    roughness_scale: 1.0,
                    metallic_scale: 1.0,
                    hdri_rotation: 0.0,
                    hdri_intensity: solarxy_core::view_config::DEFAULT_HDRI_INTENSITY,
                },
                cameras: [None, None, None, None],
                active_pane: 0,
                cameras_linked: true,
            },
            gui,
            scene: None,
            engine: None,
            engine_scene: None,
            selected_object: None,
            raster,
            env,
            env_bounds,
            pending_scene_deltas: Vec::new(),
            environment: solarxy_renderer::environment::EnvironmentTracker::default(),
            #[cfg(debug_assertions)]
            dev_environment_on: false,
            input: InputState {
                cursor_pos: (0.0, 0.0),
                uv_last_mouse_pos: None,
                uv_left_pressed: false,
                uv_middle_pressed: false,
                modifiers: ModifiersState::empty(),
            },
            review: super::review::ReviewState {
                author: preferences.review.author.clone(),
                panel_open: preferences.review.panel_open,
                ..super::review::ReviewState::default()
            },
            last_project_config_toast: None,
            pending_load: None,
            pending_hdri: None,
            pending_capture: None,
            viewport_context_menu: None,
            capture_requested: false,
            screenshot_expand_review: false,
            quit_requested: false,
            last_frame_time: Instant::now(),
            dt: 0.0,
            _backend_info: backend_info,
            preferences,
            window,
        };

        // Routed through the same handler the Open dialog and a drag and
        // drop use, rather than straight into the model loader. Startup used
        // to be the one entry point that could not open a scene, which made
        // a supported file look unsupported; a second extension check here
        // is how the three would drift apart again.
        if let Some(path) = model_path {
            state.handle_dropped_file(std::path::PathBuf::from(path));
        }

        Ok(state)
    }
}
