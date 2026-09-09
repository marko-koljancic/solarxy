//! Construction, dispatch, and the frame the page drives.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    /// Boots over a canvas: WebGPU surface/device/queue, the full renderer
    /// (`uv_checker_png` is the checker texture asset the shell ships), the
    /// scene environment, and the engine with the host clock installed.
    #[allow(clippy::too_many_lines)] // linear boot sequence; splitting obscures it
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        uv_checker_png: Vec<u8>,
    ) -> Result<SolarxyApp, JsError> {
        let window = web_sys::window().ok_or_else(|| JsError::new("no window"))?;
        let dpr = window.device_pixel_ratio();
        let css_w = f64::from(canvas.client_width().max(1));
        let css_h = f64::from(canvas.client_height().max(1));
        let width = (css_w * dpr) as u32;
        let height = (css_h * dpr) as u32;
        canvas.set_width(width);
        canvas.set_height(height);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsError::new(&format!("create_surface: {e}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| JsError::new(&format!("request_adapter: {e}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("solarxy-web device"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: solarxy_renderer::limits::required_limits(&adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| JsError::new(&format!("request_device: {e}")))?;

        // Before anything uses the device. The browser's own default for
        // an uncaptured error is a console line nobody reads; this routes
        // it through the host event stream so the frontend can toast it
        // and the crash reporter can attach it.
        let gpu_faults = solarxy_renderer::faults::install(&device);

        // Chrome exposes only non-sRGB surface formats; render into an
        // sRGB view of the surface texture so the tone-mapped composite
        // output is gamma-encoded correctly.
        let caps = surface.get_capabilities(&adapter);
        let base_format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let render_format = base_format.add_srgb_suffix();
        let view_formats = if render_format == base_format {
            vec![]
        } else {
            vec![render_format]
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: base_format,
            width,
            height,
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // The renderer builds its pipelines against the sRGB view format.
        let render_config = wgpu::SurfaceConfiguration {
            format: render_format,
            view_formats: vec![],
            ..config.clone()
        };
        let background = BackgroundMode::GRADIENT.resolve(&[]);
        let (sky_top, sky_bottom) = background.sky_colors();
        let init = RendererInit {
            msaa_sample_count: MSAA_SAMPLES,
            gradient_top: [0.35, 0.41, 0.47, 1.0],
            gradient_bottom: [0.66, 0.70, 0.72, 1.0],
            sky_top,
            sky_bottom,
            wireframe_color: background.wireframe_color(),
            // Only the seed for the renderer's first frame: every pane's real
            // weight arrives through `PaneDisplaySettings::line_weight` (see
            // the per-pane `width_px()` reads below). Taken from the shared
            // default rather than naming a variant, because this hardcoded
            // `Medium` silently disagreed with the desktop's persisted
            // `Light` default, so the same scene drew different wireframes in
            // the two shells out of the box.
            wireframe_line_width: solarxy_core::preferences::LineWeight::default().width_px(),
            // Match the desktop's shipped defaults. Both stayed hard false
            // for six releases, which left the AO Preview mode a white
            // screen and bloom inert in every browser; the preference
            // toggles arrive through `set_display_defaults` below.
            bloom_enabled: true,
            ssao_enabled: true,
            tone_mode: ToneMode::AcesFilmic,
            exposure: 1.0,
            ibl_mode: IblMode::Full,
            uv_checker_png: &uv_checker_png,
        };
        let renderer = Renderer::new(&device, &queue, &render_config, &init)
            .map_err(|e| JsError::new(&format!("Renderer::new: {e}")))?;
        // Built before the renderer moves into the host: the backend keeps its
        // own handle on the layouts so it can upload without being handed the
        // renderer back.
        let raster = solarxy_host::RasterBackend::new(std::sync::Arc::clone(&renderer.layouts));

        let bounds = default_bounds();
        let vis = VisualizationState::new_from_parts(
            &device,
            &renderer.layouts,
            &bounds,
            &[],
            None,
            background.grid_color(),
        );
        let mut env = SceneEnvironment::new(
            &device,
            &queue,
            &renderer.layouts,
            &bounds,
            width as f32 / height.max(1) as f32,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
            SHADOW_MAP_SIZE,
            vis,
        );
        env.light_bind_group = create_light_bind_group(
            &device,
            &renderer.layouts,
            &env.light_buffer,
            &renderer.ibl_res.ibl,
            &renderer.ibl_res.brdf_lut,
            &renderer.ibl_res.ltc,
        );

        let mut engine = Engine::new().map_err(|e| JsError::new(&format!("engine: {e}")))?;
        engine.set_clock(web_now);
        engine.set_epoch_clock(web_epoch_ms);
        // Imports run off the main thread: cooks yield a ParseModel job the
        // frontend pumps to the import worker (`take_import_jobs` ->
        // `submit_parsed_model`), rather than parsing inline.
        engine.set_async_jobs(true);

        #[cfg(feature = "diagnostics")]
        log(&format!(
            "solarxy-web: booted ({width}x{height}, {} node types, full renderer)",
            engine.registry().len()
        ));

        Ok(SolarxyApp {
            instance,
            surface,
            device,
            queue,
            config,
            render_format,
            renderer,
            preview: None,
            raster,
            env,
            environment: solarxy_renderer::environment::EnvironmentTracker::default(),
            env_bounds: bounds,
            view: HostViewState {
                pane_settings: [default_pane_settings(); 4],
                display: default_display_settings(),
                cameras: [None, None, None, None],
                active_pane: 0,
                cameras_linked: false,
            },
            pane_looks: [PaneLook::default(); 4],
            look_through: [None; 4],
            camera_locked: solarxy_host::cameras::CameraLocks::default(),
            camera_editing: [false; 4],
            engine,
            host_events: Vec::new(),
            gpu_faults,
            player_mode: false,
            last_pane_rects: Vec::new(),
            dpr: dpr as f32,
            pointer_buttons_down: 0,
            selected_object: None,
            pending_validate: Vec::new(),
            pending_image: Vec::new(),
            pending_hdri: Vec::new(),
            current_ctx: GraphContext::Root,
            uv_scene: SceneObjects::new(),
            uv_use_preview: false,
            last_uv_source: None,
            last_overlap: (None, false),
            last_pointer: (0.0, 0.0),
            hdri: None,
            screenshot_request: None,
            turntable_request: None,
            pending_screenshot: None,
            still: None,
            still_camera: None,
            still_look: CompositeLook::default(),
            tracer: None,
            traced_env_dirty: true,
            still_tiles: std::collections::VecDeque::new(),
            still_previews: std::collections::VecDeque::new(),
            still_started_ms: 0.0,
            still_passes: None,
            still_pass_request: [false; 3],
            still_writes_aovs: false,
            still_float: None,
            // On, matching the shipped behaviour, until a preference push
            // says otherwise; boot pushes one before the first traced frame.
            preview_denoise: true,
            traced_cam_keys: [None; 4],
            traced_env_params: (0.0, 0.0),
            last_pane_samples: [None; 4],
            viz_dirty: true,
            attr_viz: AttrVizState::default(),
            label_colors: {
                let d = solarxy_renderer::labels::LabelStyle::new_default();
                [d.text, d.chip, d.dot]
            },
            attr_dirty: false,
            display_defaults: DisplayDefaults::default(),
            gizmo: GizmoState::default(),
            gizmo_addr: None,
            gizmo_readout: None,
            last_capability: None,
        })
    }

    /// Applies one command, returning the `EventBatch` for the mirror.
    pub fn dispatch(&mut self, cmd: JsValue) -> Result<JsValue, JsError> {
        let command: Command = serde_wasm_bindgen::from_value(cmd)
            .map_err(|e| JsError::new(&format!("bad command: {e}")))?;
        let batch = self
            .engine
            .apply(command)
            .map_err(|e| JsError::new(&format!("{e}")))?;
        to_js(&batch)
    }

    /// A transient param preview during a drag: no event, no undo, but it
    /// dirties the node so the next `frame` previews it. `ctx`/`value` are
    /// the same serde shapes as inside a `Command`.
    pub fn preview_param(
        &mut self,
        ctx: JsValue,
        node: f64,
        key: &str,
        value: JsValue,
    ) -> Result<(), JsError> {
        let ctx = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let value = serde_wasm_bindgen::from_value(value)
            .map_err(|e| JsError::new(&format!("bad value: {e}")))?;
        self.engine.preview_param(
            ctx,
            solarxy_graph::document::NodeId(node as u64),
            key,
            value,
        );
        Ok(())
    }

    /// Cooks under a frame budget, applies the scene delta, renders every
    /// pane, and returns the cook `EventBatch` (status + stats).
    pub fn frame(&mut self, dt_ms: f64) -> Result<JsValue, JsError> {
        // The clock advances BEFORE the cook, so this frame's geometry is
        // this frame's time. Fixed step (one tick is one frame), so a heavy
        // scene plays slowly rather than skipping and `$T` stays exactly
        // `frame / fps`. A stopped clock returns immediately.
        let mut events = self.engine.tick().events;

        // Cook the dirty set under a wall-clock budget.
        let deadline = web_now() + COOK_BUDGET_MS;
        events.extend(self.engine.cook(&mut || web_now() < deadline));

        // Apply the fresh scene delta to the multi-object scene.
        let delta = self.engine.take_scene_delta();
        if !delta.ops.is_empty() {
            self.viz_dirty = true;
            self.attr_dirty = true;
            self.raster.apply(&self.device, &self.queue, &delta);
            // The backend collects upload failures rather than logging them:
            // it has no logging facility and this host is the layer that knows
            // where a message belongs.
            //
            // A console line alone was not enough. A mesh the device cannot
            // hold is refused here, and the whole symptom that reported it
            // was a viewport that stopped drawing with nothing in the
            // interface saying why, so the refusal rides the same notice
            // channel the renderer's other omissions use and reaches the
            // user as a toast.
            for e in self.raster.take_errors() {
                error(&format!("scene delta apply failed: {e}"));
                self.host_events.push(HostEvent::RenderNotice {
                    message: e.to_string(),
                });
            }
            self.apply_scene_environment(&delta);
        }
        self.feed_traced_preview(&delta);

        self.sync_gizmo();

        self.sync_env_bounds();
        self.sync_visualization();
        self.sync_attr_channels();
        self.sync_uv_preview();
        self.ensure_pane_cameras();
        self.follow_look_through_cameras();
        // `f64::clamp` RETURNS NaN for a NaN input (every comparison with NaN is
        // false), so the clamp alone is not a guard. A non-finite delta reaching
        // a camera transition integrates straight into eye/target, and the next
        // projection panics inside cgmath with a NaN far plane. Callers are
        // supposed to pass a real frame delta; treat anything else as one frame.
        let dt_ms = if dt_ms.is_finite() { dt_ms } else { 16.0 };
        let dt = (dt_ms / 1000.0).clamp(0.0, 0.1) as f32;
        // Live turntable spin: a constant angular velocity on each pane
        // whose toggle is on. rpm is the global display setting; the spin is
        // session-temporary (reset on load) and drives the pane's scratch camera.
        let rpm = self.view.display.turntable_rpm;
        if rpm.abs() > 1e-6 {
            let yaw = rpm * std::f32::consts::TAU / 60.0 * dt;
            for i in 0..self.view.cameras.len() {
                if self.view.pane_settings[i].turntable_active
                    && let Some(cam) = self.view.cameras[i].as_mut()
                {
                    cam.inject_orbit_yaw(yaw);
                }
            }
        }
        for cam in self.view.cameras.iter_mut().flatten() {
            cam.update(&self.queue, dt);
        }
        self.update_lights();
        self.sync_render_target_dims();

        // Render every pane into the surface.
        let output = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return to_js(&EventBatch {
                    revision: self.engine.revision(),
                    events,
                });
            }
            Err(e) => return Err(JsError::new(&format!("acquire: {e}"))),
        };
        let surface_view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
        });

        let pane_rects = self.compute_panes();
        if self.still.is_some() {
            // The job owns the frame. The surface is acquired and presented
            // anyway so the browser does not treat the canvas as stalled, and
            // it keeps whatever the last ordinary frame left on it.
            self.pump_still_render();
        } else {
            let is_split = pane_rects.len() > 1;
            for (i, pane) in pane_rects.iter().enumerate() {
                self.render_pane(i, *pane, &surface_view, is_split);
            }
        }
        output.present();

        // Pump the overlap readback and mirror its progress to React.
        self.renderer.uv_overlap.poll_readback(&self.device);
        let overlap = (
            self.renderer.uv_overlap.overlap_pct,
            self.renderer.uv_overlap.readback_pending,
        );
        if overlap != self.last_overlap {
            self.last_overlap = overlap;
            self.host_events.push(HostEvent::UvOverlap {
                pct: overlap.0,
                pending: overlap.1,
            });
        }

        self.push_pane_rects_if_changed(&pane_rects);

        // A requested screenshot renders offscreen at capture resolution
        // after the on-screen frame (the next frame's target sync restores
        // the layout dimensions).
        if let Some(opts) = self.screenshot_request.take() {
            self.render_screenshot(&opts);
        }
        if let Some((pane, azimuth, opts)) = self.turntable_request.take() {
            self.render_turntable_frame(pane, azimuth, &opts);
        }

        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Resizes the surface and render targets. `dpr` is the LIVE device pixel
    /// ratio: it is not constant for the session (browser zoom and a move to a
    /// different-density monitor both change it), and every pointer coordinate,
    /// pane rect, and marker projection is scaled by it, so the shell re-reads
    /// it on each resize and pushes it through here.
    pub fn resize(&mut self, width: u32, height: u32, dpr: f32) {
        if width == 0 || height == 0 {
            return;
        }
        if dpr > 0.0 {
            self.dpr = dpr;
            // Label px metrics scale by dpr; keep them honest across
            // browser-zoom and monitor-density changes.
            self.renderer.write_label_dpr(&self.queue, dpr);
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.sync_render_target_dims();
        let aspect = self.renderer.target_width as f32 / self.renderer.target_height.max(1) as f32;
        for cam in self.view.cameras.iter_mut().flatten() {
            cam.resize(aspect);
        }
    }
}
