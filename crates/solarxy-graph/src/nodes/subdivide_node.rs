//! The `subdivide` node (Phase 14): linear 1-to-4 triangle subdivision
//! with crack-free shared-edge midpoints and attribute interpolation.
//! `scheme` is an enum with one variant so Catmull-Clark slots in later
//! without a schema change; the iteration count is hard-capped and the
//! kernel's output-triangle ceiling turns runaway growth into a cook
//! error instead of a stall.

use solarxy_kernel::subdivide::subdivide_linear;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{EnumVariant, ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "subdivide",
        version: 1,
        display_name: "Subdivide",
        category: Category::Modifiers,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true).default_port(),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Subdivide",
            vec![
                ParamSpec::new(
                    "scheme",
                    "Scheme",
                    "subdivision",
                    ParamType::Enum {
                        variants: vec![EnumVariant::new("linear", "Linear")],
                    },
                    ParamValue::Enum("linear".into()),
                ),
                ParamSpec::new(
                    "iterations",
                    "Iterations",
                    "subdivision",
                    ParamType::Int,
                    ParamValue::Int(1),
                )
                .hard(1.0, 5.0)
                .soft(1.0, 3.0)
                .step(1.0),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Splits every triangle into four at its edge midpoints, \
              interpolating normals, UVs, and attributes.",
        search_aliases: &["subdivide", "smooth", "tessellate", "refine"],
        cook: cook_subdivide,
        migrate: None,
    }
}

fn cook_subdivide(
    p: &ResolvedParams,
    inputs: &Inputs,
    _cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let iterations = u32::try_from(p.i64("iterations").max(1)).unwrap_or(1);

    match subdivide_linear(input, iterations) {
        Ok(set) => Ok(CookOutcome::Done(Outputs::geometry(set))),
        Err(message) => Err(CookError::Failed { message }),
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
    fn cooks_at_defaults_and_quadruples() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let set = GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1));
        let inputs = Inputs::new(
            [(
                "geometry".to_string(),
                InputSlot::Single(Value::Geometry(Arc::new(set))),
            )]
            .into_iter()
            .collect(),
        );
        let assets = crate::assets::AssetTable::default();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(outputs) = cook_subdivide(&resolved, &inputs, &mut cx).unwrap()
        else {
            panic!("synchronous cook");
        };
        let out = outputs.get("geometry").unwrap().as_geometry().unwrap();
        assert_eq!(out.meshes[0].triangle_count(), 8);
    }
}
