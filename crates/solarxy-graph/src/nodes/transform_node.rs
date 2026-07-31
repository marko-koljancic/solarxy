//! The `transform` modifier. Absolute
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
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "transform",
        version: 2,
        display_name: "Transform",
        category: Category::Transform,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc(
                    "The geometry to transform. Required: leaving it unwired is a cook \
                     error rather than an empty result, because disconnecting a wire is \
                     something you meant to do.",
                ),
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
                .unit(Unit::Meters)
                .doc(
                    "How far to move the geometry along each axis, in metres. It is \
                     applied after the rotation and the scale, and it is the one part \
                     of the transform the pivot has no say in.",
                ),
                ParamSpec::new(
                    "rotate",
                    "Rotate",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Degrees)
                .doc(
                    "Euler angles in degrees about each axis. The rotation happens about \
                     the pivot, and Rotate Order decides how the three angles combine.",
                ),
                rotate_order_param().doc(
                    "Which order the three Euler angles compose in. The name reads left to \
                     right as the matrix product, so XYZ is Rx * Ry * Rz and the Z angle is \
                     the one that turns the geometry first. It only changes anything when \
                     two or more of the angles are nonzero.",
                ),
                ParamSpec::new(
                    "scale",
                    "Scale",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([1.0; 3]),
                )
                .hard(0.0001, 10000.0)
                .soft(0.01, 100.0)
                .doc(
                    "Per-axis scale factor, applied about the pivot. 1 leaves an axis \
                     alone, below 1 shrinks it. Squashing one axis is safe: normals go \
                     through the inverse transpose, so they stay perpendicular to the \
                     surface instead of shearing off it.",
                ),
                ParamSpec::new(
                    "uniform_scale",
                    "Uniform Scale",
                    "transform",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.0001, 10000.0)
                .soft(0.01, 100.0)
                .doc(
                    "A single factor multiplied into all three Scale lanes, so the two \
                     compound rather than override: Scale (2, 1, 1) at Uniform Scale 3 \
                     gives (6, 3, 3). Resize with this and keep Scale for proportions.",
                ),
                ParamSpec::new(
                    "pivot",
                    "Pivot",
                    "transform",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Meters)
                .doc(
                    "The point the rotation and the scale act about, in the input's own \
                     space. The pivot itself does not move under them, so scaling about a \
                     corner pins that corner and grows everything away from it. The \
                     default (0, 0, 0) is the world origin, which is only the object's \
                     centre if the object happens to be sitting there.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Moves, rotates, and scales the input, baking the result straight \
              into the point positions. The composition is fixed: scale, then \
              rotation about the pivot, then translation, with normals carried \
              through the inverse transpose so they survive a non-uniform \
              scale.\n\n\
              This is the placement node. It sits between a primitive or an \
              import and a `merge`, and it is what turns three boxes into a \
              blockout instead of a pile at the origin. Nothing is stored as a \
              separate object transform, so chaining two transforms simply \
              composes them and every downstream node reads geometry that has \
              already moved.\n\n\
              Rotate Order and Pivot are the two that catch people out. The \
              angles compose in the order named -- XYZ means Rx * Ry * Rz, so \
              the Z angle turns the geometry first -- and everything except \
              Translate happens about the pivot, which defaults to the world \
              origin rather than to the object's centre.",
        search_aliases: &["move", "rotate", "scale", "xform"],
        glyph: "transform",
        role: NodeRole::Standard,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    // The required-input guard already ran in the driver; a connected but
    // empty upstream flows here as None and yields empty (keep-last-good).
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let input = &super::common::baked_input(input, cx)?;

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
