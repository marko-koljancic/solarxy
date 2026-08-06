//! The `attribute_randomize` modifier: fills a point-domain lane with
//! seeded uniform values. With the default `color` name it drives the
//! vertex-color display directly, which makes the attribute system
//! visible in a single node.

use solarxy_kernel::attribute_ops::{RandomRange, attribute_randomize};

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
        type_id: "attribute_randomize",
        version: 1,
        display_name: "Attribute Randomize",
        category: Category::Attribute,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry whose points receive the randomized lane."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Attribute Randomize",
            vec![
                ParamSpec::new(
                    "attr_name",
                    "Name",
                    "attribute",
                    ParamType::AttributeName,
                    ParamValue::Text("color".into()),
                )
                .doc(
                    "The lane to fill. The default `color` drives vertex-color \
                     display immediately; `pscale` (float) and free-form names \
                     feed downstream consumers instead.",
                ),
                ParamSpec::new(
                    "type",
                    "Type",
                    "attribute",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("float", "Float"),
                            EnumVariant::new("vec3", "Vec3"),
                            EnumVariant::new("vec4", "Vec4"),
                        ],
                    },
                    ParamValue::Enum("vec4".into()),
                )
                .doc(
                    "The lane's component count; each component draws \
                     independently between its Min and Max. `color` consumers \
                     expect vec4.",
                ),
                ParamSpec::new(
                    "min_float",
                    "Min",
                    "attribute",
                    ParamType::Float,
                    ParamValue::Float(0.0),
                )
                .show_if("type", show_for("float"))
                .doc("The lower bound of the uniform draw."),
                ParamSpec::new(
                    "max_float",
                    "Max",
                    "attribute",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .show_if("type", show_for("float"))
                .doc("The upper bound of the uniform draw."),
                ParamSpec::new(
                    "min_vec3",
                    "Min",
                    "attribute",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0, 0.0, 0.0]),
                )
                .show_if("type", show_for("vec3"))
                .doc("The per-component lower bounds of the uniform draw."),
                ParamSpec::new(
                    "max_vec3",
                    "Max",
                    "attribute",
                    ParamType::Vec3,
                    ParamValue::Vec3([1.0, 1.0, 1.0]),
                )
                .show_if("type", show_for("vec3"))
                .doc("The per-component upper bounds of the uniform draw."),
                ParamSpec::new(
                    "min_vec4",
                    "Min",
                    "attribute",
                    ParamType::Vec4,
                    ParamValue::Vec4([0.0, 0.0, 0.0, 1.0]),
                )
                .show_if("type", show_for("vec4"))
                .doc(
                    "The per-component lower bounds of the uniform draw. The \
                     default pins alpha at 1 so randomized colors stay opaque.",
                ),
                ParamSpec::new(
                    "max_vec4",
                    "Max",
                    "attribute",
                    ParamType::Vec4,
                    ParamValue::Vec4([1.0, 1.0, 1.0, 1.0]),
                )
                .show_if("type", show_for("vec4"))
                .doc("The per-component upper bounds of the uniform draw."),
                ParamSpec::new(
                    "seed",
                    "Seed",
                    "attribute",
                    ParamType::Int,
                    ParamValue::Int(0),
                )
                .hard(0.0, 2_147_483_647.0)
                .soft(0.0, 9999.0)
                .step(1.0)
                .doc(
                    "Selects which random values you get. Any change redraws \
                     every point; the same seed always cooks the same values, \
                     so a saved scene reproduces exactly.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Fills an attribute lane with seeded uniform random values, one \
              draw per point, each component between its Min and Max. At the \
              defaults it writes `color`, so wiring any geometry through it \
              paints every point a different color and the result displays \
              immediately: the quickest proof the attribute system is live.\n\n\
              On `scatter` output it is the variation workhorse: randomize \
              `color` for per-point tinting, or a free-form lane that a later \
              release's consumers read. The draw is per point, deterministic \
              in the seed, and independent per component, so a fixed alpha is \
              just Min equal to Max in that lane.\n\n\
              Writing a reserved name with the wrong type is legal but inert \
              (the node warns): `color` wants vec4, `N` vec3, `uv` vec2, \
              `pscale` float. It replaces any existing lane under the same \
              name.",
        search_aliases: &[
            "random",
            "variation",
            "jitter",
            "noise",
            "color",
            "attribute",
        ],
        glyph: "attribute_randomize",
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
    let input = &super::common::baked_input(input, cx)?;
    let name = p.text("attr_name").trim().to_string();
    if name.is_empty() {
        cx.warn("attribute_randomize has no attribute name; the input passes through unchanged");
        return Ok(CookOutcome::Done(Outputs::geometry((**input).clone())));
    }

    let ty = p.enum_key("type");
    let range = match ty {
        "float" => RandomRange::Float {
            min: p.f32("min_float"),
            max: p.f32("max_float"),
        },
        "vec3" => RandomRange::Vec3 {
            min: p.vec3_f32("min_vec3"),
            max: p.vec3_f32("max_vec3"),
        },
        _ => RandomRange::Vec4 {
            min: p.vec4("min_vec4").map(|v| v as f32),
            max: p.vec4("max_vec4").map(|v| v as f32),
        },
    };
    warn_reserved_lane_mismatch(cx, &name, ty);
    warn_input_lane_type_replaced(cx, input, &name, ty);
    Ok(CookOutcome::Done(Outputs::geometry(attribute_randomize(
        input,
        &name,
        range,
        p.u32("seed"),
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::primitives::generate_box;
    use solarxy_kernel::{GeometrySet, reserved};
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
    fn the_defaults_drive_vertex_color_display() {
        // The acceptance line for the node: at defaults, the written lane
        // crosses to the renderer contract as per-point colors.
        let (out, warnings) = run(BTreeMap::new());
        let cooked = set_of(&out).to_cooked();
        let colors = cooked.meshes[0]
            .colors
            .as_ref()
            .expect("the default color lane reaches CookedMesh.colors");
        assert_eq!(colors.len(), 24);
        assert!(
            colors.iter().any(|c| c[0..3] != colors[0][0..3]),
            "points draw distinct colors"
        );
        assert!(colors.iter().all(|c| (c[3] - 1.0).abs() < 1e-6));
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_reserved_name_with_the_wrong_type_warns() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            ParamSource::Literal(ParamValue::Text(reserved::UV.into())),
        );
        let (_, warnings) = run(stored);
        // Two distinct facts, both said: the reserved contract (uv wants
        // vec2) and the replacement of the box's fixed uv buffer.
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("vec2"), "{warnings:?}");
        assert!(warnings[1].contains("replaces"), "{warnings:?}");
    }

    fn run_with_input(
        stored: BTreeMap<String, ParamSource>,
        input: GeometrySet,
    ) -> (Outputs, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(input))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("cooks synchronously");
        };
        (out, cx.take_warnings())
    }

    /// The reporter's scene: a free-form vec3 lane on the input, the
    /// node's vec4 default silently retyping it. Not silent anymore.
    #[test]
    fn replacing_an_input_lane_of_a_different_type_warns() {
        let mut mesh =
            solarxy_kernel::KernelMesh::points("pts", vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        mesh.attributes.insert(
            "velocity".into(),
            solarxy_kernel::AttributeData::Vec3(Arc::new(vec![[0.0; 3]; 2])),
        );
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            ParamSource::Literal(ParamValue::Text("velocity".into())),
        );
        let (_, warnings) = run_with_input(stored, GeometrySet::from_mesh(mesh));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("vec3") && warnings[0].contains("vec4"),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_same_type_replacement_stays_silent() {
        let mut mesh = solarxy_kernel::KernelMesh::points("pts", vec![[0.0, 0.0, 0.0]]);
        mesh.attributes.insert(
            "velocity".into(),
            solarxy_kernel::AttributeData::Vec4(Arc::new(vec![[0.0; 4]; 1])),
        );
        let mut stored = BTreeMap::new();
        stored.insert(
            "attr_name".to_string(),
            ParamSource::Literal(ParamValue::Text("velocity".into())),
        );
        let (_, warnings) = run_with_input(stored, GeometrySet::from_mesh(mesh));
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
