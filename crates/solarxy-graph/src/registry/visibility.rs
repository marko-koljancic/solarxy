//! Visibility: whether a parameter row is shown, and whether a node is.
//!
//! Two questions that share a word and nothing else. A parameter's
//! visibility is the evaluator for a spec's `show_if` clauses; a node's is
//! whether its `visible` param hides it in the viewport. They sit together
//! because both read a declaration beside the stored parameters and answer
//! for a surface without being one, and because a reader looking for
//! either looks here first. The names keep them apart: `param_visible`
//! against `node_visible`.
//!
//! This is a fact about a node type rather than about a surface, which is
//! why it lives on the registry resolution path beside the declaration it
//! reads. Both shells and the documentation generator ask the same
//! question and get the same answer; before this module the only
//! implementation was in the browser, so the desktop could not evaluate a
//! clause at all and its Properties panel had to assert that no parameter
//! it drew declared one.
//!
//! **A missing referenced parameter is not an error.** A clause naming a
//! key no node declares resolves to no value, and each predicate answers
//! for itself: `Truthy` and `Eq` and `In` are false, `Neq` is true. That
//! asymmetry is deliberate rather than an oversight, and it matches the
//! browser rule this replaces.

use std::collections::BTreeMap;

use crate::params::{ParamSource, ParamValue};
use crate::registry::NodeTypeDescriptor;
use crate::registry::param_spec::{ParamSpec, Pred};

/// The value `key` currently resolves to: the stored literal, else the
/// declared default, else nothing when no spec declares the key.
///
/// An expression source falls through to the default, because the
/// expression reserve refuses to evaluate and a panel that hid rows
/// behind an unevaluated expression would hide them at random.
fn current_value<'a>(
    key: &str,
    specs: &'a [ParamSpec],
    params: &'a BTreeMap<String, ParamSource>,
) -> Option<&'a ParamValue> {
    if let Some(ParamSource::Literal(value)) = params.get(key) {
        return Some(value);
    }
    specs.iter().find(|s| s.key == key).map(|s| &s.default)
}

/// Whether a value counts as set, for a bare `Truthy` clause.
///
/// The arms reproduce the coercion the browser applied to the same value,
/// because this evaluator replaces that one and a clause must not change
/// meaning on the day it moves. Only two shapes are reachable from the
/// registry today, a bool and one float (the material node hides its
/// clearcoat roughness while clearcoat is zero), so the rest are written
/// from the rule rather than from a case that exists.
///
/// The one deliberate departure is `NodeRef`, which asks whether the
/// reference is set. The browser asked whether the raw id was non-zero,
/// which is the same question for every id the engine mints and would
/// differ only for a node numbered zero. No clause targets a node path,
/// so the difference has never been reachable.
fn truthy(value: Option<&ParamValue>) -> bool {
    match value {
        None => false,
        Some(ParamValue::Bool(b)) => *b,
        Some(ParamValue::Float(f)) => *f != 0.0 && !f.is_nan(),
        Some(ParamValue::Int(i)) => *i != 0,
        Some(ParamValue::Text(s) | ParamValue::Enum(s)) => !s.is_empty(),
        Some(ParamValue::Asset(id)) => !id.0.is_empty(),
        Some(
            ParamValue::Vec2(_) | ParamValue::Vec3(_) | ParamValue::Vec4(_) | ParamValue::Color(_),
        ) => true,
        Some(ParamValue::NodeRef(id)) => id.is_some(),
    }
}

/// Whether `spec` is visible given the node's stored parameters. A spec
/// declaring no clauses is always visible, and where it declares
/// several they all have to hold.
#[must_use]
pub fn param_visible(
    spec: &ParamSpec,
    specs: &[ParamSpec],
    params: &BTreeMap<String, ParamSource>,
) -> bool {
    spec.show_if.iter().all(|cond| {
        let value = current_value(&cond.param, specs, params);
        match &cond.pred {
            Pred::Truthy => truthy(value),
            Pred::Eq(want) => value == Some(want),
            Pred::Neq(want) => value != Some(want),
            Pred::In(wants) => value.is_some_and(|v| wants.contains(v)),
        }
    })
}

/// The keys of every currently visible parameter, in declaration order.
///
/// One call answers for a whole node, which is what the browser asks
/// across the WebAssembly boundary: a call per parameter would be one
/// crossing per row per render of the parameter panel.
#[must_use]
pub fn visible_param_keys(
    specs: &[ParamSpec],
    params: &BTreeMap<String, ParamSource>,
) -> Vec<String> {
    specs
        .iter()
        .filter(|spec| param_visible(spec, specs, params))
        .map(|spec| spec.key.clone())
        .collect()
}

/// Whether a node type declares the root visibility parameter, and so
/// whether the affordance exists for it.
///
/// Registry-driven rather than a list of type ids: a node type that
/// declares `visible` gets the affordance, and one that does not gets none
/// by construction. A note gets no eye without anyone saying so, and a
/// root-placeable type added later gets one for free.
#[must_use]
pub fn declares_node_visibility(desc: &NodeTypeDescriptor) -> bool {
    desc.params.iter().any(|p| p.key == "visible")
}

/// Whether a node is currently shown.
///
/// Anything but an explicit literal `false` reads as visible, an
/// expression included. Parameters are override-only, so a freshly added
/// node carries no entry at all and has to default to shown; and a node
/// whose visibility is driven by an expression the reserve refuses to
/// evaluate is better shown than silently hidden.
///
/// Root visibility is a different thing from the display flag a network
/// carries: separate storage, separate command, separate affordance.
#[must_use]
pub fn node_visible(params: &BTreeMap<String, ParamSource>) -> bool {
    !matches!(
        params.get("visible"),
        Some(ParamSource::Literal(ParamValue::Bool(false)))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::param_spec::{EnumVariant, ParamType};

    fn float(key: &str) -> ParamSpec {
        ParamSpec::new(
            key,
            key,
            "attribute",
            ParamType::Float,
            ParamValue::Float(0.0),
        )
    }

    fn selector() -> ParamSpec {
        ParamSpec::new(
            "type",
            "Type",
            "attribute",
            ParamType::Enum {
                variants: vec![
                    EnumVariant {
                        key: "float".into(),
                        label: "Float".into(),
                    },
                    EnumVariant {
                        key: "vec3".into(),
                        label: "Vector 3".into(),
                    },
                ],
            },
            ParamValue::Enum("float".into()),
        )
    }

    fn enum_val(key: &str) -> ParamValue {
        ParamValue::Enum(key.into())
    }

    /// The three specs the browser suite built its cases on: an enum
    /// selector and two variant rows gated on it.
    fn specs() -> Vec<ParamSpec> {
        vec![
            selector(),
            float("value_float").show_if("type", Pred::Eq(enum_val("float"))),
            ParamSpec::new(
                "value_vec3",
                "Value",
                "attribute",
                ParamType::Vec3,
                ParamValue::Vec3([0.0, 0.0, 0.0]),
            )
            .show_if("type", Pred::Eq(enum_val("vec3"))),
        ]
    }

    fn stored(pairs: &[(&str, ParamValue)]) -> BTreeMap<String, ParamSource> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), ParamSource::Literal(v.clone())))
            .collect()
    }

    fn visible(key: &str, specs: &[ParamSpec], params: &BTreeMap<String, ParamSource>) -> bool {
        let spec = specs.iter().find(|s| s.key == key).expect("spec declared");
        param_visible(spec, specs, params)
    }

    #[test]
    fn shows_only_the_variant_matching_the_stored_enum_value() {
        let specs = specs();
        let params = stored(&[("type", enum_val("vec3"))]);
        assert!(!visible("value_float", &specs, &params));
        assert!(visible("value_vec3", &specs, &params));
    }

    #[test]
    fn falls_back_to_the_referenced_params_default_when_nothing_is_stored() {
        let specs = specs();
        let empty = BTreeMap::new();
        assert!(visible("value_float", &specs, &empty));
        assert!(!visible("value_vec3", &specs, &empty));
    }

    #[test]
    fn a_spec_without_clauses_is_always_visible() {
        assert!(visible("type", &specs(), &BTreeMap::new()));
    }

    #[test]
    fn evaluates_truthy_neq_and_in() {
        let flag = ParamSpec::new(
            "flag",
            "Flag",
            "general",
            ParamType::Bool,
            ParamValue::Bool(false),
        );
        let pair = vec![flag, float("gated").show_if("flag", Pred::Truthy)];
        assert!(!visible("gated", &pair, &BTreeMap::new()));
        assert!(visible(
            "gated",
            &pair,
            &stored(&[("flag", ParamValue::Bool(true))])
        ));

        let mut with_neq = specs();
        with_neq.push(float("neq").show_if("type", Pred::Neq(enum_val("float"))));
        assert!(!visible("neq", &with_neq, &BTreeMap::new()));
        assert!(visible(
            "neq",
            &with_neq,
            &stored(&[("type", enum_val("vec2"))])
        ));

        let mut with_in = specs();
        with_in.push(
            float("one_of").show_if("type", Pred::In(vec![enum_val("vec2"), enum_val("vec3")])),
        );
        assert!(visible(
            "one_of",
            &with_in,
            &stored(&[("type", enum_val("vec3"))])
        ));
        assert!(!visible(
            "one_of",
            &with_in,
            &stored(&[("type", enum_val("float"))])
        ));
    }

    #[test]
    fn compares_vector_values_componentwise() {
        let anchor = ParamSpec::new(
            "anchor",
            "Anchor",
            "general",
            ParamType::Vec3,
            ParamValue::Vec3([0.0, 1.0, 0.0]),
        );
        let pair = vec![
            anchor,
            float("gated").show_if("anchor", Pred::Eq(ParamValue::Vec3([0.0, 1.0, 0.0]))),
        ];
        assert!(visible("gated", &pair, &BTreeMap::new()));
        assert!(!visible(
            "gated",
            &pair,
            &stored(&[("anchor", ParamValue::Vec3([1.0, 1.0, 0.0]))])
        ));
    }

    #[test]
    fn ands_multiple_clauses() {
        let mut all = specs();
        all.push(ParamSpec::new(
            "flag",
            "Flag",
            "general",
            ParamType::Bool,
            ParamValue::Bool(false),
        ));
        all.push(
            float("both")
                .show_if("type", Pred::Eq(enum_val("float")))
                .show_if("flag", Pred::Truthy),
        );
        assert!(!visible("both", &all, &BTreeMap::new()));
        assert!(visible(
            "both",
            &all,
            &stored(&[("flag", ParamValue::Bool(true))])
        ));
    }

    #[test]
    fn a_clause_naming_no_declared_param_hides_all_but_neq() {
        // The browser resolved a missing key to `undefined` and let each
        // predicate answer for itself. Nothing pinned that, and the
        // asymmetry is the easiest thing in this module to lose in a port.
        for (pred, expected) in [
            (Pred::Truthy, false),
            (Pred::Eq(enum_val("x")), false),
            (Pred::Neq(enum_val("x")), true),
            (Pred::In(vec![enum_val("x")]), false),
        ] {
            let one = vec![float("row").show_if("nobody_declares_this", pred)];
            assert_eq!(
                param_visible(&one[0], &one, &BTreeMap::new()),
                expected,
                "a clause on an undeclared key"
            );
        }
    }

    #[test]
    fn an_expression_source_falls_back_to_the_default() {
        let specs = specs();
        let mut params = BTreeMap::new();
        params.insert(
            "type".to_string(),
            ParamSource::Expression {
                expr: "ch(\"../other/type\")".into(),
            },
        );
        // The default is `float`, so the float row shows and the vec3 row
        // does not, exactly as with nothing stored.
        assert!(visible("value_float", &specs, &params));
        assert!(!visible("value_vec3", &specs, &params));
    }

    #[test]
    fn truthy_reads_zero_and_empty_as_unset() {
        // The material node is the live case: clearcoat roughness hides
        // while clearcoat is zero.
        assert!(!truthy(Some(&ParamValue::Float(0.0))));
        assert!(truthy(Some(&ParamValue::Float(0.001))));
        assert!(!truthy(Some(&ParamValue::Float(f64::NAN))));
        assert!(!truthy(Some(&ParamValue::Int(0))));
        assert!(truthy(Some(&ParamValue::Int(-1))));
        assert!(!truthy(Some(&ParamValue::Text(String::new()))));
        assert!(truthy(Some(&ParamValue::Text("x".into()))));
        assert!(truthy(Some(&ParamValue::Vec3([0.0, 0.0, 0.0]))));
        assert!(!truthy(Some(&ParamValue::NodeRef(None))));
        assert!(!truthy(None));
    }

    #[test]
    fn visible_keys_answer_for_a_whole_node_in_declaration_order() {
        let specs = specs();
        assert_eq!(
            visible_param_keys(&specs, &stored(&[("type", enum_val("vec3"))])),
            ["type", "value_vec3"]
        );
    }

    // A clause naming a param its node does not declare needs no test
    // here: the registry refuses to build at all, at
    // `Registry::invariant_violations` (`registry/mod.rs`), which also
    // forbids a clause on the param it gates. A guard was written here
    // first and was vacuous, because `builtin_registry()` returns an
    // error before any assertion of ours runs.

    #[test]
    fn every_registered_clause_evaluates_against_its_nodes_defaults() {
        // The whole catalog run through the evaluator once: no panic, and
        // every node keeps at least one visible param, which is what stops
        // a future clause from emptying a panel.
        let registry = crate::nodes::builtin_registry().expect("builtin registry");
        for desc in registry.descriptors() {
            if desc.params.is_empty() {
                continue;
            }
            let visible = visible_param_keys(&desc.params, &BTreeMap::new());
            assert!(
                !visible.is_empty(),
                "{} hides every param at its own defaults",
                desc.type_id
            );
        }
    }
    #[test]
    fn only_an_explicit_false_hides_a_node() {
        // Parameters are override-only, so a fresh node carries no entry
        // and must read as shown. An expression reads as shown too: the
        // reserve refuses to evaluate it, and hiding on an unevaluated
        // expression would make nodes disappear at random.
        let stored = |src: ParamSource| BTreeMap::from([("visible".to_string(), src)]);
        assert!(node_visible(&BTreeMap::new()));
        assert!(node_visible(&stored(ParamSource::Literal(
            ParamValue::Bool(true)
        ))));
        assert!(!node_visible(&stored(ParamSource::Literal(
            ParamValue::Bool(false)
        ))));
        assert!(node_visible(&stored(ParamSource::Expression {
            expr: "0 > 1".to_string()
        })));
    }

    #[test]
    fn exactly_the_types_that_put_something_in_the_viewport_offer_the_eye() {
        // Named rather than derived, so an eighth type gaining the
        // affordance is a deliberate edit here. The rule is that the
        // affordance follows what renders: the lights and the geometry
        // network. A camera, a note, a shading network and a render node
        // are all placeable at the root and none of them draws, so none
        // of them gets an eye.
        let registry = crate::nodes::builtin_registry().expect("builtin registry");
        let mut offering: Vec<&str> = registry
            .descriptors()
            .filter(|d| declares_node_visibility(d))
            .map(|d| d.type_id)
            .collect();
        offering.sort_unstable();
        assert_eq!(
            offering,
            [
                "ambient_light",
                "directional_light",
                "hemisphere_light",
                "point_light",
                "rect_area_light",
                "sopnet",
                "spot_light",
            ]
        );
    }
}
