//! Per-model GPU scene state. Holds everything the renderer needs to draw a
//! loaded model: GPU buffers, light bind group, shadow state, validation
//! map. Owned by `solarxy_app::State` (one per loaded model) and passed by
//! reference to the per-frame render passes in `frame.rs`.

use cgmath::Rotation3;
use solarxy_core::preferences::BackgroundMode;
use solarxy_core::validation::ValidationReport;
use wgpu::util::DeviceExt;

use crate::bind_groups::BindGroupLayouts;
use crate::camera::Camera;
use crate::camera_state::CameraState;
use crate::ibl::{BrdfLut, IblState};
use crate::light::{LightEntry, LightsUniform};
use crate::model::Model;
use crate::pipelines::Instance;
use crate::resources::{self, ModelStats};
use crate::shadow::ShadowState;
use crate::validation;
use crate::visualization::VisualizationState;

pub trait BackgroundModeExt {
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

pub struct ModelScene {
    pub model: Model,
    pub cam: CameraState,
    pub lights_uniform: LightsUniform,
    pub light_buffer: wgpu::Buffer,
    pub light_bind_group: wgpu::BindGroup,
    pub instance_buffer: wgpu::Buffer,
    pub shadow: ShadowState,
    pub vis: VisualizationState,
    #[allow(dead_code)]
    pub model_path: String,
    pub stats: ModelStats,
    pub validation: ValidationReport,
    pub validation_mesh_cat: Vec<Option<usize>>,
}

impl ModelScene {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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

        let placeholder_ibl = IblState::fallback(device, queue);
        let lights_uniform = lights_from_camera(
            &cam.camera,
            &model.bounds,
            placeholder_ibl.irradiance_average,
        );
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Light VB"),
            contents: bytemuck::cast_slice(&[lights_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let light_bind_group =
            create_light_bind_group(device, layouts, &light_buffer, &placeholder_ibl, brdf_lut);

        let shadow = ShadowState::new(device, layouts, &lights_uniform, &model, shadow_map_size);
        let vis =
            VisualizationState::new(device, layouts, &model, &normals_geo, initial_grid_color);

        let validation_mesh_cat = validation::build_mesh_category_map(
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

pub fn create_light_bind_group(
    device: &wgpu::Device,
    layouts: &BindGroupLayouts,
    light_buffer: &wgpu::Buffer,
    ibl: &IblState,
    brdf_lut: &BrdfLut,
) -> wgpu::BindGroup {
    create_light_bind_group_selective(device, layouts, light_buffer, ibl, ibl, brdf_lut)
}

pub fn create_light_bind_group_selective(
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

pub fn lights_from_camera(
    camera: &Camera,
    bounds: &solarxy_core::AABB,
    ibl_avg: [f32; 3],
) -> LightsUniform {
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
        ibl_avg_r: ibl_avg[0],
        ibl_avg_g: ibl_avg[1],
        ibl_avg_b: ibl_avg[2],
    }
}
