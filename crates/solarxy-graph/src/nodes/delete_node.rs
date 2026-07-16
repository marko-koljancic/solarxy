//! The `delete` modifier (node catalog part II, Tier-2, Phase 15). Removes
//! triangles by region (centroid inside a box) or by facing (face normal within
//! an angle of a direction), with an invert. The debugging knife.
//!
//! The kernel has no per-face attributes, groups, or primitive ids, so the
//! selection model is defined by this node: see `solarxy_kernel::delete`.

use solarxy_kernel::delete::{DeleteMode, delete};

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "delete",
        version: 1,
        display_name: "Delete",
        category: Category::Modifiers,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to cull triangles from."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Delete",
            vec![
                ParamSpec::new(
                    "mode",
                    "Mode",
                    "delete",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("bbox", "Bounding Box"),
                            EnumVariant::new("normal", "Normal Direction"),
                        ],
                    },
                    ParamValue::Enum("bbox".into()),
                )
                .doc("Select triangles by where they are, or by which way they face."),
                ParamSpec::new(
                    "invert",
                    "Invert",
                    "delete",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                )
                .doc("Delete everything the selection does NOT cover."),
                ParamSpec::new(
                    "center",
                    "Center",
                    "delete",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0; 3]),
                )
                .unit(Unit::Meters)
                .show_if("mode", Pred::Eq(ParamValue::Enum("bbox".into())))
                .doc("The center of the region box."),
                ParamSpec::new(
                    "size",
                    "Size",
                    "delete",
                    ParamType::Vec3,
                    ParamValue::Vec3([1.0; 3]),
                )
                .unit(Unit::Meters)
                .show_if("mode", Pred::Eq(ParamValue::Enum("bbox".into())))
                .doc("The size of the region box. A triangle goes when its centroid is inside."),
                ParamSpec::new(
                    "direction",
                    "Direction",
                    "delete",
                    ParamType::Vec3,
                    ParamValue::Vec3([0.0, 1.0, 0.0]),
                )
                .show_if("mode", Pred::Eq(ParamValue::Enum("normal".into())))
                .doc("The facing to match against."),
                ParamSpec::new(
                    "angle",
                    "Angle",
                    "delete",
                    ParamType::Float,
                    ParamValue::Float(45.0),
                )
                .hard(0.0, 180.0)
                .unit(Unit::Degrees)
                .show_if("mode", Pred::Eq(ParamValue::Enum("normal".into())))
                .doc("How far off the direction a face can point and still be selected."),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Removes triangles by region or by facing. Vertices left orphaned \
              by the removal are compacted away. Deleting everything is allowed \
              and produces empty geometry, not an error.",
        search_aliases: &["remove", "cull", "erase", "filter"],
        glyph: "delete",
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

    let mode = match p.enum_key("mode") {
        "normal" => DeleteMode::Normal {
            direction: p.vec3_f32("direction"),
            angle_rad: p.f32("angle"), // already radians (Unit::Degrees)
        },
        _ => DeleteMode::Bbox {
            center: p.vec3_f32("center"),
            size: p.vec3_f32("size"),
        },
    };

    let result = delete(input, mode, p.bool("invert"));
    if result.degenerate_direction {
        cx.warn("delete direction is zero-length; nothing was selected");
    }
    if result.set.is_renderable_empty() && !input.is_renderable_empty() {
        cx.warn("delete removed all geometry");
    }
    Ok(CookOutcome::Done(Outputs::geometry(result.set)))
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

    fn run(stored: BTreeMap<String, ParamSource>) -> (Arc<GeometrySet>, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_box(2.0, 2.0, 2.0, 1, 1, 1),
            )))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("delete cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        (Arc::clone(set), cx.take_warnings())
    }

    fn enum_of(v: &str) -> ParamSource {
        ParamSource::Literal(ParamValue::Enum(v.into()))
    }

    #[test]
    fn cooks_at_defaults() {
        // Default bbox is a 1x1x1 region at the origin. The box's faces have
        // centroids at +/-1 on one axis, so none is inside: nothing deleted.
        let (set, warns) = run(BTreeMap::new());
        assert_eq!(set.triangle_count(), 12);
        assert!(warns.is_empty());
    }

    #[test]
    fn normal_mode_culls_the_faces_pointing_that_way() {
        let mut stored = BTreeMap::new();
        stored.insert("mode".to_string(), enum_of("normal"));
        let (set, _) = run(stored);
        // Default direction +Y, angle 45: only the top face qualifies.
        assert_eq!(set.triangle_count(), 10, "the +Y face's 2 triangles went");
    }

    #[test]
    fn invert_flips_which_side_is_deleted() {
        let mut stored = BTreeMap::new();
        stored.insert("mode".to_string(), enum_of("normal"));
        stored.insert(
            "invert".to_string(),
            ParamSource::Literal(ParamValue::Bool(true)),
        );
        let (set, _) = run(stored);
        assert_eq!(set.triangle_count(), 2, "only the +Y face survives");
    }

    #[test]
    fn deleting_everything_warns_and_yields_empty() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "size".to_string(),
            ParamSource::Literal(ParamValue::Vec3([100.0; 3])),
        );
        let (set, warns) = run(stored);
        assert!(set.is_renderable_empty());
        assert!(
            warns.iter().any(|w| w.contains("removed all geometry")),
            "got: {warns:?}"
        );
    }

    #[test]
    fn a_zero_direction_warns_and_deletes_nothing() {
        let mut stored = BTreeMap::new();
        stored.insert("mode".to_string(), enum_of("normal"));
        stored.insert(
            "direction".to_string(),
            ParamSource::Literal(ParamValue::Vec3([0.0; 3])),
        );
        let (set, warns) = run(stored);
        assert_eq!(set.triangle_count(), 12);
        assert!(
            warns.iter().any(|w| w.contains("zero-length")),
            "got: {warns:?}"
        );
    }
}
