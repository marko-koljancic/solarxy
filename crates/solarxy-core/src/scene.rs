//! GPU-free runtime scene types: the contract between the node engine
//! (`solarxy-graph`, next milestone phase) and the renderer's multi-object
//! scene (`solarxy-renderer:scene_objects`).
//!
//! The engine and the renderer never depend on each other; they communicate
//! exclusively through [`SceneDelta`] values built from these types. On the
//! web both sides compile into one wasm instance, so [`CookedGeometry`]'s
//! `Arc`-shared attribute buffers move engine-to-renderer as a pointer
//! handoff — cooked geometry never crosses into JavaScript.
//!
//! Everything here is plain data: no wgpu, no filesystem, no windowing.

use std::sync::Arc;

use crate::aabb::AABB;
use crate::geometry::{LutCube, MeshTopology, RawImageHdr, RawMaterialData};
use crate::validation::ValidationResult;

/// Stable identity of one renderable object in the scene, minted by the
/// producer (the node engine derives it from the owning node). The renderer
/// treats it as an opaque key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneObjectId(pub u64);

/// One cooked mesh: attribute-complete CPU geometry, connected per its
/// `topology` (triangle triples, segment pairs, or a point cloud).
/// Attribute buffers are `Arc`-shared so the engine's cook cache and the
/// renderer's upload path reference the same allocation.
#[derive(Debug, Clone)]
pub struct CookedMesh {
    pub name: String,
    pub positions: Arc<Vec<[f32; 3]>>,
    /// Per-vertex normals; `None` means the consumer computes or skips.
    pub normals: Option<Arc<Vec<[f32; 3]>>>,
    pub tex_coords: Option<Arc<Vec<[f32; 2]>>>,
    pub indices: Arc<Vec<u32>>,
    /// Index into the owning [`CookedGeometry::materials`].
    pub material_index: Option<usize>,
    /// How `indices` connect `positions` into primitives.
    pub topology: MeshTopology,
    /// Per-vertex linear RGBA colors (the kernel's reserved `color` lane);
    /// `None` renders as white. Always position-count length when present.
    pub colors: Option<Arc<Vec<[f32; 4]>>>,
    /// Per-instance placements for this mesh, or `None` for the ordinary
    /// single-placement case.
    ///
    /// **`None` preserves the pre-instancing meaning exactly**: one implicit
    /// identity placement. Every producer and consumer that predates
    /// instancing is unaffected, which is what makes this additive rather
    /// than breaking.
    ///
    /// On the mesh rather than on [`CookedGeometry`] because a set can hold
    /// meshes with different placements: merging a scatter's prototype with
    /// the surface it was scattered over is one set holding one instanced
    /// mesh and one plain one, and a single list for the whole set cannot
    /// say that. The copy operations keep a multi-mesh prototype rigid
    /// within each copy by handing every one of its meshes the same shared
    /// list, so the level costs nothing there.
    pub instances: Option<Arc<Vec<InstanceXform>>>,
}

impl CookedMesh {
    /// How many placements this mesh draws: the placement count, or 1 for
    /// the implicit identity.
    ///
    /// The single place that turns "no list" into "one draw", so no caller
    /// has to remember which absence means what.
    #[must_use]
    pub fn instance_count(&self) -> u32 {
        self.instances
            .as_ref()
            .map_or(1, |i| u32::try_from(i.len()).unwrap_or(u32::MAX))
    }
}

/// The cooked output of one displayed node: an ordered list of meshes plus
/// their materials, with precomputed bounds for framing and culling.
#[derive(Debug, Clone)]
pub struct CookedGeometry {
    pub meshes: Vec<CookedMesh>,
    pub materials: Vec<Arc<RawMaterialData>>,
    /// Union bounds over all meshes, **including every placement** (object
    /// space), so camera framing and culling see the scatter rather than
    /// the prototype sitting at the origin.
    pub bounds: AABB,
}

/// One instance placement: a column-major 4x4 model matrix, the same
/// convention [`SceneOp::SetTransform`] uses.
///
/// The renderer derives each instance's normal matrix from this, so a
/// mirrored or non-uniformly scaled placement shades correctly rather
/// than inside out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InstanceXform(pub [[f32; 4]; 4]);

impl InstanceXform {
    /// The identity placement, which is what `instances: None` means.
    pub const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    /// Transform a point by this placement.
    #[must_use]
    pub fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
    }
}

impl CookedGeometry {
    /// Whether any mesh carries placements.
    #[must_use]
    pub fn is_instanced(&self) -> bool {
        self.meshes.iter().any(|m| m.instances.is_some())
    }
}

/// Light variety, mirroring the six light node types.
/// `Ambient` and `Hemisphere` modulate the ambient/IBL term and do not
/// consume one of the renderer's per-light slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    Point,
    Directional,
    Spot,
    /// V1 renders rect-area as a soft point-light approximation
    /// ; `area_extent` is retained for the LTC upgrade.
    RectArea,
    Ambient,
    Hemisphere,
}

/// One light's resolved runtime description. Field applicability by kind is
/// documented per field; irrelevant fields hold their defaults. Angles are
/// radians here — the engine's param resolver owns the degrees-to-radians
/// conversion (units convention).
#[derive(Debug, Clone, PartialEq)]
pub struct LightDef {
    pub kind: LightKind,
    /// Where the light sits. The renderer's SHADING ignores this for a
    /// directional light (whose shadow frustum auto-fits scene bounds), but the
    /// helper has to draw its arrow somewhere, so it is filled for every
    /// positional type now, directional included.
    pub position: [f32; 3],
    /// Directional / Spot: unit vector the light travels along
    /// (from the light toward the scene).
    pub direction: [f32; 3],
    /// Linear RGB. Hemisphere uses `color` as the sky color and
    /// `ground_color` for the lower hemisphere.
    pub color: [f32; 3],
    pub intensity: f32,
    /// Point / Spot cutoff distance; `0` means unlimited.
    pub range: f32,
    /// Point / Spot falloff exponent.
    pub decay: f32,
    /// Spot inner cone half-angle (radians); full intensity inside.
    pub inner_cone: f32,
    /// Spot outer cone half-angle (radians); zero intensity outside.
    pub outer_cone: f32,
    /// `RectArea` width/height (meters).
    pub area_extent: [f32; 2],
    /// `RectArea` orientation as XYZ Euler angles in radians, applied to a
    /// panel that lies in the XZ plane and emits straight down.
    ///
    /// Only rect-area lights read this. The other kinds are described by
    /// `direction`, which has no roll to lose; a rectangle does, because a
    /// 10x1 panel rolled a quarter turn is a different light.
    pub rotate: [f32; 3],
    /// `RectArea`: emit from both faces rather than only the one the
    /// normal points along.
    pub two_sided: bool,
    /// Hemisphere ground color (linear RGB).
    pub ground_color: [f32; 3],
    /// Exclusive shadow caster: the engine enforces
    /// radio semantics, so at most one visible light carries `true`.
    pub cast_shadow: bool,
    /// Shadow map resolution for the caster (512-4096).
    pub shadow_map_size: u32,
    pub shadow_bias: f32,
    pub visible: bool,
    /// Draw the viewport helper for this light (its `show_helper` param).
    pub show_helper: bool,
    /// The helper's world-space size, in meters (its `helper_size` param).
    pub helper_size: f32,
}

impl LightDef {
    /// Whether this light occupies one of the renderer's per-light slots
    /// (ambient and hemisphere fold into the ambient/IBL term instead).
    #[must_use]
    pub fn consumes_slot(&self) -> bool {
        !matches!(self.kind, LightKind::Ambient | LightKind::Hemisphere)
    }

    /// The rectangle's half-edge vectors in world space, and the face
    /// normal it emits along.
    ///
    /// Lives here rather than in the renderer because two places need it
    /// and they must not disagree: the shading integrates over these
    /// corners, and the viewport helper draws them. A helper that traces a
    /// different rectangle from the one being integrated is worse than no
    /// helper, because it looks authoritative.
    ///
    /// The unrotated panel lies in the XZ plane with its width along `+x`,
    /// its height along `+z`, and its normal pointing down `-y`, which is
    /// the orientation `rect_area_light` has always drawn and described.
    /// Euler angles apply in XYZ order.
    #[must_use]
    pub fn rect_basis(&self) -> RectBasis {
        use cgmath::{Matrix3, Rad, Vector3};
        let [rx, ry, rz] = self.rotate;
        // XYZ order: x applies first, so it is right-most in the product.
        let rotation = Matrix3::from_angle_z(Rad(rz))
            * Matrix3::from_angle_y(Rad(ry))
            * Matrix3::from_angle_x(Rad(rx));
        let half_x = rotation * Vector3::new(self.area_extent[0] * 0.5, 0.0, 0.0);
        let half_y = rotation * Vector3::new(0.0, 0.0, self.area_extent[1] * 0.5);
        let normal = rotation * Vector3::new(0.0, -1.0, 0.0);
        RectBasis {
            half_x: half_x.into(),
            half_y: half_y.into(),
            normal: normal.into(),
        }
    }
}

/// A rect-area light's oriented frame, from [`LightDef::rect_basis`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectBasis {
    /// Half the width edge, world space.
    pub half_x: [f32; 3],
    /// Half the height edge, world space.
    pub half_y: [f32; 3],
    /// Unit normal of the emitting face.
    pub normal: [f32; 3],
}

/// The projection model of a camera node. `Physical` is a perspective camera
/// whose `fov_y` was derived from a focal length and sensor size, so the
/// renderer treats it exactly like `Perspective`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKind {
    Perspective,
    Orthographic,
    Physical,
}

/// The tone-mapping transform the composite pass ends on.
///
/// Duplicates the variant set of `preferences::ToneMode`, and does so
/// deliberately: `CameraDef` names a tone curve, `scene` is ungated, and
/// `preferences` sits behind the `serde` feature, so the scene contract
/// cannot reach the one with the serde derives on it. This copy owns the
/// numbering the shader switches on; `ToneMode` keeps the user-facing
/// labels, the config-file serialization and the `Shift+T` cycle, and
/// converts to and from this. A drift test pins the two together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToneCurve {
    /// Clip to the displayable range and nothing else.
    None,
    /// Also a clip; distinct from [`Self::None`] only in name and in what
    /// a user reads it to mean.
    Linear,
    Reinhard,
    #[default]
    AcesFilmic,
}

impl ToneCurve {
    /// The discriminant `composite.wgsl` switches on. **This function is
    /// the numbering**; anything else that needs it converts to here
    /// first, so there is one place a new curve has to be added.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Linear => 1,
            Self::Reinhard => 2,
            Self::AcesFilmic => 3,
        }
    }
}

/// A shot's rendering intent: everything that changes the picture without
/// changing the scene.
///
/// Owned by the camera, which is the point rather than an implementation
/// detail. A grade that lives in application state is lost the moment the
/// tab closes and cannot be reviewed by anyone else; a grade that lives on
/// the camera node saves in the `.slxy`, travels with the shot, and is
/// visible in the graph beside the framing it belongs to.
///
/// [`Default`] is the neutral look, and neutral means **bit-identical**
/// output rather than merely similar: `tone: None` inherits whatever the
/// pane was already doing, and the renderer skips the grade entirely at
/// these values rather than multiplying by one.
#[derive(Debug, Clone)]
pub struct CameraLook {
    /// Linear multiplier applied before tone mapping. 1.0 is as rendered.
    pub exposure: f32,
    /// The tone curve to use, or `None` to inherit the pane's own choice.
    /// Inheriting is the default so that adding a camera to an existing
    /// scene does not silently restyle it.
    pub tone: Option<ToneCurve>,
    /// Added after the tone map: raises or lowers the floor. Neutral 0.
    pub lift: [f32; 3],
    /// Applied as a power last. Neutral 1.
    pub gamma: [f32; 3],
    /// Multiplied first: scales the ceiling. Neutral 1.
    pub gain: [f32; 3],
    /// The pre-tone-map table, sampled on log-encoded light. This is where
    /// a tone transform goes.
    pub lut_a: Option<Arc<LutCube>>,
    pub lut_a_strength: f32,
    /// The display-referred table, sampled after tone mapping. This is
    /// where an ordinary look LUT goes.
    pub lut_b: Option<Arc<LutCube>>,
    pub lut_b_strength: f32,
}

impl Default for CameraLook {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            tone: None,
            lift: [0.0; 3],
            gamma: [1.0; 3],
            gain: [1.0; 3],
            lut_a: None,
            lut_a_strength: 1.0,
            lut_b: None,
            lut_b_strength: 1.0,
        }
    }
}

/// Compares tables by content hash rather than by contents.
///
/// Hand-written because the derived form would compare two 33-cubed
/// `Vec<f32>` element by element, and `CameraDef` equality is reached once
/// per camera per delta. The hash is what identity means for a table
/// everywhere else in the pipeline, including the renderer's upload dedupe.
impl PartialEq for CameraLook {
    fn eq(&self, other: &Self) -> bool {
        let table = |t: &Option<Arc<LutCube>>| t.as_ref().map(|c| c.hash);
        self.exposure == other.exposure
            && self.tone == other.tone
            && self.lift == other.lift
            && self.gamma == other.gamma
            && self.gain == other.gain
            && self.lut_a_strength == other.lut_a_strength
            && self.lut_b_strength == other.lut_b_strength
            && table(&self.lut_a) == table(&other.lut_a)
            && table(&self.lut_b) == table(&other.lut_b)
    }
}

/// One camera node's resolved runtime description. Lowered from a `camera`
/// root node the same way [`LightDef`] is lowered from a light node, and read
/// back by the host to drive a pane's look-through camera and its wireframe
/// gizmo. `fov_y` is in radians (the engine's param resolver owns the
/// degrees-to-radians conversion and the physical focal-length derivation).
#[derive(Debug, Clone, PartialEq)]
pub struct CameraDef {
    /// The producing `camera` node id (the object key).
    pub id: SceneObjectId,
    pub kind: CameraKind,
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    /// Vertical field of view in radians (perspective / physical).
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    /// Orthographic half-height.
    pub ortho_scale: f32,
    /// The framing aspect (width / height) for the viewport gate and the
    /// default export aspect.
    pub aspect: f32,
    /// Draw the wireframe camera gizmo in the viewport.
    pub show_gizmo: bool,
    /// The gizmo's world-space size, in meters.
    pub gizmo_size: f32,
    /// The shot's rendering intent. A pane looking through this camera
    /// composites with it; a free pane uses its own.
    pub look: CameraLook,
}

/// One scene mutation. Transforms are column-major world matrices; a
/// transform-only change never re-uploads geometry (the param-drag and
/// geo-node-transform fast path).
#[derive(Debug, Clone)]
pub enum SceneOp {
    /// Create the object or replace its geometry (the cook-output commit).
    UpsertGeometry {
        id: SceneObjectId,
        geometry: Arc<CookedGeometry>,
    },
    /// Set the object's world transform (column-major).
    SetTransform {
        id: SceneObjectId,
        transform: [[f32; 4]; 4],
    },
    SetVisible {
        id: SceneObjectId,
        visible: bool,
    },
    /// Whether the object is drawn into the shadow map. Orthogonal to the
    /// light-side exclusive-caster rule: the light decides which light owns
    /// the shadow map, the object decides whether it participates.
    SetCastShadow {
        id: SceneObjectId,
        cast_shadow: bool,
    },
    /// Attach or clear the object's effective validation result (the
    /// nearest validation on the displayed chain: a validate node's
    /// report or an import's implicit load validation). `None` clears the
    /// overlay. The renderer dedupes by `Arc` identity, so re-sending an
    /// unchanged result each frame is free.
    SetValidation {
        id: SceneObjectId,
        validation: Option<Arc<ValidationResult>>,
    },
    Remove {
        id: SceneObjectId,
    },
    /// Replace the full light list (lights are few; diffing buys nothing).
    SetLights {
        lights: Vec<LightDef>,
    },
    /// Replace the full camera list (cameras are few; diffing buys nothing).
    /// Cameras are non-drawn scene objects the host reads back to drive a
    /// pane's look-through view and its wireframe gizmo.
    SetCameras {
        cameras: Vec<CameraDef>,
    },
    /// Replace the whole lighting environment, following the `SetLights` and
    /// `SetCameras` precedent for the same reason: there is exactly one, so
    /// diffing buys nothing and costs a reconciliation bug surface.
    ///
    /// `hdri: None` means **no environment**, which is deliberately distinct
    /// from a black one: the host keeps whatever background and procedural
    /// sky it had, rather than going dark. Rebuilding image-based lighting
    /// is expensive, so a host is expected to dedupe on
    /// [`RawImageHdr::hash`](crate::RawImageHdr) and do nothing when the
    /// same environment arrives twice.
    SetEnvironment {
        hdri: Option<Arc<RawImageHdr>>,
        /// Yaw applied to both the visible sky and the lighting it
        /// derives, in radians.
        rotation: f32,
        /// Multiplier on the lighting contribution; `1.0` is as authored.
        intensity: f32,
        background: BackgroundKind,
    },
    /// Remove every object and light (document replaced).
    Clear,
}

/// What the environment asks the background to be.
///
/// Deliberately not a full background enum. Solid and gradient backdrops
/// are per-pane host state (`preferences::BackgroundMode`), and a scene-wide
/// op that could also set them would make two systems authoritative over one
/// pixel. This says only whether the environment claims the backdrop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundKind {
    /// Leave the background alone: each pane keeps its own. The default,
    /// and the state every scene authored before the environment node
    /// existed is in.
    #[default]
    Keep,
    /// Draw the HDRI itself as the backdrop. Meaningless without an `hdri`,
    /// and a host receiving it with `hdri: None` keeps its own background.
    HdriSky,
}

/// An ordered batch of scene mutations, drained by the renderer once per
/// frame (`SceneObjects::apply`). Order matters: an `UpsertGeometry`
/// followed by `SetTransform` for the same id applies both.
#[derive(Debug, Clone, Default)]
pub struct SceneDelta {
    pub ops: Vec<SceneOp>,
}

impl SceneDelta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn push(&mut self, op: SceneOp) {
        self.ops.push(op);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(kind: LightKind) -> LightDef {
        LightDef {
            kind,
            position: [0.0; 3],
            direction: [0.0, -1.0, 0.0],
            color: [1.0; 3],
            intensity: 1.0,
            range: 0.0,
            decay: 2.0,
            inner_cone: 0.0,
            outer_cone: 0.0,
            area_extent: [0.0; 2],
            rotate: [0.0; 3],
            two_sided: false,
            ground_color: [0.0; 3],
            cast_shadow: false,
            shadow_map_size: 1024,
            shadow_bias: 0.0,
            visible: true,
            show_helper: false,
            helper_size: 1.0,
        }
    }

    #[test]
    fn ambient_and_hemisphere_do_not_consume_slots() {
        assert!(light(LightKind::Point).consumes_slot());
        assert!(light(LightKind::Directional).consumes_slot());
        assert!(light(LightKind::Spot).consumes_slot());
        assert!(light(LightKind::RectArea).consumes_slot());
        assert!(!light(LightKind::Ambient).consumes_slot());
        assert!(!light(LightKind::Hemisphere).consumes_slot());
    }

    #[test]
    fn delta_default_is_empty_and_push_appends_in_order() {
        let mut delta = SceneDelta::default();
        assert!(delta.is_empty());
        delta.push(SceneOp::Clear);
        delta.push(SceneOp::SetVisible {
            id: SceneObjectId(7),
            visible: false,
        });
        assert!(!delta.is_empty());
        assert_eq!(delta.ops.len(), 2);
        assert!(matches!(delta.ops[0], SceneOp::Clear));
        assert!(
            matches!(delta.ops[1], SceneOp::SetVisible { id, visible: false } if id == SceneObjectId(7))
        );
    }
}
