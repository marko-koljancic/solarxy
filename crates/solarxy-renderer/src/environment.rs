//! [`SceneEnvironment`]: the scene-level GPU state every render pass needs
//! regardless of where the geometry came from — the light rig, the shared
//! identity instance buffer for floor/overlay draws, the shadow map, and
//! the grid/floor/gizmo visualization buffers.
//!
//! Owned by the shell, one per session, so a shell with no file-loaded
//! model still drives the full pass set: the web host's geometry arrives
//! through `SceneObjects` deltas, and the desktop shell keeps one beside
//! its `Option<ModelScene>` so an empty viewport still draws its
//! background, grid, floor and axes.

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

/// The bounds an environment is fitted to before any geometry exists: a
/// 4-unit box centred on the origin, which frames the grid and the floor at
/// a usable size for an empty viewport.
///
/// Shared so the two shells cannot disagree about how big an empty scene is.
#[must_use]
pub fn placeholder_bounds() -> AABB {
    AABB {
        min: cgmath::Point3::new(-2.0, -2.0, -2.0),
        max: cgmath::Point3::new(2.0, 2.0, 2.0),
    }
}

/// The host-side memory that makes
/// [`solarxy_core::scene::SceneOp::SetEnvironment`] idempotent.
///
/// The engine re-emits the whole environment on every scene rebuild, the
/// same way it re-emits the whole light and camera lists, because there is
/// exactly one and diffing it would cost a reconciliation bug surface. But
/// an environment is not a light list: installing one decodes nothing yet
/// convolves an irradiance cubemap and runs a GPU prefilter, so applying it
/// afresh each frame would be ruinous.
///
/// This holds the content hash of what is installed and skips the rebuild
/// when it has not moved. [`solarxy_core::RawImageHdr`] stamps that hash at
/// construction precisely so identity is cheap here.
///
/// Both shells own one. It lives beside [`SceneEnvironment`] rather than
/// inside it because the IBL belongs to the renderer, not to the
/// environment, and the tracker only needs the two.
#[derive(Debug, Default)]
pub struct EnvironmentTracker {
    installed: Option<EnvironmentIdentity>,
}

/// What is currently on the GPU. `None` for the hash means "no HDRI",
/// which is distinct from any real hash and so compares correctly against
/// a scene that never had one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnvironmentIdentity {
    hdri: Option<u64>,
}

/// What [`EnvironmentTracker::apply`] did, so the caller knows which of its
/// own follow-ups to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentOutcome {
    /// The environment was already installed. The caller should do nothing:
    /// no bind-group rebuild, no skybox repoint.
    Unchanged,
    /// A new HDRI was installed. The caller must rebuild its light bind
    /// group (the IBL chokepoint) and repoint the skybox.
    HdriInstalled,
    /// The environment was cleared. The caller must fall back to its own
    /// procedural sky and rebuild the light bind group.
    Cleared,
}

impl EnvironmentTracker {
    /// Install `hdri` as the scene environment if it differs from what is
    /// already there, convolving and uploading it. Returns [`Unchanged`]
    /// when the hash matches, which is the common case: the engine emits
    /// this op on every rebuild.
    ///
    /// Clearing (`hdri: None`) leaves `ibl.ibl` alone rather than writing a
    /// black environment; the caller substitutes its own procedural sky,
    /// because "no environment" is a host-background question and the two
    /// shells answer it differently.
    ///
    /// [`Unchanged`]: EnvironmentOutcome::Unchanged
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ibl: &mut crate::frame::IblResources,
        hdri: Option<&std::sync::Arc<solarxy_core::RawImageHdr>>,
    ) -> EnvironmentOutcome {
        match self.decide(hdri.map(|h| h.hash)) {
            EnvironmentOutcome::HdriInstalled => {
                // `decide` only says an HDRI is wanted when one was passed.
                if let Some(image) = hdri {
                    ibl.ibl = IblState::from_hdr_image(device, queue, image);
                }
                EnvironmentOutcome::HdriInstalled
            }
            other => other,
        }
    }

    /// The dedupe decision on its own, with no GPU work.
    ///
    /// Split out so the idempotence that makes this type worth having is
    /// testable on a machine with no adapter. The GPU suite skips itself
    /// without one, and a rule that only runs where there is a GPU is a
    /// rule that does not run.
    fn decide(&mut self, incoming_hash: Option<u64>) -> EnvironmentOutcome {
        let incoming = EnvironmentIdentity {
            hdri: incoming_hash,
        };
        if self.installed == Some(incoming) {
            return EnvironmentOutcome::Unchanged;
        }
        self.installed = Some(incoming);
        if incoming_hash.is_some() {
            EnvironmentOutcome::HdriInstalled
        } else {
            EnvironmentOutcome::Cleared
        }
    }

    /// Record that `hash` is already live on the GPU, installed by a route
    /// other than [`Self::apply`].
    ///
    /// The web host needs this: its worker returns an HDRI already
    /// convolved, so it installs the IBL directly and would otherwise see
    /// the very next scene delta carry the same image and convolve it
    /// again, on the main thread, which is exactly the cost the worker
    /// exists to avoid.
    pub fn note_installed(&mut self, hash: u64) {
        self.installed = Some(EnvironmentIdentity { hdri: Some(hash) });
    }

    /// Forget what is installed, so the next [`Self::apply`] rebuilds even
    /// if the hash matches. The shells call this when something outside the
    /// scene contract replaces the IBL: the HDRI drop, the sidebar picker,
    /// or a background change that re-derives the procedural sky. Without
    /// it, setting an HDRI by hand and then reopening the same one through
    /// the node would be a no-op against a GPU state that had moved on.
    pub fn invalidate(&mut self) {
        self.installed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{EnvironmentOutcome, EnvironmentTracker};

    #[test]
    fn the_same_environment_installs_once_however_often_it_arrives() {
        // This is the whole point of the type. The engine re-emits
        // SetEnvironment on every scene rebuild, which is every cook, and
        // installing one convolves an irradiance cubemap and runs a GPU
        // prefilter. Without the dedupe a scene with an environment node
        // would do that work every frame.
        let mut t = EnvironmentTracker::default();
        assert_eq!(t.decide(Some(0xAB)), EnvironmentOutcome::HdriInstalled);
        for _ in 0..100 {
            assert_eq!(t.decide(Some(0xAB)), EnvironmentOutcome::Unchanged);
        }
    }

    #[test]
    fn a_different_hdri_reinstalls() {
        let mut t = EnvironmentTracker::default();
        assert_eq!(t.decide(Some(1)), EnvironmentOutcome::HdriInstalled);
        assert_eq!(t.decide(Some(2)), EnvironmentOutcome::HdriInstalled);
        assert_eq!(t.decide(Some(2)), EnvironmentOutcome::Unchanged);
    }

    #[test]
    fn no_environment_is_a_state_of_its_own_not_a_repeat_of_none() {
        // `None` means "no environment", which the host answers with its
        // own procedural sky. It has to clear exactly once: clearing on
        // every frame would rebuild that sky forever, and never clearing
        // would leave a deleted node's HDRI lighting the scene.
        let mut t = EnvironmentTracker::default();
        assert_eq!(t.decide(Some(7)), EnvironmentOutcome::HdriInstalled);
        assert_eq!(t.decide(None), EnvironmentOutcome::Cleared);
        assert_eq!(t.decide(None), EnvironmentOutcome::Unchanged);
        assert_eq!(t.decide(Some(7)), EnvironmentOutcome::HdriInstalled);
    }

    #[test]
    fn a_fresh_tracker_installs_even_an_empty_environment() {
        // Startup has installed nothing, so the first op must act even
        // when it carries no HDRI: the host has to reach a known state
        // rather than assume its default already matches.
        let mut t = EnvironmentTracker::default();
        assert_eq!(t.decide(None), EnvironmentOutcome::Cleared);
    }

    #[test]
    fn invalidate_forces_a_reinstall_of_the_same_hdri() {
        // The shells replace the IBL outside the scene contract: the HDRI
        // picker, the Clear button, a background change. After any of
        // those, re-selecting the same HDRI through a node must reinstall
        // rather than match a hash whose GPU state has moved on.
        let mut t = EnvironmentTracker::default();
        assert_eq!(t.decide(Some(9)), EnvironmentOutcome::HdriInstalled);
        assert_eq!(t.decide(Some(9)), EnvironmentOutcome::Unchanged);
        t.invalidate();
        assert_eq!(t.decide(Some(9)), EnvironmentOutcome::HdriInstalled);
    }
}
