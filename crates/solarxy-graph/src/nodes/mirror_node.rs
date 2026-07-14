//! The `mirror` modifier (node catalog part II, Tier-2, Phase 15). Reflects
//! the input across an axis-aligned plane, optionally keeping the original.
//! The kernel op flips winding after the reflection (a negative determinant
//! reverses triangle orientation) while leaving normals alone, because the
//! bake's inverse-transpose already reflects them correctly.

use solarxy_kernel::mirror::{Axis, mirror};

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "mirror",
        version: 1,
        display_name: "Mirror",
        category: Category::Modifiers,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to reflect."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Mirror",
            vec![
                ParamSpec::new(
                    "axis",
                    "Axis",
                    "mirror",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("x", "X"),
                            EnumVariant::new("y", "Y"),
                            EnumVariant::new("z", "Z"),
                        ],
                    },
                    ParamValue::Enum("x".into()),
                )
                .doc("The axis the mirror plane is perpendicular to."),
                ParamSpec::new(
                    "offset",
                    "Offset",
                    "mirror",
                    ParamType::Float,
                    ParamValue::Float(0.0),
                )
                .hard(-10000.0, 10000.0)
                .soft(-10.0, 10.0)
                .unit(Unit::Meters)
                .doc("Where the mirror plane sits along the axis."),
                ParamSpec::new(
                    "keep_original",
                    "Keep Original",
                    "mirror",
                    ParamType::Bool,
                    ParamValue::Bool(true),
                )
                .doc("Merge the reflection with the original instead of replacing it."),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Reflects the input across an axis-aligned plane, flipping the \
              winding so the reflected surface faces outward. Keep Original \
              merges both halves, which is the usual way to build a symmetric \
              model from one side.",
        search_aliases: &["reflect", "symmetry", "flip"],
        cook,
        migrate: None,
    }
}

fn cook(p: &ResolvedParams, inputs: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let axis = match p.enum_key("axis") {
        "y" => Axis::Y,
        "z" => Axis::Z,
        _ => Axis::X,
    };

    match mirror(input, axis, p.f32("offset"), p.bool("keep_original")) {
        Ok(set) => Ok(CookOutcome::Done(Outputs::geometry(set))),
        Err(message) => Err(CookError::Failed { message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::generate_box;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(stored: BTreeMap<String, ParamSource>) -> Arc<GeometrySet> {
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
            panic!("mirror cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        Arc::clone(set)
    }

    #[test]
    fn cooks_at_defaults_and_keeps_the_original() {
        let set = run(BTreeMap::new());
        // Default: X axis, offset 0, keep_original true. The box is symmetric
        // about x = 0, so both halves overlap but there are still two meshes.
        assert_eq!(set.mesh_count(), 2);
    }

    #[test]
    fn keep_original_off_replaces_the_input() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "keep_original".to_string(),
            ParamSource::Literal(ParamValue::Bool(false)),
        );
        let set = run(stored);
        assert_eq!(set.mesh_count(), 1);
    }

    #[test]
    fn the_offset_places_the_mirror_plane() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "offset".to_string(),
            ParamSource::Literal(ParamValue::Float(4.0)),
        );
        stored.insert(
            "keep_original".to_string(),
            ParamSource::Literal(ParamValue::Bool(false)),
        );
        let set = run(stored);
        // Box at -0.5..0.5 reflected across x = 4 lands at 7.5..8.5.
        assert!((set.bounds.min.x - 7.5).abs() < 1e-4, "{:?}", set.bounds);
        assert!((set.bounds.max.x - 8.5).abs() < 1e-4, "{:?}", set.bounds);
    }

    #[test]
    fn the_axis_param_selects_the_plane() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "axis".to_string(),
            ParamSource::Literal(ParamValue::Enum("y".into())),
        );
        stored.insert(
            "offset".to_string(),
            ParamSource::Literal(ParamValue::Float(3.0)),
        );
        stored.insert(
            "keep_original".to_string(),
            ParamSource::Literal(ParamValue::Bool(false)),
        );
        let set = run(stored);
        assert!((set.bounds.min.y - 5.5).abs() < 1e-4, "{:?}", set.bounds);
        assert!(set.bounds.min.x < 0.0, "X is untouched");
    }
}
