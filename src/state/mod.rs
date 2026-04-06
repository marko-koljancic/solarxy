mod capture;
mod init;
mod input;
mod render;
mod update;

use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::bloom::BloomState;
use crate::cgi::camera::Camera;
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
use crate::cgi::visualization::VisualizationState;
use crate::preferences::{
    self, BackgroundMode, IblMode, LineWeight, NormalsMode, Preferences, ToneMode, UvMode, ViewMode,
};
use cgmath::Rotation3;
use std::sync::{mpsc, Arc};
use std::time::Instant;
use wgpu::util::DeviceExt;
use winit::{keyboard::ModifiersState, window::Window};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BoundsMode {
    Off,
    WholeModel,
    PerMesh,
}

impl BoundsMode {
    pub const ALL: &[Self] = &[Self::Off, Self::WholeModel, Self::PerMesh];
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

#[derive(Clone, Copy, PartialEq, Default)]
pub(crate) enum ViewLayout {
    #[default]
    Single,
    SplitVertical,
    SplitHorizontal,
}

struct Pane {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
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

        let shadow = ShadowState::new(device, layouts, &lights_uniform, &model, shadow_map_size);
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

pub(super) struct PendingLoad {
    receiver: mpsc::Receiver<anyhow::Result<ModelScene>>,
    filename: String,
    path: String,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GradientUniform {
    top_color: [f32; 4],
    bottom_color: [f32; 4],
    uv_y_offset: f32,
    uv_y_scale: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct WireframeParams {
    color: [f32; 4],
    line_width: f32,
    screen_width: f32,
    screen_height: f32,
    _pad: f32,
}

pub(super) struct RenderTargets {
    pub depth_texture: texture::Texture,
    pub msaa_hdr_view: wgpu::TextureView,
    pub _hdr_resolve_texture: wgpu::Texture,
    pub hdr_resolve_view: wgpu::TextureView,
}

pub(super) struct PostProcessing {
    pub bloom: BloomState,
    pub bloom_enabled: bool,
    pub ssao: SsaoState,
    pub ssao_enabled: bool,
    pub composite: CompositeState,
    pub tone_mode: ToneMode,
    pub exposure: f32,
}

pub(super) struct IblResources {
    pub ibl: IblState,
    pub ibl_fallback: IblState,
    pub brdf_lut: BrdfLut,
    pub ibl_mode: IblMode,
    pub last_active_ibl_mode: IblMode,
}

#[derive(Clone)]
pub(super) struct PaneDisplaySettings {
    pub view_mode: ViewMode,
    pub prev_non_ghosted_mode: ViewMode,
    pub ghosted_wireframe: bool,
    pub normals_mode: NormalsMode,
    pub background_mode: BackgroundMode,
    pub uv_mode: UvMode,
    pub bounds_mode: BoundsMode,
    pub line_weight: LineWeight,
    pub show_grid: bool,
    pub show_axis_gizmo: bool,
    pub show_local_axes: bool,
}

pub(super) struct DisplaySettings {
    pub turntable_active: bool,
    pub turntable_rpm: f32,
    pub lights_locked: bool,
    pub layout: ViewLayout,
}

pub(super) struct WireframeResources {
    pub _gradient_buffer: wgpu::Buffer,
    pub gradient_bind_group: wgpu::BindGroup,
    pub wireframe_params_buffer: wgpu::Buffer,
    pub wireframe_params_bind_group: wgpu::BindGroup,
    pub _checker_texture: texture::Texture,
    pub uv_checker_bind_group: wgpu::BindGroup,
}

pub struct State {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) is_surface_configured: bool,
    pub(super) targets: RenderTargets,
    pub(super) post: PostProcessing,
    pub(super) ibl_res: IblResources,
    pub(super) display: DisplaySettings,
    pub(super) wire: WireframeResources,
    pub(super) layouts: Arc<BindGroupLayouts>,
    pub(super) pipelines: Pipelines,
    pub(super) gui: EguiRenderer,
    pub(super) scene: Option<ModelScene>,
    pub(super) pane_settings: [PaneDisplaySettings; 2],
    pub(super) secondary_cam: Option<CameraState>,
    pub(super) active_pane: usize,
    pub(super) cursor_pos: (f32, f32),
    pub(super) cameras_linked: bool,
    pub(super) pending_load: Option<PendingLoad>,
    pub(super) capture_requested: bool,
    pub(super) modifiers: ModifiersState,
    pub(super) last_frame_time: Instant,
    pub(super) dt: f32,
    pub(super) _backend_info: String,
    pub(super) preferences: Preferences,
    #[allow(unused)]
    pub(super) shared_samplers: SharedSamplers,
    pub(super) msaa_sample_count: u32,
    pub(super) target_width: u32,
    pub(super) target_height: u32,
    pub window: Arc<Window>,
}

impl State {
    pub(super) fn target_dimensions(&self) -> (u32, u32) {
        compute_target_dimensions(self.display.layout, self.config.width, self.config.height)
    }

    fn compute_panes(&self) -> Vec<Pane> {
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        match self.display.layout {
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
        if self.display.layout == ViewLayout::Single {
            return 0;
        }
        let panes = self.compute_panes();
        hit_test_pane(&panes, self.cursor_pos)
    }

    fn compute_divider_rect(&self) -> Option<egui::Rect> {
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        let ppp = self.window.scale_factor() as f32;
        match self.display.layout {
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
        use crate::cgi::gui::SidebarState;

        self.window.request_redraw();
        if !self.is_surface_configured {
            return Ok(());
        }

        let frame_ms = self.dt * 1000.0;

        self.gui.clear_expired_toast();

        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let panes = self.compute_panes();
        let is_split = panes.len() > 1;

        for (i, pane) in panes.iter().enumerate() {
            let pane_aspect = pane.width / pane.height;

            let cam_data = if i == 0 {
                self.scene.as_ref().map(|s| s.cam.camera)
            } else {
                self.secondary_cam
                    .as_ref()
                    .map(|c| c.camera)
                    .or(self.scene.as_ref().map(|s| s.cam.camera))
            };

            let pds = self.pane_settings[i.min(1)].clone();

            let Some(cam_data) = cam_data else {
                self.render_empty_pass(&mut encoder, &pds);
                let viewport = if is_split {
                    Some([pane.x, pane.y, pane.width, pane.height])
                } else {
                    None
                };
                self.post.composite.render(
                    &mut encoder,
                    &self.pipelines,
                    &view,
                    self.post.ssao_enabled,
                    &self.post.ssao,
                    viewport,
                    i == 0,
                );
                continue;
            };

            if i == 0 {
                if let Some(scene) = &mut self.scene {
                    scene.cam.write_with_aspect(&self.queue, pane_aspect);
                }
            } else if let Some(sec) = &mut self.secondary_cam {
                sec.write_with_aspect(&self.queue, pane_aspect);
            }

            if is_split && i == 1 {
                if !self.display.lights_locked
                    && let Some(scene) = &mut self.scene
                {
                    scene.lights_uniform = lights_from_camera(&cam_data, &scene.model.bounds);
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
                if let Some(sec) = &self.secondary_cam {
                    self.post
                        .ssao
                        .rebuild_bind_groups(&self.device, &self.layouts, &sec.buffer);
                }
                if let Some(sec_buf) = self.secondary_cam.as_ref().map(|c| &c.buffer)
                    && let Some(scene) = &mut self.scene
                {
                    scene
                        .vis
                        .rebuild_camera_bind_groups(&self.device, &self.layouts, sec_buf);
                }
            }

            self.write_wireframe_params_for(&pds);
            self.write_gradient_colors_for(&pds);
            if let Some(scene) = &self.scene {
                let color = pds.background_mode.grid_color();
                self.queue.write_buffer(
                    &scene.vis.grid_uniform_buf,
                    4,
                    bytemuck::cast_slice(&color),
                );
            }

            if (i == 0 || !self.display.lights_locked)
                && let Some(scene) = &self.scene
            {
                self.render_shadow_pass(&mut encoder, scene);
            }

            let cam_bg = if i == 0 {
                self.scene.as_ref().map(|s| &s.cam.bind_group)
            } else {
                self.secondary_cam
                    .as_ref()
                    .map(|c| &c.bind_group)
                    .or(self.scene.as_ref().map(|s| &s.cam.bind_group))
            };
            if let (Some(scene), Some(cam_bg)) = (&self.scene, cam_bg) {
                if self.post.ssao_enabled {
                    self.render_gbuffer_pass(&mut encoder, scene, cam_bg);
                }
                self.render_main_pass(&mut encoder, scene, cam_bg, &cam_data, &pds);
            } else {
                self.render_empty_pass(&mut encoder, &pds);
            }

            if self.post.ssao_enabled {
                self.render_ssao_passes(&mut encoder);
            }

            if self.post.bloom_enabled {
                self.post.bloom.render(
                    &mut encoder,
                    &self.pipelines,
                    &self.queue,
                    self.target_width,
                    self.target_height,
                );
            }

            let viewport = if is_split {
                Some([pane.x, pane.y, pane.width, pane.height])
            } else {
                None
            };
            self.post.composite.render(
                &mut encoder,
                &self.pipelines,
                &view,
                self.post.ssao_enabled,
                &self.post.ssao,
                viewport,
                i == 0,
            );
        }

        let divider_rect = self.compute_divider_rect();

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        let active_pane_rect = if is_split {
            let ppp = self.window.scale_factor() as f32;
            panes.get(self.active_pane).map(|p| {
                egui::Rect::from_min_size(
                    egui::pos2(p.x / ppp, p.y / ppp),
                    egui::vec2(p.width / ppp, p.height / ppp),
                )
            })
        } else {
            None
        };

        let ap = self.active_pane;
        let pds = &mut self.pane_settings[ap];
        let mut sidebar = SidebarState {
            view_mode: &mut pds.view_mode,
            normals_mode: &mut pds.normals_mode,
            background_mode: &mut pds.background_mode,
            uv_mode: &mut pds.uv_mode,
            bounds_mode: &mut pds.bounds_mode,
            line_weight: &mut pds.line_weight,
            show_grid: &mut pds.show_grid,
            show_axis_gizmo: &mut pds.show_axis_gizmo,
            show_local_axes: &mut pds.show_local_axes,
            turntable_active: &mut self.display.turntable_active,
            turntable_rpm: &mut self.display.turntable_rpm,
            lights_locked: &mut self.display.lights_locked,
            bloom_enabled: &mut self.post.bloom_enabled,
            ssao_enabled: &mut self.post.ssao_enabled,
            tone_mode: &mut self.post.tone_mode,
            exposure: &mut self.post.exposure,
            ibl_mode: &mut self.ibl_res.ibl_mode,
            cameras_linked: &mut self.cameras_linked,
            is_split,
        };
        let changes = self.gui.render_ui(
            &mut sidebar,
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
