mod capture;
mod init;
mod input;
pub(crate) mod renderer;
mod update;
pub(crate) mod view_state;

pub(super) use renderer::{
    GradientUniform, IblResources, PostProcessing, Renderer, RenderTargets, UvOverlapResources,
    ValidationColorResources, WireframeParams, WireframeResources,
};
pub(super) use view_state::{BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout, ViewState};

use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::bloom::BloomState;
use crate::cgi::camera::{Camera, CameraUniform};
use crate::cgi::camera_state::CameraState;
use crate::cgi::composite::CompositeState;
use crate::cgi::gui::EguiRenderer;
use crate::cgi::ibl::{BrdfLut, IblState};
use crate::cgi::light::{LightEntry, LightsUniform};
use crate::cgi::model::{self, Model};
use crate::cgi::pipelines::{Instance, Pipelines};
use crate::cgi::resources::{self, ModelStats};
use crate::cgi::shadow::ShadowState;
use crate::cgi::ssao::SsaoState;
use crate::cgi::texture::{self, SharedSamplers};
use crate::cgi::visualization::{GridUniform, VisualizationState};
use crate::preferences::{
    self, BackgroundMode, IblMode, InspectionMode, MaterialOverride, PaneMode, Preferences,
    UvMapBackground, ViewMode,
};
use cgmath::Rotation3;
use std::sync::{mpsc, Arc};
use std::time::Instant;
use wgpu::util::DeviceExt;
use winit::{keyboard::ModifiersState, window::Window};

struct Pane {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

pub(super) trait BackgroundModeExt {
    fn clear_color(self) -> wgpu::Color;
    fn wireframe_color(self) -> [f32; 4];
    fn sky_colors(self) -> ([f32; 3], [f32; 3]);
    fn grid_color(self) -> [f32; 3];
    fn effective_luminance(self) -> f32;
}

impl BackgroundModeExt for BackgroundMode {
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
        if self.effective_luminance() < 0.3 {
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
        let lum = self.effective_luminance();
        if lum < 0.3 {
            let v = (lum + 0.15).min(1.0);
            [v, v, v]
        } else {
            let v = (lum * 0.55).clamp(0.0, 1.0);
            [v, v, v]
        }
    }

    fn effective_luminance(self) -> f32 {
        if self == Self::Gradient {
            let (top, bot) = self.sky_colors();
            let lum_top = 0.2126 * top[0] + 0.7152 * top[1] + 0.0722 * top[2];
            let lum_bot = 0.2126 * bot[0] + 0.7152 * bot[1] + 0.0722 * bot[2];
            (lum_top + lum_bot) * 0.5
        } else {
            let c = self.clear_color();
            (0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b) as f32
        }
    }
}

pub(super) struct ModelScene {
    pub(super) model: Model,
    pub(super) cam: CameraState,
    pub(super) lights_uniform: LightsUniform,
    pub(super) light_buffer: wgpu::Buffer,
    pub(super) light_bind_group: wgpu::BindGroup,
    pub(super) instance_buffer: wgpu::Buffer,
    pub(super) shadow: ShadowState,
    pub(super) vis: VisualizationState,
    #[allow(dead_code)]
    pub(super) model_path: String,
    pub(super) stats: ModelStats,
    pub(super) validation: crate::validation::ValidationReport,
    pub(super) validation_mesh_cat: Vec<Option<usize>>,
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
    #[allow(clippy::too_many_arguments)]
    fn new(
        model_path: String,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        config: &wgpu::SurfaceConfiguration,
        initial_grid_color: [f32; 3],
        brdf_lut: &BrdfLut,
        shadow_map_size: u32,
    ) -> anyhow::Result<Self> {
        let (model, normals_geo, stats, viewer_validation) = resources::load_model_any(
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

        let shadow = ShadowState::new(device, layouts, &lights_uniform, &model, shadow_map_size);
        let vis =
            VisualizationState::new(device, layouts, &model, &normals_geo, initial_grid_color);

        let validation_mesh_cat = crate::validation::build_mesh_category_map(
            &viewer_validation.report,
            model.meshes.len(),
            &viewer_validation.raw_to_gpu,
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
            validation: viewer_validation.report,
            validation_mesh_cat,
        })
    }
}

pub(super) struct PendingLoad {
    receiver: mpsc::Receiver<anyhow::Result<ModelScene>>,
    filename: String,
    path: String,
}

pub(super) struct InputState {
    pub(super) cursor_pos: (f32, f32),
    pub(super) modifiers: ModifiersState,
    pub(super) uv_last_mouse_pos: Option<(f32, f32)>,
    pub(super) uv_left_pressed: bool,
    pub(super) uv_middle_pressed: bool,
}

pub struct State {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) is_surface_configured: bool,
    pub(super) renderer: Renderer,
    pub(super) gui: EguiRenderer,
    pub(super) scene: Option<ModelScene>,
    pub(super) view: ViewState,
    pub(super) input: InputState,
    pub(super) pending_load: Option<PendingLoad>,
    pub(super) capture_requested: bool,
    pub(super) last_frame_time: Instant,
    pub(super) dt: f32,
    pub(super) _backend_info: String,
    pub(super) preferences: Preferences,
    pub window: Arc<Window>,
}

impl State {
    pub(super) fn target_dimensions(&self) -> (u32, u32) {
        compute_target_dimensions(
            self.view.display.layout,
            self.config.width,
            self.config.height,
        )
    }

    fn compute_panes(&self) -> Vec<Pane> {
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        match self.view.display.layout {
            ViewLayout::Single => vec![Pane {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
            }],
            ViewLayout::SplitVertical => {
                let half = (w * 0.5).floor();
                vec![
                    Pane {
                        x: 0.0,
                        y: 0.0,
                        width: half - 1.0,
                        height: h,
                    },
                    Pane {
                        x: half + 1.0,
                        y: 0.0,
                        width: w - half - 1.0,
                        height: h,
                    },
                ]
            }
            ViewLayout::SplitHorizontal => {
                let half = (h * 0.5).floor();
                vec![
                    Pane {
                        x: 0.0,
                        y: 0.0,
                        width: w,
                        height: half - 1.0,
                    },
                    Pane {
                        x: 0.0,
                        y: half + 1.0,
                        width: w,
                        height: h - half - 1.0,
                    },
                ]
            }
        }
    }

    fn active_pane_index(&self) -> usize {
        if self.view.display.layout == ViewLayout::Single {
            return 0;
        }
        let panes = self.compute_panes();
        hit_test_pane(&panes, self.input.cursor_pos)
    }

    fn compute_divider_rect(&self) -> Option<egui::Rect> {
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        let ppp = self.window.scale_factor() as f32;
        match self.view.display.layout {
            ViewLayout::Single => None,
            ViewLayout::SplitVertical => {
                let cx = (w * 0.5).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2((cx - 1.0) / ppp, 0.0),
                    egui::vec2(2.0 / ppp, h / ppp),
                ))
            }
            ViewLayout::SplitHorizontal => {
                let cy = (h * 0.5).floor();
                Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, (cy - 1.0) / ppp),
                    egui::vec2(w / ppp, 2.0 / ppp),
                ))
            }
        }
    }

    pub fn render(&mut self) -> anyhow::Result<()> {
        self.window.request_redraw();
        if !self.is_surface_configured {
            return Ok(());
        }

        let frame_ms = self.dt * 1000.0;
        self.gui.clear_expired_toast();

        let output = self.surface.get_current_texture()?;
        let surface_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.poll_overlap_stats();

        let panes = self.compute_panes();
        let is_split = panes.len() > 1;

        for (i, pane) in panes.iter().enumerate() {
            self.render_pane(i, pane, &surface_view, is_split);
        }

        self.render_gui_overlay(&output, &panes, is_split, frame_ms);
        output.present();
        Ok(())
    }

    fn render_pane(
        &mut self,
        i: usize,
        pane: &Pane,
        surface_view: &wgpu::TextureView,
        is_split: bool,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Pane Encoder"),
            });
        let pane_aspect = pane.width / pane.height;

        let cam_data = if i == 0 {
            self.scene.as_ref().map(|s| s.cam.camera)
        } else {
            self.view
                .secondary_cam
                .as_ref()
                .map(|c| c.camera)
                .or(self.scene.as_ref().map(|s| s.cam.camera))
        };

        let pds = self.view.pane_settings[i.min(1)];

        let Some(cam_data) = cam_data else {
            self.renderer.render_empty_pass(&mut encoder, &pds);
            self.composite_and_submit(encoder, surface_view, i, pane, is_split, false);
            return;
        };

        let is_uv_map = pds.pane_mode == PaneMode::UvMap;

        if is_uv_map {
            self.render_uv_map_pane(&mut encoder, pane_aspect, &pds);
        } else {
            if i == 0 {
                if let Some(scene) = &mut self.scene {
                    scene.cam.write_with_aspect(&self.queue, pane_aspect);
                }
            } else if let Some(sec) = &mut self.view.secondary_cam {
                sec.write_with_aspect(&self.queue, pane_aspect);
            }

            if is_split && i == 1 {
                self.setup_split_secondary(&cam_data);
            }

            self.write_3d_pane_uniforms(i, &pds);
            self.render_3d_passes(&mut encoder, i, &cam_data, &pds);
        }

        self.composite_and_submit(encoder, surface_view, i, pane, is_split, is_uv_map);
    }

    fn composite_and_submit(
        &self,
        mut encoder: wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        i: usize,
        pane: &Pane,
        is_split: bool,
        is_uv_map: bool,
    ) {
        let pane_bloom = self.renderer.post.bloom_enabled && !is_uv_map;
        let pane_ssao = self.renderer.post.ssao_enabled && !is_uv_map;
        self.renderer.post.composite.write_params(
            &self.queue,
            pane_bloom,
            pane_ssao,
            self.renderer.post.tone_mode,
            self.renderer.post.exposure,
        );
        let viewport = if is_split {
            Some([pane.x, pane.y, pane.width, pane.height])
        } else {
            None
        };
        self.renderer.post.composite.render(
            &mut encoder,
            &self.renderer.pipelines,
            surface_view,
            pane_ssao,
            &self.renderer.post.ssao,
            viewport,
            i == 0,
        );
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    fn render_uv_map_pane(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pane_aspect: f32,
        pds: &PaneDisplaySettings,
    ) {
        if let Some(scene) = &self.scene {
            if scene.model.has_uvs {
                self.renderer
                    .uv_cam
                    .write(&self.queue, pds.uv_offset, pds.uv_zoom, pane_aspect);
                let uv_wire = WireframeParams {
                    color: [0.8, 0.8, 0.8, 1.0],
                    line_width: pds.line_weight.width_px(),
                    screen_width: self.renderer.target_width as f32,
                    screen_height: self.renderer.target_height as f32,
                    _pad: 0.0,
                };
                self.queue.write_buffer(
                    &self.renderer.wire.wireframe_params_buffer,
                    0,
                    bytemuck::bytes_of(&uv_wire),
                );
                if pds.show_uv_overlap {
                    self.renderer.render_uv_overlap_count_pass(
                        encoder,
                        scene,
                        &self.renderer.uv_cam.bind_group,
                        &self.renderer.uv_overlap.count_view,
                    );
                    if self.renderer.uv_overlap.stats_dirty
                        && !self.renderer.uv_overlap.readback_pending
                    {
                        self.renderer
                            .uv_cam
                            .write(&self.queue, [0.0, 0.0], 1.0, 1.0);
                        self.renderer.render_uv_overlap_count_pass(
                            encoder,
                            scene,
                            &self.renderer.uv_cam.bind_group,
                            &self.renderer.uv_overlap.stats_view,
                        );
                        request_overlap_readback_impl(
                            &self.device,
                            &mut self.renderer.uv_overlap,
                            encoder,
                        );
                        self.renderer.uv_cam.write(
                            &self.queue,
                            pds.uv_offset,
                            pds.uv_zoom,
                            pane_aspect,
                        );
                    }
                }
                if pds.uv_bg == UvMapBackground::Dark {
                    let dark = GradientUniform {
                        top_color: [0.10, 0.10, 0.10, 1.0],
                        bottom_color: [0.10, 0.10, 0.10, 1.0],
                        uv_y_offset: 0.0,
                        uv_y_scale: 1.0,
                        _pad: [0.0; 2],
                    };
                    self.queue.write_buffer(
                        &self.renderer.wire._gradient_buffer,
                        0,
                        bytemuck::bytes_of(&dark),
                    );
                }
                self.renderer.render_uv_map_pass(
                    encoder,
                    scene,
                    &self.renderer.uv_cam.bind_group,
                    pds,
                );
            } else {
                self.renderer.render_empty_pass(encoder, pds);
            }
        } else {
            self.renderer.render_empty_pass(encoder, pds);
        }
    }

    fn write_3d_pane_uniforms(&self, i: usize, pds: &PaneDisplaySettings) {
        self.write_wireframe_params_for(pds);
        self.write_gradient_colors_for(pds);
        if let Some(scene) = &self.scene {
            let color = pds.background_mode.grid_color();
            self.queue.write_buffer(
                &scene.vis.grid_uniform_buf,
                GridUniform::COLOR_OFFSET,
                bytemuck::cast_slice(&color),
            );
        }

        let (cam_buf, depth_bounds) = if i == 0 {
            (
                self.scene.as_ref().map(|s| &s.cam.buffer),
                self.scene
                    .as_ref()
                    .map(|s| Self::compute_depth_bounds(&s.cam.camera, &s.model.bounds)),
            )
        } else {
            (
                self.view.secondary_cam.as_ref().map(|c| &c.buffer),
                self.view
                    .secondary_cam
                    .as_ref()
                    .zip(self.scene.as_ref())
                    .map(|(c, s)| Self::compute_depth_bounds(&c.camera, &s.model.bounds)),
            )
        };
        if let Some(buf) = cam_buf {
            let (depth_near, depth_far) = depth_bounds.unwrap_or((0.01, 100.0));
            let data: [u32; 5] = [
                pds.inspection_mode.as_u32(),
                pds.texel_density_target.to_bits(),
                pds.material_override.as_u32(),
                depth_near.to_bits(),
                depth_far.to_bits(),
            ];
            self.queue.write_buffer(
                buf,
                CameraUniform::INSPECTION_OFFSET,
                bytemuck::cast_slice(&data),
            );
        }
    }

    fn compute_depth_bounds(
        camera: &crate::cgi::camera::Camera,
        bounds: &crate::aabb::AABB,
    ) -> (f32, f32) {
        let view = camera.build_view_matrix();
        let mut z_min = f32::INFINITY;
        let mut z_max = f32::NEG_INFINITY;
        for corner in &bounds.corners() {
            let vp = view * corner.to_homogeneous();
            let z = -vp.z;
            z_min = z_min.min(z);
            z_max = z_max.max(z);
        }
        z_min = z_min.max(0.001);
        if z_max <= z_min {
            z_max = z_min + 1.0;
        }
        (z_min, z_max)
    }

    fn render_3d_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        i: usize,
        cam_data: &Camera,
        pds: &PaneDisplaySettings,
    ) {
        if (i == 0 || !self.view.display.lights_locked)
            && let Some(scene) = &self.scene
        {
            self.renderer.render_shadow_pass(encoder, scene);
        }

        let cam_bg = if i == 0 {
            self.scene.as_ref().map(|s| &s.cam.bind_group)
        } else {
            self.view
                .secondary_cam
                .as_ref()
                .map(|c| &c.bind_group)
                .or(self.scene.as_ref().map(|s| &s.cam.bind_group))
        };
        if let (Some(scene), Some(cam_bg)) = (&self.scene, cam_bg) {
            if self.renderer.post.ssao_enabled {
                self.renderer.render_gbuffer_pass(encoder, scene, cam_bg);
            }
            self.renderer
                .render_main_pass(encoder, scene, cam_bg, cam_data, pds);
        } else {
            self.renderer.render_empty_pass(encoder, pds);
        }

        if self.renderer.post.ssao_enabled
            && let Some(cam_bg) = cam_bg
        {
            self.renderer.render_ssao_passes(encoder, cam_bg);
        }

        if self.renderer.post.bloom_enabled {
            self.renderer.post.bloom.render(
                encoder,
                &self.renderer.pipelines,
                &self.queue,
                self.renderer.target_width,
                self.renderer.target_height,
            );
        }
    }

    fn setup_split_secondary(&mut self, cam_data: &Camera) {
        if !self.view.display.lights_locked
            && let Some(scene) = &mut self.scene
        {
            scene.lights_uniform = lights_from_camera(cam_data, &scene.model.bounds);
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

    fn render_gui_overlay(
        &mut self,
        output: &wgpu::SurfaceTexture,
        panes: &[Pane],
        is_split: bool,
        frame_ms: f32,
    ) {
        use crate::cgi::gui::{GuiSnapshot, HudInfo};

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("UI Encoder"),
            });

        let divider_rect = self.compute_divider_rect();

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        let active_pane_rect = if is_split {
            let ppp = self.window.scale_factor() as f32;
            panes.get(self.view.active_pane).map(|p| {
                egui::Rect::from_min_size(
                    egui::pos2(p.x / ppp, p.y / ppp),
                    egui::vec2(p.width / ppp, p.height / ppp),
                )
            })
        } else {
            None
        };

        let ap = self.view.active_pane;
        let pds = &self.view.pane_settings[ap];

        let pane_label = {
            let pane_mode_str = match pds.pane_mode {
                PaneMode::Scene3D => "Scene3D",
                PaneMode::UvMap => "UV Map",
            };
            let mut label = if is_split {
                let mode_detail = if pds.pane_mode == PaneMode::Scene3D {
                    format!("{} \u{00b7} {}", pane_mode_str, pds.view_mode)
                } else {
                    pane_mode_str.to_string()
                };
                format!("Pane {} \u{00b7} {}", ap + 1, mode_detail)
            } else if pds.pane_mode == PaneMode::Scene3D {
                format!("{} \u{00b7} {}", pane_mode_str, pds.view_mode)
            } else {
                pane_mode_str.to_string()
            };
            if pds.material_override != MaterialOverride::None {
                label = format!("{} \u{00b7} {}", label, pds.material_override);
            }
            label
        };

        let snap_before = GuiSnapshot::from_state(
            pds,
            &self.view.display,
            &self.renderer.post,
            self.renderer.ibl_res.ibl_mode,
            self.view.cameras_linked,
            is_split,
        );
        let hud = HudInfo {
            pane_label,
            cameras_linked: if is_split {
                Some(self.view.cameras_linked)
            } else {
                None
            },
            has_uvs: self.scene.as_ref().is_some_and(|s| s.model.has_uvs),
            uv_overlap_pct: self.renderer.uv_overlap.overlap_pct,
        };
        let validation_report = self.scene.as_ref().map(|s| &s.validation);

        let snap_after = self.gui.render_ui(
            snap_before,
            &hud,
            validation_report,
            &self.device,
            &self.queue,
            &mut encoder,
            &self.window,
            &output.texture,
            screen,
            frame_ms,
            divider_rect,
            active_pane_rect,
        );

        let changes = snap_after.diff(&snap_before);
        snap_after.write_back_pane(&mut self.view.pane_settings[ap]);
        snap_after.write_back_display(&mut self.view.display);
        snap_after.write_back_post(&mut self.renderer.post);
        self.renderer.ibl_res.ibl_mode = snap_after.ibl_mode;
        self.view.cameras_linked = snap_after.cameras_linked;

        if changes.background_changed {
            self.apply_background_change();
        } else if changes.wireframe_params_changed {
            self.update_wireframe_params();
        }
        if changes.composite_params_changed {
            self.apply_composite_params();
        }
        if changes.ibl_changed {
            self.apply_ibl_change();
        }

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
    }

    fn poll_overlap_stats(&mut self) {
        if !self.renderer.uv_overlap.readback_pending {
            return;
        }
        let Some(buf) = self.renderer.uv_overlap.staging_buffer.take() else {
            return;
        };
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        if rx.recv().is_ok_and(|r| r.is_ok()) {
            let data = slice.get_mapped_range();
            let mut total_nonzero = 0u64;
            let mut overlap = 0u64;
            for &byte in data.iter() {
                if byte > 0 {
                    total_nonzero += 1;
                }
                if byte > 1 {
                    overlap += 1;
                }
            }
            drop(data);
            buf.unmap();
            self.renderer.uv_overlap.overlap_pct = if total_nonzero > 0 {
                Some(overlap as f32 / total_nonzero as f32 * 100.0)
            } else {
                Some(0.0)
            };
        }
        self.renderer.uv_overlap.readback_pending = false;
    }
}

fn request_overlap_readback_impl(
    device: &wgpu::Device,
    uv_overlap: &mut UvOverlapResources,
    encoder: &mut wgpu::CommandEncoder,
) {
    const STATS_SIZE: u32 = 512;
    let bytes_per_row = STATS_SIZE;
    let buffer_size = u64::from(bytes_per_row * STATS_SIZE);
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("UV Overlap Readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &uv_overlap.stats_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(STATS_SIZE),
            },
        },
        wgpu::Extent3d {
            width: STATS_SIZE,
            height: STATS_SIZE,
            depth_or_array_layers: 1,
        },
    );
    uv_overlap.staging_buffer = Some(staging);
    uv_overlap.readback_pending = true;
    uv_overlap.stats_dirty = false;
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

fn compute_target_dimensions(layout: ViewLayout, width: u32, height: u32) -> (u32, u32) {
    match layout {
        ViewLayout::Single => (width, height),
        ViewLayout::SplitVertical => {
            let half = (width as f32 * 0.5).floor() as u32;
            (half.max(1), height)
        }
        ViewLayout::SplitHorizontal => {
            let half = (height as f32 * 0.5).floor() as u32;
            (width, half.max(1))
        }
    }
}

fn hit_test_pane(panes: &[Pane], cursor: (f32, f32)) -> usize {
    let (cx, cy) = cursor;
    for (i, pane) in panes.iter().enumerate() {
        if cx >= pane.x && cx < pane.x + pane.width && cy >= pane.y && cy < pane.y + pane.height {
            return i;
        }
    }
    0
}

fn cam_routing(active_pane: usize, cameras_linked: bool) -> (bool, bool) {
    (
        active_pane == 0 || cameras_linked,
        active_pane == 1 || cameras_linked,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(x: f32, y: f32, width: f32, height: f32) -> Pane {
        Pane {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn hit_test_single_pane() {
        let panes = [pane(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(hit_test_pane(&panes, (500.0, 500.0)), 0);
        assert_eq!(hit_test_pane(&panes, (0.0, 0.0)), 0);
        assert_eq!(hit_test_pane(&panes, (1919.0, 1079.0)), 0);
    }

    #[test]
    fn hit_test_vertical_split() {
        let half = 960.0_f32;
        let panes = [
            pane(0.0, 0.0, half - 1.0, 1080.0),
            pane(half + 1.0, 0.0, 1920.0 - half - 1.0, 1080.0),
        ];

        assert_eq!(hit_test_pane(&panes, (100.0, 500.0)), 0);
        assert_eq!(hit_test_pane(&panes, (958.0, 500.0)), 0);

        assert_eq!(hit_test_pane(&panes, (962.0, 500.0)), 1);
        assert_eq!(hit_test_pane(&panes, (1500.0, 500.0)), 1);

        assert_eq!(hit_test_pane(&panes, (960.0, 500.0)), 0);
    }

    #[test]
    fn hit_test_horizontal_split() {
        let half = 540.0_f32;
        let panes = [
            pane(0.0, 0.0, 1920.0, half - 1.0),
            pane(0.0, half + 1.0, 1920.0, 1080.0 - half - 1.0),
        ];

        assert_eq!(hit_test_pane(&panes, (500.0, 100.0)), 0);
        assert_eq!(hit_test_pane(&panes, (500.0, 600.0)), 1);
        assert_eq!(hit_test_pane(&panes, (500.0, 540.0)), 0);
    }

    #[test]
    fn hit_test_cursor_outside_window() {
        let panes = [pane(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(hit_test_pane(&panes, (-10.0, 500.0)), 0);
        assert_eq!(hit_test_pane(&panes, (2000.0, 500.0)), 0);
    }

    #[test]
    fn hit_test_exact_boundaries() {
        let panes = [pane(0.0, 0.0, 100.0, 100.0), pane(102.0, 0.0, 100.0, 100.0)];
        assert_eq!(hit_test_pane(&panes, (0.0, 0.0)), 0);
        assert_eq!(hit_test_pane(&panes, (99.9, 50.0)), 0);
        assert_eq!(hit_test_pane(&panes, (100.0, 50.0)), 0);
        assert_eq!(hit_test_pane(&panes, (102.0, 0.0)), 1);
    }

    #[test]
    fn hit_test_empty_panes() {
        let panes: [Pane; 0] = [];
        assert_eq!(hit_test_pane(&panes, (500.0, 500.0)), 0);
    }

    #[test]
    fn cam_routing_single_pane() {
        assert_eq!(cam_routing(0, false), (true, false));
    }

    #[test]
    fn cam_routing_split_unlinked() {
        assert_eq!(cam_routing(0, false), (true, false));
        assert_eq!(cam_routing(1, false), (false, true));
    }

    #[test]
    fn cam_routing_split_linked() {
        assert_eq!(cam_routing(0, true), (true, true));
        assert_eq!(cam_routing(1, true), (true, true));
    }

    #[test]
    fn target_dims_single() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::Single, 1920, 1080),
            (1920, 1080)
        );
    }

    #[test]
    fn target_dims_vertical_split() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitVertical, 1920, 1080),
            (960, 1080)
        );
    }

    #[test]
    fn target_dims_horizontal_split() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitHorizontal, 1920, 1080),
            (1920, 540)
        );
    }

    #[test]
    fn target_dims_odd_width() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitVertical, 1921, 1080),
            (960, 1080)
        );
    }

    #[test]
    fn target_dims_minimum() {
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitVertical, 2, 2),
            (1, 2)
        );
        assert_eq!(
            compute_target_dimensions(ViewLayout::SplitHorizontal, 2, 2),
            (2, 1)
        );
    }
}
