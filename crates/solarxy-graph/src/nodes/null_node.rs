//! The `null` utility node. A
//! pass-through anchor: it exists to give a subflow a stable, tidy display-flag
//! target (the idiomatic "OUT" node) that does not move when the graph behind
//! it is rewired. The cook is an `Arc` clone, so it costs nothing.

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::registry::coerce::{DataType, Value};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "null",
        version: 1,
        display_name: "Null",
        category: Category::Utility,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .carries_placements()
                .doc(
                    "The geometry to pass through unchanged. Left \
                     unconnected the null cooks to empty geometry rather \
                     than failing, so an anchor placed before its upstream \
                     exists is not an error.",
                ),
        ],
        outputs: vec![geometry_output()],
        params: params_with("Null", vec![]),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Passes its input geometry through untouched. The cook is a \
              refcount bump on the incoming geometry, not a copy, so a null \
              costs effectively nothing however large the model.\n\n\
              A graph wants a no-op for two reasons. The first is a stable \
              reference point: put a null at the end of a subflow, name it \
              OUT, and point the display flag at it. You can then rewire, \
              insert, and delete everything upstream and the flag never moves \
              -- whereas a flag pointed at whichever node happened to be last \
              has to be re-set every time you extend the chain. The second is \
              routing: a null is a reroute, a place to give a long wire a \
              corner and a name.\n\n\
              Nothing about it is inert to the graph, only to the geometry. It \
              is a real node that cooks, appears in the dependency chain, and \
              can be bypassed (bypassing passes the input straight through, \
              which for a null is what it already did).",
        search_aliases: &["out", "output", "anchor", "passthrough"],
        glyph: "null",
        role: NodeRole::Terminal,
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(_p: &ResolvedParams, inputs: &Inputs, _cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    let Some(input) = inputs.geometry("geometry") else {
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    };
    // Refcount bump, not a copy.
    Ok(CookOutcome::Done(Outputs::single(
        "geometry",
        Value::Geometry(std::sync::Arc::clone(input)),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::generate_box;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn passes_the_input_arc_through_untouched() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let set = Arc::new(GeometrySet::from_mesh(generate_box(1.0, 1.0, 1.0, 1, 1, 1)));
        let mut slots = BTreeMap::new();
        slots.insert(
            "geometry".to_string(),
            InputSlot::Single(Value::Geometry(Arc::clone(&set))),
        );
        let inputs = Inputs::new(slots);

        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("null cooks synchronously");
        };
        let Some(Value::Geometry(got)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        assert!(Arc::ptr_eq(got, &set), "null must not copy the geometry");
    }

    #[test]
    fn an_empty_upstream_yields_empty() {
        let resolved =
            crate::registry::resolve::resolve_params(&BTreeMap::new(), &descriptor().params)
                .unwrap();
        let inputs = Inputs::new(BTreeMap::new());
        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("null cooks synchronously");
        };
        assert!(out.is_renderable_empty());
    }
}
