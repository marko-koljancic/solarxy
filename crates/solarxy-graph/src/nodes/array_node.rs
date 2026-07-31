//! The `array` modifier. Duplicates
//! the input `count` times (the original included), either stepping linearly by
//! an offset or revolving radially about an axis.
//!
//! Copy Mode decides what a copy is. Instance, the default, keeps the input
//! once and carries a placement matrix per copy. Bake composes the existing
//! transform bake and merge, so materials survive duplication and dedup to a
//! single entry.

use solarxy_kernel::array::{ArrayMode, Axis, array};

use super::common::{copy_mode_from_key, copy_mode_param, geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType, Pred, Unit};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "array",
        version: 2,
        display_name: "Array",
        category: Category::Copy,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to duplicate."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Array",
            vec![
                ParamSpec::new(
                    "mode",
                    "Mode",
                    "array",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("linear", "Linear"),
                            EnumVariant::new("radial", "Radial"),
                        ],
                    },
                    ParamValue::Enum("linear".into()),
                )
                .doc(
                    "How the copies are placed. Linear steps each one along a fixed \
                     offset, for fence posts and stair treads. Radial revolves them \
                     about an axis, for spokes and bolt circles, and turns each copy to \
                     follow the revolution. The mode decides which of the placement \
                     parameters below apply; the rest hide.",
                ),
                ParamSpec::new(
                    "count",
                    "Count",
                    "array",
                    ParamType::Int,
                    ParamValue::Int(3),
                )
                .hard(1.0, 512.0)
                .soft(1.0, 32.0)
                .step(1.0)
                .doc("How many copies in total, counting the original. 1 is a no-op."),
                ParamSpec::new(
                    "offset",
                    "Offset",
                    "array",
                    ParamType::Vec3,
                    ParamValue::Vec3([1.0, 0.0, 0.0]),
                )
                .unit(Unit::Meters)
                .show_if("mode", Pred::Eq(ParamValue::Enum("linear".into())))
                .doc("The step between copies: copy i is offset by i times this."),
                ParamSpec::new(
                    "axis",
                    "Axis",
                    "array",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("x", "X"),
                            EnumVariant::new("y", "Y"),
                            EnumVariant::new("z", "Z"),
                        ],
                    },
                    ParamValue::Enum("y".into()),
                )
                .show_if("mode", Pred::Eq(ParamValue::Enum("radial".into())))
                .doc("The axis the copies revolve about."),
                ParamSpec::new(
                    "radius",
                    "Radius",
                    "array",
                    ParamType::Float,
                    ParamValue::Float(0.0),
                )
                .hard(0.0, 10000.0)
                .soft(0.0, 20.0)
                .unit(Unit::Meters)
                .show_if("mode", Pred::Eq(ParamValue::Enum("radial".into())))
                .doc("How far each copy sits from the axis before it revolves."),
                ParamSpec::new(
                    "sweep",
                    "Sweep",
                    "array",
                    ParamType::Float,
                    ParamValue::Float(360.0),
                )
                .hard(-360.0, 360.0)
                .unit(Unit::Degrees)
                .show_if("mode", Pred::Eq(ParamValue::Enum("radial".into())))
                .doc(
                    "The total angle the copies span. Each copy steps by \
                     sweep/count, so a full 360 tiles evenly without doubling up \
                     at the seam.",
                ),
                copy_mode_param("array"),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Duplicates the input Count times, counting the original, either \
              stepping each copy linearly along an offset or revolving it \
              about an axis.\n\n\
              Copy Mode decides what a copy is. Instance, the default, keeps \
              the input once and carries a placement matrix per copy, so a \
              long fence costs one post. Bake makes every copy a real baked \
              transform of the input, concatenated as though you had merged \
              them yourself, with identical materials collapsing to one table \
              entry rather than one per copy.\n\n\
              It replaces the branch you would otherwise wire by hand: a \
              `transform` and a `merge` for every copy. Put it after whatever \
              makes the single unit, a primitive or a small assembly you have \
              already merged, and then change one number instead of \
              rewiring.\n\n\
              Count includes the original, so 1 is a no-op rather than one \
              extra copy. The radial step is Sweep divided by Count rather \
              than by Count minus 1, which is what lets a full 360 tile evenly \
              instead of stacking a copy on the original at the seam. And \
              Radius defaults to 0, which leaves every radial copy sitting on \
              the axis spinning in place: give it a radius to get a ring.",
        search_aliases: &["duplicate", "repeat", "clone", "radial", "grid", "instance"],
        glyph: "array",
        role: NodeRole::Standard,
        cook,
        migrate: Some(super::common::migrate_pin_copy_mode_to_bake),
    }
}

fn cook(p: &ResolvedParams, inputs: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let count = p.u32("count");
    let mode = match p.enum_key("mode") {
        "radial" => ArrayMode::Radial {
            axis: axis_from_key(p.enum_key("axis")),
            radius: p.f32("radius"),
            sweep_rad: p.f32("sweep"), // already radians (Unit::Degrees)
        },
        _ => ArrayMode::Linear {
            offset: p.vec3_f32("offset"),
        },
    };

    match array(
        input,
        count,
        mode,
        copy_mode_from_key(p.enum_key("copy_mode")),
    ) {
        Ok(set) => Ok(CookOutcome::Done(Outputs::geometry(set))),
        Err(message) => Err(CookError::Failed { message }),
    }
}

fn axis_from_key(key: &str) -> Axis {
    match key {
        "x" => Axis::X,
        "z" => Axis::Z,
        _ => Axis::Y,
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

    fn run(stored: BTreeMap<String, ParamSource>) -> Outputs {
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
            panic!("array cooks synchronously");
        };
        out
    }

    fn set_of(out: &Outputs) -> &Arc<GeometrySet> {
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        set
    }

    /// Stored params pinned to Bake. Instance became the descriptor default
    /// in 0.8.2, so a test whose subject is real concatenated geometry has to
    /// say which mode it means rather than inherit one.
    fn bake() -> BTreeMap<String, ParamSource> {
        let mut stored = BTreeMap::new();
        stored.insert(
            "copy_mode".to_string(),
            ParamSource::Literal(ParamValue::Enum("bake".into())),
        );
        stored
    }

    #[test]
    fn cooks_at_defaults() {
        let out = run(BTreeMap::new());
        let set = set_of(&out);
        // Default: linear, count 3, offset (1,0,0), Instance. One prototype
        // and three placements, not three meshes.
        assert_eq!(set.mesh_count(), 1);
        assert_eq!(set.meshes[0].instances.as_ref().map(|i| i.len()), Some(3));
    }

    #[test]
    fn bake_mode_still_concatenates_real_copies() {
        let out = run(bake());
        let set = set_of(&out);
        assert_eq!(set.mesh_count(), 3);
        assert!(
            !set.is_instanced(),
            "baked output carries no placement list; the copies ARE the geometry"
        );
    }

    /// The two modes describe the same arrangement, which is the property
    /// that makes Instance a representation choice rather than a different
    /// result. Bounds are the cheapest observable both modes share: the
    /// instanced set derives them from the placements.
    #[test]
    fn instance_and_bake_agree_on_where_the_copies_are() {
        let mut stored = bake();
        stored.insert(
            "count".to_string(),
            ParamSource::Literal(ParamValue::Int(4)),
        );
        stored.insert(
            "offset".to_string(),
            ParamSource::Literal(ParamValue::Vec3([3.0, 0.0, 0.0])),
        );
        let baked = run(stored.clone());
        stored.remove("copy_mode");
        let instanced = run(stored);
        assert_bounds_agree(set_of(&baked), set_of(&instanced));
    }

    /// `AABB` carries no `PartialEq`, and the two modes reach the same box by
    /// different arithmetic (transformed corners against transformed points),
    /// so the comparison is per-component and tolerant.
    fn assert_bounds_agree(a: &GeometrySet, b: &GeometrySet) {
        for (x, y) in [
            (a.bounds.min.x, b.bounds.min.x),
            (a.bounds.min.y, b.bounds.min.y),
            (a.bounds.min.z, b.bounds.min.z),
            (a.bounds.max.x, b.bounds.max.x),
            (a.bounds.max.y, b.bounds.max.y),
            (a.bounds.max.z, b.bounds.max.z),
        ] {
            assert!(
                (x - y).abs() < 1e-4,
                "the modes disagree on where the copies are: {x} vs {y}"
            );
        }
    }

    #[test]
    fn linear_places_copies_along_the_offset() {
        let mut stored = bake();
        stored.insert(
            "count".to_string(),
            ParamSource::Literal(ParamValue::Int(4)),
        );
        stored.insert(
            "offset".to_string(),
            ParamSource::Literal(ParamValue::Vec3([3.0, 0.0, 0.0])),
        );
        let out = run(stored);
        let set = set_of(&out);
        assert_eq!(set.mesh_count(), 4);
        // Copies at 0, 3, 6, 9; the box spans +/-0.5.
        assert!((set.bounds.max.x - 9.5).abs() < 1e-4, "{:?}", set.bounds);
    }

    #[test]
    fn radial_reads_its_own_params_and_revolves() {
        let mut stored = bake();
        stored.insert(
            "mode".to_string(),
            ParamSource::Literal(ParamValue::Enum("radial".into())),
        );
        stored.insert(
            "count".to_string(),
            ParamSource::Literal(ParamValue::Int(4)),
        );
        stored.insert(
            "radius".to_string(),
            ParamSource::Literal(ParamValue::Float(5.0)),
        );
        let out = run(stored);
        let set = set_of(&out);
        assert_eq!(set.mesh_count(), 4);
        // A full ring at radius 5 reaches out on both X and Z.
        assert!(set.bounds.max.x > 5.0 && set.bounds.min.x < -5.0);
        assert!(set.bounds.max.z > 5.0 && set.bounds.min.z < -5.0);
    }

    #[test]
    fn an_over_ceiling_count_is_a_cook_error_not_a_panic() {
        // Bake is the mode that allocates per copy, so it is the mode with a
        // count-driven ceiling at all: Instance counts the input once and
        // this same graph places happily, which is the point of the modes.
        let mut stored = bake();
        // The hard range caps count at 512; 512 copies of a 12-triangle box is
        // fine, so drive the ceiling with a dense box instead.
        stored.insert(
            "count".to_string(),
            ParamSource::Literal(ParamValue::Int(512)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_box(1.0, 1.0, 1.0, 100, 100, 100),
            )))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let err = cook(&resolved, &inputs, &mut cx).unwrap_err();
        let CookError::Failed { message } = err else {
            panic!("expected a Failed cook error");
        };
        assert!(message.contains("ceiling"), "got: {message}");
    }

    /// The same graph that cannot bake places without complaint, which is
    /// what the ceiling message tells the user to try.
    #[test]
    fn the_count_that_cannot_bake_still_instances() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "count".to_string(),
            ParamSource::Literal(ParamValue::Int(512)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_box(1.0, 1.0, 1.0, 100, 100, 100),
            )))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("array cooks synchronously");
        };
        let set = set_of(&out);
        assert_eq!(set.mesh_count(), 1);
        assert_eq!(set.meshes[0].instances.as_ref().map(|i| i.len()), Some(512));
    }

    #[test]
    fn count_is_clamped_by_the_hard_range() {
        let mut stored = BTreeMap::new();
        stored.insert(
            "count".to_string(),
            ParamSource::Literal(ParamValue::Int(0)),
        );
        let out = run(stored);
        // 0 clamps to the hard minimum of 1: an identity copy, not empty.
        assert_eq!(set_of(&out).mesh_count(), 1);
    }
}
