//! The `transform` modifier (node catalog part II, section 13). Absolute
//! matrix-bake (the Houdini SOP model): composes M = T * R(order) * S about
//! a pivot and bakes it into point positions; normals via inverse
//! transpose. Required geometry input; missing input is a hard error.

use solarxy_kernel::transform::{bake_transform, compose_trs};

use super::common::{
    geometry_output, migrate_strip_rendering_group, params_with, rotate_order_from_key,
    rotate_order_param,
};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "transform",
        version: 2,
        display_name: "Transform",
        category: Category::Modifiers,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to transform."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Transform",
            vec![
                ParamSpec::new(
                    "translate",
                    "Translate",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Meters),
                ParamSpec::new(
                    "rotate",
                    "Rotate",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Degrees),
                rotate_order_param(),
                ParamSpec::new(
                    "scale",
                    "Scale",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([1.0; 3]),
                )
                .hard(0.0001, 10000.0)
                .soft(0.01, 100.0),
                ParamSpec::new(
                    "uniform_scale",
                    "Uniform Scale",
                    "transform",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.0001, 10000.0)
                .soft(0.01, 100.0),
                ParamSpec::new(
                    "pivot",
                    "Pivot",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Meters),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Translates, rotates, and scales the input geometry, baking \
              the transform into point positions.",
        search_aliases: &["move", "rotate", "scale", "xform"],
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

fn cook(p: &ResolvedParams, inputs: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    // The required-input guard already ran in the driver; a connected but
    // empty upstream flows here as None and yields empty (keep-last-good).
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let translate = p.vec3_f32("translate");
    let rotate = p.vec3_f32("rotate"); // already radians (Unit::Degrees)
    let order = rotate_order_from_key(p.enum_key("rotate_order"));
    let scale = p.vec3_f32("scale");
    let uniform = p.f32("uniform_scale");
    let scale = [scale[0] * uniform, scale[1] * uniform, scale[2] * uniform];
    let pivot = p.vec3_f32("pivot");

    let matrix = compose_trs(translate, rotate, order, scale, pivot);
    match bake_transform(input, &matrix) {
        Ok(set) => Ok(CookOutcome::Done(Outputs::geometry(set))),
        Err(e) => Err(CookError::Failed {
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::registry::coerce::Value;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::generate_plane;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn translates_the_input_geometry() {
        let resolved = {
            let mut stored = BTreeMap::new();
            stored.insert(
                "translate".to_string(),
                crate::params::ParamSource::Literal(ParamValue::Vec3([5.0, 0.0, 0.0])),
            );
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap()
        };
        let input = Arc::new(GeometrySet::from_mesh(generate_plane(2.0, 2.0, 1, 1)));
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(input)),
        );
        let inputs = Inputs::new(slots);

        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("transform cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        // The plane centered at origin (min x = -1) shifts to min x = 4.
        assert!((set.bounds.min.x - 4.0).abs() < 1e-5);
    }
}
