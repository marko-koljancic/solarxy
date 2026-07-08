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

use cgmath::{Matrix4, Rad, Vector3};
use solarxy_core::scene::{LightDef, SceneDelta, SceneObjectId, SceneOp};

use crate::cook::CookEngine;
use crate::document::{Document, GraphContext, NodeId};
use crate::params::ParamValue;
use crate::registry::Registry;
use crate::registry::coerce::Value;
use crate::registry::resolve::resolve_params;

/// Builds a full scene delta from scratch each frame. The renderer diffs
/// against its own state, so a full rebuild is safe (and light lists are
/// tiny). `Clear`-then-rebuild is deliberately avoided: object ids are
/// stable per geo node, so the renderer keeps unchanged uploads.
#[must_use]
pub fn build_scene_delta(doc: &Document, registry: &Registry, cook: &CookEngine) -> SceneDelta {
    let mut delta = SceneDelta::default();
    let Ok(root) = doc.graph(GraphContext::Root) else {
        return delta;
    };

    let mut lights: Vec<LightDef> = Vec::new();

    for node in root.nodes() {
        match node.type_id.as_str() {
            "geo" => emit_geo(doc, registry, cook, node.id, &mut delta),
            id if is_light(id) => {
                if let Some(light) = light_from_node(registry, node) {
                    lights.push(light);
                }
            }
            _ => {}
        }
    }

    delta.push(SceneOp::SetLights { lights });
    delta
}

/// Whether a type id is one of the six light nodes.
fn is_light(type_id: &str) -> bool {
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

/// Emits the `UpsertGeometry` + `SetTransform` for one geo container.
fn emit_geo(
    doc: &Document,
    _registry: &Registry,
    cook: &CookEngine,
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
    if let Some(outputs) = cook.outputs(display)
        && let Some(Value::Geometry(set)) = outputs.get("geometry")
    {
        delta.push(SceneOp::UpsertGeometry {
            id: object_id,
            geometry: std::sync::Arc::new(set.to_cooked()),
        });
    }
    // The geo node's transform, applied as the object transform. Resolved
    // through the standard param path (degrees to radians for `rotate`).
    if let Some(node) = doc.graph(GraphContext::Root).ok().and_then(|g| g.node(geo))
        && let Some(desc) = _registry.get("geo")
        && let Ok(p) = resolve_params(&node.params, &desc.params)
    {
        let translate = p.vec3_f32("translate");
        let rotate = p.vec3_f32("rotate"); // radians
        let scale = p.vec3_f32("scale");
        let uniform = p.f32("uniform_scale");
        let scale = [scale[0] * uniform, scale[1] * uniform, scale[2] * uniform];
        let matrix = geo_matrix(translate, rotate, scale);
        delta.push(SceneOp::SetTransform {
            id: object_id,
            transform: matrix,
        });
    }
}

/// Column-major `T * Rz * Ry * Rx * S` world matrix for a geo container
/// (the container transform is not baked; the renderer applies it).
fn geo_matrix(translate: [f32; 3], rotate: [f32; 3], scale: [f32; 3]) -> [[f32; 4]; 4] {
    let t = Matrix4::from_translation(Vector3::from(translate));
    let rx = Matrix4::from_angle_x(Rad(rotate[0]));
    let ry = Matrix4::from_angle_y(Rad(rotate[1]));
    let rz = Matrix4::from_angle_z(Rad(rotate[2]));
    let s = Matrix4::from_nonuniform_scale(scale[0], scale[1], scale[2]);
    let m = t * rz * ry * rx * s;
    m.into()
}

/// Resolves a light node's params into a `LightDef` (the resolver already
/// converts angles to radians; `LightDef` stores radians). Returns `None`
/// for a non-light node or one missing its descriptor.
fn light_from_node(registry: &Registry, node: &crate::document::NodeData) -> Option<LightDef> {
    use solarxy_core::scene::LightKind;

    let desc = registry.get(&node.type_id)?;
    let p = resolve_params(&node.params, &desc.params).ok()?;
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
