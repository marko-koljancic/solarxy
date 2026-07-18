//! The `merge` modifier. Concatenates
//! its variadic Geometry inputs in port order, deduplicating materials by
//! content. An empty merge outputs empty geometry with a warning (not an
//! error). Replaces Minimystix's fixed-input Combine.

use std::sync::Arc;

use super::common::{geometry_output, migrate_strip_rendering_group, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::registry::coerce::DataType;
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "merge",
        version: 2,
        display_name: "Merge",
        category: Category::Modifiers,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::variadic("inputs", "Inputs", DataType::Geometry, 0)
                .default_port()
                .doc(
                    "The geometry sets to concatenate. Wire as many as you like. Order \
                     matters in three ways: it fixes the mesh order of the result, it \
                     fixes the order of the deduplicated material table, and a bypassed \
                     merge passes through the FIRST connected input. A wire whose \
                     upstream has no geometry yet is skipped, and it contributes exactly \
                     nothing -- the result is the same as if the wire were not there. \
                     Connecting nothing at all is a warning, not an error.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with("Merge", vec![]),
        // The first connected sub-input passes through when bypassed.
        bypass: BypassBehavior::PassThrough {
            input: "inputs".to_string(),
        },
        doc: "Concatenates every connected geometry input into a single set, in \
              port order. Materials are deduplicated by content as it goes, so \
              four copies of the same red plastic arrive as one table entry \
              rather than four identical ones.\n\n\
              This is the recombine end of a fan-out: a `box` down one branch \
              and a `sphere` down another, each with its own `transform`, \
              merged back into the one set that a `validate` or an output node \
              sees. It takes as many inputs as you wire into it.\n\n\
              Nothing is welded, intersected, or fused geometrically. Two \
              meshes that overlap stay two overlapping meshes and the point \
              count is exactly the sum of the inputs. What order buys you is \
              the mesh order downstream and which input a bypass passes \
              through (the first). Merging nothing is legal: empty geometry \
              and a warning.",
        search_aliases: &["combine", "join", "union", "append"],
        glyph: "merge",
        role: NodeRole::Gather,
        cook,
        migrate: Some(migrate_strip_rendering_group),
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(_p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let sets: Vec<Arc<solarxy_kernel::GeometrySet>> = inputs
        .geometry_list("inputs")
        .into_iter()
        .map(Arc::clone)
        .collect();
    let merged = solarxy_kernel::merge::merge(&sets);
    if merged.is_renderable_empty() {
        cx.warn("merge produced no geometry");
    }
    Ok(CookOutcome::Done(Outputs::geometry(merged)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use crate::registry::coerce::Value;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::{generate_box, generate_plane};
    use std::collections::BTreeMap;

    fn variadic_inputs(sets: Vec<GeometrySet>) -> Inputs {
        slots_from(
            sets.into_iter()
                .map(|s| Some(Value::Geometry(Arc::new(s))))
                .collect(),
        )
    }

    /// Holes included: a `None` is a connected wire whose upstream has no
    /// committed value.
    fn slots_from(values: Vec<Option<Value>>) -> Inputs {
        let mut slots = BTreeMap::new();
        slots.insert("inputs".to_string(), InputSlot::Variadic(values));
        Inputs::new(slots)
    }

    #[test]
    fn concatenates_in_order() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let inputs = variadic_inputs(vec![
            GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1)),
            GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)),
        ]);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("merge cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        // Box (24 verts) + plane (4 verts).
        assert_eq!(set.point_count(), 28);
        assert_eq!(set.mesh_count(), 2);
        assert!(cx.take_warnings().is_empty());
    }

    /// The gather keeps a hole for every connected wire whose upstream has no
    /// committed value. Merge concatenates, so it must skip them and behave
    /// exactly as if the wire were not there.
    #[test]
    fn holes_are_skipped_not_concatenated() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let inputs = slots_from(vec![
            Some(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_box(1.0, 1.0, 1.0, 1, 1, 1),
            )))),
            None,
            Some(Value::Geometry(Arc::new(GeometrySet::from_mesh(
                generate_plane(1.0, 1.0, 1, 1),
            )))),
        ]);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("merge cooks synchronously");
        };
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert_eq!(
            set.point_count(),
            28,
            "box + plane, the hole contributes nothing"
        );
        assert_eq!(set.mesh_count(), 2);
    }

    #[test]
    fn empty_merge_warns() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let inputs = variadic_inputs(vec![]);
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("merge cooks synchronously");
        };
        assert!(out.is_renderable_empty());
        assert_eq!(cx.take_warnings().len(), 1);
    }
}
