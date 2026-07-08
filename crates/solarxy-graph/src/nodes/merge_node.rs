//! The `merge` modifier (node catalog part II, section 13). Concatenates
//! its variadic Geometry inputs in port order, deduplicating materials by
//! content. An empty merge outputs empty geometry with a warning (not an
//! error). Replaces Minimystix's fixed-input Combine (decision 25).

use std::sync::Arc;

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::registry::coerce::DataType;
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "merge",
        version: 1,
        display_name: "Merge",
        category: Category::Modifiers,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::variadic("inputs", "Inputs", DataType::Geometry, 0)
                .default_port()
                .doc("The geometry sets to concatenate, in order."),
        ],
        outputs: vec![geometry_output()],
        params: params_with("Merge", vec![]),
        // The first connected sub-input passes through when bypassed.
        bypass: BypassBehavior::PassThrough {
            input: "inputs".to_string(),
        },
        doc: "Concatenates its inputs into one geometry set, in port order, \
              deduplicating identical materials.",
        search_aliases: &["combine", "join", "union", "append"],
        cook,
        migrate: None,
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
        let values = sets
            .into_iter()
            .map(|s| Value::Geometry(Arc::new(s)))
            .collect();
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
