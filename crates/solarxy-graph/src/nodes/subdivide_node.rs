//! The `subdivide` node: linear 1-to-4 triangle subdivision
//! with crack-free shared-edge midpoints and attribute interpolation. The
//! iteration count is hard-capped and the kernel's output-triangle ceiling
//! turns runaway growth into a cook error instead of a stall.
//!
//! v2 dropped `scheme` (param audit). It was an enum with a single
//! variant, read by nothing, so the UI showed a dropdown with one option that
//! did nothing -- exactly the dead-param class the audit removed. It was
//! originally kept to reserve the key for Catmull-Clark, but a param that never
//! had an effect must not freeze into schema v1; when a second scheme lands it
//! comes back as an ordinary default-filling addition.

use solarxy_kernel::subdivide::subdivide_linear;

use super::common::{geometry_output, params_with, strip_keys};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{
    BypassBehavior, Category, ContextSet, MigrateError, NodeRole, NodeTypeDescriptor, PortSpec,
};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "subdivide",
        version: 2,
        display_name: "Subdivide",
        category: Category::Topology,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .carries_placements()
                .doc(
                    "The geometry to refine. Every mesh in the set is subdivided, and it \
                     is the triangle count of the whole set that the output ceiling is \
                     measured against.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Subdivide",
            vec![
                ParamSpec::new(
                    "iterations",
                    "Iterations",
                    "subdivision",
                    ParamType::Int,
                    ParamValue::Int(1),
                )
                .hard(1.0, 5.0)
                .soft(1.0, 3.0)
                .step(1.0)
                .doc(
                    "How many subdivision passes to run. Each pass splits every triangle \
                     into four, so the multiplier is 4 to this power: 2 is 16 times the \
                     input, 3 is 64, 5 is over a thousand. The cook fails rather than \
                     stalls once the result would pass 8 million triangles, which is \
                     what the ceiling of 5 exists to keep you away from.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Splits every triangle into four at its edge midpoints, one pass \
              per iteration, interpolating normals, UVs, and any named \
              attributes onto the new points. Neighbouring triangles look \
              their shared edge's midpoint up in an edge map instead of each \
              making its own, so both sides land on the same point and the \
              surface stays crack-free.\n\n\
              Reach for it to buy resolution for something downstream that \
              needs points to work with: a displacement, a noise, a deform. It \
              sits between the source geometry and that node, and it is the \
              answer when the source is an import rather than a primitive \
              whose segment counts you could have raised instead.\n\n\
              The subdivision is linear: it adds points onto the existing \
              surface without moving any of them, so a subdivided box is still \
              a box with the same silhouette and four times the triangles. \
              Nothing here smooths. And the growth compounds hard -- at 5 \
              iterations a 10,000-triangle mesh projects to over 10 million, \
              past the kernel's 8 million ceiling, which is a cook error \
              rather than a stall. Point clouds and polylines have no \
              triangles to split and pass through untouched with a warning.",
        search_aliases: &["subdivide", "smooth", "tessellate", "refine"],
        glyph: "subdivide",
        role: NodeRole::Standard,
        cook: cook_subdivide,
        migrate: Some(migrate_drop_scheme),
    }
}

/// v1 -> v2: silently drop `scheme`.
///
/// Silent rather than the registry's default drop-with-warning: the param never
/// had any effect, so a toast about losing it would be noise about nothing.
#[allow(clippy::unnecessary_wraps)] // signature matches MigrateFn
fn migrate_drop_scheme(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError> {
    if from == 1 {
        strip_keys(params, &["scheme"]);
    }
    Ok(())
}

fn cook_subdivide(
    p: &ResolvedParams,
    inputs: &Inputs,
    cx: &mut CookCtx,
) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    let iterations = u32::try_from(p.i64("iterations").max(1)).unwrap_or(1);

    if input.has_non_triangle_meshes() {
        cx.warn(
            "subdivide applies to triangle meshes; line and point meshes pass \
             through unchanged",
        );
    }

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

    #[test]
    fn a_point_cloud_warns_and_passes_through() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let set =
            GeometrySet::from_mesh(solarxy_kernel::KernelMesh::points("p", vec![[0.0; 3]; 4]));
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
        assert_eq!(out.meshes[0].vertex_count(), 4, "cloud untouched");
        let warns = cx.take_warnings();
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("pass"), "got: {}", warns[0]);
    }
}
