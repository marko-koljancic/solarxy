//! The `line` primitive: the first curve primitive, a straight polyline
//! between two endpoints with an even point count for downstream deforms.

use solarxy_kernel::GeometrySet;
use solarxy_kernel::primitives::generate_line;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

fn endpoint(key: &str, label: &str, default: [f64; 3], which: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "geometry",
        ParamType::Vec3,
        ParamValue::Vec3(default),
    )
    .unit(Unit::Meters)
    .driven_by_port(format!("{key}_point"))
    .doc(format!(
        "The {which} end of the line, in metres. Both ends are free points; \
         nothing pins the line to the origin. A geometry wired into the \
         matching input overrides this with its first point."
    ))
}

fn point_input(key: &str, label: &str, which: &str) -> PortSpec {
    // Carrying here is not "placements survive into the output" -- nothing
    // of the input survives, the output is a fresh two-point polyline. It is
    // the narrower claim the driver acts on: hand the set through unbaked.
    // This node reads one vertex, and baking ten thousand copies of a
    // scatter to read one of them would be a cliff with nothing at the
    // bottom of it. `first_point` consults the placement list instead.
    PortSpec::single(key, label, DataType::Geometry, false)
        .carries_placements()
        .doc(format!(
            "Optional. When connected, the {which} endpoint snaps to point 0 of \
             this geometry's first non-empty mesh, overriding the parameter."
        ))
}

/// Point 0 of the first non-empty mesh, the anchor rule for connected
/// endpoint inputs: deterministic under recooks, unlike a centroid.
///
/// Placed through the mesh's first placement when it carries one. This port
/// declares that it carries placements, which spares a node that reads a
/// single vertex from materialising ten thousand copies of a scatter to
/// find it, and that bargain only holds if the vertex is read where the
/// copy actually sits. Reading the prototype's untransformed position
/// instead would put the endpoint somewhere no visible point occupies.
fn first_point(set: &GeometrySet) -> Option<[f32; 3]> {
    set.meshes.iter().find_map(|mesh| {
        let p = mesh.positions.first().copied()?;
        Some(
            match mesh.instances.as_ref().and_then(|list| list.first()) {
                Some(placement) => placement.apply(p),
                None => p,
            },
        )
    })
}

fn resolve_endpoint(inputs: &Inputs, cx: &mut CookCtx, port: &str, fallback: [f32; 3]) -> [f32; 3] {
    match inputs.geometry(port) {
        Some(set) => first_point(set).unwrap_or_else(|| {
            cx.warn(format!(
                "line input `{port}` is connected but has no points; using the parameter instead"
            ));
            fallback
        }),
        None => fallback,
    }
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "line",
        version: 1,
        display_name: "Line",
        category: Category::Generators,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            point_input("start_point", "Start Point", "starting"),
            point_input("end_point", "End Point", "finishing"),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Line",
            vec![
                endpoint("start", "Start", [0.0, 0.0, 0.0], "starting"),
                endpoint("end", "End", [0.0, 1.0, 0.0], "finishing"),
                ParamSpec::new(
                    "points",
                    "Points",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(2),
                )
                .hard(2.0, 1025.0)
                .soft(2.0, 65.0)
                .step(1.0)
                .doc(
                    "How many evenly spaced points the line carries, endpoints \
                     included. 2 is a single segment; raise it when a deform or \
                     scatter downstream needs interior points to work with.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A straight polyline from Start to End, subdivided into evenly \
              spaced points. It is the first curve primitive: line topology has \
              no surface, so it draws as an unlit wire at a constant one-pixel \
              width, unaffected by lights and materials.\n\n\
              At the default 2 points it is a single segment. More points give \
              downstream nodes something to grab: a deform has interior points \
              to move, and `copy_to_points` can stamp a template along the \
              line's vertices.\n\n\
              The default runs from the origin one metre up the Y axis. Wires \
              and edges are unpickable in the viewport; select the node on the \
              canvas.\n\n\
              The two optional inputs anchor the ends to existing geometry: a \
              connected input overrides its parameter with point 0 of the \
              geometry's first non-empty mesh, so a single-point `scatter` or \
              `points_from_geo` upstream pins that end and the line follows it \
              on every recook.",
        search_aliases: &["curve", "segment", "polyline", "wire", "path"],
        glyph: "line",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let start = resolve_endpoint(inputs, cx, "start_point", p.vec3_f32("start"));
    let end = resolve_endpoint(inputs, cx, "end_point", p.vec3_f32("end"));
    let set = GeometrySet::from_mesh(generate_line(start, end, p.u32("points")));
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;
    use crate::cook::InputSlot;
    use crate::registry::coerce::Value;
    use solarxy_kernel::KernelMesh;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(inputs: &Inputs) -> (Outputs, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, inputs, &mut cx).unwrap() else {
            panic!("line cooks synchronously");
        };
        (out, cx.take_warnings())
    }

    fn positions_of(out: &Outputs) -> Vec<[f32; 3]> {
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        (*set.meshes[0].positions).clone()
    }

    fn point_slot(positions: Vec<[f32; 3]>) -> InputSlot {
        InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(
            KernelMesh::points("pts", positions),
        ))))
    }

    #[test]
    fn cooks_a_renderable_polyline_at_defaults() {
        let (out, warnings) = run(&Inputs::default());
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert_eq!(
            set.meshes[0].topology,
            solarxy_core::geometry::MeshTopology::Lines
        );
        assert_eq!(set.meshes[0].vertex_count(), 2);
        assert!(set.meshes[0].is_renderable());
        assert!(warnings.is_empty());
    }

    #[test]
    fn connected_inputs_override_the_endpoint_params() {
        let mut slots = BTreeMap::new();
        slots.insert(
            "start_point".to_string(),
            point_slot(vec![[3.0, 0.0, 0.0], [9.0, 9.0, 9.0]]),
        );
        slots.insert("end_point".to_string(), point_slot(vec![[0.0, 0.0, 5.0]]));
        let (out, warnings) = run(&Inputs::new(slots));
        let positions = positions_of(&out);
        assert_eq!(positions[0], [3.0, 0.0, 0.0], "point 0 wins, not point 1");
        assert_eq!(positions[1], [0.0, 0.0, 5.0]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn one_connected_input_leaves_the_other_param_in_charge() {
        let mut slots = BTreeMap::new();
        slots.insert("start_point".to_string(), point_slot(vec![[2.0, 2.0, 2.0]]));
        let (out, warnings) = run(&Inputs::new(slots));
        let positions = positions_of(&out);
        assert_eq!(positions[0], [2.0, 2.0, 2.0]);
        assert_eq!(positions[1], [0.0, 1.0, 0.0], "end param default holds");
        assert!(warnings.is_empty());
    }

    #[test]
    fn an_instanced_input_is_read_where_its_first_copy_actually_sits() {
        // The endpoint ports carry placements rather than baking, so that a
        // node reading one vertex does not materialise a whole scatter to
        // find it. That only produces the right answer if the vertex is
        // read through the placement: the prototype's untransformed
        // position is somewhere no visible point occupies.
        let mut translate = solarxy_core::scene::InstanceXform::IDENTITY;
        translate.0[3] = [10.0, 20.0, 30.0, 1.0];
        let set = solarxy_kernel::GeometrySet::from_parts_instanced(
            vec![KernelMesh::points("pts", vec![[1.0, 2.0, 3.0]])],
            Vec::new(),
            vec![translate, solarxy_core::scene::InstanceXform::IDENTITY],
        );

        let mut slots = BTreeMap::new();
        slots.insert(
            "start_point".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(set))),
        );
        let (out, warnings) = run(&Inputs::new(slots));

        assert_eq!(positions_of(&out)[0], [11.0, 22.0, 33.0]);
        assert!(warnings.is_empty(), "carrying warns about nothing");
    }

    #[test]
    fn an_empty_connected_input_warns_and_falls_back() {
        let mut slots = BTreeMap::new();
        slots.insert("start_point".to_string(), point_slot(Vec::new()));
        let (out, warnings) = run(&Inputs::new(slots));
        let positions = positions_of(&out);
        assert_eq!(positions[0], [0.0, 0.0, 0.0], "start param default holds");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("start_point"), "{warnings:?}");
    }
}
