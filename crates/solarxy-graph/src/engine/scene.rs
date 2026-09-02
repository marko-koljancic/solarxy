//! Lowering cooked geometry to a `solarxy_core::scene::SceneDelta` for the
//! renderer, the sole engine-to-renderer contract.
//!
//! The mapping:
//!
//! - Each root `geo` container becomes one `SceneObject`: its subflow's
//!   active display node's cooked geometry is the object geometry
//!   (`UpsertGeometry`), and the geo node's own transform params become the
//!   object transform (`SetTransform`), NOT baked into vertices, so
//!   transform edits never recook the subflow.
//! - Each root light node cooks to a `LightDef`; the full list replaces the
//!   scene lights (`SetLights`).
//!
//! This module is total but inert until the `geo` and light node types
//! land (N2): it keys on `type_id`, so an empty or primitive-only root
//! yields an empty delta.

use std::collections::BTreeSet;

use cgmath::{InnerSpace, Matrix4, Point3, SquareMatrix, Transform, Vector3};
use solarxy_core::geometry::compute_bounds;
use solarxy_core::raycast::{MeshView, Ray, raycast_meshes};
use solarxy_core::scene::{
    CameraDef, CameraLook, LightDef, SceneDelta, SceneObjectId, SceneOp, ToneCurve,
};

use crate::cook::CookEngine;
use crate::document::{Document, GraphContext, NodeId};
use crate::nodes::common::rotate_order_from_key;
use crate::params::ParamValue;
use crate::previews::{Previews, effective_params};
use crate::registry::Registry;
use crate::registry::coerce::Value;
use crate::registry::resolve::resolve_params_with;
use solarxy_kernel::transform::compose_trs;

/// The evaluation context for a root-context node resolved outside a cook.
///
/// These sites have no gathered inputs, so the geometry queries are
/// genuinely unavailable and say so; `ch()` is available, because it reads
/// document state rather than cook output. The clock is stopped: it stays
/// that way until the runtime lands, and every one of these resolves runs
/// per frame, so a wrong clock here would desynchronise the scene from the
/// cook.
fn root_refs<'a>(
    doc: &'a Document,
    registry: &'a Registry,
    previews: &'a Previews,
    node: NodeId,
) -> crate::refs::DocRefs<'a> {
    crate::refs::DocRefs::new(
        doc,
        registry,
        previews,
        GraphContext::Root,
        node,
        crate::expr::SceneTime::default(),
    )
}

/// Builds a full scene delta from scratch each frame, and reports which
/// object ids the scene now contains. The renderer diffs against its own
/// state, so a full rebuild is safe (and light lists are tiny).
/// `Clear`-then-rebuild is deliberately avoided: object ids are stable per
/// geo node, so the renderer keeps unchanged uploads.
///
/// The returned set is what the renderer should be holding afterwards.
/// Rebuilding says nothing about objects that *stopped* existing, so the
/// caller diffs this against the previous set and emits [`SceneOp::Remove`]
/// for the difference. Without that, deleting a geo node leaves its GPU
/// object resident and drawn forever.
#[must_use]
pub fn build_scene_delta(
    doc: &Document,
    registry: &Registry,
    cook: &CookEngine,
    previews: &Previews,
) -> (SceneDelta, BTreeSet<SceneObjectId>) {
    let mut delta = SceneDelta::default();
    let mut present = BTreeSet::new();
    let Ok(root) = doc.graph(GraphContext::Root) else {
        return (delta, present);
    };

    let mut lights: Vec<LightDef> = Vec::new();
    let mut cameras: Vec<CameraDef> = Vec::new();
    // There is exactly one environment, so the first in document order
    // wins and later ones are ignored. The node's own help says so.
    let mut environment: Option<SceneOp> = None;

    for node in root.nodes() {
        match node.type_id.as_str() {
            "geo" => {
                if emit_geo(doc, registry, cook, previews, node.id, &mut delta) {
                    present.insert(SceneObjectId(node.id.0));
                }
            }
            "camera" => {
                if let Some(cam) = camera_from_node(doc, registry, previews, cook, node) {
                    cameras.push(cam);
                }
            }
            "environment" => {
                if environment.is_none() {
                    environment = environment_from_node(doc, registry, previews, cook, node);
                }
            }
            id if is_light(id) => {
                if let Some(light) = light_from_node(doc, registry, previews, node) {
                    lights.push(light);
                }
            }
            _ => {}
        }
    }

    delta.push(SceneOp::SetLights { lights });
    delta.push(SceneOp::SetCameras { cameras });
    // Pushed unconditionally, like the lists above: deleting the node has
    // to clear the environment, and the host's tracker makes the repeat
    // free when nothing moved.
    delta.push(environment.unwrap_or(SceneOp::SetEnvironment {
        hdri: None,
        rotation: 0.0,
        intensity: solarxy_core::view_config::DEFAULT_HDRI_INTENSITY,
        background: solarxy_core::scene::BackgroundKind::Keep,
    }));
    (delta, present)
}

/// Whether a type id is one of the six light nodes.
pub(crate) fn is_light(type_id: &str) -> bool {
    matches!(
        type_id,
        "point_light"
            | "directional_light"
            | "spot_light"
            | "ambient_light"
            | "hemisphere_light"
            | "rect_area_light"
    )
}

/// The committed cooked geometry a `geo` container currently displays: its
/// subflow's active output's `geometry`. Shared by scene lowering, review
/// staleness/markers, and the visualization aggregation so they can never
/// disagree about what "the displayed geometry" means.
pub(crate) fn display_output<'a>(
    doc: &Document,
    cook: &'a CookEngine,
    geo: NodeId,
) -> Option<&'a std::sync::Arc<solarxy_kernel::GeometrySet>> {
    let subflow = doc.graph(GraphContext::Subflow(geo)).ok()?;
    let display = subflow.active_output?;
    match cook.outputs(display)?.get("geometry")? {
        Value::Geometry(set) => Some(set),
        _ => None,
    }
}

/// A geo container's resolved root render flags. Both default `true` when
/// the node, its descriptor, or its params are unavailable (they are
/// additive gates, so unknown means shown and casting).
#[derive(Clone, Copy)]
pub(crate) struct GeoRenderFlags {
    pub visible: bool,
    pub cast_shadow: bool,
}

/// Resolves a geo's render flags through the standard param path. Shared
/// by scene lowering, picking, the marker projection, and the
/// visualization aggregation so they can never disagree about what
/// "hidden" means.
pub(crate) fn geo_render_flags(
    doc: &Document,
    registry: &Registry,
    previews: &Previews,
    geo: NodeId,
) -> GeoRenderFlags {
    let on = GeoRenderFlags {
        visible: true,
        cast_shadow: true,
    };
    let Some(node) = doc.graph(GraphContext::Root).ok().and_then(|g| g.node(geo)) else {
        return on;
    };
    let Some(desc) = registry.get("geo") else {
        return on;
    };
    let params = effective_params(previews, node.id, &node.params);
    let refs = root_refs(doc, registry, previews, node.id);
    let eval = crate::expr::EvalCtx::new(crate::expr::SceneTime::default()).with_refs(&refs);
    let Ok(p) = resolve_params_with(&params, &desc.params, &eval) else {
        return on;
    };
    GeoRenderFlags {
        visible: !matches!(p.get("visible"), Some(ParamValue::Bool(false))),
        cast_shadow: !matches!(p.get("cast_shadow"), Some(ParamValue::Bool(false))),
    }
}

/// The `visible` half of [`geo_render_flags`] (the picking, marker, and
/// visualization gates).
pub(crate) fn geo_visible(
    doc: &Document,
    registry: &Registry,
    previews: &Previews,
    geo: NodeId,
) -> bool {
    geo_render_flags(doc, registry, previews, geo).visible
}

/// Emits the `UpsertGeometry` + `SetVisible` + `SetCastShadow` +
/// `SetValidation` + `SetTransform` for one geo container. Hidden objects
/// still upsert (hidden-but-cooked: the geometry stays GPU-resident so
/// re-show is instant); `SetVisible` is the render gate, never a cook
/// gate.
/// Emits one geo container's ops. Returns whether the object is present in
/// the scene at all: `false` means it has no subflow or nothing flagged for
/// display, which is indistinguishable to the renderer from the node having
/// been deleted, and in both cases the object should stop being drawn.
///
/// Presence is deliberately *not* the same as having cooked geometry. An
/// object mid-cook has no `display_output` yet but must stay resident, or it
/// would be torn down and re-uploaded on every frame of a long cook.
fn emit_geo(
    doc: &Document,
    registry: &Registry,
    cook: &CookEngine,
    previews: &Previews,
    geo: NodeId,
    delta: &mut SceneDelta,
) -> bool {
    let object_id = SceneObjectId(geo.0);
    let Ok(subflow) = doc.graph(GraphContext::Subflow(geo)) else {
        return false;
    };
    let Some(display) = subflow.active_output else {
        return false;
    };
    // The displayed node's cooked geometry.
    if let Some(set) = display_output(doc, cook, geo) {
        delta.push(SceneOp::UpsertGeometry {
            id: object_id,
            geometry: std::sync::Arc::new(set.to_cooked()),
        });
    }
    // The geo's root render flags (the delta is rebuilt fully each pass,
    // so re-emission is free; the renderer's handlers are bool
    // assignments).
    let flags = geo_render_flags(doc, registry, previews, geo);
    delta.push(SceneOp::SetVisible {
        id: object_id,
        visible: flags.visible,
    });
    delta.push(SceneOp::SetCastShadow {
        id: object_id,
        cast_shadow: flags.cast_shadow,
    });
    // The object's effective validation: the nearest cached result on the
    // displayed chain (the display node itself, else breadth-first
    // upstream -- a validate node's report or an import's load
    // validation). `None` clears; the renderer dedupes by Arc identity,
    // so re-sending per frame is free.
    delta.push(SceneOp::SetValidation {
        id: object_id,
        validation: effective_validation(subflow, cook, display),
    });
    // The geo node's transform, applied as the object transform (not baked;
    // the renderer applies it), resolved through the shared world-matrix
    // helper so picking and rendering agree.
    delta.push(SceneOp::SetTransform {
        id: object_id,
        transform: geo_world_matrix(doc, registry, previews, geo).into(),
    });
    true
}

/// The nearest cached validation result at or upstream of `display`,
/// breadth-first (so the most-downstream validate node wins over an
/// import's implicit validation further up the chain).
fn effective_validation(
    graph: &crate::document::Graph,
    cook: &CookEngine,
    display: NodeId,
) -> Option<std::sync::Arc<solarxy_core::validation::ValidationResult>> {
    use std::collections::{BTreeSet, VecDeque};
    let mut queue = VecDeque::from([display]);
    let mut seen = BTreeSet::from([display]);
    while let Some(node) = queue.pop_front() {
        if let Some(validation) = cook.validation(node) {
            return Some(std::sync::Arc::clone(validation));
        }
        for edge in graph.incoming(node) {
            if seen.insert(edge.from) {
                queue.push_back(edge.from);
            }
        }
    }
    None
}

/// The column-major `T * R(order) * S` world matrix for a geo container,
/// resolved through the standard param path (degrees to radians for `rotate`,
/// `uniform_scale` folded into `scale`). Identity when the node, its
/// descriptor, or its params are unavailable. Shared by scene lowering and
/// picking so they can never disagree.
///
/// Composed by the kernel's `compose_trs`, exactly like the `transform` node,
/// with a zero pivot (a geo's pivot is its origin). It used to hand-roll
/// `T * Rz * Ry * Rx * S`, which is ZYX, while `transform` defaulted to XYZ:
/// identical angles on the two nodes meant different orientations. Old
/// documents keep their appearance because `migrate_geo` stamps `zyx` on any
/// geo where the order was actually observable.
pub(crate) fn geo_world_matrix(
    doc: &Document,
    registry: &Registry,
    previews: &Previews,
    geo: NodeId,
) -> Matrix4<f32> {
    let Some(node) = doc.graph(GraphContext::Root).ok().and_then(|g| g.node(geo)) else {
        return Matrix4::identity();
    };
    let Some(desc) = registry.get("geo") else {
        return Matrix4::identity();
    };
    let params = effective_params(previews, node.id, &node.params);
    let refs = root_refs(doc, registry, previews, node.id);
    let eval = crate::expr::EvalCtx::new(crate::expr::SceneTime::default()).with_refs(&refs);
    let Ok(p) = resolve_params_with(&params, &desc.params, &eval) else {
        return Matrix4::identity();
    };
    let scale = p.vec3_f32("scale");
    let uniform = p.f32("uniform_scale");
    compose_trs(
        p.vec3_f32("translate"),
        p.vec3_f32("rotate"), // radians: the resolver owns the conversion
        rotate_order_from_key(p.enum_key("rotate_order")),
        [scale[0] * uniform, scale[1] * uniform, scale[2] * uniform],
        [0.0; 3],
    )
}

/// What the viewport needs in order to test a click against a light's marker,
/// which is a screen-space question the ray alone cannot answer.
///
/// The engine has no renderer and must not gain one, so nothing here asks the
/// renderer where it drew anything: the marker's position is projected from
/// the same camera the pick ray was built from, and `radius_px` is the
/// renderer's own `MARKER_PX`, handed down by the host exactly the way it
/// already hands `GIZMO_PX * world_per_pixel` to the manipulator. That is what
/// keeps the drawn marker and its click target the same size without either
/// side knowing about the other.
#[derive(Debug, Clone, Copy)]
pub struct MarkerPick {
    /// The pane's view-projection, as the camera built it.
    pub view_proj: [[f32; 4]; 4],
    /// The pane's size in the same pixels `cursor_px` is measured in.
    pub viewport_px: [f32; 2],
    /// The cursor, in pane-relative pixels with the origin top left.
    pub cursor_px: [f32; 2],
    /// How far from a marker's centre still counts as hitting it.
    pub radius_px: f32,
}

/// Picks the root `geo` container whose displayed, world-transformed
/// geometry the ray hits nearest (single-pane picking; pane-awareness is
/// Runs entirely in Rust over CPU-retained cooked geometry, so
/// nothing crosses into JavaScript. Returns the producing geo node's id.
///
/// With `markers`, a light's marker is tested FIRST and wins outright. That is
/// not a preference between two candidates but a consequence of how they are
/// drawn: the marker is painted over the scene, so a click that lands on one
/// takes it rather than falling through to whatever is behind. Passing `None`
/// is the geometry-only behaviour, unchanged.
#[must_use]
pub fn pick_node(
    doc: &Document,
    registry: &Registry,
    cook: &CookEngine,
    previews: &Previews,
    origin: [f32; 3],
    direction: [f32; 3],
    markers: Option<MarkerPick>,
) -> Option<NodeId> {
    let dir = Vector3::from(direction);
    if dir.magnitude2() <= 1e-12 {
        return None;
    }
    let ray = Ray {
        origin: Point3::from(origin),
        direction: dir.normalize(),
    };
    if let Some(m) = markers
        && let Some(hit) = pick_light_marker(doc, registry, previews, &m)
    {
        return Some(hit);
    }
    let root = doc.graph(GraphContext::Root).ok()?;
    let mut best: Option<(f32, NodeId)> = None;
    for node in root.nodes() {
        if node.type_id != "geo" {
            continue;
        }
        let geo = node.id;
        // Hidden objects are not click-selectable (they are not rendered).
        if !geo_visible(doc, registry, previews, geo) {
            continue;
        }
        let Ok(subflow) = doc.graph(GraphContext::Subflow(geo)) else {
            continue;
        };
        let Some(display) = subflow.active_output else {
            continue;
        };
        let Some(outputs) = cook.outputs(display) else {
            continue;
        };
        let Some(Value::Geometry(set)) = outputs.get("geometry") else {
            continue;
        };
        let matrix = geo_world_matrix(doc, registry, previews, geo);
        for mesh in &set.meshes {
            // The raycaster reads indices as triangle triples; line and
            // point meshes are unpickable in the viewport.
            if mesh.topology != solarxy_core::MeshTopology::Triangles {
                continue;
            }
            // Transform this mesh's vertices to world space, then raycast.
            let world: Vec<[f32; 3]> = mesh
                .positions
                .iter()
                .map(|p| {
                    let tp = matrix.transform_point(Point3::from(*p));
                    [tp.x, tp.y, tp.z]
                })
                .collect();
            let view = MeshView {
                positions: &world,
                indices: mesh.indices.as_slice(),
                bounds: compute_bounds(&world),
            };
            if let Some(hit) = raycast_meshes(&ray, std::slice::from_ref(&view))
                && best.is_none_or(|(d, _)| hit.distance < d)
            {
                best = Some((hit.distance, geo));
            }
        }
    }
    best.map(|(_, node)| node)
}

/// The light whose marker the cursor is nearest, within the marker's own
/// radius, or `None`.
///
/// Screen space rather than a ray, because a light has no geometry to
/// intersect. That is also what gives a marker a predictable click area
/// wherever the light is: a distant light is exactly as easy to hit as a near
/// one, which a world-space test could not promise.
///
/// Ties break toward the camera. Two markers can genuinely coincide, because
/// ambient and hemisphere lights have no position and both mark the world
/// origin; the nearer one wins, and the other stays selectable from the node
/// canvas.
fn pick_light_marker(
    doc: &Document,
    registry: &Registry,
    previews: &Previews,
    m: &MarkerPick,
) -> Option<NodeId> {
    let root = doc.graph(GraphContext::Root).ok()?;
    let (w, h) = (m.viewport_px[0], m.viewport_px[1]);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let vp = Matrix4::from(m.view_proj);
    let mut best: Option<(f32, f32, NodeId)> = None;

    for node in root.nodes() {
        if !is_light(&node.type_id) {
            continue;
        }
        let Some(light) = light_from_node(doc, registry, previews, node) else {
            continue;
        };
        // An invisible light draws no marker, so it catches no clicks.
        if !light.visible {
            continue;
        }
        let anchor = match light.kind {
            // No position: their markers sit at the world origin, which is
            // where their helper has always drawn for the same reason.
            solarxy_core::scene::LightKind::Ambient
            | solarxy_core::scene::LightKind::Hemisphere => [0.0, 0.0, 0.0],
            _ => light.position,
        };

        let clip = vp * cgmath::Vector4::new(anchor[0], anchor[1], anchor[2], 1.0);
        // Behind the eye, or exactly on the plane through it: not on screen.
        if clip.w <= 1e-6 {
            continue;
        }
        let ndc = (clip.x / clip.w, clip.y / clip.w);
        // NDC to pane pixels, y flipped: clip space is y-up and a cursor is
        // y-down.
        let px = (ndc.0 * 0.5 + 0.5) * w;
        let py = (0.5 - ndc.1 * 0.5) * h;
        let d = (px - m.cursor_px[0]).hypot(py - m.cursor_px[1]);
        if d > m.radius_px {
            continue;
        }
        let depth = clip.w;
        if best.is_none_or(|(_, bd, _)| depth < bd) {
            best = Some((d, depth, node.id));
        }
    }
    best.map(|(_, _, node)| node)
}

/// [`pick_node`] with the full hit detail the review workflow anchors to:
/// the mesh index within the displayed set, the face, the barycentric
/// coordinate, and the world-space hit point. The per-mesh raycast means
/// `RaycastHit::mesh_index` is always 0 relative to its one-mesh slice, so
/// the enumerate index is tracked here instead.
#[must_use]
pub(crate) fn pick_node_detailed(
    doc: &Document,
    registry: &Registry,
    cook: &CookEngine,
    previews: &Previews,
    origin: [f32; 3],
    direction: [f32; 3],
) -> Option<super::PickDetail> {
    let dir = Vector3::from(direction);
    if dir.magnitude2() <= 1e-12 {
        return None;
    }
    let ray = Ray {
        origin: Point3::from(origin),
        direction: dir.normalize(),
    };
    let root = doc.graph(GraphContext::Root).ok()?;
    let mut best: Option<super::PickDetail> = None;
    for node in root.nodes() {
        if node.type_id != "geo" {
            continue;
        }
        let geo = node.id;
        // Hidden objects are not click-selectable (they are not rendered).
        if !geo_visible(doc, registry, previews, geo) {
            continue;
        }
        let Some(set) = display_output(doc, cook, geo) else {
            continue;
        };
        let matrix = geo_world_matrix(doc, registry, previews, geo);
        for (mesh_index, mesh) in set.meshes.iter().enumerate() {
            // Unpickable off-triangles, matching pick_node.
            if mesh.topology != solarxy_core::MeshTopology::Triangles {
                continue;
            }
            let world: Vec<[f32; 3]> = mesh
                .positions
                .iter()
                .map(|p| {
                    let tp = matrix.transform_point(Point3::from(*p));
                    [tp.x, tp.y, tp.z]
                })
                .collect();
            let view = MeshView {
                positions: &world,
                indices: mesh.indices.as_slice(),
                bounds: compute_bounds(&world),
            };
            if let Some(hit) = raycast_meshes(&ray, std::slice::from_ref(&view))
                && best.as_ref().is_none_or(|b| hit.distance < b.distance)
            {
                best = Some(super::PickDetail {
                    node: geo,
                    mesh: u32::try_from(mesh_index).unwrap_or(u32::MAX),
                    face: hit.face_index,
                    barycentric: hit.barycentric,
                    world_pos: [hit.world_pos.x, hit.world_pos.y, hit.world_pos.z],
                    distance: hit.distance,
                });
            }
        }
    }
    best
}

/// Resolves a light node's params into a `LightDef` (the resolver already
/// converts angles to radians; `LightDef` stores radians). Returns `None`
/// for a non-light node or one missing its descriptor.
/// Resolves a `camera` root node to a `CameraDef` (the camera analog of
/// `light_from_node`). `fov_y` comes out in radians: a perspective camera
/// reads it directly (the resolver converts its degrees param), a physical
/// camera derives it from focal length + sensor width, and an orthographic
/// camera does not use it. Previews are honored so a locked-camera reframe
/// (which streams param previews) tracks live.
fn camera_from_node(
    doc: &Document,
    registry: &Registry,
    previews: &Previews,
    cook: &CookEngine,
    node: &crate::document::NodeData,
) -> Option<CameraDef> {
    use solarxy_core::scene::CameraKind;

    let desc = registry.get(&node.type_id)?;
    let params = effective_params(previews, node.id, &node.params);
    let refs = root_refs(doc, registry, previews, node.id);
    let eval = crate::expr::EvalCtx::new(crate::expr::SceneTime::default()).with_refs(&refs);
    let p = resolve_params_with(&params, &desc.params, &eval).ok()?;
    let f32p = |key: &str| -> f32 {
        match p.get(key) {
            Some(ParamValue::Float(v)) => *v as f32,
            _ => 0.0,
        }
    };
    let kind = match p.get("kind") {
        Some(ParamValue::Enum(k)) if k == "orthographic" => CameraKind::Orthographic,
        Some(ParamValue::Enum(k)) if k == "physical" => CameraKind::Physical,
        _ => CameraKind::Perspective,
    };
    let fov_y = if kind == CameraKind::Physical {
        let focal = f32p("focal_length").max(1e-3);
        let sensor = f32p("sensor_width").max(1e-3);
        2.0 * (sensor / (2.0 * focal)).atan()
    } else {
        let v = f32p("fov_y");
        if v > 1e-4 { v } else { 45.0_f32.to_radians() }
    };
    let aspect = f32p("aspect");

    // The look. Tables come from the cook's per-node side cache rather than
    // from a param, for the reason the environment's image does: they have
    // no wire to travel on. A camera whose table failed to parse simply
    // reports none, and the node carries the diagnostic.
    let tables = cook.luts(node.id);
    let table = |slot: usize| {
        tables
            .and_then(|t| t[slot].as_ref())
            .map(std::sync::Arc::clone)
    };
    let tone = match p.get("tone") {
        Some(ParamValue::Enum(k)) if k == "none" => Some(ToneCurve::None),
        Some(ParamValue::Enum(k)) if k == "linear" => Some(ToneCurve::Linear),
        Some(ParamValue::Enum(k)) if k == "reinhard" => Some(ToneCurve::Reinhard),
        Some(ParamValue::Enum(k)) if k == "aces" => Some(ToneCurve::AcesFilmic),
        // Anything else, including the `inherit` default and a v1 camera
        // with no such param at all, leaves the pane's choice alone.
        _ => None,
    };
    let exposure = match p.get("exposure") {
        Some(ParamValue::Float(v)) => *v as f32,
        // Absent means as-rendered, never black.
        _ => 1.0,
    };
    let strength = |key: &str| match p.get(key) {
        Some(ParamValue::Float(v)) => (*v as f32).clamp(0.0, 1.0),
        _ => 1.0,
    };
    let vec3_or = |key: &str, fallback: [f32; 3]| match p.get(key) {
        Some(ParamValue::Vec3(_)) => p.vec3_f32(key),
        _ => fallback,
    };
    let look = CameraLook {
        exposure,
        tone,
        lift: vec3_or("lift", [0.0; 3]),
        gamma: vec3_or("gamma", [1.0; 3]),
        gain: vec3_or("gain", [1.0; 3]),
        lut_a: table(0),
        lut_a_strength: strength("lut_a_strength"),
        lut_b: table(1),
        lut_b_strength: strength("lut_b_strength"),
    };

    // The lens, resolved the way `fov_y` above is: the runtime description of
    // a camera should not make its consumer work out which of three
    // projections the user authored.
    //
    // An f-number is the focal length over the aperture's diameter, so the
    // radius is `focal / (2 * f)`. A physical camera states its focal length in
    // millimetres; a perspective one has none, so one is derived back out of
    // the field of view against the same 36mm sensor width the node documents
    // as driving it. That is what makes f/2.8 mean the same blur on both, and
    // it is the exact inverse of the formula the physical arm above applies.
    let lens = if kind == CameraKind::Orthographic {
        // Parallel rays have no lens and nothing to focus. The node hides the
        // controls here too, but a stored value survives a projection change,
        // so this is what stops one taking effect where it means nothing.
        solarxy_core::scene::CameraLens::default()
    } else {
        let f_stop = f32p("f_stop");
        let aperture_radius = if f_stop > 0.0 {
            let focal_mm = if kind == CameraKind::Physical {
                f32p("focal_length").max(1e-3)
            } else {
                const DEFAULT_SENSOR_MM: f32 = 36.0;
                DEFAULT_SENSOR_MM / (2.0 * (fov_y * 0.5).tan().max(1e-6))
            };
            // Millimetres to world units, which are metres everywhere else in
            // the scene contract.
            focal_mm / (2.0 * f_stop) / 1000.0
        } else {
            0.0
        };
        solarxy_core::scene::CameraLens {
            aperture_radius,
            focus_distance: f32p("focus_distance").max(0.0),
            blades: match p.get("aperture_blades") {
                Some(ParamValue::Int(v)) => u32::try_from(*v).unwrap_or(0),
                _ => 0,
            },
        }
    };

    Some(CameraDef {
        id: SceneObjectId(node.id.0),
        kind,
        position: p.vec3_f32("position"),
        target: p.vec3_f32("target"),
        up: [0.0, 1.0, 0.0],
        fov_y,
        near: f32p("near"),
        far: f32p("far"),
        ortho_scale: f32p("ortho_scale"),
        aspect: if aspect > 1e-3 { aspect } else { 16.0 / 9.0 },
        show_gizmo: matches!(p.get("show_gizmo"), Some(ParamValue::Bool(true))),
        gizmo_size: f32p("gizmo_size"),
        look,
        lens,
    })
}

/// Build the environment op from an `environment` node.
///
/// The decoded image comes from the cook's per-node side cache rather than
/// from an output value: `Value` has no float-image variant, and adding one
/// would mean a new `DataType`, which is a deliberate frontend change. A
/// node whose HDRI has not finished decoding yet (the web path parks on a
/// worker job) simply reports no image, and the next delta carries it.
fn environment_from_node(
    doc: &Document,
    registry: &Registry,
    previews: &Previews,
    cook: &CookEngine,
    node: &crate::document::NodeData,
) -> Option<SceneOp> {
    use solarxy_core::scene::BackgroundKind;

    let desc = registry.get(&node.type_id)?;
    let params = effective_params(previews, node.id, &node.params);
    let refs = root_refs(doc, registry, previews, node.id);
    let eval = crate::expr::EvalCtx::new(crate::expr::SceneTime::default()).with_refs(&refs);
    let p = resolve_params_with(&params, &desc.params, &eval).ok()?;

    let f32p = |key: &str| -> f32 {
        match p.get(key) {
            Some(ParamValue::Float(v)) => *v as f32,
            _ => 0.0,
        }
    };
    let background = match p.get("background") {
        Some(ParamValue::Enum(k)) if k == crate::nodes::environment_node::BACKGROUND_HDRI_SKY => {
            BackgroundKind::HdriSky
        }
        _ => BackgroundKind::Keep,
    };
    // The param is degrees, because that is what a user dials; the
    // contract is radians, because that is what the shader rotates by.
    let rotation = f32p("rotation").to_radians();
    let intensity = match p.get("intensity") {
        Some(ParamValue::Float(v)) => *v as f32,
        // Absent means as-authored, never unlit.
        _ => solarxy_core::view_config::DEFAULT_HDRI_INTENSITY,
    };

    Some(SceneOp::SetEnvironment {
        hdri: cook.environment(node.id).map(std::sync::Arc::clone),
        rotation,
        intensity,
        background,
    })
}

fn light_from_node(
    doc: &Document,
    registry: &Registry,
    previews: &Previews,
    node: &crate::document::NodeData,
) -> Option<LightDef> {
    use solarxy_core::scene::LightKind;

    let desc = registry.get(&node.type_id)?;
    let params = effective_params(previews, node.id, &node.params);
    let refs = root_refs(doc, registry, previews, node.id);
    let eval = crate::expr::EvalCtx::new(crate::expr::SceneTime::default()).with_refs(&refs);
    let p = resolve_params_with(&params, &desc.params, &eval).ok()?;
    let color = |key: &str| -> [f32; 3] {
        match p.get(key) {
            Some(ParamValue::Color(c)) => [c[0], c[1], c[2]],
            _ => [1.0; 3],
        }
    };
    let f32p = |key: &str| -> f32 {
        match p.get(key) {
            Some(ParamValue::Float(v)) => *v as f32,
            _ => 0.0,
        }
    };
    let boolp = |key: &str| matches!(p.get(key), Some(ParamValue::Bool(true)));

    let mut light = LightDef {
        // Derived the same way a geo's and a camera's are, so a marker click
        // comes back naming the node the canvas and the panel already mean.
        id: solarxy_core::scene::SceneObjectId(node.id.0),
        kind: LightKind::Point,
        position: [0.0; 3],
        direction: [0.0, -1.0, 0.0],
        color: color("color"),
        intensity: f32p("intensity"),
        range: 0.0,
        decay: 0.0,
        radius: 0.0,
        inner_cone: 0.0,
        outer_cone: 0.0,
        area_extent: [0.0; 2],
        rotate: [0.0; 3],
        two_sided: false,
        ground_color: [0.0; 3],
        cast_shadow: false,
        shadow_map_size: 1024,
        shadow_bias: f32p("bias"),
        visible: !matches!(p.get("visible"), Some(ParamValue::Bool(false))),
        // Declared on every light since and read by nothing until now.
        show_helper: boolp("show_helper"),
        helper_size: f32p("helper_size"),
    };

    // The unit direction from a light's position toward its target.
    let direction_to_target = |p: &crate::registry::resolve::ResolvedParams| -> [f32; 3] {
        let pos = p.vec3_f32("position");
        let tgt = p.vec3_f32("target");
        let d = [tgt[0] - pos[0], tgt[1] - pos[1], tgt[2] - pos[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len > 1e-6 {
            [d[0] / len, d[1] / len, d[2] / len]
        } else {
            [0.0, -1.0, 0.0]
        }
    };

    match node.type_id.as_str() {
        "point_light" => {
            light.kind = LightKind::Point;
            light.position = p.vec3_f32("position");
            light.range = f32p("range");
            light.decay = f32p("decay");
            light.radius = f32p("radius");
            light.cast_shadow = boolp("cast_shadow");
            light.shadow_map_size = map_size(&p);
        }
        "directional_light" => {
            light.kind = LightKind::Directional;
            // The SHADING ignores a directional light's position (its shadow
            // frustum auto-fits the scene), which is why this was never filled.
            // The helper arrow still has to be drawn somewhere, though, and the
            // node has always carried the position it should be drawn at.
            light.position = p.vec3_f32("position");
            light.direction = direction_to_target(&p);
            light.cast_shadow = boolp("cast_shadow");
            light.shadow_map_size = map_size(&p);
        }
        "spot_light" => {
            light.kind = LightKind::Spot;
            light.position = p.vec3_f32("position");
            light.direction = direction_to_target(&p);
            light.range = f32p("range");
            light.decay = f32p("decay");
            light.radius = f32p("radius");
            // `angle` is the outer cone half-angle (radians after resolve);
            // penumbra narrows the inner cone toward it.
            let outer = f32p("angle");
            let penumbra = f32p("penumbra").clamp(0.0, 1.0);
            light.outer_cone = outer;
            light.inner_cone = outer * (1.0 - penumbra);
            light.cast_shadow = boolp("cast_shadow");
            light.shadow_map_size = map_size(&p);
        }
        "ambient_light" => {
            light.kind = LightKind::Ambient;
        }
        "hemisphere_light" => {
            light.kind = LightKind::Hemisphere;
            light.color = color("sky_color");
            light.ground_color = color("ground_color");
        }
        "rect_area_light" => {
            light.kind = LightKind::RectArea;
            light.position = p.vec3_f32("translate");
            light.area_extent = [f32p("width"), f32p("height")];
            // Radians: the resolver owns the degrees conversion, as it does
            // for every other angle in the registry.
            light.rotate = p.vec3_f32("rotate");
            light.two_sided = boolp("two_sided");
            // The helper draws its arrow along `direction`, so keep it as
            // the face normal rather than the default straight-down.
            light.direction = light.rect_basis().normal;
        }
        _ => return None,
    }
    Some(light)
}

/// The shadow map resolution enum (`"512"` / `"1024"` / `"2048"`).
fn map_size(p: &crate::registry::resolve::ResolvedParams) -> u32 {
    match p.get("map_size") {
        Some(ParamValue::Enum(s)) => s.parse().unwrap_or(1024),
        _ => 1024,
    }
}
