//! The viewport's own menu bar: one View menu.
//!
//! This is where the old global Render and Layout menus' viewport commands
//! go, which is what the browser does and is defensible on its own: they
//! act on the viewport, so they belong to it. The bar is drawn inside the
//! Viewport tab, so it moves with the panel and is not there when the panel
//! is closed.
//!
//! **The menu is a table, then a match.** [`VIEW_MENU`] states the entries
//! and their order as data, and the draw walks it, so a test can read the
//! browser's own menu and hold this one to the same entries in the same
//! order. An entry typed straight into the draw could drift and nothing
//! would say so.
//!
//! One entry is listed and cannot be used yet: the turntable export, which
//! waits on work this release files for later, and says so.

use solarxy_core::preferences::GizmoOrientation;
use solarxy_core::view_config::ViewLayout;

use super::menu_items::{check_entry, entry, waiting_entry};
use super::pane_toolbar::PaneView;
use super::panel_bar::{MAXIMIZE_LABEL, maximize_entry, panel_bar};
use crate::gui::dock::SolarxyTab;
use crate::gui::intent::{
    CaptureIntent, FileIntent, Intent, Intents, LayoutIntent, ToolIntent, TransportIntent,
};
use crate::gui::settings::PanelSettings;
use crate::gui::theme::Theme;
use crate::state::keymap::Action;

/// One row of the View menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    FitView,
    PaneLayout,
    GizmoOrientation,
    Playbar,
    Divider,
    Environment,
    Screenshot,
    Turntable,
    Maximize,
}

impl Item {
    /// The label as drawn, or `None` for a divider.
    const fn label(self) -> Option<&'static str> {
        match self {
            Self::FitView => Some("Fit View"),
            Self::PaneLayout => Some("Pane Layout"),
            Self::GizmoOrientation => Some("Gizmo Orientation"),
            Self::Playbar => Some("Playbar"),
            Self::Divider => None,
            Self::Environment => Some("Environment\u{2026}"),
            Self::Screenshot => Some("Save Screenshot\u{2026}"),
            Self::Turntable => Some("Export Turntable\u{2026}"),
            Self::Maximize => Some(MAXIMIZE_LABEL),
        }
    }
}

/// The View menu, in the browser's order.
const VIEW_MENU: &[Item] = &[
    Item::FitView,
    Item::PaneLayout,
    Item::GizmoOrientation,
    Item::Playbar,
    Item::Divider,
    Item::Environment,
    Item::Screenshot,
    Item::Turntable,
    Item::Divider,
    Item::Maximize,
];

/// The two frames the handles can align to, in the browser's order.
const ORIENTATIONS: [GizmoOrientation; 2] = [GizmoOrientation::World, GizmoOrientation::Local];

/// The five ways to split the viewport, with the binding each one has.
const PANE_LAYOUTS: [(ViewLayout, &str, Action); 5] = [
    (ViewLayout::Single, "Single", Action::LayoutSingle),
    (
        ViewLayout::SplitVertical,
        "Split Vertical",
        Action::LayoutSplitVertical,
    ),
    (
        ViewLayout::SplitHorizontal,
        "Split Horizontal",
        Action::LayoutSplitHorizontal,
    ),
    (ViewLayout::Quad, "Quad", Action::LayoutQuad),
    (
        ViewLayout::ThreeLeftBig,
        "Three Left Big",
        Action::LayoutThreeLeftBig,
    ),
];

/// Draw the bar across the top of the Viewport tab.
pub(in crate::gui) fn draw(
    ui: &mut egui::Ui,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
    theme: Theme,
) {
    panel_bar(ui, theme, |ui| {
        ui.menu_button("View", |ui| {
            for item in VIEW_MENU {
                draw_item(ui, *item, settings, intents);
            }
        });
    });
}

fn draw_item(ui: &mut egui::Ui, item: Item, settings: PanelSettings<'_>, intents: &mut Intents) {
    let Some(label) = item.label() else {
        ui.separator();
        return;
    };
    match item {
        Item::Divider => {}
        Item::FitView => {
            if entry(ui, label, Some(Action::FitView)).clicked() {
                intents.raise(Intent::PaneView {
                    pane: settings.active,
                    view: PaneView::Fit,
                });
                ui.close();
            }
        }
        Item::PaneLayout => {
            ui.menu_button(label, |ui| {
                for (layout, name, action) in PANE_LAYOUTS {
                    if check_entry(ui, settings.display.layout == layout, name, Some(action))
                        .clicked()
                    {
                        intents.raise(Intent::Layout(LayoutIntent::SetLayout(layout)));
                        ui.close();
                    }
                }
            });
        }
        Item::GizmoOrientation => {
            // Writes the same preference the orientation key and the
            // preferences dialog write, so the three can never disagree
            // about which frame the handles are in.
            ui.menu_button(label, |ui| {
                for orientation in ORIENTATIONS {
                    let checked = settings.tools.orientation == orientation;
                    if check_entry(ui, checked, orientation.label(), None).clicked() {
                        intents.raise(Intent::Tool(ToolIntent::SetOrientation(orientation)));
                        ui.close();
                    }
                }
            });
        }
        Item::Playbar => {
            // Writes the saved preference directly, like Gizmo Orientation
            // above: one source of truth, so the menu tick and the
            // Preferences row can never disagree.
            if check_entry(ui, settings.transport_bar, label, None).clicked() {
                intents.raise(Intent::Transport(TransportIntent::ToggleBar));
                ui.close();
            }
        }
        Item::Environment => {
            if entry(ui, label, None).clicked() {
                intents.raise(Intent::File(FileIntent::OpenEnvironment));
                ui.close();
            }
        }
        Item::Screenshot => {
            if entry(ui, label, Some(Action::Screenshot)).clicked() {
                intents.raise(Intent::Capture(CaptureIntent::Screenshot));
                ui.close();
            }
        }
        Item::Turntable => {
            waiting_entry(
                ui,
                label,
                "Turntable export is not on this shell yet; it renders as an image sequence when it arrives",
            );
        }
        Item::Maximize => maximize_entry(ui, SolarxyTab::Viewport, intents),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::keymap::hint;

    fn browser_source() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/ViewportMenuBar.tsx"))
            .expect("the browser's viewport menu")
    }

    /// Every quoted `label:` in a stretch of the browser's source, in order.
    fn labels(block: &str) -> Vec<String> {
        block
            .split("label: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect()
    }

    /// The same entries in the same order as the browser's menu. Its
    /// submenu rows are built from tables rather than written as quoted
    /// labels, so what is read here is exactly its top-level entries.
    #[test]
    fn the_view_menu_lists_the_browsers_entries_in_its_order() {
        let source = browser_source();
        let start = source
            .find("const entries: MenuEntry[] = [")
            .expect("the entry list");
        let block = &source[start..];
        let block = &block[..block.find("\n  ];").expect("the list's end")];
        let browser = labels(block);
        assert!(
            browser.len() >= 8,
            "read {} entries from the browser, so the reader is broken",
            browser.len()
        );

        let here: Vec<String> = VIEW_MENU
            .iter()
            .filter_map(|item| item.label())
            .map(|label| label.replace('\u{2026}', "..."))
            .collect();
        assert_eq!(here, browser);
    }

    /// The five pane layouts, named as the browser names them and showing
    /// the key the binding table gives each, which is the browser's too.
    #[test]
    fn the_pane_layouts_are_the_browsers_with_their_keys() {
        let source = browser_source();
        let start = source.find("const PANE_LAYOUTS").expect("the layout table");
        let block = &source[start..];
        let block = &block[..block.find("];").expect("the table's end")];
        let names = labels(block);
        let keys: Vec<String> = block
            .split("shortcut: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect();
        assert_eq!(names.len(), 5, "the reader found the five layouts");

        let here_names: Vec<&str> = PANE_LAYOUTS.iter().map(|(_, name, _)| *name).collect();
        let here_keys: Vec<String> = PANE_LAYOUTS
            .iter()
            .map(|(_, _, action)| hint(*action).expect("every layout is bound"))
            .collect();
        assert_eq!(here_names, names);
        assert_eq!(here_keys, keys);
    }

    /// The two handle frames, named as the browser names them and in its
    /// order, read from its `ORIENTATIONS` table.
    #[test]
    fn the_orientations_are_the_browsers_in_its_order() {
        let source = browser_source();
        let start = source
            .find("const ORIENTATIONS")
            .expect("the orientation table");
        let block = &source[start..];
        let block = &block[..block.find("];").expect("the table's end")];
        let names = labels(block);
        let values: Vec<String> = block
            .split("value: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect();
        assert_eq!(names.len(), 2, "the reader found the two frames");

        let here_names: Vec<&str> = ORIENTATIONS.iter().map(|o| o.label()).collect();
        let here_values: Vec<&str> = ORIENTATIONS.iter().map(|o| o.as_str()).collect();
        assert_eq!(here_names, names);
        assert_eq!(here_values, values);
    }

    /// The layouts offered are every layout there is, once each.
    #[test]
    fn every_pane_layout_is_offered_once() {
        let offered: Vec<ViewLayout> = PANE_LAYOUTS.iter().map(|(layout, _, _)| *layout).collect();
        assert_eq!(
            offered,
            [
                ViewLayout::Single,
                ViewLayout::SplitVertical,
                ViewLayout::SplitHorizontal,
                ViewLayout::Quad,
                ViewLayout::ThreeLeftBig,
            ]
        );
    }
}
