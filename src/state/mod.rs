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
use crate::cgi::hud::HudRenderer;
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

pub(super) struct DisplaySettings {
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
    pub turntable_active: bool,
    pub turntable_rpm: f32,
    pub lights_locked: bool,
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
    pub(super) hud: HudRenderer,
    pub(super) gui: EguiRenderer,
    pub(super) scene: Option<ModelScene>,
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
    pub window: Arc<Window>,
}

impl State {
    pub fn render(&mut self) -> anyhow::Result<()> {
        use crate::cgi::gui::SidebarState;

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
            if self.post.ssao_enabled {
                self.render_gbuffer_pass(&mut encoder, scene);
            }
            self.render_main_pass(&mut encoder, scene);
        } else {
            self.render_empty_pass(&mut encoder);
        }

        if self.post.ssao_enabled {
            self.render_ssao_passes(&mut encoder);
        }

        if self.post.bloom_enabled {
            self.post.bloom.render(
                &mut encoder,
                &self.pipelines,
                &self.queue,
                self.config.width,
                self.config.height,
            );
        }
        self.post.composite.render(
            &mut encoder,
            &self.pipelines,
            &view,
            self.post.ssao_enabled,
            &self.post.ssao,
        );

        let (projection_str, normals_str) = if let Some(scene) = &self.scene {
            (
                scene.cam.camera.projection.to_string(),
                self.display.normals_mode.to_string(),
            )
        } else {
            (String::new(), String::new())
        };

        let mode_str = if self.display.uv_mode != UvMode::Off {
            format!("{} [UV: {}]", self.display.view_mode, self.display.uv_mode)
        } else if self.display.view_mode == ViewMode::Ghosted && self.display.ghosted_wireframe {
            "Ghosted+Wire".to_string()
        } else {
            self.display.view_mode.to_string()
        };

        let bounds_info = match self.display.bounds_mode {
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
            &self.display.background_mode.to_string(),
            {
                let eff = f64::from(self.display.background_mode.effective_luminance());
                wgpu::Color {
                    r: eff,
                    g: eff,
                    b: eff,
                    a: 1.0,
                }
            },
            frame_ms,
            has_model,
            self.display.show_grid,
            self.display.lights_locked,
            self.display.show_axis_gizmo,
            self.display.show_local_axes,
            &self.display.bounds_mode.to_string(),
            &bounds_info,
            &self.display.line_weight.to_string(),
            &self.ibl_res.ibl_mode.to_string(),
            self.post.ssao_enabled,
            &self.post.tone_mode.to_string(),
            self.post.exposure,
        );

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };
        let mut sidebar = SidebarState {
            view_mode: &mut self.display.view_mode,
            normals_mode: &mut self.display.normals_mode,
            background_mode: &mut self.display.background_mode,
            uv_mode: &mut self.display.uv_mode,
            bounds_mode: &mut self.display.bounds_mode,
            line_weight: &mut self.display.line_weight,
            show_grid: &mut self.display.show_grid,
            show_axis_gizmo: &mut self.display.show_axis_gizmo,
            show_local_axes: &mut self.display.show_local_axes,
            turntable_active: &mut self.display.turntable_active,
            turntable_rpm: &mut self.display.turntable_rpm,
            lights_locked: &mut self.display.lights_locked,
            bloom_enabled: &mut self.post.bloom_enabled,
            ssao_enabled: &mut self.post.ssao_enabled,
            tone_mode: &mut self.post.tone_mode,
            exposure: &mut self.post.exposure,
            ibl_mode: &mut self.ibl_res.ibl_mode,
        };
        let changes = self.gui.render_with_sidebar(
            &mut sidebar,
            &self.device,
            &self.queue,
            &mut encoder,
            &self.window,
            &output.texture,
            screen,
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
