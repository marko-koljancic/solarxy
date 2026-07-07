//! Phase 0 WebGPU spike: compile the real solarxy-renderer to wasm32,
//! render the runtime-fetched dragon through the real shadow + PBR main +
//! composite passes, orbit it, and report FPS. THROWAWAY at go/no-go.
//!
//! Deliberately routed around (renderer wasm hazards, see the plan):
//! filesystem loaders, HDR/EXR file paths, `BrdfLut::generate` and the sky
//! convolution loops (fallback 1x1 IBL instead), blocking readbacks.

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use cgmath::Rotation3;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wgpu::util::DeviceExt;

use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ResolvedBackground, ToneMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::view_config::{BoundsMode, PaneDisplaySettings};
use solarxy_core::{RawMeshData, RawModelData};
use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::bloom::BloomState;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeState;
use solarxy_renderer::frame::{
    GradientUniform, IblResources, PostProcessing, RenderTargets, Renderer, UvOverlapResources,
    ValidationColorResources, WireframeParams, WireframeResources,
};
use solarxy_renderer::ibl::{BrdfLut, IblState};
use solarxy_renderer::model::GizmoVertex;
use solarxy_renderer::pipelines::{Instance, Pipelines};
use solarxy_renderer::scene::{
    BackgroundModeExt, ModelScene, create_light_bind_group, lights_from_camera,
};
use solarxy_renderer::shadow::ShadowState;
use solarxy_renderer::texture::{self, SharedSamplers};
use solarxy_renderer::uv_camera::UvCameraState;
use solarxy_renderer::visualization::VisualizationState;
use solarxy_renderer::{resources, validation};

const MSAA_SAMPLES: u32 = 4;
const SHADOW_MAP_SIZE: u32 = 2048;
const ORBIT_SPEED_RAD_PER_SEC: f32 = 0.35;
const MODEL_URL: &str = "/res/models/xyzrgb_dragon.obj";

fn log(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

fn err(msg: &str) {
    web_sys::console::error_1(&msg.into());
}

struct SpikeApp {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    scene: ModelScene,
    cam: CameraState,
    ibl_avg: [f32; 3],
    pds: PaneDisplaySettings,
    bg: ResolvedBackground,
    width: u32,
    height: u32,
    last_t_ms: f64,
    frames: u32,
    fps_window_start_ms: f64,
    total_frames: u64,
    cpu_ms_accum: f64,
    stats_el: Option<web_sys::Element>,
}

impl SpikeApp {
    fn frame(&mut self, t_ms: f64) {
        let dt = if self.last_t_ms > 0.0 {
            ((t_ms - self.last_t_ms) / 1000.0) as f32
        } else {
            1.0 / 60.0
        };
        self.last_t_ms = t_ms;

        // Auto-orbit + camera uniform upload (the real controller path).
        self.cam.inject_orbit_yaw(ORBIT_SPEED_RAD_PER_SEC * dt);
        self.cam.update(&self.queue, dt);

        // Camera-relative light rig + shadow VP, as the desktop update does.
        self.scene.lights_uniform =
            lights_from_camera(&self.cam.camera, &self.scene.model.bounds, self.ibl_avg);
        self.queue.write_buffer(
            &self.scene.light_buffer,
            0,
            bytemuck::cast_slice(&[self.scene.lights_uniform]),
        );
        let key = self.scene.lights_uniform.lights[0].position;
        self.scene.shadow.update_light_vp(
            &self.queue,
            cgmath::Point3::new(key[0], key[1], key[2]),
            self.scene.model.bounds.center(),
            self.scene.model.bounds.diagonal() / 2.0,
        );

        let perf = web_sys::window().and_then(|w| w.performance());
        let cpu_start = perf.as_ref().map_or(0.0, web_sys::Performance::now);

        let output = match self.surface.get_current_texture() {
            Ok(o) => o,
            Err(e) => {
                err(&format!("SPIKE_SURFACE_ERROR {e:?}"));
                return;
            }
        };
        let surface_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Spike Encoder"),
            });

        let objects = [self.scene.draw_object()];
        self.renderer
            .render_shadow_pass(&mut encoder, &self.scene, &objects);
        self.renderer.render_main_pass(
            &mut encoder,
            &self.scene,
            &objects,
            &self.cam.bind_group,
            &self.cam.camera,
            &self.pds,
            self.bg,
        );

        self.renderer.post.composite.write_params(
            &self.queue,
            false,
            false,
            ToneMode::AcesFilmic,
            1.0,
            InspectionMode::Shaded,
        );
        self.renderer.post.composite.render(
            &mut encoder,
            &self.renderer.pipelines,
            &surface_view,
            false,
            &self.renderer.post.ssao,
            Some([0.0, 0.0, self.width as f32, self.height as f32]),
            true,
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        if let Some(p) = &perf {
            self.cpu_ms_accum += p.now() - cpu_start;
        }

        if self.total_frames == 0 {
            log("SPIKE_FIRST_FRAME_OK");
        }
        self.total_frames += 1;
        self.frames += 1;
        let window_ms = t_ms - self.fps_window_start_ms;
        if window_ms >= 1000.0 {
            let fps = f64::from(self.frames) * 1000.0 / window_ms;
            let line = format!(
                "SPIKE_FPS {fps:.1} ({:.2} ms/frame wall, {:.2} ms/frame cpu-encode, {} frames total)",
                window_ms / f64::from(self.frames),
                self.cpu_ms_accum / f64::from(self.frames),
                self.total_frames
            );
            self.cpu_ms_accum = 0.0;
            log(&line);
            if let Some(el) = &self.stats_el {
                el.set_text_content(Some(&line));
            }
            self.frames = 0;
            self.fps_window_start_ms = t_ms;
        }
    }
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let resp: web_sys::Response =
        wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
            .await?
            .dyn_into()?;
    if !resp.ok() {
        return Err(format!("fetch {url} failed: HTTP {}", resp.status()).into());
    }
    let buf = wasm_bindgen_futures::JsFuture::from(resp.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

fn parse_obj(bytes: &[u8]) -> Result<RawModelData, String> {
    let (models, _materials) = tobj::load_obj_buf(
        &mut Cursor::new(bytes),
        &tobj::GPU_LOAD_OPTIONS,
        // No MTL on the web spike: resolve every material library to empty.
        |_p| Ok((Vec::new(), Default::default())),
    )
    .map_err(|e| format!("tobj parse failed: {e}"))?;

    let mut meshes = Vec::with_capacity(models.len());
    let mut polygon_count = 0usize;
    for m in models {
        let mesh = m.mesh;
        let positions: Vec<[f32; 3]> = mesh
            .positions
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        if positions.is_empty() {
            continue;
        }
        polygon_count += mesh.indices.len() / 3;
        let normals = if mesh.normals.is_empty() {
            None
        } else {
            Some(
                mesh.normals
                    .chunks_exact(3)
                    .map(|c| [c[0], c[1], c[2]])
                    .collect(),
            )
        };
        let tex_coords = if mesh.texcoords.is_empty() {
            None
        } else {
            Some(
                mesh.texcoords
                    .chunks_exact(2)
                    .map(|c| [c[0], c[1]])
                    .collect(),
            )
        };
        meshes.push(RawMeshData {
            name: m.name,
            positions,
            indices: mesh.indices,
            normals,
            tex_coords,
            material_index: None,
        });
    }

    Ok(RawModelData {
        meshes,
        materials: Vec::new(),
        polygon_count,
    })
}

/// A tiny in-memory checker so the UV-checker bind group exists without
/// fetching the 1k PNG or pulling `image` decode into the wasm path.
fn tiny_checker_pixels() -> Vec<u8> {
    let mut px = Vec::with_capacity(4 * 4 * 4);
    for y in 0..4u32 {
        for x in 0..4u32 {
            let v = if (x + y) % 2 == 0 { 200u8 } else { 60u8 };
            px.extend_from_slice(&[v, v, v, 255]);
        }
    }
    px
}

#[allow(clippy::too_many_lines)]
async fn run() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let canvas: web_sys::HtmlCanvasElement = document
        .get_element_by_id("spike-canvas")
        .ok_or("no #spike-canvas")?
        .dyn_into()?;
    let stats_el = document.get_element_by_id("spike-stats");

    let dpr = window.device_pixel_ratio();
    let css_w = canvas.client_width().max(1) as f64;
    let css_h = canvas.client_height().max(1) as f64;
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
        .map_err(|e| JsValue::from_str(&format!("create_surface: {e}")))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| JsValue::from_str(&format!("request_adapter: {e}")))?;
    let info = adapter.get_info();
    log(&format!(
        "SPIKE_ADAPTER backend={:?} name={} driver={}",
        info.backend, info.name, info.driver_info
    ));

    // The correct-WebGPU-limits data point: Limits::default() resolved
    // against the adapter, NOT downlevel_webgl2_defaults.
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| JsValue::from_str(&format!("request_device: {e}")))?;

    device.set_device_lost_callback(|reason, message| {
        web_sys::console::error_1(&format!("SPIKE_DEVICE_LOST {reason:?}: {message}").into());
    });
    device.on_uncaptured_error(Arc::new(|e: wgpu::Error| {
        web_sys::console::error_1(&format!("SPIKE_UNCAPTURED_ERROR {e}").into());
    }));

    let caps = surface.get_capabilities(&adapter);
    let surface_format = caps
        .formats
        .iter()
        .find(|f| f.is_srgb())
        .copied()
        .unwrap_or(caps.formats[0]);
    log(&format!(
        "SPIKE_SURFACE format={surface_format:?} present_modes={:?} alpha_modes={:?}",
        caps.present_modes, caps.alpha_modes
    ));
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width,
        height,
        present_mode: caps.present_modes[0],
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    // ---- Renderer assembly: mirrors solarxy-app/src/state/init.rs, with
    // fallback IBL/LUT instead of the heavy generators. ----
    let layouts = Arc::new(BindGroupLayouts::new(&device));
    let pipelines = Pipelines::new(&device, &config, &layouts, MSAA_SAMPLES);

    let depth_texture =
        texture::Texture::create_depth_texture(&device, width, height, "depth", MSAA_SAMPLES);
    let msaa_hdr_view = texture::create_msaa_hdr_texture(&device, width, height, MSAA_SAMPLES);
    let (hdr_resolve_texture, hdr_resolve_view) =
        texture::create_hdr_resolve_texture(&device, width, height);

    let bg = BackgroundMode::GRADIENT.resolve(&[]);
    let (sky_top, sky_bottom) = bg.sky_colors();
    let _ = (sky_top, sky_bottom); // sky convolution deliberately skipped

    let brdf_lut = BrdfLut::fallback(&device, &queue);
    let ibl = IblState::fallback(&device, &queue);
    let ibl_fallback = IblState::fallback(&device, &queue);
    let ibl_avg = ibl.irradiance_average;

    let gradient_uniform = GradientUniform {
        top_color: [bg.sky_top[0], bg.sky_top[1], bg.sky_top[2], 1.0],
        bottom_color: [bg.sky_bottom[0], bg.sky_bottom[1], bg.sky_bottom[2], 1.0],
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

    let wireframe_params = WireframeParams {
        color: bg.wireframe_color(),
        line_width: LineWeight::Medium.width_px(),
        screen_width: width as f32,
        screen_height: height as f32,
        _pad: 0.0,
    };
    let wireframe_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Wireframe Params Uniform"),
        contents: bytemuck::bytes_of(&wireframe_params),
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

    let checker_texture = texture::Texture::from_raw_rgba(
        &device,
        &queue,
        &tiny_checker_pixels(),
        4,
        4,
        Some("uv_checker_tiny"),
        false,
    )
    .map_err(|e| JsValue::from_str(&format!("checker: {e}")))?;
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
                resource: wgpu::BindingResource::Sampler(&shared_samplers.linear_repeat),
            },
        ],
    });

    let bloom = BloomState::new(
        &device,
        &layouts,
        &hdr_resolve_view,
        shared_samplers.linear_clamp.clone(),
        width,
        height,
    );
    let composite = CompositeState::new(
        &device,
        &layouts,
        &hdr_resolve_view,
        &bloom.ping_view,
        &bloom.sampler,
        false,
        false,
        ToneMode::AcesFilmic,
        1.0,
    );
    let ssao = solarxy_renderer::ssao::SsaoState::new(&device, &queue, &layouts, width, height);
    let uv_cam = UvCameraState::new(&device, &layouts.camera);

    let yellow = [1.0, 0.85, 0.0];
    let boundary_verts: [GizmoVertex; 8] = [
        GizmoVertex { position: [0.0, 1.0, 0.0], color: yellow },
        GizmoVertex { position: [1.0, 1.0, 0.0], color: yellow },
        GizmoVertex { position: [1.0, 1.0, 0.0], color: yellow },
        GizmoVertex { position: [1.0, 0.0, 0.0], color: yellow },
        GizmoVertex { position: [1.0, 0.0, 0.0], color: yellow },
        GizmoVertex { position: [0.0, 0.0, 0.0], color: yellow },
        GizmoVertex { position: [0.0, 0.0, 0.0], color: yellow },
        GizmoVertex { position: [0.0, 1.0, 0.0], color: yellow },
    ];
    let uv_boundary_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("UV Boundary Buffer"),
        contents: bytemuck::cast_slice(&boundary_verts),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let (count_tex, count_view) =
        texture::create_overlap_count_texture(&device, width, height, false);
    let (stats_tex, stats_view) = texture::create_overlap_count_texture(&device, 512, 512, true);
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
                label: Some(&format!("Validation Color {cat:?}")),
                contents: bytemuck::cast_slice(&color),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("Validation Color BG {cat:?}")),
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

    let overdraw =
        solarxy_renderer::overdraw::OverdrawResources::new(&device, &layouts, width, height);

    let renderer = Renderer {
        targets: RenderTargets {
            depth_texture,
            msaa_hdr_view,
            _hdr_resolve_texture: hdr_resolve_texture,
            hdr_resolve_view,
        },
        post: PostProcessing {
            bloom,
            bloom_enabled: false,
            ssao,
            ssao_enabled: false,
            composite,
            tone_mode: ToneMode::AcesFilmic,
            exposure: 1.0,
        },
        ibl_res: IblResources {
            ibl,
            ibl_fallback,
            brdf_lut,
            ibl_mode: IblMode::Full,
            last_active_ibl_mode: IblMode::Full,
        },
        wire: WireframeResources {
            _gradient_buffer: gradient_buffer,
            gradient_bind_group,
            wireframe_params_buffer,
            wireframe_params_bind_group,
            _checker_texture: checker_texture,
            uv_checker_bind_group,
        },
        layouts: layouts.clone(),
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
            map_receiver: None,
        },
        validation_colors,
        overdraw,
        skybox_bind_group: None,
        shared_samplers,
        msaa_sample_count: MSAA_SAMPLES,
        target_width: width,
        target_height: height,
    };

    // ---- Model: fetch + parse + GPU upload through the real seam. ----
    log(&format!("SPIKE_FETCH {MODEL_URL}"));
    let obj_bytes = fetch_bytes(MODEL_URL).await?;
    log(&format!("SPIKE_FETCHED {} bytes", obj_bytes.len()));
    let raw = parse_obj(&obj_bytes).map_err(|e| JsValue::from_str(&e))?;

    let (model, normals_geo, stats, viewer_validation) = resources::upload_model(
        raw,
        "xyzrgb_dragon.obj",
        &device,
        &queue,
        &layouts.texture,
        &layouts.edge_geometry,
    )
    .map_err(|e| JsValue::from_str(&format!("upload_model: {e}")))?;
    log(&format!(
        "SPIKE_MODEL meshes={} tris={} verts={} validation_issues={}",
        model.meshes.len(),
        stats.tris,
        stats.verts,
        viewer_validation.report.issues.len()
    ));

    // ---- ModelScene assembly: mirrors scene.rs minus the path loader. ----
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

    let aspect = width as f32 / height as f32;
    let initial_cam =
        solarxy_renderer::camera::camera_from_bounds(&model.bounds, aspect);
    let lights_uniform = lights_from_camera(&initial_cam, &model.bounds, ibl_avg);
    let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Light VB"),
        contents: bytemuck::cast_slice(&[lights_uniform]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let light_bind_group = create_light_bind_group(
        &device,
        &layouts,
        &light_buffer,
        &renderer.ibl_res.ibl,
        &renderer.ibl_res.brdf_lut,
    );

    let shadow = ShadowState::new(&device, &layouts, &lights_uniform, &model, SHADOW_MAP_SIZE);
    let vis = VisualizationState::new(&device, &layouts, &model, &normals_geo, bg.grid_color());

    let validation_mesh_cat = validation::build_mesh_category_map(
        &viewer_validation.report,
        model.meshes.len(),
        &viewer_validation.raw_to_gpu,
    );
    let edge_index_lists = validation::build_mesh_edge_indices(
        &viewer_validation.report,
        model.meshes.len(),
        &viewer_validation.raw_to_gpu,
    );
    let validation_edge_buffers: Vec<Option<(wgpu::Buffer, u32)>> = edge_index_lists
        .into_iter()
        .map(|indices| {
            if indices.is_empty() {
                None
            } else {
                let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Validation Edge Indices"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                Some((buf, indices.len() as u32))
            }
        })
        .collect();

    let scene = ModelScene {
        model,
        lights_uniform,
        light_buffer,
        light_bind_group,
        instance_buffer,
        shadow,
        vis,
        model_path: "xyzrgb_dragon.obj".to_string(),
        stats,
        validation: viewer_validation.report,
        validation_mesh_cat,
        validation_edge_buffers,
        validation_raw_to_gpu: viewer_validation.raw_to_gpu,
    };

    let cam = CameraState::new(&device, &layouts.camera, &scene.model.bounds, aspect);

    let pds = PaneDisplaySettings {
        view_mode: ViewMode::Shaded,
        prev_non_ghosted_mode: ViewMode::Shaded,
        ghosted_wireframe: false,
        normals_mode: NormalsMode::Off,
        background_mode: BackgroundMode::GRADIENT,
        uv_mode: UvMode::Off,
        bounds_mode: BoundsMode::Off,
        line_weight: LineWeight::Medium,
        show_grid: true,
        show_axis_gizmo: false,
        show_local_axes: false,
        inspection_mode: InspectionMode::Shaded,
        material_override: MaterialOverride::None,
        texel_density_target: 1.0,
        pane_mode: PaneMode::Scene3D,
        uv_bg: UvMapBackground::Dark,
        uv_offset: [0.0, 0.0],
        uv_zoom: 1.0,
        show_uv_overlap: false,
        show_validation: false,
    };

    let app = Rc::new(RefCell::new(SpikeApp {
        surface,
        device,
        queue,
        renderer,
        scene,
        cam,
        ibl_avg,
        pds,
        bg,
        width,
        height,
        last_t_ms: 0.0,
        frames: 0,
        fps_window_start_ms: 0.0,
        total_frames: 0,
        cpu_ms_accum: 0.0,
        stats_el,
    }));

    // requestAnimationFrame loop.
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let app_for_loop = app.clone();
    *g.borrow_mut() = Some(Closure::new(move |t: f64| {
        {
            let mut a = app_for_loop.borrow_mut();
            if a.fps_window_start_ms == 0.0 {
                a.fps_window_start_ms = t;
            }
            a.frame(t);
        }
        raf(f.borrow().as_ref().expect("raf closure"));
    }));
    raf(g.borrow().as_ref().expect("raf closure"));

    log("SPIKE_STARTED");
    Ok(())
}

fn raf(f: &Closure<dyn FnMut(f64)>) {
    if let Some(w) = web_sys::window() {
        let _ = w.request_animation_frame(f.as_ref().unchecked_ref());
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    wasm_bindgen_futures::spawn_local(async {
        if let Err(e) = run().await {
            err(&format!("SPIKE_FATAL {e:?}"));
        }
    });
}
