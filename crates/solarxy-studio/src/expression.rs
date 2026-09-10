//! The expression lane: which parameters accept an expression, what text a
//! freshly opened field starts from, how a resolved value reads, and where
//! a failed one went wrong.
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

/// Where an engine error points, when its message names a place.
///
/// Both halves are used by an editor that draws one: the line tints the
/// row, the column underlines the token.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPosition {
    pub line: usize,
    pub column: usize,
    /// The message as it arrived, so a caller that has the position does
    /// not also have to carry the text it came from.
    pub message: String,
}

/// Reads a cook error back into a position.
///
/// **This function should not have to exist, and the shape of its
/// replacement is already known.** [`solarxy_graph::expr::ExprError`]
/// carries the offending byte span, and `line_col` turns it into exactly
/// the pair returned here. What loses it is `CookStatus::Error`, which
/// holds a formatted `String` and nothing else, so by the time a shell
/// sees a failure the position survives only as text inside a sentence.
/// Whoever widens that variant to carry the position deletes this
/// function and its tests rather than hunting for a decoder in one of the
/// shells, which is where this rule lived until 0.10.0.
///
/// Until then this is the only place the `line N, column M` shape is
/// decoded, so if the engine ever changes how it formats one, exactly one
/// thing breaks.
///
/// Written as a scan rather than a pattern match because this crate takes
/// three dependencies and a regular-expression engine is not going to be
/// the fourth for two integers.
#[must_use]
pub fn error_position(message: &str) -> Option<ErrorPosition> {
    let line = number_after(message, "line")?;
    if line == 0 {
        return None;
    }
    let column = number_after(message, "column")
        .filter(|c| *c > 0)
        .unwrap_or(1);
    Some(ErrorPosition {
        line,
        column,
        message: message.to_string(),
    })
}

/// The first `<word> <digits>` in `text`, with `word` starting a word.
///
/// The word boundary is what stops `underline 3` answering for `line`, and
/// the search continues past a boundary-matching word that is not followed
/// by a number, so `line breaks, line 4` answers 4 rather than nothing.
fn number_after(text: &str, word: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut from = 0;
    while let Some(hit) = text[from..].find(word) {
        let at = from + hit;
        from = at + 1;
        if at > 0 && is_word(bytes[at - 1]) {
            continue;
        }
        let after = at + word.len();
        if bytes.get(after) != Some(&b' ') {
            continue;
        }
        let digits: String = text[after + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            continue;
        }
        // A number too large for the address space is not a position.
        if let Ok(n) = digits.parse::<usize>() {
            return Some(n);
        }
    }
    None
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
    #[test]
    fn reads_both_halves_of_the_engines_format() {
        // The exact shape `ExprError::line_col` produces.
        let m = "line 3, column 12: unknown function `noize`";
        assert_eq!(
            error_position(m),
            Some(ErrorPosition {
                line: 3,
                column: 12,
                message: m.to_string(),
            })
        );
    }

    #[test]
    fn falls_back_to_column_one_when_only_a_line_is_named() {
        // Not every engine error goes through the expression formatter;
        // those still tint their line rather than being dropped.
        let p = error_position("line 7: something went wrong").expect("a position");
        assert_eq!((p.line, p.column), (7, 1));
    }

    #[test]
    fn a_message_with_no_position_has_none() {
        assert_eq!(error_position("this program assigns nothing"), None);
        assert_eq!(error_position(""), None);
    }

    #[test]
    fn refuses_a_zero_line_rather_than_marking_one() {
        // Lines are 1-based; a zero would index before the document.
        assert_eq!(error_position("line 0, column 4: x"), None);
    }

    #[test]
    fn refuses_a_zero_column_rather_than_trusting_it() {
        assert_eq!(
            error_position("line 2, column 0: x").map(|p| p.column),
            Some(1)
        );
    }

    #[test]
    fn keeps_the_whole_message_rather_than_the_tail() {
        // The message is what the hover shows, so truncating it to the
        // part after the colon would lose the position the user can read.
        let m = "line 1, column 5: `@Cd` cannot be assigned a float";
        assert_eq!(error_position(m).map(|p| p.message), Some(m.to_string()));
    }

    #[test]
    fn a_word_ending_in_line_is_not_a_line() {
        // The browser's regular expression anchored on a word boundary and
        // this scan has to as well, or a message mentioning an underline
        // or a baseline names a row it never meant to.
        assert_eq!(error_position("underline 3 of the guide"), None);
        assert_eq!(error_position("baseline 2 is wrong"), None);
    }

    #[test]
    fn keeps_looking_past_a_line_that_names_no_number() {
        // A boundary-matching word followed by prose is not a position, and
        // stopping there would drop the real one further along.
        let p = error_position("line breaks are fine, line 4: x").expect("a position");
        assert_eq!(p.line, 4);
    }

    #[test]
    fn a_number_no_address_space_could_hold_is_not_a_position() {
        // The digits are read from a message, not from the engine's own
        // integer, so nothing bounds them but this.
        assert_eq!(error_position("line 999999999999999999999999: x"), None);
    }

    #[test]
    fn the_position_the_engine_already_holds_agrees_with_the_decoded_one() {
        // The whole reason this decoder is temporary: the engine has the
        // pair structurally and formats it away. Pinning them together
        // here means the day the formatting changes, this fails rather
        // than the underline quietly landing in the wrong place.
        let src = "a;\nbb;\nccc";
        let err = solarxy_graph::expr::ExprError::new("x", 7..8);
        let (line, col) = err.line_col(src);
        let formatted = format!("line {line}, column {col}: {err}");
        let decoded = error_position(&formatted).expect("a position");
        assert_eq!((decoded.line, decoded.column), (line, col));
    }
}
