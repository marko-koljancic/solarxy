//! The Text panel: every snippet in the document, with an editor.
//!
//! **A view of the document, not a second place text lives.** The list is
//! the shared rule `solarxy_studio::text::snippets`, folded from every
//! context each frame the tab is up; the editor writes the node's own
//! `body` parameter, through the draft-and-commit contract the parameter
//! panel's rows follow, so an edit is one command and one undo step.
//! Creating a snippet creates a `text` node in the network the user is
//! looking at, and deleting one removes the node; the panel holds nothing
//! of its own but which row is selected.
//!
//! **No error is ever marked here**, and that is the browser's behaviour
//! too: a text node cooks no program, so it has no cook error to point at.
//! The line marking lives on the parameter panel's snippet control, where
//! a wrangle's program does cook. The completion vocabulary is offered when
//! the snippet's language is wrangle, with no attribute lanes, because a
//! snippet has no input geometry whose lanes it could read.

use solarxy_graph::document::{Document, GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::registry::Registry;
use solarxy_studio::text::{BODY_KEY, LANGUAGE_KEY, SnippetRow, context_chip, snippets};

use super::nodes::CanvasAction;
use super::params::{Draft, draft_step, shown_text};
use crate::gui::code_editor::code_editor;
use crate::gui::intent::{Intents, PanelIntent};
use crate::gui::theme::Theme;

/// What the panel draws.
#[derive(Clone, Copy)]
pub(crate) enum TextSource<'a> {
    Empty,
    Scene {
        doc: &'a Document,
        registry: &'a Registry,
        /// The graph the user is looking at, where a new snippet lands.
        current: GraphContext,
    },
}

/// The panel's own state: the selected row and the edit in flight.
#[derive(Default)]
pub(crate) struct TextState {
    selected: Option<NodeId>,
    draft: Option<Draft>,
}

impl TextState {
    pub(crate) fn reset(&mut self) {
        self.selected = None;
        self.draft = None;
    }
}

/// The row to edit: the selected one, else the first, as the browser's
/// list falls back when its selection is deleted.
pub(super) fn selected_row(rows: &[SnippetRow], selected: Option<NodeId>) -> Option<&SnippetRow> {
    selected
        .and_then(|id| rows.iter().find(|r| r.node == id))
        .or_else(|| rows.first())
}

/// How many rows the editor shows at least: the browser's `minLines`.
const MIN_LINES: usize = 20;

/// Render the Text panel into `ui`.
pub(in crate::gui) fn draw_text_content(
    ui: &mut egui::Ui,
    source: TextSource<'_>,
    state: &mut TextState,
    intents: &mut Intents,
    theme: Theme,
) {
    let TextSource::Scene {
        doc,
        registry,
        current,
    } = source
    else {
        return placeholder(ui, "No scene yet.", theme);
    };
    let rows = snippets(doc, registry);
    let selected = selected_row(&rows, state.selected).cloned();

    // The strip: new, delete, and the language of the selected snippet.
    ui.horizontal(|ui| {
        if ui.button("New Snippet").clicked() {
            intents.panel(PanelIntent::Canvas(CanvasAction::AddNode(
                current,
                "text".to_string(),
                [0.0, 0.0],
            )));
        }
        if ui
            .add_enabled(selected.is_some(), egui::Button::new("Delete Snippet"))
            .clicked()
            && let Some(row) = &selected
        {
            intents.panel(PanelIntent::Canvas(CanvasAction::RemoveNodes(
                row.ctx,
                vec![row.node],
            )));
            state.draft = None;
        }
        if let Some(row) = &selected {
            ui.separator();
            ui.label(
                egui::RichText::new("Language")
                    .color(theme.muted)
                    .size(10.0),
            );
            for (value, label) in [("plain", "Plain"), ("wrangle", "Wrangle")] {
                if ui.selectable_label(row.language == value, label).clicked()
                    && row.language != value
                {
                    intents.panel(PanelIntent::Canvas(CanvasAction::SetParams(
                        row.ctx,
                        row.node,
                        vec![(
                            LANGUAGE_KEY.to_string(),
                            ParamSource::Literal(ParamValue::Enum(value.to_string())),
                        )],
                    )));
                }
            }
        }
    });
    ui.separator();

    if rows.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No snippets yet.").weak());
            ui.horizontal(|ui| {
                if ui.link("Create one").clicked() {
                    intents.panel(PanelIntent::Canvas(CanvasAction::AddNode(
                        current,
                        "text".to_string(),
                        [0.0, 0.0],
                    )));
                }
                ui.label(
                    egui::RichText::new("to keep a wrangle program or a note with the scene.")
                        .color(theme.muted)
                        .size(10.0),
                );
            });
        });
        return;
    }

    ui.horizontal_top(|ui| {
        // The list.
        ui.vertical(|ui| {
            ui.set_width(160.0);
            egui::ScrollArea::vertical()
                .id_salt("text-list")
                .show(ui, |ui| {
                    for row in &rows {
                        let is_selected = selected.as_ref().is_some_and(|s| s.node == row.node);
                        ui.horizontal(|ui| {
                            if ui.selectable_label(is_selected, &row.label).clicked() {
                                state.selected = Some(row.node);
                                state.draft = None;
                            }
                            ui.label(
                                egui::RichText::new(context_chip(row.ctx))
                                    .color(theme.muted)
                                    .size(9.0),
                            );
                        });
                    }
                });
        });
        ui.separator();
        // The editor.
        ui.vertical(|ui| {
            let Some(row) = &selected else {
                return placeholder(ui, "Select a snippet to edit it.", theme);
            };
            let mut buffer =
                shown_text(state.draft.as_ref(), row.node, BODY_KEY, &row.body).to_string();
            let vocabulary: Option<&[(String, String)]> =
                (row.language == "wrangle").then_some(&[][..]);
            let out = code_editor(
                ui,
                egui::Id::new(("text-body", row.node)),
                &mut buffer,
                MIN_LINES,
                None,
                vocabulary,
                theme,
            );
            if let Some(body) = draft_step(
                ui,
                &mut state.draft,
                row.node,
                BODY_KEY,
                &row.body,
                true,
                out.changed,
                &out.response,
                buffer,
            ) {
                intents.panel(PanelIntent::Canvas(CanvasAction::SetParams(
                    row.ctx,
                    row.node,
                    vec![(
                        BODY_KEY.to_string(),
                        ParamSource::Literal(ParamValue::Text(body)),
                    )],
                )));
            }
        });
    });
}

fn placeholder(ui: &mut egui::Ui, text: &str, theme: Theme) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).color(theme.muted));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(node: u64, label: &str) -> SnippetRow {
        SnippetRow {
            ctx: GraphContext::Root,
            node: NodeId(node),
            label: label.to_string(),
            language: "plain".to_string(),
            body: String::new(),
        }
    }

    /// The selection survives its row: a deleted selection falls back to
    /// the first row, and an empty list to nothing.
    #[test]
    fn the_selection_falls_back_to_the_first_row() {
        let rows = vec![row(1, "a"), row(2, "b")];
        assert_eq!(
            selected_row(&rows, Some(NodeId(2))).map(|r| r.node),
            Some(NodeId(2))
        );
        assert_eq!(
            selected_row(&rows, Some(NodeId(9))).map(|r| r.node),
            Some(NodeId(1))
        );
        assert_eq!(selected_row(&rows, None).map(|r| r.node), Some(NodeId(1)));
        assert!(selected_row(&[], Some(NodeId(1))).is_none());
    }
}
