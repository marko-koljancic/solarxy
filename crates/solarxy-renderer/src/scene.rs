//! [`ModelScene`]: per-loaded-model GPU state — the model, its stats and
//! validation resources. The scene-level [`SceneEnvironment`] is owned by
//! the shell, not by this type, so a shell renders with or without a file
//! model; [`LoadedModel::load`] builds one alongside the model and returns
//! the pair. Plus the [`lights_from_camera`] and [`create_light_bind_group`]
//! / [`create_light_bind_group_selective`] helpers used by both construction
//! and the per-frame update.

// Imports used only by the std-fs-gated `LoadedModel::load` are gated with
// it so the no-std-fs (wasm) build stays warning-free.
use solarxy_core::preferences::{BgKind, ResolvedBackground};
use solarxy_core::scene::{LightDef, LightKind};
use solarxy_core::validation::ValidationReport;
#[cfg(feature = "std-fs")]
use wgpu::util::DeviceExt;

use crate::bind_groups::BindGroupLayouts;
use crate::camera::Camera;
use crate::environment::SceneEnvironment;
use crate::frame::ObjectValidationGpu;
use crate::ibl::{BrdfLut, IblState};
use crate::light::LightsUniform;
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
            f32::midpoint(lum(self.sky_top), lum(self.sky_bottom))
        } else {
            lum(self.clear)
        }
    }
}

/// A freshly loaded file model and the scene environment built around its
/// bounds.
///
/// The two arrive together because the environment's heavy half is derived
/// from the model: [`crate::visualization::VisualizationState`] builds the
/// normal-arrow line buffers, whose size tracks the triangle count (tens of
/// megabytes for a dense scan). Building both inside [`LoadedModel::load`]
/// keeps that work on whatever thread the caller loads on, which for the
/// desktop shell is the loader worker, and leaves the frame loop with two
/// owned values to move into place.
pub struct LoadedModel {
    pub scene: ModelScene,
    pub env: SceneEnvironment,
}

pub struct ModelScene {
    pub model: Model,
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
    /// file-loaded model's contribution to the multi-object draw loop.
    ///
    /// The instance buffer is a parameter rather than a field read because
    /// the scene environment is not part of this type: the shell owns one
    /// environment for the whole viewport and hands its identity instance
    /// buffer in here.
    #[must_use]
    pub fn draw_object<'a>(
        &'a self,
        instance_buffer: &'a wgpu::Buffer,
    ) -> crate::frame::DrawObject<'a> {
        crate::frame::DrawObject {
            model: &self.model,
            instance_buffer,
            validation: Some(&self.validation_gpu),
            selected: false,
            cast_shadow: true,
        }
    }
}

#[cfg(feature = "std-fs")]
impl LoadedModel {
    /// Load a model file from disk and build its full GPU scene state,
    /// together with the scene environment fitted to its bounds.
    /// Path-based by nature; a byte-fed scene assembles the same public
    /// fields from `resources::upload_model` output (multi-object scenes
    /// replace this).
    ///
    /// The environment is built here, on the caller's thread, because its
    /// visualization half is sized by the model's triangle count. The
    /// desktop shell loads on a worker precisely so that build never lands
    /// on the frame loop.
    #[allow(clippy::too_many_arguments)]
    pub fn load(
        model_path: String,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        config: &wgpu::SurfaceConfiguration,
        initial_grid_color: [f32; 3],
        brdf_lut: &BrdfLut,
        ltc: &LtcLuts,
        shadow_map_size: u32,
    ) -> Result<LoadedModel, crate::error::RendererError> {
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

        Ok(LoadedModel {
            scene: ModelScene {
                model,
                model_path,
                stats,
                validation: viewer_validation.report,
                validation_gpu: ObjectValidationGpu {
                    mesh_cat: validation_mesh_cat,
                    edge_buffers: validation_edge_buffers,
                },
                validation_raw_to_gpu: viewer_validation.raw_to_gpu,
            },
            env,
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

/// The camera-relative rig a scene that authors no lights of its own is lit by.
///
/// Three point lights placed around the camera's target so that a model is
/// visible the moment it loads, with range and decay at zero so every
/// generalized attenuation path multiplies by one, and the key as the exclusive
/// shadow caster.
///
/// # Why this returns definitions rather than uniform entries
///
/// Because two renderers read it, and only one of them reads a uniform. The
/// rasterizer folds these into its lights uniform ([`lights_from_camera`],
/// immediately below); the tracer walks a light array that comes from the scene
/// itself. While the rig existed only as uniform entries, a scene with no light
/// nodes was lit in the viewport and lit by nothing at all in a traced still of
/// the same file. Stating it as scene data removes the special case rather than
/// writing it a second time.
///
/// The intensities are in the units light nodes use, which is what lets both
/// consumers read them without a scale factor of their own.
#[must_use]
pub fn viewer_rig(camera: &Camera) -> [LightDef; 3] {
    use cgmath::InnerSpace;

    let target = camera.target;
    let radius = (camera.eye - camera.target).magnitude() * 2.0;

    let forward = (camera.target - camera.eye).normalize();
    let right = forward.cross(camera.up).normalize();
    let up = right.cross(forward);

    let key_dir = (right * -0.5 + up * 0.8 + (-forward) * 0.5).normalize();
    let fill_dir = (right * 1.0 + up * 0.5 + (-forward) * 0.5).normalize();
    let rim_dir = (right * 0.0 + up * 0.5 + (forward) * 1.5).normalize();

    let light = |position: cgmath::Point3<f32>, color: [f32; 3], intensity: f32, key: bool| {
        LightDef {
            kind: LightKind::Point,
            position: [position.x, position.y, position.z],
            // Unread for a point light by either renderer, and stated rather
            // than left at zero so the definition describes a light rather than
            // a degenerate one.
            direction: [0.0, -1.0, 0.0],
            color,
            intensity,
            range: 0.0,
            decay: 0.0,
            // A mathematical point, so the tracer's soft-shadow sampling
            // collapses to the hard shadow the shadow map draws.
            radius: 0.0,
            inner_cone: 0.0,
            outer_cone: 0.0,
            area_extent: [0.0, 0.0],
            rotate: [0.0; 3],
            two_sided: false,
            ground_color: [0.0; 3],
            cast_shadow: key,
            shadow_map_size: 2048,
            shadow_bias: 0.0,
            visible: true,
            // The rig is not authored, so there is nothing for a helper to
            // point at and nothing a user could select.
            show_helper: false,
            helper_size: 0.0,
        }
    };

    // Key, fill, rim. These were 2.0 / 1.0 / 0.8 while the shader
    // multiplied every light by three; they are the same three lights at
    // the same three brightnesses, now stated in the units the shader
    // actually uses. **This is what makes dropping that multiplier
    // golden-neutral**: the golden scenes carry no light node, so they
    // render entirely on this path, and moving the shader without moving
    // these would darken every capture by two thirds while looking like an
    // ordinary re-baseline.
    [
        light(target + key_dir * radius, [1.0, 0.98, 0.95], 6.0, true),
        light(target + fill_dir * radius, [0.90, 0.93, 1.00], 3.0, false),
        light(target + rim_dir * radius, [1.0, 1.00, 1.00], 2.4, false),
    ]
}

/// The viewer rig as the rasterizer's lights uniform.
///
/// Nothing here decides what the rig *is*; it is [`viewer_rig`] through the same
/// conversion every authored light takes, which is what keeps one rig from
/// becoming two.
pub fn lights_from_camera(
    camera: &Camera,
    bounds: &solarxy_core::AABB,
    ibl_avg: [f32; 3],
) -> LightsUniform {
    LightsUniform::from_defs(&viewer_rig(camera), bounds.diagonal() * 0.04, ibl_avg)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // rig constants and pass-through values, compared bit-exact

    use super::*;
    use cgmath::{InnerSpace, Point3, Vector3};
    use solarxy_core::preferences::ProjectionMode;

    fn camera() -> Camera {
        Camera {
            eye: Point3::new(0.0, 0.0, 4.0),
            target: Point3::new(0.0, 0.0, 0.0),
            up: Vector3::new(0.0, 1.0, 0.0),
            aspect: 1.0,
            fovy: 45.0,
            znear: 0.01,
            zfar: 100.0,
            projection: ProjectionMode::Perspective,
            ortho_scale: 1.0,
        }
    }

    /// The three brightnesses, pinned.
    ///
    /// Every golden capture renders on this rig, because the capture scenes
    /// carry no light node. A change to one of these numbers moves twenty-two
    /// images at once and looks exactly like an ordinary re-baseline, so it is
    /// worth failing here first, where the failure names the value.
    #[test]
    fn the_viewer_rig_is_key_fill_and_rim_at_the_brightnesses_the_captures_expect() {
        let rig = viewer_rig(&camera());
        assert_eq!(
            rig.iter().map(|l| l.intensity).collect::<Vec<_>>(),
            [6.0, 3.0, 2.4]
        );
        assert_eq!(rig[0].color, [1.0, 0.98, 0.95]);
        assert_eq!(rig[1].color, [0.90, 0.93, 1.00]);
        assert_eq!(rig[2].color, [1.0, 1.00, 1.00]);
    }

    /// Unbounded, undecaying point lights, and exactly one caster.
    ///
    /// Range and decay at zero are what make every generalized attenuation path
    /// multiply by one; a non-zero either would dim the rig in the rasterizer
    /// and in the tracer by different amounts, because they attenuate through
    /// different code.
    #[test]
    fn the_rig_is_three_unattenuated_point_lights_with_one_shadow_caster() {
        let rig = viewer_rig(&camera());
        for light in &rig {
            assert_eq!(light.kind, LightKind::Point);
            assert_eq!(light.range, 0.0);
            assert_eq!(light.decay, 0.0);
            assert_eq!(light.radius, 0.0, "a rig light is a mathematical point");
            assert!(light.visible, "an invisible rig light lights nothing");
        }
        assert_eq!(
            rig.iter().filter(|l| l.cast_shadow).count(),
            1,
            "the exclusive shadow caster is the rule the engine enforces for \
             authored lights, and the rig may not break it"
        );
        assert!(rig[0].cast_shadow, "the key is the caster");
    }

    /// The rig the rasterizer uploads is the rig, not a second one.
    ///
    /// This is the join the bug lived at: while the uniform was built by hand
    /// here, the tracer had no way to read the same three lights, and the two
    /// renderers lit a lightless scene differently. Asserting the conversion
    /// rather than the literals is the point, because the conversion is what
    /// both consumers now share.
    #[test]
    fn the_rasterizer_uniform_carries_exactly_the_rig() {
        let cam = camera();
        let rig = viewer_rig(&cam);
        let unit = solarxy_core::AABB {
            min: Point3::new(-1.0, -1.0, -1.0),
            max: Point3::new(1.0, 1.0, 1.0),
        };
        let uniform = lights_from_camera(&cam, &unit, [0.0; 3]);
        assert_eq!(uniform.count, 3);
        for (slot, def) in rig.iter().enumerate() {
            let entry = uniform.lights[slot];
            assert_eq!(entry.position, def.position);
            assert_eq!(entry.color, def.color);
            assert_eq!(entry.intensity, def.intensity);
            assert_eq!(entry.kind, crate::light::LIGHT_KIND_POINT);
            assert_eq!(entry.shadowed, f32::from(u8::from(def.cast_shadow)));
        }
    }

    /// The rig follows the camera, which is what makes it a viewer rig rather
    /// than a lighting setup.
    #[test]
    fn the_rig_sits_around_the_cameras_target_and_moves_with_it() {
        let mut cam = camera();
        let near = viewer_rig(&cam);
        cam.eye = Point3::new(0.0, 0.0, 8.0);
        let far = viewer_rig(&cam);
        let distance = |l: &LightDef| Vector3::from(l.position).magnitude();
        for (a, b) in near.iter().zip(far.iter()) {
            assert!(
                distance(b) > distance(a) * 1.5,
                "pulling the eye back should push the rig out with it"
            );
        }
    }
}
