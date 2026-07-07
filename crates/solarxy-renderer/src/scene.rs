//! [`ModelScene`]: per-loaded-model GPU state — vertex/index buffers, bind
//! groups, shadow state, validation map. Plus the [`lights_from_camera`] and
//! [`create_light_bind_group`] / [`create_light_bind_group_selective`]
//! helpers used by both `ModelScene` construction and the per-frame update.

// Imports used only by the std-fs-gated `ModelScene::new` are gated with
// it so the no-std-fs (wasm) build stays warning-free.
#[cfg(feature = "std-fs")]
use cgmath::Rotation3;
use solarxy_core::preferences::{BgKind, ResolvedBackground};
use solarxy_core::validation::ValidationReport;
#[cfg(feature = "std-fs")]
use wgpu::util::DeviceExt;

use crate::bind_groups::BindGroupLayouts;
#[cfg(feature = "std-fs")]
use crate::camera::camera_from_bounds;
use crate::camera::Camera;
use crate::ibl::{BrdfLut, IblState};
use crate::light::{LightEntry, LightsUniform};
use crate::model::Model;
#[cfg(feature = "std-fs")]
use crate::pipelines::Instance;
use crate::resources::ModelStats;
#[cfg(feature = "std-fs")]
use crate::resources::{self};
use crate::shadow::ShadowState;
#[cfg(feature = "std-fs")]
use crate::validation;
use crate::visualization::VisualizationState;

pub trait BackgroundModeExt {
    fn clear_color(self) -> wgpu::Color;
    fn wireframe_color(self) -> [f32; 4];
    fn sky_colors(self) -> ([f32; 3], [f32; 3]);
    fn grid_color(self) -> [f32; 3];
    fn effective_luminance(self) -> f32;
}

impl BackgroundModeExt for ResolvedBackground {
    fn clear_color(self) -> wgpu::Color {
        wgpu::Color {
            r: f64::from(self.clear[0]),
            g: f64::from(self.clear[1]),
            b: f64::from(self.clear[2]),
            a: 1.0,
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
        (self.sky_top, self.sky_bottom)
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
        let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        // A gradient's contrast tracks the mean of its sky band; a solid
        // (or the pre-load HDRI fallback) tracks the flat clear colour.
        if self.kind == BgKind::Gradient {
            (lum(self.sky_top) + lum(self.sky_bottom)) * 0.5
        } else {
            lum(self.clear)
        }
    }
}

pub struct ModelScene {
    pub model: Model,
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
    pub validation_edge_buffers: Vec<Option<(wgpu::Buffer, u32)>>,
    /// Raw-mesh-index → GPU-mesh-index map (empty raw meshes are filtered
    /// out). Retained so a validation issue's raw `IssueScope` index can be
    /// remapped to `Model::mesh_bounds` for camera fly-to.
    pub validation_raw_to_gpu: Vec<Option<usize>>,
}

impl ModelScene {
    /// Load a model file from disk and build its full GPU scene state.
    /// Path-based by nature; a byte-fed scene assembles the same public
    /// fields from `resources::upload_model` output (multi-object scenes
    /// replace this in the web milestone's phase 2).
    #[cfg(feature = "std-fs")]
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
    ) -> Result<Self, crate::error::RendererError> {
        let (model, normals_geo, stats, viewer_validation) = resources::load_model_any(
            &model_path,
            device,
            queue,
            &layouts.texture,
            &layouts.edge_geometry,
        )?;

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
        // Pane cameras now live in the view layer (`ViewState::cameras`);
        // a throwaway bounds-framed camera seeds the initial light rig.
        let initial_cam =
            camera_from_bounds(&model.bounds, config.width as f32 / config.height as f32);
        let lights_uniform = lights_from_camera(
            &initial_cam,
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

        let edge_index_lists = validation::build_mesh_edge_indices(
            &viewer_validation.report,
            model.meshes.len(),
            &viewer_validation.raw_to_gpu,
        );
        let validation_edge_buffers: Vec<Option<(wgpu::Buffer, u32)>> = edge_index_lists
            .into_iter()
            .enumerate()
            .map(|(mi, indices)| {
                if indices.is_empty() {
                    None
                } else {
                    let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("Validation Edge Indices {mi}")),
                        contents: bytemuck::cast_slice(&indices),
                        usage: wgpu::BufferUsages::INDEX,
                    });
                    Some((buf, indices.len() as u32))
                }
            })
            .collect();

        Ok(ModelScene {
            model,
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
            validation_edge_buffers,
            validation_raw_to_gpu: viewer_validation.raw_to_gpu,
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
