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
use crate::geometry::{MeshTopology, RawMaterialData};
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
}

/// The cooked output of one displayed node: an ordered list of meshes plus
/// their materials, with precomputed bounds for framing and culling.
#[derive(Debug, Clone)]
pub struct CookedGeometry {
    pub meshes: Vec<CookedMesh>,
    pub materials: Vec<Arc<RawMaterialData>>,
    /// Union bounds over all meshes (object space).
    pub bounds: AABB,
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
    /// Remove every object and light (document replaced).
    Clear,
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
