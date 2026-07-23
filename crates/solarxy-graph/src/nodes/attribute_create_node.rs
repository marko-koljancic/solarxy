//! The `attribute_create` modifier: writes a constant point-domain
//! attribute lane, the first user-facing door into the attribute system.

use solarxy_kernel::attribute_ops::{AttributeValue, attribute_create};

use super::common::{
    geometry_output, params_with, warn_input_lane_type_replaced, warn_reserved_lane_mismatch,
};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

fn show_for(ty: &str) -> Pred {
    Pred::Eq(ParamValue::Enum(ty.into()))
}

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "attribute_create",
        version: 1,
        display_name: "Attribute Create",
        category: Category::Attribute,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry the lane is written onto."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Attribute Create",
            vec![
                ParamSpec::new(
                    "attr_name",
                    "Name",
                    "attribute",
                    ParamType::AttributeName,
                    ParamValue::Text("value".into()),
                )
                .doc(
                    "The lane's name. Free-form names are yours to consume \
                     downstream; the reserved names carry contracts: `color` \
                     (vec4) drives vertex-color display, `N` (vec3) is the \
                     point normal copies orient to, `uv` (vec2) the texture \
                     coordinate, `pscale` (float) a per-point scale reserved \
                     for later.",
                ),
                ParamSpec::new(
                    "type",
                    "Type",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("float", "Float"),
                            EnumVariant::new("vec2", "Vec2"),
                            EnumVariant::new("vec3", "Vec3"),
                            EnumVariant::new("vec4", "Vec4"),
                        ],
                    },
                    ParamValue::Enum("float".into()),
                )
                .doc(
                    "The lane's component count. Pick the type a reserved \
                     name's consumers expect (vec4 for `color`); the value \
                     parameter below follows the choice.",
                ),
                ParamSpec::new(
                    "value_float",
                    "Value",
                    "attribute",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .show_if("type", show_for("float"))
                .doc("The constant every point receives."),
                ParamSpec::new(
                    "value_vec2",
                    "Value",
                    "attribute",
                    ParamType::Vec2,
                    ParamValue::Vec2([0.0, 0.0]),
                )
                .show_if("type", show_for("vec2"))
                .doc("The constant every point receives."),
                ParamSpec::new(
                    "value_vec3",
                    "Value",
                    "attribute",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0, 0.0, 0.0]),
                )
                .show_if("type", show_for("vec3"))
                .doc("The constant every point receives."),
                ParamSpec::new(
                    "value_vec4",
                    "Value",
                    "attribute",
                    ParamType::Vec4,
                    ParamValue::Vec4([1.0, 1.0, 1.0, 1.0]),
                )
                .show_if("type", show_for("vec4"))
                .doc(
                    "The constant every point receives. As a `color` it is \
                     linear RGBA: opaque white by default.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Writes a constant attribute lane onto every point of the input, \
              replacing any lane already under that name. Attributes are named \
              per-point values that ride the geometry through the graph; \
              downstream nodes consume them by name.\n\n\
              The reserved names are where this shows immediately: write \
              `color` as a vec4 and the geometry displays vertex-colored; \
              write `N` as a vec3 and `copy_to_points` orients to it; `uv` \
              (vec2) feeds texturing. Any other name is free-form data for \
              your own downstream use.\n\n\
              Writing a reserved name with the wrong type is legal but inert, \
              and the node warns rather than guessing. For seeded per-point \
              variation instead of a constant, reach for \
              `attribute_randomize`.",
        search_aliases: &["attribute", "lane", "constant", "color", "tag"],
        glyph: "attribute_create",
        role: NodeRole::Standard,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let name = p.text("attr_name").trim().to_string();
    if name.is_empty() {
        cx.warn("attribute_create has no attribute name; the input passes through unchanged");
        return Ok(CookOutcome::Done(Outputs::geometry((**input).clone())));
    }

    let ty = p.enum_key("type");
    let value = match ty {
        "vec2" => AttributeValue::Vec2(p.vec2("value_vec2").map(|v| v as f32)),
        "vec3" => AttributeValue::Vec3(p.vec3_f32("value_vec3")),
        "vec4" => AttributeValue::Vec4(p.vec4("value_vec4").map(|v| v as f32)),
        _ => AttributeValue::Float(p.f32("value_float")),
    };
    warn_reserved_lane_mismatch(cx, &name, ty);
    warn_input_lane_type_replaced(cx, input, &name, ty);
    Ok(CookOutcome::Done(Outputs::geometry(attribute_create(
        input, &name, value,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::primitives::generate_box;
    use solarxy_kernel::{AttributeData, GeometrySet, reserved};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(stored: BTreeMap<String, ParamSource>) -> (Outputs, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_box(1.0, 1.0, 1.0, 1, 1, 1),
            )))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("cooks synchronously");
        };
        (out, cx.take_warnings())
    }

    fn set_of(out: &Outputs) -> &Arc<GeometrySet> {
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        set
    }

    #[test]
    fn writes_the_default_float_lane() {
        let (out, warnings) = run(BTreeMap::new());
        let Some(AttributeData::Float(lane)) = set_of(&out).meshes[0].attributes.get("value")
        else {
            panic!("float lane written");
        };
        assert_eq!(lane.len(), 24);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_reserved_name_with_the_wrong_type_warns_but_writes() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            ParamSource::Literal(ParamValue::Text(reserved::COLOR.into())),
        );
        let (out, warnings) = run(stored);
        assert!(
            set_of(&out).meshes[0]
                .attributes
                .contains_key(reserved::COLOR)
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("vec4"), "{warnings:?}");
    }

    #[test]
    fn an_empty_name_passes_through_with_a_warning() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            ParamSource::Literal(ParamValue::Text("  ".into())),
        );
        let (out, warnings) = run(stored);
        assert!(set_of(&out).meshes[0].attributes.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
