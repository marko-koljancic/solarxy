//! The `null` utility node (node catalog part II, Tier-2, Phase 15). A
//! pass-through anchor: it exists to give a subflow a stable, tidy display-flag
//! target (the idiomatic "OUT" node) that does not move when the graph behind
//! it is rewired. The cook is an `Arc` clone, so it costs nothing.

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::registry::coerce::{DataType, Value};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "null",
        version: 1,
        display_name: "Null",
        category: Category::Utility,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, true)
                .default_port()
                .doc("The geometry to pass through unchanged."),
        ],
        outputs: vec![geometry_output()],
        params: params_with("Null", vec![]),
        bypass: BypassBehavior::PassThrough {
            input: "geometry".to_string(),
        },
        doc: "Passes geometry through unchanged. Use it as a stable output \
              anchor for a subflow: point the display flag at the null and it \
              stays put while you rework the graph feeding it.",
        search_aliases: &["out", "output", "anchor", "passthrough"],
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
