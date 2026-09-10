//! The list view: the same graph, read as rows.
//!
//! **The same selection and the same operations as the canvas**, which is
//! the criterion and also the reason this is a presentation rather than a
//! second surface: it reads the document the canvas reads and raises the
//! actions the canvas raises. Nothing here can do anything the canvas
//! cannot, and nothing the canvas can do is missing.
//!
//! It exists because a graph is sometimes a list. Finding one node among
//! sixty is a scan, not a search of a plane, and a status column reads at
//! a glance where sixty scattered badges do not.

use std::collections::BTreeMap;

use egui::Ui;
use solarxy_graph::cook::state::CookState;
use solarxy_graph::document::{GraphContext, NodeId};

use super::seed::NodeCook;
use crate::gui::intent::Intents;
use crate::gui::theme::Theme;

/// What a row's action column asked for, resolved by the caller so the
/// list and the ring end up in the same handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowAction {
    Select(NodeId),
    Rename(NodeId),
    Dive(NodeId),
    Info(NodeId),
    Bypass(NodeId, bool),
    Delete(NodeId),
}

/// Draw the rows, and answer what one of them asked for.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ui: &mut Ui,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    cook: &BTreeMap<NodeId, NodeCook>,
    intents: &mut Intents,
    theme: Theme,
) -> Option<RowAction> {
    let Ok(graph) = doc.graph(ctx) else {
        return None;
    };
    let mut action = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new(ui.id().with("node-list"))
            .num_columns(5)
            .striped(true)
            .show(ui, |ui| {
                for column in ["Node", "Type", "Info", "Status", ""] {
                    ui.label(egui::RichText::new(column).color(theme.muted).size(10.0));
                }
                ui.end_row();

                for node in graph.nodes() {
                    let desc = registry.get(&node.type_id);
                    let selected = graph.selection.contains(&node.id);
                    let name = solarxy_graph::naming::node_name(node, registry);
                    let facts = cook.get(&node.id).cloned().unwrap_or_default();

                    if ui.selectable_label(selected, name).clicked() {
                        action = Some(RowAction::Select(node.id));
                    }
                    ui.label(
                        egui::RichText::new(&node.type_id)
                            .color(theme.muted)
                            .size(10.0),
                    );
                    ui.label(
                        egui::RichText::new(
                            desc.and_then(|d| {
                                solarxy_studio::node::node_info_line(d, &node.params, None)
                            })
                            .unwrap_or_default(),
                        )
                        .color(theme.accent)
                        .size(10.0),
                    );
                    let (status, colour) = status_of(&facts, node.bypassed, theme);
                    ui.label(egui::RichText::new(status).color(colour).size(10.0));

                    ui.horizontal(|ui| {
                        // The same six the ring offers, in the same
                        // order, and disabled for the same reasons: a
                        // user who learns one surface has learned both.
                        if ui.small_button("ab").on_hover_text("Rename").clicked() {
                            action = Some(RowAction::Rename(node.id));
                        }
                        let container = super::viewer::opens_a_network(registry, &node.type_id);
                        if ui
                            .add_enabled(container, egui::Button::new("in").small())
                            .on_hover_text(if container {
                                "Enter subflow"
                            } else {
                                "Not a container"
                            })
                            .clicked()
                        {
                            action = Some(RowAction::Dive(node.id));
                        }
                        if ui.small_button("i").on_hover_text("Node info").clicked() {
                            action = Some(RowAction::Info(node.id));
                        }
                        let bypassable = desc.is_some_and(|d| {
                            !matches!(
                                d.bypass,
                                solarxy_graph::registry::BypassBehavior::NotBypassable
                            )
                        });
                        if ui
                            .add_enabled(bypassable, egui::Button::new("byp").small())
                            .on_hover_text(if bypassable {
                                "Toggle bypass"
                            } else {
                                "Not bypassable"
                            })
                            .clicked()
                        {
                            action = Some(RowAction::Bypass(node.id, !node.bypassed));
                        }
                        if ui
                            .small_button("del")
                            .on_hover_text("Delete node")
                            .clicked()
                        {
                            action = Some(RowAction::Delete(node.id));
                        }
                    });
                    ui.end_row();
                }
            });
    });
    let _ = intents;
    action
}

/// What a row says about its node's last cook, in the same vocabulary the
/// canvas paints.
fn status_of(cook: &NodeCook, bypassed: bool, theme: Theme) -> (String, egui::Color32) {
    if cook.error.is_some() {
        return ("error".to_string(), theme.severity_error);
    }
    if bypassed {
        return ("bypassed".to_string(), theme.severity_warn);
    }
    match cook.state {
        CookState::Pending(_) => ("cooking".to_string(), theme.muted),
        CookState::Dirty => ("stale".to_string(), theme.severity_warn),
        #[allow(clippy::cast_precision_loss)]
        CookState::Clean if cook.last_us > 0 => (
            format!("{:.1} ms", cook.last_us as f64 / 1000.0),
            theme.muted,
        ),
        CookState::Clean => (String::new(), theme.muted),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::preferences::ThemeChoice;

    fn theme() -> Theme {
        Theme::from_choice(ThemeChoice::default())
    }

    /// A failing cook outranks everything: a node that is bypassed and
    /// also erroring is a node whose error a user needs to see.
    #[test]
    fn a_failing_cook_is_what_a_row_says_first() {
        let failing = NodeCook {
            error: Some("no".to_string()),
            ..NodeCook::default()
        };
        assert_eq!(status_of(&failing, true, theme()).0, "error");
    }

    /// Stale and cooking are different words because they are different
    /// states: one is waiting to be asked, the other is already working.
    #[test]
    fn stale_and_cooking_do_not_read_the_same() {
        let stale = NodeCook {
            state: CookState::Dirty,
            ..NodeCook::default()
        };
        let cooking = NodeCook {
            state: CookState::Pending(1),
            ..NodeCook::default()
        };
        assert_eq!(status_of(&stale, false, theme()).0, "stale");
        assert_eq!(status_of(&cooking, false, theme()).0, "cooking");
    }

    /// A clean node that has never cooked says nothing rather than
    /// claiming a duration it does not have, and one that has says how
    /// long it took.
    #[test]
    fn a_clean_node_says_its_duration_or_nothing() {
        let never = NodeCook {
            state: CookState::Clean,
            ..NodeCook::default()
        };
        assert_eq!(status_of(&never, false, theme()).0, "");
        let cooked = NodeCook {
            state: CookState::Clean,
            last_us: 2_500,
            ..NodeCook::default()
        };
        assert_eq!(status_of(&cooked, false, theme()).0, "2.5 ms");
    }

    /// A node the cook map has never heard of reads as stale, and that is
    /// the engine's own default rather than this surface inventing one: a
    /// node that has just been added is dirty until it first cooks, so
    /// the fallback says the true thing rather than a blank.
    #[test]
    fn a_node_with_no_cook_facts_reads_as_stale() {
        assert_eq!(status_of(&NodeCook::default(), false, theme()).0, "stale");
    }
}
