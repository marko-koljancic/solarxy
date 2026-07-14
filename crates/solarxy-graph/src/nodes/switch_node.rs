//! The `switch` utility node (node catalog part II, Tier-2, Phase 15).
//! Selects one of N variadic geometry inputs by index.
//!
//! **Selection is positional, and that is load-bearing.** The index addresses
//! the *n*th connected wire in `port_order`, not the *n*th wire that happened
//! to produce a value. Before Phase 15 the cook driver compacted absent values
//! out of a variadic gather, so a branch that errored, was bypassed to empty,
//! or had not cooked yet would silently shift every later branch down one and
//! this node would select the wrong geometry with no indication. The gather now
//! preserves holes ([`Inputs::geometry_slots`]) and this node reads them.
//!
//! A selected wire that IS a hole yields empty geometry plus a warning: the one
//! thing we must never do is quietly substitute a neighbour.

use super::common::{geometry_output, params_with};
use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
use crate::params::ParamValue;
use crate::registry::coerce::{DataType, Value};
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::ResolvedParams;
use crate::registry::{BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec};

#[must_use]
pub fn descriptor() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "switch",
        version: 1,
        display_name: "Switch",
        category: Category::Utility,
        contexts: ContextMask::SUBFLOW,
        inputs: vec![
            PortSpec::variadic("inputs", "Inputs", DataType::Geometry, 0)
                .default_port()
                .doc("The candidate geometries, in wire order. The index selects among them."),
        ],
        outputs: vec![geometry_output()],
        params: params_with(
            "Switch",
            vec![
                ParamSpec::new(
                    "index",
                    "Index",
                    "switch",
                    ParamType::Int,
                    ParamValue::Int(0),
                )
                .hard(0.0, 255.0)
                .soft(0.0, 8.0)
                .step(1.0)
                .doc(
                    "Which input wire to pass through, counting from 0 in wire \
                     order. Clamped to the number of connected wires.",
                ),
            ],
        ),
        bypass: BypassBehavior::PassThrough {
            input: "inputs".to_string(),
        },
        doc: "Passes exactly one of its inputs through, chosen by index. The \
              index counts wires in the order they are connected, so an input \
              that fails to cook does not shift the selection.",
        search_aliases: &["select", "choose", "multiplex", "if"],
        cook,
        migrate: None,
    }
}

#[allow(clippy::unnecessary_wraps)] // signature matches CookFn
fn cook(p: &ResolvedParams, inputs: &Inputs, cx: &mut CookCtx) -> Result<CookOutcome, CookError> {
    // Positional: one entry per connected wire, holes included.
    let slots = inputs.geometry_slots("inputs");
    if slots.is_empty() {
        cx.warn("switch has no connected inputs");
        return Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )));
    }

    let requested = p.i64("index");
    let last = i64::try_from(slots.len() - 1).unwrap_or(i64::MAX);
    let index = requested.clamp(0, last);
    if index != requested {
        cx.warn(format!(
            "switch index {requested} is outside the {} connected input(s); using {index}",
            slots.len()
        ));
    }

    if let Some(set) = slots[usize::try_from(index).unwrap_or(0)] {
        Ok(CookOutcome::Done(Outputs::single(
            "geometry",
            Value::Geometry(std::sync::Arc::clone(set)),
        )))
    } else {
        // The wire exists but its upstream produced nothing. Say so; never
        // silently fall through to a neighbouring input.
        cx.warn(format!(
            "switch input {index} has no geometry (its upstream is empty or failed)"
        ));
        Ok(CookOutcome::Done(Outputs::geometry(
            solarxy_kernel::GeometrySet::empty(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::InputSlot;
    use solarxy_kernel::GeometrySet;
    use solarxy_kernel::primitives::{generate_box, generate_plane};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// A filled wire. `None` in a slot list is a hole (an upstream with no
    /// committed value).
    fn geom(set: GeometrySet) -> Value {
        Value::Geometry(Arc::new(set))
    }

    fn run(slots_in: Vec<Option<Value>>, index: i64) -> (Outputs, Vec<String>) {
        let mut stored = BTreeMap::new();
        stored.insert(
            "index".to_string(),
            crate::params::ParamSource::Literal(ParamValue::Int(index)),
        );
        let resolved =
            crate::registry::resolve::resolve_params(&stored, &descriptor().params).unwrap();

        let mut slots = BTreeMap::new();
        slots.insert("inputs".to_string(), InputSlot::Variadic(slots_in));
        let inputs = Inputs::new(slots);

        let assets = crate::assets::AssetTable::new();
        let mut cx = CookCtx::new(&assets, false);
        let CookOutcome::Done(out) = cook(&resolved, &inputs, &mut cx).unwrap() else {
            panic!("switch cooks synchronously");
        };
        (out, cx.take_warnings())
    }

    fn tri_count(out: &Outputs) -> u64 {
        let Some(Value::Geometry(set)) = out.get("geometry") else {
            panic!("outputs geometry");
        };
        set.triangle_count()
    }

    #[test]
    fn selects_the_indexed_input() {
        // Box (12 tris) at 0, plane (2 tris) at 1.
        let (out, warns) = run(
            vec![
                Some(geom(GeometrySet::from_mesh(generate_box(
                    1.0, 1.0, 1.0, 1, 1, 1,
                )))),
                Some(geom(GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)))),
            ],
            1,
        );
        assert_eq!(tri_count(&out), 2, "selected the plane");
        assert!(warns.is_empty());
    }

    /// The regression this node exists to prevent. Wire 0 has no value (its
    /// upstream errored). Index 1 must still be the SECOND wire, not the first
    /// surviving one. Under the old compacting gather this returned the box.
    #[test]
    fn a_hole_does_not_shift_the_selection() {
        let (out, _) = run(
            vec![
                None, // wire 0: upstream errored
                Some(geom(GeometrySet::from_mesh(generate_box(
                    1.0, 1.0, 1.0, 1, 1, 1,
                )))),
                Some(geom(GeometrySet::from_mesh(generate_plane(1.0, 1.0, 1, 1)))),
            ],
            2,
        );
        assert_eq!(
            tri_count(&out),
            2,
            "index 2 is the third wire (the plane), not the second survivor"
        );
    }

    #[test]
    fn selecting_a_hole_yields_empty_and_warns_instead_of_substituting() {
        let (out, warns) = run(
            vec![
                None,
                Some(geom(GeometrySet::from_mesh(generate_box(
                    1.0, 1.0, 1.0, 1, 1, 1,
                )))),
            ],
            0,
        );
        assert_eq!(tri_count(&out), 0, "no silent fallback to the box");
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("no geometry"), "got: {}", warns[0]);
    }

    #[test]
    fn an_out_of_range_index_clamps_and_warns() {
        let (out, warns) = run(
            vec![Some(geom(GeometrySet::from_mesh(generate_plane(
                1.0, 1.0, 1, 1,
            ))))],
            7,
        );
        assert_eq!(tri_count(&out), 2, "clamped to the only input");
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("outside"), "got: {}", warns[0]);
    }

    #[test]
    fn no_inputs_yields_empty_and_warns() {
        let (out, warns) = run(vec![], 0);
        assert_eq!(tri_count(&out), 0);
        assert_eq!(warns.len(), 1);
    }
}
