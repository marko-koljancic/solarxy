//! The `line` primitive: the first curve primitive, a straight polyline
//! between two endpoints with an even point count for downstream deforms.

use solarxy_kernel::primitives::generate_line;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

fn endpoint(key: &str, label: &str, default: [f64; 3], which: &str) -> ParamSpec {
    ParamSpec::new(
        key,
        label,
        "geometry",
        ParamType::Vec3,
        ParamValue::Vec3(default),
    )
    .unit(Unit::Meters)
    .doc(format!(
        "The {which} end of the line, in metres. Both ends are free points; \
         nothing pins the line to the origin."
    ))
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "line",
        version: 1,
        display_name: "Line",
        category: Category::Primitives,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
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
              canvas.",
        search_aliases: &["curve", "segment", "polyline", "wire", "path"],
        glyph: "line",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_line(
        p.vec3_f32("start"),
        p.vec3_f32("end"),
        p.u32("points"),
    ));
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::coerce::Value;
    use std::collections::BTreeMap;

    #[test]
    fn cooks_a_renderable_polyline_at_defaults() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &Inputs::default(), &mut cx).unwrap() else {
            panic!("line cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert_eq!(
            set.meshes[0].topology,
            solarxy_core::geometry::MeshTopology::Lines
        );
        assert_eq!(set.meshes[0].vertex_count(), 2);
        assert!(set.meshes[0].is_renderable());
    }
}
