//! The `copy_to_points` modifier. Stamps a template onto every point of a
//! points input (scatter output being the canonical source), optionally
//! orienting each copy to the point's normal and varying its size with a
//! seeded jitter. The copies of each template mesh flatten into one
//! concatenated mesh, so a ten-thousand-point copy stays a handful of draw
//! objects rather than ten thousand.

use solarxy_kernel::copy::{CopyOrient, copy_to_points};

use super::common::{geometry_output, params_with};
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
        version: 1,
        display_name: "Copy to Points",
        category: Category::Modifiers,
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
                .doc("A uniform size factor applied to every copy before it lands."),
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
              The copies of each template mesh flatten into one concatenated \
              mesh, keeping the viewport responsive at thousands of copies, \
              and the template's materials ride along shared, not duplicated. \
              A copy count whose output would exceed the eight-million \
              primitive ceiling fails the cook with a clear message before \
              anything is allocated.",
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
        migrate: None,
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

    #[test]
    fn cooks_copies_at_defaults() {
        let template = GeometrySet::from_mesh(generate_box(0.2, 0.2, 0.2, 1, 1, 1));
        let points = GeometrySet::from_mesh(KernelMesh::points(
            "p",
            vec![[0.0; 3], [3.0, 0.0, 0.0], [0.0, 0.0, 3.0]],
        ));
        let (result, warnings) = run(BTreeMap::new(), template, points);
        let CookOutcome::Done(out) = result.unwrap() else {
            panic!("cooks synchronously");
        };
        assert_eq!(set_of(&out).meshes[0].primitive_count(), 12 * 3);
        assert!(warnings.is_empty());
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
        let template = GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1));
        let cloud = GeometrySet::from_mesh(KernelMesh::points("big", vec![[0.0; 3]; 1_000_000]));
        let (result, _) = run(BTreeMap::new(), template, cloud);
        let CookError::Failed { message } = result.unwrap_err() else {
            panic!("expected a Failed cook error");
        };
        assert!(message.contains("ceiling"), "got: {message}");
    }
}
