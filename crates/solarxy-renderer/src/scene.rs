//! [`ModelScene`]: per-loaded-model GPU state — the model, its stats and
//! validation resources, and the shared [`SceneEnvironment`] (lights,
//! shadow, instance buffer, visualization). Plus the
//! [`lights_from_camera`] and [`create_light_bind_group`] /
//! [`create_light_bind_group_selective`] helpers used by both
//! construction and the per-frame update.

// Imports used only by the std-fs-gated `ModelScene::new` are gated with
// it so the no-std-fs (wasm) build stays warning-free.
use solarxy_core::preferences::{BgKind, ResolvedBackground};
use solarxy_core::validation::ValidationReport;
#[cfg(feature = "std-fs")]
use wgpu::util::DeviceExt;

use crate::bind_groups::BindGroupLayouts;
use crate::camera::Camera;
use crate::environment::SceneEnvironment;
use crate::frame::ObjectValidationGpu;
use crate::ibl::{BrdfLut, IblState};
use crate::light::{LightEntry, LightsUniform};
use crate::ltc::LtcLuts;
use crate::model::Model;
use crate::resources::ModelStats;
#[cfg(feature = "std-fs")]
use crate::resources::{self};
#[cfg(feature = "std-fs")]
use crate::validation;
#[cfg(feature = "std-fs")]
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
    /// Scene-level GPU state (lights, shadow, instance buffer, vis) —
    /// extracted so shells without a file-loaded model share the type.
    pub env: SceneEnvironment,
    #[allow(dead_code)]
    pub model_path: String,
    pub stats: ModelStats,
    pub validation: ValidationReport,
    /// Per-mesh overlay GPU resources for `validation` (category tints +
    /// non-manifold edge lines), handed to the passes through
    /// [`crate::frame::DrawObject::validation`].
    pub validation_gpu: ObjectValidationGpu,
    /// Raw-mesh-index → GPU-mesh-index map (empty raw meshes are filtered
    /// out). Retained so a validation issue's raw `IssueScope` index can be
    /// remapped to `Model::mesh_bounds` for camera fly-to.
    pub validation_raw_to_gpu: Vec<Option<usize>>,
}

impl ModelScene {
    /// This scene's geometry as one [`crate::frame::DrawObject`] — the
    /// single-model path's contribution to the multi-object draw loop.
    #[must_use]
    pub fn draw_object(&self) -> crate::frame::DrawObject<'_> {
        crate::frame::DrawObject {
            model: &self.model,
            instance_buffer: &self.env.instance_buffer,
            validation: Some(&self.validation_gpu),
            selected: false,
            cast_shadow: true,
            // A file-loaded model is one placement. Instancing arrives
            // through the node engine's cooked geometry, which this path
            // does not have.
            instances: 1,
        }
    }

    /// Load a model file from disk and build its full GPU scene state.
    /// Path-based by nature; a byte-fed scene assembles the same public
    /// fields from `resources::upload_model` output (multi-object scenes
    /// replace this).
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
        ltc: &LtcLuts,
        shadow_map_size: u32,
    ) -> Result<Self, crate::error::RendererError> {
        let (model, normals_geo, stats, viewer_validation) = resources::load_model_any(
            &model_path,
            device,
            queue,
            &layouts.texture,
            &layouts.edge_geometry,
        )?;

        let vis =
            VisualizationState::new(device, layouts, &model, &normals_geo, initial_grid_color);
        let env = SceneEnvironment::new(
            device,
            queue,
            layouts,
            &model.bounds,
            config.width as f32 / config.height as f32,
            brdf_lut,
            ltc,
            shadow_map_size,
            vis,
        );

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
            env,
            model_path,
            stats,
            validation: viewer_validation.report,
            validation_gpu: ObjectValidationGpu {
                mesh_cat: validation_mesh_cat,
                edge_buffers: validation_edge_buffers,
            },
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
    ltc: &LtcLuts,
) -> wgpu::BindGroup {
    create_light_bind_group_selective(device, layouts, light_buffer, ibl, ibl, brdf_lut, ltc)
}

pub fn create_light_bind_group_selective(
    device: &wgpu::Device,
    layouts: &BindGroupLayouts,
    light_buffer: &wgpu::Buffer,
    diffuse_src: &IblState,
    specular_src: &IblState,
    brdf_lut: &BrdfLut,
    ltc: &LtcLuts,
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
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&ltc.transform_view),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&ltc.magnitude_view),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::Sampler(&ltc.sampler),
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

    // The synthesized viewer rig: three point lights with range = 0 and
    // decay = 0, so the generalized attenuation paths all multiply by 1.0
    // and desktop output matches the pre-generalization renderer exactly.
    // The key light (entry 0) is the exclusive shadow caster.
    let rig = |position: cgmath::Point3<f32>, color: [f32; 3], intensity: f32, shadowed: f32| {
        LightEntry {
            position: [position.x, position.y, position.z],
            kind: crate::light::LIGHT_KIND_POINT,
            direction: [0.0, -1.0, 0.0],
            intensity,
            color,
            range: 0.0,
            decay: 0.0,
            cos_inner: 0.0,
            cos_outer: 0.0,
            shadowed,
            // The rig is point lights only, so it carries no rectangle.
            half_x: [0.0; 3],
            two_sided: 0.0,
            half_y: [0.0; 3],
            _pad_entry: 0.0,
        }
    };

    let mut lights = [LightEntry::disabled(); crate::light::MAX_LIGHTS];
    lights[0] = rig(key, [1.0, 0.98, 0.95], 2.0, 1.0);
    lights[1] = rig(fill, [0.90, 0.93, 1.00], 1.0, 0.0);
    lights[2] = rig(rim, [1.0, 1.00, 1.00], 0.8, 0.0);

    LightsUniform {
        lights,
        count: 3,
        sphere_scale: bounds.diagonal() * 0.04,
        ibl_avg_r: ibl_avg[0],
        ibl_avg_g: ibl_avg[1],
        ibl_avg_b: ibl_avg[2],
        hemi_sky_r: 0.0,
        hemi_sky_g: 0.0,
        hemi_sky_b: 0.0,
        hemi_ground_r: 0.0,
        hemi_ground_g: 0.0,
        hemi_ground_b: 0.0,
        ibl_intensity: solarxy_core::view_config::DEFAULT_HDRI_INTENSITY,
    }
}
