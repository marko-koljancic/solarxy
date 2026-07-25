//! The `circle` primitive: a closed curve loop around one coordinate
//! axis, and the profile source the extrude family will consume.

use solarxy_kernel::array::Axis;
use solarxy_kernel::primitives::generate_circle;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "circle",
        version: 1,
        display_name: "Circle",
        category: Category::Generators,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: vec![geometry_output()],
        params: params_with(
            "Circle",
            vec![
                ParamSpec::new(
                    "radius",
                    "Radius",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(0.5),
                )
                .hard(0.001, 10000.0)
                .soft(0.01, 100.0)
                .unit(Unit::Meters)
                .doc("The loop's radius, in metres, centred on the origin."),
                ParamSpec::new(
                    "segments",
                    "Segments",
                    "geometry",
                    ParamType::Int,
                    ParamValue::Int(32),
                )
                .hard(3.0, 512.0)
                .soft(3.0, 128.0)
                .step(1.0)
                .doc(
                    "How many straight segments approximate the circle. 3 is a \
                     triangle, 32 reads as smooth at typical sizes; raise it \
                     only when the circle is large on screen or feeds a \
                     downstream operation that needs the density.",
                ),
                ParamSpec::new(
                    "axis",
                    "Axis",
                    "geometry",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("x", "X"),
                            EnumVariant::new("y", "Y"),
                            EnumVariant::new("z", "Z"),
                        ],
                    },
                    ParamValue::Enum("y".into()),
                )
                .doc(
                    "The axis the circle rings around: the loop lies in the \
                     plane perpendicular to it. The default Y lays it flat in \
                     the ground plane.",
                ),
            ],
        ),
        bypass: BypassBehavior::Mute,
        doc: "A closed loop of straight segments at Radius around the chosen \
              axis, centred on the origin. Like `line` it is a curve: no \
              surface, no normals, drawn as an unlit one-pixel wire.\n\n\
              It is the standard profile shape: the upcoming extrude family \
              consumes closed loops like this one, and until then it serves as \
              a guide, a path for copies, or a scatter-free ring of points via \
              `points_from_geo`.\n\n\
              The default Y axis lays the loop flat in the ground plane, \
              winding counter-clockwise seen from above. Segments trades \
              smoothness against point count: each segment is one straight \
              piece and one carrier point.",
        search_aliases: &["ring", "loop", "profile", "disc", "curve"],
        glyph: "circle",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, _in: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let axis = match p.enum_key("axis") {
        "x" => Axis::X,
        "z" => Axis::Z,
        _ => Axis::Y,
    };
    let set = solarxy_kernel::GeometrySet::from_mesh(generate_circle(
        p.f32("radius"),
        p.u32("segments"),
        axis,
    ));
    Ok(CookOutcome::Done(Outputs::geometry(set)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use std::collections::BTreeMap;

    #[test]
    fn cooks_a_closed_ground_plane_loop_at_defaults() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &Inputs::default(), &mut cx).unwrap() else {
            panic!("circle cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        let mesh = &set.meshes[0];
        assert_eq!(mesh.vertex_count(), 32);
        assert_eq!(mesh.primitive_count(), 32, "closed loop");
        assert!(mesh.positions.iter().all(|p| p[1].abs() < 1e-6));
    }

    #[test]
    fn the_axis_param_reorients_the_loop() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "axis".to_string(),
            ParamSource::Literal(ParamValue::Enum("z".into())),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &Inputs::default(), &mut cx).unwrap() else {
            panic!("circle cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert!(set.meshes[0].positions.iter().all(|p| p[2].abs() < 1e-6));
    }
}
