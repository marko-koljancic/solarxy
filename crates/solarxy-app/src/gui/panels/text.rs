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

/// The bar's two menus, by the titles the browser gives them.
const FILE_MENU_TITLE: &str = "File";
const VIEW_MENU_TITLE: &str = "View";

/// One row of the File menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileItem {
    NewSnippet,
    DeleteSnippet,
    Divider,
    Language,
}

impl FileItem {
    /// The label as drawn, or `None` for a divider.
    const fn label(self) -> Option<&'static str> {
        match self {
            Self::NewSnippet => Some("New Snippet"),
            Self::DeleteSnippet => Some("Delete Snippet"),
            Self::Divider => None,
            Self::Language => Some("Language"),
        }
    }
}

/// The File menu, in the browser's order. A table the draw walks, so a test
/// can read the browser's menu and hold this one to the same entries.
const FILE_MENU: &[FileItem] = &[
    FileItem::NewSnippet,
    FileItem::DeleteSnippet,
    FileItem::Divider,
    FileItem::Language,
];

/// The languages a snippet can be in, as the stored value and the label.
const LANGUAGES: [(&str, &str); 2] = [("plain", "Plain"), ("wrangle", "Wrangle")];

const NO_SNIPPET: &str = "Select a snippet first";

fn draw_file_item(
    ui: &mut egui::Ui,
    item: FileItem,
    current: GraphContext,
    selected: Option<&SnippetRow>,
    state: &mut TextState,
    intents: &mut Intents,
) {
    use crate::gui::chrome::menu_items::{check_entry, entry, entry_if};

    let Some(label) = item.label() else {
        ui.separator();
        return;
    };
    match item {
        FileItem::Divider => {}
        FileItem::NewSnippet => {
            if entry(ui, label, None).clicked() {
                intents.panel(PanelIntent::Canvas(CanvasAction::AddNode(
                    current,
                    "text".to_string(),
                    [0.0, 0.0],
                )));
                ui.close();
            }
        }
        FileItem::DeleteSnippet => {
            if entry_if(ui, selected.is_some(), label, None, NO_SNIPPET).clicked()
                && let Some(row) = selected
            {
                intents.panel(PanelIntent::Canvas(CanvasAction::RemoveNodes(
                    row.ctx,
                    vec![row.node],
                )));
                state.draft = None;
                ui.close();
            }
        }
        FileItem::Language => {
            ui.add_enabled_ui(selected.is_some(), |ui| {
                ui.menu_button(label, |ui| {
                    let Some(row) = selected else {
                        return;
                    };
                    for (value, name) in LANGUAGES {
                        if check_entry(ui, row.language == value, name, None).clicked() {
                            if row.language != value {
                                intents.panel(PanelIntent::Canvas(CanvasAction::SetParams(
                                    row.ctx,
                                    row.node,
                                    vec![(
                                        LANGUAGE_KEY.to_string(),
                                        ParamSource::Literal(ParamValue::Enum(value.to_string())),
                                    )],
                                )));
                            }
                            ui.close();
                        }
                    }
                })
                .response
                .on_disabled_hover_text(NO_SNIPPET);
            });
        }
    }
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

    // The panel's own bar. It raises exactly the commands the strip of
    // buttons it replaced raised, so nothing about what a snippet is or how
    // it is stored moves with it.
    crate::gui::chrome::panel_bar::panel_bar(ui, theme, |ui| {
        ui.menu_button(FILE_MENU_TITLE, |ui| {
            for item in FILE_MENU {
                draw_file_item(ui, *item, current, selected.as_ref(), state, intents);
            }
        });
        ui.menu_button(VIEW_MENU_TITLE, |ui| {
            crate::gui::chrome::panel_bar::maximize_entry(
                ui,
                crate::gui::dock::SolarxyTab::Text,
                intents,
            );
        });
    });

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

    // ---- held against the browser's menu ----

    fn browser_source() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/TextPane.tsx"))
            .expect("the browser's text panel")
    }

    /// The stretch of the browser's source from `from` to the next `until`.
    fn block<'a>(source: &'a str, from: &str, until: &str) -> &'a str {
        let start = source.find(from).unwrap_or_else(|| panic!("no {from}"));
        let rest = &source[start..];
        &rest[..rest
            .find(until)
            .unwrap_or_else(|| panic!("no {until} after {from}"))]
    }

    /// Every quoted value of `key` in a stretch of source, in order.
    fn quoted(block: &str, key: &str) -> Vec<String> {
        block
            .split(&format!("{key}: \""))
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect()
    }

    /// The bar has the browser's two menus, by its titles and in its order.
    #[test]
    fn the_bar_has_the_browsers_two_menus() {
        let source = browser_source();
        let titles: Vec<String> = source
            .split("<MenuItem title=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect();
        assert_eq!(titles, [FILE_MENU_TITLE, VIEW_MENU_TITLE]);
    }

    /// The File menu lists the browser's entries in its order. The
    /// language rows are built from a table there rather than written as
    /// quoted labels, so what is read is exactly the top-level entries.
    #[test]
    fn the_file_menu_lists_the_browsers_entries_in_its_order() {
        let source = browser_source();
        let browser = quoted(
            block(&source, "const fileEntries: MenuEntry[] = [", "\n  ];"),
            "label",
        );
        assert!(
            browser.len() >= 3,
            "read {} entries from the browser, so the reader is broken",
            browser.len()
        );
        let here: Vec<&str> = FILE_MENU.iter().filter_map(|item| item.label()).collect();
        assert_eq!(here, browser);
    }

    /// The languages offered are the browser's, by stored value and by
    /// label. The value is what a scene file carries, so it has to match.
    #[test]
    fn the_languages_are_the_browsers_by_value_and_label() {
        let source = browser_source();
        let table = block(&source, "const LANGUAGES", "];");
        let values = quoted(table, "value");
        let labels = quoted(table, "label");
        assert_eq!(values.len(), 2, "the reader found the two languages");
        let here_values: Vec<&str> = LANGUAGES.iter().map(|(value, _)| *value).collect();
        let here_labels: Vec<&str> = LANGUAGES.iter().map(|(_, label)| *label).collect();
        assert_eq!(here_values, values);
        assert_eq!(here_labels, labels);
    }

    /// The View menu is the one entry every panel's View menu ends with.
    #[test]
    fn the_view_menu_is_the_common_maximize_entry() {
        let source = browser_source();
        let browser = quoted(
            block(&source, "const viewEntries: MenuEntry[] = [", "\n  ];"),
            "label",
        );
        assert_eq!(browser, [crate::gui::chrome::panel_bar::MAXIMIZE_LABEL]);
    }
}
