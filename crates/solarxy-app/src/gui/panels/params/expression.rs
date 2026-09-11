//! The expression lane: driving a parameter with a program instead of a
//! value.
//!
//! A parameter of an expressible type is in one of two states, and the `=`
//! beside its label moves between them. A literal draws its ordinary
//! control; an expression draws a fixed-width field, the value it
//! currently resolves to underneath it, and the parser's complaint when it
//! does not resolve.
//!
//! ## Three rules that look like details and are not
//!
//! **Switching on seeds from the value the parameter already had**, spelled
//! the way the grammar spells it. A blank expression is a parse error, so
//! a field that opened empty would badge the node the instant anyone
//! reached for the affordance.
//!
//! **Switching off writes back the value the expression last resolved
//! to**, not the declared default. Writing the default would move the
//! object on a gesture whose whole meaning is "stop computing this and
//! keep what it says". The default is the fallback only when nothing
//! resolves, which is the honest answer when the expression is broken.
//!
//! **Switching off parks the text; the field's clear control discards
//! it.** An expression switched off and on again in one sitting comes back
//! verbatim, so the text has to live somewhere between those two clicks.
//! It does not live in the document: the scene schema is frozen and a
//! per-session convenience is not worth a schema version. So it is
//! interface memory belonging to this shell, exactly as the browser keeps
//! its own, and this is where it lives. Conflating the two controls would
//! make an accidental toggle destroy an expression someone had written.
//!
//! Escape inside the field abandons the **edit**, never the expression:
//! that is the toggle's job, and conflating them would make a mistyped
//! character destroy the whole program.
//!
//! The readout is **pulled** rather than pushed. An expression's value
//! moves when what it reads moves, which is every applied command, and
//! under a playing runtime that would be one event per expression per
//! frame. The panel asks the engine once a frame instead, for the rows
//! that are actually driven.

use std::collections::BTreeMap;

use egui::Ui;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::registry::param_spec::ParamSpec;
use solarxy_studio::expression;

use super::controls::{ControlEdit, ControlEnv};
use super::draft::{self, Draft};
use crate::gui::theme::Theme;

/// Expression text switched off and not yet discarded.
///
/// Interface memory, per session, keyed by the row it belongs to. See the
/// module docs for why it is not in the document.
pub(super) type Parked = BTreeMap<(GraphContext, NodeId, String), String>;

/// What a row's parameter currently resolves to, when it is driven by an
/// expression. `Err` carries the parser's message verbatim, because that
/// message is what the reader has to act on and paraphrasing it loses the
/// position.
pub(crate) type Resolved = BTreeMap<String, Result<ParamValue, String>>;

/// Where a row's parked text lives.
///
/// One function rather than two composed keys, because the store is
/// written when a row switches off and read when it switches on, and two
/// spellings of the same key would lose the text between those two clicks
/// with nothing to notice: parking would appear to work and the round
/// trip would silently seed from the value instead.
#[must_use]
pub(super) fn park_key(
    ctx: GraphContext,
    node: NodeId,
    key: &str,
) -> (GraphContext, NodeId, String) {
    (ctx, node, key.to_string())
}

/// Whether this row offers the `=` at all.
#[must_use]
pub(in crate::gui::panels) fn offers_toggle(spec: &ParamSpec) -> bool {
    expression::accepts_expression(&spec.ty)
}

/// The expression driving this row, if one is.
#[must_use]
pub(super) fn driving(stored: Option<&ParamSource>) -> Option<&str> {
    expression::param_expression(stored)
}

/// The source the `=` writes when it switches a row **on**.
///
/// Parked text where there is some, else the current value spelled as an
/// expression.
#[must_use]
pub(super) fn switch_on(parked: Option<&str>, current: &ParamValue) -> ParamSource {
    ParamSource::Expression {
        expr: parked.map_or_else(|| expression::seed_expression(current), ToString::to_string),
    }
}

/// The value the `=` writes when it switches a row **off**.
///
/// What the expression last resolved to. The declared default only when
/// nothing resolves, which is the honest answer for an expression that
/// does not parse: there is no last value to keep.
#[must_use]
pub(super) fn switch_off(
    resolved: Option<&Result<ParamValue, String>>,
    spec: &ParamSpec,
) -> ParamValue {
    match resolved {
        Some(Ok(value)) => value.clone(),
        _ => spec.default.clone(),
    }
}

/// What the readout under the field says, and whether it is a complaint.
#[must_use]
pub(super) fn readout(resolved: Option<&Result<ParamValue, String>>) -> (String, bool) {
    match resolved {
        Some(Ok(value)) => (expression::format_resolved(value), false),
        Some(Err(message)) => (message.clone(), true),
        // Nothing to say rather than a guess: a row whose node has not
        // cooked has no resolved value and inventing one would read as a
        // number the document does not hold.
        None => (String::new(), false),
    }
}

/// What the toggle beside a label asked for.
pub(super) enum Toggle {
    /// Switch this row to an expression.
    On,
    /// Switch it back to a value, keeping the text for the rest of the
    /// session.
    Off,
}

/// Draw the `=` beside a label.
pub(super) fn draw_toggle(ui: &mut Ui, active: bool, theme: Theme) -> Option<Toggle> {
    let label = egui::RichText::new("=")
        .monospace()
        .size(10.0)
        .color(if active { theme.accent } else { theme.muted });
    let hint = if active {
        "Show the value instead, keeping the expression"
    } else {
        "Drive this with an expression"
    };
    ui.add(egui::Button::new(label).small().frame(active))
        .on_hover_text(hint)
        .clicked()
        .then_some(if active { Toggle::Off } else { Toggle::On })
}

/// Draw an expression row: the field, the readout, and the clear control.
pub(super) fn draw_row(
    ui: &mut Ui,
    spec: &ParamSpec,
    expr: &str,
    resolved: Option<&Result<ParamValue, String>>,
    env: &ControlEnv<'_>,
    draft: &mut Option<Draft>,
    theme: Theme,
) -> Option<ControlEdit> {
    let mut edit = None;
    let mut text = draft::shown_text(draft.as_ref(), env.node, &spec.key, expr).to_string();
    let (message, failed) = readout(resolved);

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(180.0)
                    .font(egui::TextStyle::Monospace)
                    .text_color(if failed {
                        theme.severity_error
                    } else {
                        theme.fg
                    }),
            );
            if response.changed() {
                if let Some(existing) = draft.as_mut().filter(|d| d.owns(env.node, &spec.key)) {
                    existing.set(text.clone());
                } else {
                    let mut fresh = Draft::begin(env.node, &spec.key, expr);
                    fresh.set(text.clone());
                    *draft = Some(fresh);
                }
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape))
                && (response.has_focus() || response.lost_focus())
            {
                // Abandons the edit and nothing else. Removing the
                // expression is the clear control's job.
                *draft = None;
            } else if response.lost_focus() {
                if let Some(next) = draft
                    .as_mut()
                    .filter(|d| d.owns(env.node, &spec.key))
                    .and_then(Draft::take_commit)
                {
                    edit = Some(ControlEdit::Write(vec![(
                        spec.key.clone(),
                        ParamSource::Expression { expr: next },
                    )]));
                }
                *draft = None;
            }
            if ui
                .small_button("\u{d7}")
                .on_hover_text("Remove the expression and go back to a value")
                .clicked()
            {
                *draft = None;
                edit = Some(ControlEdit::Clear(vec![spec.key.clone()]));
            }
        });
        if !message.is_empty() {
            ui.label(egui::RichText::new(message).size(9.0).color(if failed {
                theme.severity_error
            } else {
                theme.muted
            }));
        }
    });
    edit
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::registry::param_spec::ParamType;

    fn spec() -> ParamSpec {
        ParamSpec::new(
            "width",
            "Width",
            "geometry",
            ParamType::Float,
            ParamValue::Float(1.0),
        )
    }

    /// The toggle is offered per type, and the vocabulary really does
    /// split: a rule that answered the same way for everything would pass
    /// a test written on one side of it.
    #[test]
    fn the_toggle_is_offered_on_the_numeric_types_alone() {
        let mut yes = spec();
        for ty in [
            ParamType::Float,
            ParamType::Int,
            ParamType::Bool,
            ParamType::Vec2,
            ParamType::Vec3,
            ParamType::Vec4,
            ParamType::Color,
        ] {
            yes.ty = ty.clone();
            assert!(offers_toggle(&yes), "{ty:?} should offer an expression");
        }
        let mut no = spec();
        for ty in [
            ParamType::Text,
            ParamType::MultilineText,
            ParamType::AttributeName,
            ParamType::Snippet,
            ParamType::Action,
        ] {
            no.ty = ty.clone();
            assert!(!offers_toggle(&no), "{ty:?} must not offer an expression");
        }
    }

    /// A literal and an unset parameter answer the same way, because the
    /// question is whether an expression is in charge and neither is.
    #[test]
    fn only_an_expression_source_is_driving() {
        assert_eq!(driving(None), None);
        assert_eq!(
            driving(Some(&ParamSource::Literal(ParamValue::Float(2.0)))),
            None
        );
        assert_eq!(
            driving(Some(&ParamSource::Expression {
                expr: "$F".to_string()
            })),
            Some("$F")
        );
    }

    /// Switching on seeds from the current value, so the field opens on
    /// something that already resolves.
    ///
    /// A blank field is a parse error, which would badge the node the
    /// instant anyone reached for the affordance.
    #[test]
    fn switching_on_seeds_from_the_value_rather_than_from_nothing() {
        let ParamSource::Expression { expr } = switch_on(None, &ParamValue::Float(2.5)) else {
            panic!("switching on must write an expression");
        };
        assert!(!expr.is_empty(), "a blank expression is a parse error");
        assert_eq!(expr, "2.5");

        // A vector seeds as the constructor the grammar spells it with.
        let ParamSource::Expression { expr } = switch_on(None, &ParamValue::Vec3([1.0, 0.0, 2.0]))
        else {
            panic!("switching on must write an expression");
        };
        assert_eq!(expr, "set(1, 0, 2)");
    }

    /// Parked text wins over the seed, which is the whole point of parking
    /// it: an expression switched off and on again comes back verbatim.
    #[test]
    fn parked_text_comes_back_verbatim() {
        let ParamSource::Expression { expr } = switch_on(Some("$F * 0.5"), &ParamValue::Float(2.5))
        else {
            panic!("switching on must write an expression");
        };
        assert_eq!(expr, "$F * 0.5");
        assert_ne!(expr, "2.5", "the seed must not win over parked text");
    }

    /// Switching off keeps what the expression said, not what the
    /// descriptor says.
    ///
    /// Writing the default would move the object on a gesture whose whole
    /// meaning is "stop computing this and keep the answer".
    #[test]
    fn switching_off_keeps_the_last_resolved_value() {
        let spec = spec();
        assert_eq!(spec.default, ParamValue::Float(1.0));
        assert_eq!(
            switch_off(Some(&Ok(ParamValue::Float(7.25))), &spec),
            ParamValue::Float(7.25)
        );
        // A broken expression has no last value to keep, so the declared
        // default is the honest answer rather than a fabricated one.
        assert_eq!(
            switch_off(Some(&Err("line 1: bad".to_string())), &spec),
            ParamValue::Float(1.0)
        );
        assert_eq!(switch_off(None, &spec), ParamValue::Float(1.0));
    }

    /// The readout prints the value, or the parser's own message.
    #[test]
    fn the_readout_says_the_value_or_the_complaint() {
        let (text, failed) = readout(Some(&Ok(ParamValue::Vec2([1.5, 2.0]))));
        assert_eq!(text, "1.5, 2");
        assert!(!failed);

        let (text, failed) = readout(Some(&Err("line 1, column 4: unknown name".to_string())));
        assert_eq!(
            text, "line 1, column 4: unknown name",
            "the parser's message passes through, because its position is what the reader acts on"
        );
        assert!(failed);

        // A row whose node has not cooked says nothing rather than
        // inventing a number the document does not hold.
        let (text, failed) = readout(None);
        assert!(text.is_empty() && !failed);
    }

    /// The engine refuses an expression on a type that cannot carry one,
    /// which is why the toggle is gated rather than the write.
    #[test]
    fn the_engine_refuses_what_the_toggle_never_offers() {
        let mut engine = solarxy_graph::Engine::new().expect("registry builds");
        let geo = {
            let ctx = GraphContext::Root;
            engine
                .apply(solarxy_graph::Command::AddNode {
                    ctx,
                    node_type: "sopnet".to_string(),
                    position: [0.0, 0.0],
                })
                .expect("add a container");
            engine
                .document()
                .graph(ctx)
                .expect("the graph")
                .nodes()
                .next()
                .expect("the container")
                .id
        };
        let ctx = GraphContext::Subflow(geo);
        engine
            .apply(solarxy_graph::Command::AddNode {
                ctx,
                node_type: "box".to_string(),
                position: [0.0, 0.0],
            })
            .expect("add a box");
        let node = engine
            .document()
            .graph(ctx)
            .expect("the graph")
            .nodes()
            .next()
            .expect("the box")
            .id;

        // A float takes one.
        assert!(
            engine
                .apply(solarxy_graph::Command::SetParam {
                    ctx,
                    node,
                    key: "width".to_string(),
                    value: ParamSource::Expression {
                        expr: "1 + 1".to_string()
                    },
                })
                .is_ok()
        );
        // Its name does not, and the refusal is a command error rather
        // than a badge, which is why offering the toggle there would look
        // like the affordance was simply broken.
        assert!(
            engine
                .apply(solarxy_graph::Command::SetParam {
                    ctx,
                    node,
                    key: "name".to_string(),
                    value: ParamSource::Expression {
                        expr: "1 + 1".to_string()
                    },
                })
                .is_err()
        );
    }
}
