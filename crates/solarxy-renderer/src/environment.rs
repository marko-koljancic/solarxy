//! [`SceneEnvironment`]: the scene-level GPU state every render pass needs
//! regardless of where the geometry came from — the light rig, the shared
//! identity instance buffer for floor/overlay draws, the shadow map, and
//! the grid/floor/gizmo visualization buffers.
//!
//! Extracted from [`crate::scene::ModelScene`] in the web milestone's
//! so a shell without a file-loaded model (the web host, whose
//! geometry arrives through `SceneObjects` deltas) can drive the full
//! pass set. `ModelScene` recomposes as `{ env, model, ... }` with the
//! construction order copied verbatim; desktop output is bit-identical
//! (golden-verified).

use solarxy_core::AABB;

use crate::bind_groups::BindGroupLayouts;
use crate::camera::camera_from_bounds;
use crate::ibl::{BrdfLut, IblState};
use crate::light::LightsUniform;
use crate::pipelines::Instance;
use crate::scene::{create_light_bind_group, lights_from_camera};
use crate::shadow::ShadowState;
use crate::visualization::VisualizationState;

use cgmath::Rotation3;
use wgpu::util::DeviceExt;

pub struct SceneEnvironment {
    pub lights_uniform: LightsUniform,
    pub light_buffer: wgpu::Buffer,
    pub light_bind_group: wgpu::BindGroup,
    /// Identity instance buffer bound at slot 1 for scene-level draws
    /// (floor, grid, overlays); per-object loops rebind slot 1 as they go.
    pub instance_buffer: wgpu::Buffer,
    pub shadow: ShadowState,
    pub vis: VisualizationState,
}

impl SceneEnvironment {
    /// Build the scene environment around `bounds`: an identity instance
    /// buffer, a bounds-framed three-point light rig (seeded against the
    /// fallback IBL; the shell's IBL chokepoint rebuilds the bind group
    /// when a real IBL arrives), the shadow map fitted to `bounds`, and
    /// the supplied visualization state.
    ///
    /// `vis` is passed in because its contents are shell-specific: the
    /// desktop builds it from the loaded model (normals arrows, per-mesh
    /// bounds), the web from bounds alone via
    /// [`VisualizationState::new_from_parts`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        bounds: &AABB,
        aspect: f32,
        brdf_lut: &BrdfLut,
        ltc: &crate::ltc::LtcLuts,
        shadow_map_size: u32,
        vis: VisualizationState,
    ) -> Self {
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
        // Pane cameras live in the shells' view layers; a throwaway
        // bounds-framed camera seeds the initial light rig.
        let initial_cam = camera_from_bounds(bounds, aspect);
        let lights_uniform =
            lights_from_camera(&initial_cam, bounds, placeholder_ibl.irradiance_average);
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Light VB"),
            contents: bytemuck::cast_slice(&[lights_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let light_bind_group = create_light_bind_group(
            device,
            layouts,
            &light_buffer,
            &placeholder_ibl,
            brdf_lut,
            ltc,
        );

        let shadow = ShadowState::new(device, layouts, &lights_uniform, bounds, shadow_map_size);

        SceneEnvironment {
            lights_uniform,
            light_buffer,
            light_bind_group,
            instance_buffer,
            shadow,
            vis,
        }
    }
}
