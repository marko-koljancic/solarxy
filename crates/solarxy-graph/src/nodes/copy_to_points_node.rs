//! The `copy_to_points` modifier. Stamps a template onto every point of a
//! points input (scatter output being the canonical source), optionally
//! orienting each copy to the point's normal and varying its size with a
//! seeded jitter.
//!
//! Copy Mode decides what a copy is. Instance, the default, keeps the
//! template once and carries a transform per point. Bake flattens the copies
//! of each template mesh into one concatenated mesh, so even there a
//! ten-thousand-point copy stays a handful of draw objects rather than ten
//! thousand.

use solarxy_kernel::copy::{CopyOrient, copy_to_points};

use super::common::{copy_mode_from_key, copy_mode_param, geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "copy_to_points",
        version: 2,
        display_name: "Copy to Points",
        category: Category::Copy,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("template", "Template", DataType::Geometry, true).doc(
                "The geometry stamped at every point: a primitive or any \
                 small assembly you have already merged.",
            ),
            PortSpec::single("points", "Points", DataType::Geometry, true)
                .default_port()
                .doc(
                    "Where the copies land: every vertex of every input mesh is \
                     a target, whatever its topology. Scatter output is the \
                     canonical source.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Copy to Points",
            vec![
                ParamSpec::new(
                    "orient",
                    "Orient",
                    "copy",
                    ParamType::Enum {
                        variants: vec![
                            EnumVariant::new("none", "None"),
                            EnumVariant::new("normal", "Normal"),
                        ],
                    },
                    ParamValue::Enum("normal".into()),
                )
                .doc(
                    "How each copy turns at its point. Normal rotates the \
                     template's up axis onto the point's normal, so copies \
                     stand on the surface they were scattered over; points \
                     without a normal keep the template orientation. None \
                     keeps every copy axis-aligned.",
                ),
                ParamSpec::new(
                    "scale",
                    "Scale",
                    "copy",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.001, 1000.0)
                .soft(0.01, 10.0)
                .doc(
                    "A uniform size factor applied to every copy before it \
                     lands.\n\n\
                     If the points carry a `pscale` float attribute, each \
                     copy multiplies this by its own point's value, so this \
                     stays the global dial while the attribute varies around \
                     it. Author one with `attribute_wrangle`: \
                     `@pscale = fit(rand(@ptnum), 0, 1, 0.4, 1.6);` gives a \
                     scatter of mixed sizes. Points without the lane copy at \
                     this size exactly.",
                ),
                ParamSpec::new(
                    "scale_variance",
                    "Scale Variance",
                    "copy",
                    ParamType::Float,
                    ParamValue::Float(0.0),
                )
                .hard(0.0, 0.95)
                .soft(0.0, 0.5)
                .doc(
                    "Per-copy size jitter as a fraction of Scale: 0.2 lets each \
                     copy vary twenty percent bigger or smaller, seeded so the \
                     variation reproduces exactly. 0 keeps every copy the same \
                     size.",
                ),
                ParamSpec::new("seed", "Seed", "copy", ParamType::Int, ParamValue::Int(0))
                    .hard(0.0, 2_147_483_647.0)
                    .soft(0.0, 9999.0)
                    .step(1.0)
                    .doc(
                        "Selects which per-copy size jitter you get when Scale \
                     Variance is above zero. The same seed always cooks the \
                     same sizes.",
                    ),
                copy_mode_param("copy"),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "points".to_string(),
        },
        doc: "Stamps the Template input onto every point of the Points input: \
              scatter a surface, wire the cloud in here, and a forest, a \
              crowd, or a debris field is one node instead of a hand-wired \
              branch per copy. Every vertex of the points input is a target \
              whatever its topology, so mesh vertices work as well as \
              scattered clouds.\n\n\
              Orient turns each copy's up axis onto its point's normal (what \
              scatter writes), so copies stand on the surface rather than all \
              facing the same way; Scale sizes every copy and Scale Variance \
              adds seeded per-copy jitter for a natural, unrepeated look.\n\n\
              Copy Mode decides what a copy is. Instance, the default, keeps \
              the template once and carries a transform per point, so ten \
              thousand cones cost one cone. Bake makes every copy real \
              geometry you can edit downstream, flattening the copies of each \
              template mesh into one concatenated mesh so even thousands of \
              them stay a handful of draw objects. Either way the template's \
              materials ride along shared rather than duplicated, and a copy \
              count whose output would exceed the eight-million primitive \
              ceiling fails the cook before anything is allocated, with a \
              message naming the running mode and the way out.",
        search_aliases: &[
            "instance",
            "stamp",
            "duplicate",
            "clone",
            "template",
            "forest",
        ],
        glyph: "copy_to_points",
        role: NodeRole::Standard,
        cook,
        migrate: Some(super::common::migrate_pin_copy_mode_to_bake),
    }
}

fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let (Some(template), Some(points)) = (inputs.geometry("template"), inputs.geometry("points"))
    else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };

    let orient = match p.enum_key("orient") {
        "none" => CopyOrient::None,
        _ => CopyOrient::Normal,
    };
    match copy_to_points(
        template,
        points,
        orient,
        p.f32("scale"),
        p.f32("scale_variance"),
        p.u32("seed"),
        copy_mode_from_key(p.enum_key("copy_mode")),
    ) {
        Ok(set) => {
            if set.is_renderable_empty() {
                cx.warn("copy_to_points has no points to copy onto; output is empty");
            }
            Ok(CookOutcome::Done(Outputs::geometry(set)))
        }
        Err(message) => Err(CookError::Failed { message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::params::ParamSource;
    use crate::registry::coerce::Value;
    use solarxy_kernel::primitives::generate_box;
    use solarxy_kernel::{GeometrySet, KernelMesh};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn run(
        stored: BTreeMap<String, ParamSource>,
        template: GeometrySet,
        points: GeometrySet,
    ) -> (Result<CookOutcome, CookError>, Vec<String>) {
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            "template".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(template))),
        );
        slots.insert(
            "points".to_string(),
            InputSlot::Single(Value::Geometry(Arc::new(points))),
        );
        let inputs = Inputs::new(slots);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let result = cook(&resolved, &inputs, &mut cx);
        (result, cx.take_warnings())
    }

    fn set_of(out: &Outputs) -> &Arc<GeometrySet> {
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        set
    }

    /// Stored params pinned to Bake. Instance became the descriptor default
    /// in 0.8.2, so a test whose subject is real stamped geometry has to say
    /// which mode it means rather than inherit one.
    fn bake() -> BTreeMap<String, ParamSource> {
        let mut stored = BTreeMap::new();
        stored.insert(
            "copy_mode".to_string(),
            ParamSource::Literal(ParamValue::Enum("bake".into())),
        );
        stored
    }

    fn three_points() -> GeometrySet {
        GeometrySet::from_mesh(KernelMesh::points(
            "p",
            vec![[0.0; 3], [3.0, 0.0, 0.0], [0.0, 0.0, 3.0]],
        ))
    }

    #[test]
    fn cooks_copies_at_defaults() {
        let template = GeometrySet::from_mesh(generate_box(0.2, 0.2, 0.2, 1, 1, 1));
        let (result, warnings) = run(BTreeMap::new(), template, three_points());
        let CookOutcome::Done(out) = result.unwrap() else {
            panic!("cooks synchronously");
        };
        let set = set_of(&out);
        // The default is Instance: the template travels once, with a
        // placement per point. No vertex is allocated per copy at all.
        assert_eq!(set.meshes[0].primitive_count(), 12);
        assert_eq!(set.meshes[0].instances.as_ref().map(|i| i.len()), Some(3));
        assert!(warnings.is_empty());
    }

    #[test]
    fn bake_mode_still_stamps_real_geometry() {
        let template = GeometrySet::from_mesh(generate_box(0.2, 0.2, 0.2, 1, 1, 1));
        let (result, warnings) = run(bake(), template, three_points());
        let CookOutcome::Done(out) = result.unwrap() else {
            panic!("cooks synchronously");
        };
        let set = set_of(&out);
        assert_eq!(set.meshes[0].primitive_count(), 12 * 3);
        assert!(
            !set.is_instanced(),
            "baked output carries no placement list; the copies ARE the geometry"
        );
        assert!(warnings.is_empty());
    }

    /// The two modes describe the same arrangement, which is what makes
    /// Instance a representation choice rather than a different result.
    #[test]
    fn instance_and_bake_agree_on_where_the_copies_are() {
        let template = GeometrySet::from_mesh(generate_box(0.2, 0.2, 0.2, 1, 1, 1));
        let (baked, _) = run(bake(), template.clone(), three_points());
        let (instanced, _) = run(BTreeMap::new(), template, three_points());
        let (CookOutcome::Done(baked), CookOutcome::Done(instanced)) =
            (baked.unwrap(), instanced.unwrap())
        else {
            panic!("cooks synchronously");
        };
        // `AABB` carries no `PartialEq`, and the two modes reach the same box
        // by different arithmetic (transformed corners against transformed
        // points), so the comparison is per-component and tolerant.
        let (a, b) = (set_of(&baked).bounds, set_of(&instanced).bounds);
        for (x, y) in [
            (a.min.x, b.min.x),
            (a.min.y, b.min.y),
            (a.min.z, b.min.z),
            (a.max.x, b.max.x),
            (a.max.y, b.max.y),
            (a.max.z, b.max.z),
        ] {
            assert!(
                (x - y).abs() < 1e-4,
                "the modes disagree on where the copies are: {x} vs {y}"
            );
        }
    }

    #[test]
    fn an_empty_points_input_warns_and_outputs_empty() {
        let template = GeometrySet::from_mesh(generate_box(0.2, 0.2, 0.2, 1, 1, 1));
        let (result, warnings) = run(BTreeMap::new(), template, GeometrySet::empty());
        let CookOutcome::Done(out) = result.unwrap() else {
            panic!("cooks synchronously");
        };
        assert!(set_of(&out).is_renderable_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no points"), "{warnings:?}");
    }

    #[test]
    fn an_over_ceiling_projection_is_a_cook_error() {
        // Bake is the mode that allocates per point, so it is the mode with a
        // point-count-driven ceiling at all.
        let template = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let cloud = GeometrySet::from_mesh(KernelMesh::points("big", vec![[0.0; 3]; 1_000_000]));
        let (result, _) = run(bake(), template, cloud);
        let CookError::Failed { message } = result.unwrap_err() else {
            panic!("expected a Failed cook error");
        };
        assert!(message.contains("ceiling"), "got: {message}");
    }

    /// The same scatter that cannot bake places without complaint, which is
    /// what the ceiling message tells the user to try.
    #[test]
    fn the_scatter_that_cannot_bake_still_instances() {
        let template = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let cloud = GeometrySet::from_mesh(KernelMesh::points("big", vec![[0.0; 3]; 1_000_000]));
        let (result, _) = run(BTreeMap::new(), template, cloud);
        let CookOutcome::Done(out) = result.unwrap() else {
            panic!("cooks synchronously");
        };
        let set = set_of(&out);
        assert_eq!(set.meshes[0].primitive_count(), 12);
        assert_eq!(
            set.meshes[0].instances.as_ref().map(|i| i.len()),
            Some(1_000_000)
        );
    }
}
