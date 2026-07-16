//! Lowering cooked geometry to a `solarxy_core::scene::SceneDelta` for the
//! renderer, the sole engine-to-renderer contract.
//!
//! The mapping (node catalog part II):
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

use cgmath::{InnerSpace, Matrix4, Point3, SquareMatrix, Transform, Vector3};
use solarxy_core::geometry::compute_bounds;
use solarxy_core::raycast::{MeshView, Ray, raycast_meshes};
use solarxy_core::scene::{CameraDef, LightDef, SceneDelta, SceneObjectId, SceneOp};

use crate::cook::CookEngine;
use crate::document::{Document, GraphContext, NodeId};
use crate::nodes::common::rotate_order_from_key;
use crate::params::ParamValue;
use crate::previews::{Previews, effective_params};
use crate::registry::Registry;
use crate::registry::coerce::Value;
use crate::registry::resolve::resolve_params;
use solarxy_kernel::transform::compose_trs;

/// Builds a full scene delta from scratch each frame. The renderer diffs
/// against its own state, so a full rebuild is safe (and light lists are
/// tiny). `Clear`-then-rebuild is deliberately avoided: object ids are
/// stable per geo node, so the renderer keeps unchanged uploads.
#[must_use]
pub fn build_scene_delta(
    doc: &Document,
    registry: &Registry,
    cook: &CookEngine,
    previews: &Previews,
) -> SceneDelta {
    let mut delta = SceneDelta::default();
    let Ok(root) = doc.graph(GraphContext::Root) else {
        return delta;
    };

    let mut lights: Vec<LightDef> = Vec::new();
    let mut cameras: Vec<CameraDef> = Vec::new();

    for node in root.nodes() {
        match node.type_id.as_str() {
            "geo" => emit_geo(doc, registry, cook, previews, node.id, &mut delta),
            "camera" => {
                if let Some(cam) = camera_from_node(registry, previews, node) {
                    cameras.push(cam);
                }
            }
            id if is_light(id) => {
                if let Some(light) = light_from_node(registry, previews, node) {
                    lights.push(light);
                }
            }
            _ => {}
        }
    }

    delta.push(SceneOp::SetLights { lights });
    delta.push(SceneOp::SetCameras { cameras });
    delta
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
    let Ok(p) = resolve_params(&params, &desc.params) else {
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
fn emit_geo(
    doc: &Document,
    registry: &Registry,
    cook: &CookEngine,
    previews: &Previews,
    geo: NodeId,
    delta: &mut SceneDelta,
) {
    let object_id = SceneObjectId(geo.0);
    let Ok(subflow) = doc.graph(GraphContext::Subflow(geo)) else {
        return;
    };
    let Some(display) = subflow.active_output else {
        return;
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
    let Ok(p) = resolve_params(&params, &desc.params) else {
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

/// Picks the root `geo` container whose displayed, world-transformed
/// geometry the ray hits nearest (single-pane picking; pane-awareness is
/// Phase 6). Runs entirely in Rust over CPU-retained cooked geometry, so
/// nothing crosses into JavaScript. Returns the producing geo node's id.
#[must_use]
pub fn pick_node(
    doc: &Document,
    registry: &Registry,
    cook: &CookEngine,
    previews: &Previews,
    origin: [f32; 3],
    direction: [f32; 3],
) -> Option<NodeId> {
    let dir = Vector3::from(direction);
    if dir.magnitude2() <= 1e-12 {
        return None;
    }
    let ray = Ray {
        origin: Point3::from(origin),
        direction: dir.normalize(),
    };
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
    registry: &Registry,
    previews: &Previews,
    node: &crate::document::NodeData,
) -> Option<CameraDef> {
    use solarxy_core::scene::CameraKind;

    let desc = registry.get(&node.type_id)?;
    let params = effective_params(previews, node.id, &node.params);
    let p = resolve_params(&params, &desc.params).ok()?;
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
    let fov_y = match kind {
        CameraKind::Physical => {
            let focal = f32p("focal_length").max(1e-3);
            let sensor = f32p("sensor_width").max(1e-3);
            2.0 * (sensor / (2.0 * focal)).atan()
        }
        _ => {
            let v = f32p("fov_y");
            if v > 1e-4 { v } else { 45.0_f32.to_radians() }
        }
    };
    let aspect = f32p("aspect");
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
    })
}

fn light_from_node(
    registry: &Registry,
    previews: &Previews,
    node: &crate::document::NodeData,
) -> Option<LightDef> {
    use solarxy_core::scene::LightKind;

    let desc = registry.get(&node.type_id)?;
    let params = effective_params(previews, node.id, &node.params);
    let p = resolve_params(&params, &desc.params).ok()?;
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
        kind: LightKind::Point,
        position: [0.0; 3],
        direction: [0.0, -1.0, 0.0],
        color: color("color"),
        intensity: f32p("intensity"),
        range: 0.0,
        decay: 0.0,
        inner_cone: 0.0,
        outer_cone: 0.0,
        area_extent: [0.0; 2],
        ground_color: [0.0; 3],
        cast_shadow: false,
        shadow_map_size: 1024,
        shadow_bias: f32p("bias"),
        visible: !matches!(p.get("visible"), Some(ParamValue::Bool(false))),
        // Declared on every light since Phase 8 and read by nothing until now.
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
