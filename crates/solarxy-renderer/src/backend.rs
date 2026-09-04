//! The render backend contract: what a renderer is, from a host's point of
//! view.
//!
//! Two implementations arrive in this release, the raster pass chain and the
//! path tracer, and a third consumer, the headless render command. A host
//! chooses one per pane and has to state what it can do without knowing which
//! it is holding.
//!
//! # What this trait deliberately does not cover
//!
//! Device creation, surface configuration, the post chain and capture all stay
//! concrete and shared. A backend produces a linear HDR view; everything
//! downstream of that view is one code path for every backend, which is what
//! makes a traced image inherit exposure, the grading slots, tone mapping,
//! bloom and the selection rim for free rather than by discipline.
//!
//! # Capability, never identity
//!
//! [`BackendCaps`] describes what a backend *can do*. It must never grow a
//! field or method that says *which* backend it is. A `fn is_path_tracer()`
//! would let hosts branch on identity, and the first time one did, adding a
//! third backend would mean editing every host again. The test of the design
//! is that a screen-space GI backend, or a hardware-ray-tracing one, slots in
//! without a host change.
//!
//! # Per-pane state lives inside the backend
//!
//! A host holds one backend per kind, not one per pane: the scene each backend
//! keeps is per session, and four panes showing the same scene must not mean
//! four copies of the geometry on the GPU. Anything that genuinely is per pane,
//! a path tracer's accumulation buffer and sample count, lives inside the
//! backend keyed by [`FrameCtx::index`].
//!
//! That is a decision, not an accident. The alternative, handing each pane a
//! backend-specific view type, would need an associated type on the trait, and
//! an associated type makes `Box<dyn RenderBackend>` impossible, which is
//! exactly how hosts hold these. Keying on the pane index costs a small array
//! in the one backend that needs it and keeps the trait object-safe.

use solarxy_core::AABB;
use solarxy_core::preferences::ResolvedBackground;
use solarxy_core::scene::{SceneDelta, SceneObjectId};
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};

use crate::camera::Camera;
use crate::camera_state::CameraState;
use crate::composite::CompositeLook;
use crate::environment::SceneEnvironment;
use crate::frame::{DrawObject, Renderer};
use crate::panes::PaneRect;

/// A renderer, as a host drives it.
///
/// Four operations and nothing else: take a scene change, encode one pane,
/// state capabilities, and drop accumulated work. See the module documentation
/// for what is deliberately outside this contract.
pub trait RenderBackend {
    /// Ingest a scene delta.
    ///
    /// Both backends consume the same ops and each keeps its own GPU
    /// representation: the raster path uploads per-object buffers and material
    /// bind groups, the tracer packs an arena and builds hierarchies. Neither
    /// sees the other's.
    fn apply(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, delta: &SceneDelta);

    /// Encode one pane's work into `ctx`'s encoder, resolving into `target`.
    ///
    /// `target` is the linear HDR view the shared post chain reads. The raster
    /// backend passes it through to a pass chain that already writes that
    /// target; a progressive backend resolves its accumulation into it. Either
    /// way the composite runs outside this call, on whatever landed there.
    ///
    /// Returns [`FrameOutcome`], which is how sample counts and convergence
    /// reach a host. That channel is the return value on purpose: the scene
    /// delta is one-way, engine to renderer, and putting progress on it would
    /// make it two-way for the sake of one number.
    fn encode(&mut self, ctx: &mut FrameCtx<'_>, target: &wgpu::TextureView) -> FrameOutcome;

    /// What this backend can do, so a host gates its interface on capability
    /// rather than on a guess.
    fn caps(&self) -> BackendCaps;

    /// Drop any accumulated per-pane work, because something it was averaging
    /// over changed: the camera moved, the scene changed, a parameter was
    /// edited. A backend that accumulates nothing implements this as a no-op,
    /// which is not a special case but the honest answer.
    fn invalidate(&mut self);

    /// The auxiliary channels this pane described while it drew, if any.
    ///
    /// Defaulted to `None`, so a backend that writes no auxiliary output says
    /// nothing rather than implementing a refusal. What a caller does with the
    /// answer is gated on [`BackendCaps::writes_aovs`] before it ever gets
    /// here; this is where the data is, not whether it exists.
    fn aov_sources(&self, _pane: usize) -> Option<AovSources<'_>> {
        None
    }

    /// What this backend left out of the scene it last ingested, phrased for
    /// somebody who is about to wonder where their curves went.
    ///
    /// `None` when nothing was dropped, which is the ordinary case and is why
    /// this returns an option rather than a count: a caller pushes it into a
    /// warning list and says nothing when there is nothing to say.
    ///
    /// Defaulted to `None` because the rasterizer draws points and lines
    /// perfectly well and has nothing to apologise for.
    fn skipped_primitives_warning(&self) -> Option<String> {
        None
    }

    /// Focus the backend's camera model on the shot's lens.
    ///
    /// Separate from the camera itself, which arrives per frame on
    /// [`FrameCtx::camera`], because a lens is a property of the shot rather
    /// than of the view: it changes when the camera node changes, not when
    /// somebody orbits. A host calls this wherever it resolves which camera
    /// is being rendered through, and passes
    /// [`solarxy_core::scene::CameraLens::default`] for a free view, which
    /// is a pinhole.
    ///
    /// Defaulted to a no-op, and that is the rasterizer's honest answer
    /// rather than a refusal: it draws one sample per pixel and has no
    /// aperture to integrate over. Faking the effect there would be a
    /// depth-driven blur, which is a different thing wearing this one's name.
    fn set_lens(&mut self, _lens: solarxy_core::scene::CameraLens) {}

    /// Encode a depth pass for `pane` and hand back the texture it wrote.
    ///
    /// Its own call rather than a lane of [`RenderBackend::aov_sources`]
    /// because a depth is not accumulated: it is one primary ray at the pixel
    /// centre, so it is encoded once when the pane is finished rather than
    /// merged over samples. `window` tiles it exactly the way
    /// [`FrameCtx::window`] tiles the colour.
    ///
    /// Defaulted to `None`, for the same reason as above.
    fn encode_depth_aov(
        &mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        _camera: &CameraState,
        _size: [u32; 2],
        _window: Option<ImageWindow>,
    ) -> Option<&wgpu::Texture> {
        None
    }
}

/// The auxiliary channels a backend described alongside the colour.
///
/// One texture rather than two, because albedo and normal are written by the
/// same store: albedo in `rgb`, the world normal octahedrally packed into `a`,
/// unpacked through [`crate::pathtrace::unpack_aov_normal`]. A caller that
/// wants only one of them still reads both, which costs a copy it was making
/// anyway.
///
/// The values are means already, weighted by the count of samples that found a
/// surface. Nothing downstream divides them.
pub struct AovSources<'a> {
    pub auxiliary: &'a wgpu::Texture,
}

/// Whether a backend finished the pane this call, or is still converging.
///
/// A non-progressive backend always returns [`FrameOutcome::Complete`]. A host
/// reads the converging arm to drive a progress readout and to decide whether
/// to schedule another frame, without knowing which backend produced it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameOutcome {
    /// The pane is finished as of this call.
    Complete,
    /// The pane is still accumulating. Both counts are in samples, and
    /// `samples` counts what has landed, not what this call added.
    Converging { samples: u32, target_samples: u32 },
}

/// What a backend can do.
///
/// Every field is a capability. None of them, now or ever, names a backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BackendCaps {
    /// Whether repeated frames of an unchanged pane improve the image. Drives
    /// whether a host shows a sample counter and keeps redrawing a still pane.
    pub progressive: bool,
    /// The number of lights this backend renders, or `None` for unbounded.
    ///
    /// The raster path reports eight because its uniform holds eight; a tracer
    /// reads a storage array and reports `None`. A host states the difference
    /// rather than leaving a user to infer it from a scene that looks wrong.
    pub max_lights: Option<u32>,
    /// Whether per-mesh placement lists render as real instances.
    pub supports_instancing: bool,
    /// Which primitive topologies this backend draws. A host can tell a user
    /// that a pane's backend will not draw their point cloud.
    pub supports_topology: TopologyMask,
    /// Whether this backend produces auxiliary outputs (albedo, normal, depth)
    /// alongside colour.
    pub writes_aovs: bool,
    /// Whether this backend fills the screen-space occlusion buffer the
    /// finishing chain multiplies by.
    ///
    /// The rasterizer derives it from a depth and normal prepass. A backend
    /// that shades by tracing already has the occlusion in its image and runs
    /// no such prepass, so the buffer it would be multiplied by holds another
    /// pane's answer, or the last frame's. A host reads this rather than
    /// asking which backend it is, and the effect is switched off for that
    /// pane alone; bloom is unaffected, because it reads the colour every
    /// backend writes.
    pub writes_occlusion: bool,
}

/// The primitive topologies a backend draws.
///
/// A hand-rolled bit set rather than a dependency: three flags do not justify
/// widening the supply chain of a public repository.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TopologyMask(u8);

impl TopologyMask {
    /// Draws nothing. The identity for [`TopologyMask::union`].
    pub const NONE: Self = Self(0);
    pub const TRIANGLES: Self = Self(1 << 0);
    pub const LINES: Self = Self(1 << 1);
    pub const POINTS: Self = Self(1 << 2);
    /// Every topology the scene contract can carry.
    pub const ALL: Self = Self(0b111);

    /// Whether every topology in `other` is in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether this mask draws nothing at all.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// What a pane draws this frame.
///
/// The host decides which arm applies; the arm decides which pass chain runs
/// and how the composite is parameterised. Splitting it this way is what lets
/// one body serve both shells: everything that differs between them is either
/// resolved into a field of [`FrameCtx`] or done at the call site before the
/// call.
///
/// A backend is free to ignore an arm it cannot serve. The path tracer takes
/// its geometry from [`RenderBackend::apply`] rather than from the draw list
/// here, and draws no UV layout at all; that is [`BackendCaps`]'s job to state.
pub enum PaneContent<'a> {
    /// The slot has no camera yet. Clear to the background and composite as a
    /// non-UV pane with no scene.
    Empty,
    /// A 3D scene pane.
    ///
    /// The draw list is **not** here, deliberately. A backend that ingests
    /// deltas owns the scene, so it assembles its own list; what it cannot
    /// know is what the host draws that did not come down the delta stream,
    /// and which object the host considers selected. Those two are what this
    /// arm carries.
    Scene {
        /// An object drawn first, from a scene the backend does not own. The
        /// desktop shell's file-loaded model; `None` on a host that has no
        /// such thing.
        ///
        /// Drawn first because order is load-bearing: overdraw counts
        /// fragments in submission order, and the depth-equal overlays resolve
        /// against whatever landed first.
        extra: Option<DrawObject<'a>>,
        /// The host's selected object, flagged in the list so the main pass
        /// tints it and the outline stages find a silhouette. A selection that
        /// resolves to nothing drawable flags nothing, which is what stops the
        /// mask and jump-flood stages running for an empty silhouette.
        selected: Option<SceneObjectId>,
        /// The camera as it read **before** the aspect write, which is what
        /// the main pass takes. Copied by the host rather than re-read here,
        /// because the aspect write happens in between.
        cam_data: Camera,
        /// Whether this pane re-renders the shadow map.
        shadow: bool,
    },
    /// A UV layout pane.
    Uv { source: UvSource<'a> },
}

/// Where a UV pane's geometry comes from.
///
/// Two arms rather than one object, because the two hosts answer differently
/// and only one of them can hand over an object directly: the desktop's comes
/// from its file-loaded model and the web's preview from a scene of its own,
/// both outside the backend, while the web's fallback is an object the backend
/// itself owns and therefore cannot be borrowed out and passed back in.
pub enum UvSource<'a> {
    /// An object the host resolved from a scene the backend does not own.
    External(DrawObject<'a>),
    /// Resolve against the backend's own scene: this object if it is there,
    /// else the first drawable one.
    Scene { preferred: Option<SceneObjectId> },
    /// Nothing to lay out. Renders the pane background and still composites as
    /// a UV pane.
    None,
}

/// Everything encoding one pane needs.
///
/// This is the bundle the shells already assembled for the shared pane body,
/// with the GPU handles that body used to take as separate parameters folded
/// in, because [`RenderBackend::encode`] has nowhere else to put them.
///
/// `renderer` is mutable for two reasons, both on the UV path: the UV camera
/// writes through a mutable receiver, and arming the overlap readback flips a
/// flag. `camera` is mutable because the aspect write lands on it. Everything
/// else is read.
pub struct FrameCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    /// Shell-owned GPU state: pipelines, shared targets, post-processing
    /// resources. Not owned by a backend, because every shell reaches for it a
    /// hundred other ways.
    pub renderer: &'a mut Renderer,
    /// The encoder this pane owns from its first pass to the queue. A backend
    /// encodes into it and never submits it.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The pane's slot, and the key any per-pane backend state is stored under.
    /// Only pane 0 clears the surface.
    pub index: usize,
    pub rect: PaneRect,
    pub is_split: bool,
    pub pds: &'a PaneDisplaySettings,
    pub display: &'a DisplaySettings,
    /// Already resolved against whatever registry of user backgrounds the
    /// shell keeps.
    pub background: ResolvedBackground,
    /// The pane's camera, or `None` for a slot that has none yet.
    pub camera: Option<&'a mut CameraState>,
    pub env: &'a SceneEnvironment,
    pub bounds: Option<&'a AABB>,
    /// The grid plane this pane's camera wants, or `None` to leave the plane
    /// untouched. **`None` is not `Some(0)`**: a shell that has never written
    /// this offset must leave the bytes alone, not write a value that looks
    /// neutral.
    pub grid_plane: Option<u32>,
    /// Already resolved against whatever camera this pane looks through.
    pub look: CompositeLook,
    /// Whether the frame has scene content at all. The composite folds in
    /// bloom and ambient occlusion only when it does: a pane with a camera and
    /// nothing in it renders the background, the grid and the floor, and
    /// blooming that puts a glow on a bare viewport nobody asked for.
    pub scene_present: bool,
    /// Whether to blit the selection rim after tone mapping.
    pub outline: bool,
    /// Set when this pane is one tile of a larger image rather than a view in
    /// its own right. `None` for every ordinary frame.
    pub window: Option<ImageWindow>,
    pub content: PaneContent<'a>,
}

/// Where a pane sits inside a larger image.
///
/// A still render draws a picture too big for one pass by rendering windows of
/// it and assembling them. Both backends need to know, and they need it for
/// different reasons, which is why this is a field on the frame rather than a
/// setter on either of them: the rasterizer turns it into an asymmetric frustum
/// on the camera, and the path tracer turns it into a dispatch offset while
/// leaving the camera alone. A host sets one field and neither backend has to
/// be asked which it is.
///
/// Stateless on purpose. A `set_tile` that persisted would be left switched on
/// by a job that ended badly, and the next ordinary frame would render one
/// corner of itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageWindow {
    /// The top-left of this tile in the whole image, in pixels. The tile's size
    /// is [`FrameCtx::rect`].
    pub origin: [u32; 2],
    /// The whole image, in pixels.
    pub full: [u32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_mask_composes_and_answers_membership() {
        let tris_and_lines = TopologyMask::TRIANGLES.union(TopologyMask::LINES);
        assert!(tris_and_lines.contains(TopologyMask::TRIANGLES));
        assert!(tris_and_lines.contains(TopologyMask::LINES));
        assert!(!tris_and_lines.contains(TopologyMask::POINTS));
        assert!(TopologyMask::ALL.contains(tris_and_lines));
        assert!(TopologyMask::NONE.is_empty());
        assert!(!TopologyMask::TRIANGLES.is_empty());
    }

    #[test]
    fn all_is_exactly_the_three_topologies() {
        let built = TopologyMask::TRIANGLES
            .union(TopologyMask::LINES)
            .union(TopologyMask::POINTS);
        assert_eq!(built, TopologyMask::ALL);
    }

    #[test]
    fn a_bounded_light_count_is_distinguishable_from_an_unbounded_one() {
        // The whole point of the `Option`: eight is a number, unbounded is not
        // a very large number.
        let raster = BackendCaps {
            progressive: false,
            max_lights: Some(8),
            supports_instancing: true,
            supports_topology: TopologyMask::ALL,
            writes_aovs: false,
            writes_occlusion: true,
        };
        let traced = BackendCaps {
            progressive: true,
            max_lights: None,
            supports_instancing: true,
            supports_topology: TopologyMask::TRIANGLES,
            writes_aovs: true,
            writes_occlusion: false,
        };
        assert!(raster.max_lights.is_some_and(|n| n == 8));
        assert!(traced.max_lights.is_none());
        assert!(!traced.supports_topology.contains(TopologyMask::POINTS));
    }

    #[test]
    fn a_converging_outcome_carries_both_counts() {
        let out = FrameOutcome::Converging {
            samples: 12,
            target_samples: 256,
        };
        match out {
            FrameOutcome::Converging {
                samples,
                target_samples,
            } => {
                assert_eq!(samples, 12);
                assert_eq!(target_samples, 256);
            }
            FrameOutcome::Complete => panic!("expected the converging arm"),
        }
        assert_ne!(out, FrameOutcome::Complete);
    }
}
