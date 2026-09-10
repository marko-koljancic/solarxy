//! The expression lane: which parameters accept an expression, what text a
//! freshly opened field starts from, and how a resolved value reads.
//!
//! Two things about the lane are deliberately not here. **Which parameter
//! types accept an expression** is a fact about a parameter type and lives
//! on [`solarxy_graph::registry::param_spec::ParamType::accepts_expression`];
//! this module forwards to it rather than keeping a list, which is what
//! retires the drift test that used to hold a browser array against it.
//! And **parked expression text** is per-session interface memory that the
//! frozen scene schema pushed out of the document, so it belongs to
//! whichever shell is holding it, not to a rule.

use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::registry::param_spec::ParamType;

/// Whether a parameter of this type can be driven by an expression.
#[must_use]
pub fn accepts_expression(ty: &ParamType) -> bool {
    ty.accepts_expression()
}

/// The expression driving a parameter, if one is.
///
/// A literal and an unset parameter answer the same way, because the lane
/// asks whether an expression is in charge, and neither is.
#[must_use]
pub fn param_expression(source: Option<&ParamSource>) -> Option<&str> {
    match source {
        Some(ParamSource::Expression { expr }) => Some(expr),
        _ => None,
    }
}

/// The text an expression field opens on: the value the parameter already
/// had, spelled the way the grammar spells it.
///
/// Seeding rather than opening blank matters because a blank expression is
/// a parse error, so an empty field would badge the node the instant
/// someone reached for the affordance.
#[must_use]
pub fn seed_expression(value: &ParamValue) -> String {
    fn num(v: f64) -> String {
        if v.is_finite() {
            trim(v)
        } else {
            "0".to_string()
        }
    }
    fn set(parts: &[f64]) -> String {
        let inner: Vec<String> = parts.iter().map(|v| num(*v)).collect();
        format!("set({})", inner.join(", "))
    }
    match value {
        ParamValue::Float(v) => num(*v),
        ParamValue::Int(v) => v.to_string(),
        // The grammar has no boolean literal, so a comparison is how a
        // constant true is spelled.
        ParamValue::Bool(b) => if *b { "1 > 0" } else { "0 > 1" }.to_string(),
        ParamValue::Vec2(v) => set(v),
        ParamValue::Vec3(v) => set(v),
        ParamValue::Vec4(v) => set(v),
        ParamValue::Color(v) => set(&v.map(f64::from)),
        _ => "0".to_string(),
    }
}

/// A resolved value as the readout under the field prints it.
///
/// Six decimal places shows a change without turning the readout into
/// noise, and trailing zeros come off.
#[must_use]
pub fn format_resolved(value: &ParamValue) -> String {
    fn join(parts: &[f64]) -> String {
        let inner: Vec<String> = parts.iter().map(|v| trim(*v)).collect();
        inner.join(", ")
    }
    match value {
        ParamValue::Float(v) => trim(*v),
        ParamValue::Int(v) => v.to_string(),
        ParamValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        ParamValue::Vec2(v) => join(v),
        ParamValue::Vec3(v) => join(v),
        ParamValue::Vec4(v) => join(v),
        ParamValue::Color(v) => join(&v.map(f64::from)),
        ParamValue::Text(s) | ParamValue::Enum(s) => s.clone(),
        ParamValue::Asset(id) => id.0.clone(),
        ParamValue::NodeRef(id) => id.map_or_else(|| "none".to_string(), |n| n.0.to_string()),
    }
}

/// Six decimal places with the trailing zeros removed.
fn trim(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "NaN".to_string()
        } else if v > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    let fixed = format!("{v:.6}");
    let trimmed = fixed.trim_end_matches('0');
    trimmed.strip_suffix('.').unwrap_or(trimmed).to_string()
}

/// Whether a completed edit is worth sending.
///
/// The one rule behind the draft-and-commit contract every text field
/// follows: a field holds a draft while it is being typed and sends once,
/// on blur or on the commit key, and only when the draft actually differs
/// from what was last sent. Without the comparison a field that is
/// focused and left alone writes a parameter and fills the undo stack
/// with a step that changed nothing.
///
/// The rest of that contract is a shell's state machine rather than a
/// rule, and stays with each shell.
#[must_use]
pub fn should_commit(draft: &str, last_sent: &str) -> bool {
    draft != last_sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::registry::param_spec::EnumVariant;

    fn expr(text: &str) -> ParamSource {
        ParamSource::Expression {
            expr: text.to_string(),
        }
    }

    #[test]
    fn exactly_the_numeric_types_accept_an_expression() {
        // Forwarded to the engine rather than restated, which is what
        // retires the drift test that used to hold a browser array to it.
        for ty in [
            ParamType::Float,
            ParamType::Int,
            ParamType::Bool,
            ParamType::Vec2,
            ParamType::Vec3,
            ParamType::Vec4,
            ParamType::Color,
        ] {
            assert!(accepts_expression(&ty), "{ty:?}");
        }
    }

    #[test]
    fn refuses_the_types_an_expression_could_never_produce_a_value_for() {
        // There is no string in the value lattice, and an asset or a node
        // reference is an identity rather than a number.
        for ty in [
            ParamType::Text,
            ParamType::MultilineText,
            ParamType::AttributeName,
            ParamType::Snippet,
            ParamType::Action,
            ParamType::Enum {
                variants: vec![EnumVariant {
                    key: "a".into(),
                    label: "A".into(),
                }],
            },
            ParamType::AssetRef { accept: Vec::new() },
            ParamType::NodePath {
                accept: solarxy_graph::registry::param_spec::NodePathAccept::TypeIs(
                    "camera".to_string(),
                ),
            },
        ] {
            assert!(!accepts_expression(&ty), "{ty:?}");
        }
    }

    #[test]
    fn reads_the_stored_expression_and_nothing_else() {
        assert_eq!(param_expression(Some(&expr("1 + 1"))), Some("1 + 1"));
        assert_eq!(
            param_expression(Some(&ParamSource::Literal(ParamValue::Float(2.0)))),
            None
        );
        assert_eq!(param_expression(None), None);
    }

    #[test]
    fn seeds_from_the_value_the_param_already_had() {
        // Opening blank would be a parse error, badging the node the
        // instant someone reached for the affordance.
        assert_eq!(seed_expression(&ParamValue::Float(2.5)), "2.5");
        assert_eq!(seed_expression(&ParamValue::Float(0.0)), "0");
        assert_eq!(seed_expression(&ParamValue::Int(7)), "7");
    }

    #[test]
    fn spells_a_vector_with_a_constructor() {
        assert_eq!(
            seed_expression(&ParamValue::Vec3([1.0, 2.0, 3.0])),
            "set(1, 2, 3)"
        );
        assert_eq!(
            seed_expression(&ParamValue::Color([1.0, 0.0, 0.0, 1.0])),
            "set(1, 0, 0, 1)"
        );
    }

    #[test]
    fn spells_a_bool_as_a_comparison_since_the_grammar_has_no_literals() {
        assert_eq!(seed_expression(&ParamValue::Bool(true)), "1 > 0");
        assert_eq!(seed_expression(&ParamValue::Bool(false)), "0 > 1");
    }

    #[test]
    fn never_seeds_something_unparseable() {
        assert_eq!(seed_expression(&ParamValue::Float(f64::NAN)), "0");
        assert_eq!(seed_expression(&ParamValue::Float(f64::INFINITY)), "0");
        // A type an expression cannot drive still has to answer, because
        // the caller asks before it checks.
        assert_eq!(seed_expression(&ParamValue::Text("nonsense".into())), "0");
    }

    #[test]
    fn the_readout_rounds_without_leaving_trailing_zeros() {
        assert_eq!(format_resolved(&ParamValue::Float(1.0 / 3.0)), "0.333333");
        assert_eq!(format_resolved(&ParamValue::Float(2.5)), "2.5");
        assert_eq!(format_resolved(&ParamValue::Float(2.0)), "2");
    }

    #[test]
    fn the_readout_shows_vectors_component_wise() {
        assert_eq!(
            format_resolved(&ParamValue::Vec3([1.0, 2.0, 3.0])),
            "1, 2, 3"
        );
        assert_eq!(
            format_resolved(&ParamValue::Color([1.0, 0.0, 0.0, 1.0])),
            "1, 0, 0, 1"
        );
    }

    #[test]
    fn the_readout_shows_a_bool_as_a_word() {
        assert_eq!(format_resolved(&ParamValue::Bool(true)), "true");
        assert_eq!(format_resolved(&ParamValue::Bool(false)), "false");
    }

    #[test]
    fn the_readout_survives_the_non_finite_values_division_produces() {
        // Dividing by zero yields an infinity by design, so that one bad
        // element cannot blank a scene. The readout has to render one.
        assert_eq!(
            format_resolved(&ParamValue::Float(f64::INFINITY)),
            "Infinity"
        );
        assert_eq!(
            format_resolved(&ParamValue::Float(f64::NEG_INFINITY)),
            "-Infinity"
        );
        assert_eq!(format_resolved(&ParamValue::Float(f64::NAN)), "NaN");
    }

    #[test]
    fn a_completed_edit_sends_only_when_the_text_moved() {
        // A field focused and left alone must not write a parameter, or
        // the undo stack fills with steps that changed nothing.
        assert!(should_commit("2.5", "2"));
        assert!(!should_commit("2", "2"));
        assert!(should_commit("", "2"));
    }
}
