//! The node panel's own menu bar: Add, View, and the switch to rows.
//!
//! **Add is a second interpreter of the registry, beside the palette.** It
//! keeps no list of its own: the categories and the node types under each
//! come from the two functions the palette is built on, so a node type added
//! in Rust appears here, under the right category, with no change to this
//! file. `gui/panels/extensibility.rs` holds that with a fabricated type.
//!
//! **View re-presents what the keys already do.** Every entry fills the same
//! [`ChromeRequest`] the canvas's own bindings fill, so a menu entry and the
//! key it shows cannot come to mean different things.

use egui::{Rect, Vec2, pos2};
use solarxy_core::preferences::{CanvasPrefs, WireRouting};
use solarxy_graph::document::ContextKind;
use solarxy_graph::registry::{Category, NodeTypeDescriptor, Registry};

use super::chrome::{ChromeRequest, Toggle};
use super::{glyphs, palette, viewer};
use crate::gui::chrome::menu_items::{check_entry, entry, entry_if};
use crate::gui::chrome::panel_bar::{MAXIMIZE_LABEL, maximize_entry, panel_bar};
use crate::gui::dock::SolarxyTab;
use crate::gui::intent::Intents;
use crate::gui::theme::Theme;
use crate::state::keymap::Action;

/// The side of the glyph drawn before a node type's name, the browser's.
const GLYPH_SIDE: f32 = 13.0;
const GLYPH_STROKE: f32 = 1.2;

/// How far each node added from the menu is stepped from the last, and how
/// many steps before the cascade starts over.
const ADD_STEP: f32 = 24.0;
const ADD_CYCLE: usize = 5;

/// The entry the Add menu leads with, above its category submenus. One
/// constant, so the entry and the test that holds it against the browser
/// cannot come to spell it differently.
const SEARCH_NODES: &str = "Search Nodes\u{2026}";

/// One category of the Add menu, with the node types it offers.
pub(in crate::gui::panels) struct AddGroup<'a> {
    pub category: Category,
    pub types: Vec<&'a NodeTypeDescriptor>,
}

/// What the Add menu lists for a context: its categories in the shared
/// order, and under each the node types that context will take.
#[must_use]
pub(in crate::gui::panels) fn add_groups(
    registry: &Registry,
    kind: ContextKind,
) -> Vec<AddGroup<'_>> {
    palette::categories(registry, kind)
        .into_iter()
        .map(|category| AddGroup {
            category,
            types: palette::candidates(registry, kind, Some(category), ""),
        })
        .collect()
}

/// Where a node added from the menu lands, in graph space.
///
/// At the middle of what is on screen rather than at a fixed place, because
/// a menu that always adds near the origin makes a mess of a large graph the
/// user has panned away from. Each add is stepped from the last so a run of
/// them fans out instead of stacking, and the cascade starts over rather
/// than walking off the screen.
#[must_use]
pub(super) fn menu_placement(center: [f32; 2], nth: usize) -> [f32; 2] {
    #[allow(clippy::cast_precision_loss)]
    let step = (nth % ADD_CYCLE) as f32 * ADD_STEP;
    [center[0] + step, center[1] + step]
}

/// What the bar asked for this frame.
#[derive(Default)]
pub(super) struct BarRequest {
    pub chrome: ChromeRequest,
    /// Open the palette, from the Add menu's search entry.
    pub palette: bool,
    /// Add this node type.
    pub add: Option<String>,
    pub routing: Option<WireRouting>,
}

/// Draw the bar across the top of the panel.
///
/// `kind` is `None` when the context the panel is looking at has no graph,
/// in which case Add offers only the search.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_bar(
    ui: &mut egui::Ui,
    registry: &Registry,
    kind: Option<ContextKind>,
    prefs: CanvasPrefs,
    list_view: bool,
    has_selection: bool,
    intents: &mut Intents,
    theme: Theme,
) -> BarRequest {
    let mut request = BarRequest::default();
    panel_bar(ui, theme, |ui| {
        ui.menu_button("Add", |ui| draw_add(ui, registry, kind, &mut request));
        ui.menu_button("View", |ui| {
            draw_view(ui, prefs, has_selection, intents, &mut request);
        });
        // The switch between the graph and the rows sits at the far end, as
        // the browser's does, and names the view a click switches to.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, hover) = if list_view {
                ("Graph", "Read the graph as a graph")
            } else {
                ("Rows", "Read the graph as a list")
            };
            if ui.small_button(label).on_hover_text(hover).clicked() {
                request.chrome.view = true;
            }
        });
    });
    request
}

fn draw_add(
    ui: &mut egui::Ui,
    registry: &Registry,
    kind: Option<ContextKind>,
    request: &mut BarRequest,
) {
    if entry(ui, SEARCH_NODES, Some(Action::OpenNodePalette)).clicked() {
        request.palette = true;
        ui.close();
    }
    let Some(kind) = kind else {
        return;
    };
    ui.separator();
    for group in add_groups(registry, kind) {
        ui.menu_button(group.category.display_name(), |ui| {
            for desc in group.types {
                if type_entry(ui, desc).clicked() {
                    request.add = Some(desc.type_id.to_string());
                    ui.close();
                }
            }
        });
    }
}

/// A node type's row: its glyph, then its display name.
///
/// The label leaves room and the glyph is painted into it afterwards,
/// because the glyphs are stroked paths rather than textures and a button
/// takes only a texture for an icon.
fn type_entry(ui: &mut egui::Ui, desc: &NodeTypeDescriptor) -> egui::Response {
    let response = ui.add(egui::Button::new(format!("      {}", desc.display_name)));
    let chip = Rect::from_center_size(
        pos2(response.rect.left() + GLYPH_SIDE, response.rect.center().y),
        Vec2::splat(GLYPH_SIDE),
    );
    let ink = ui.style().interact(&response).text_color();
    glyphs::paint(
        ui.painter(),
        &viewer::glyph_key(Some(desc)),
        chip,
        ink,
        GLYPH_STROKE,
    );
    response.on_hover_text(desc.doc)
}

/// One row of the View menu, in the order it is drawn. A divider is a row
/// like any other, so the table is the menu rather than a list of its
/// entries, and [`draw_view`] walks it rather than writing the entries out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewRow {
    Divider,
    Grid,
    Snap,
    Minimap,
    Controls,
    ConnectionStyle,
    AutoLayout,
    NodeInfo,
    FitGraph,
    Maximize,
}

impl ViewRow {
    /// `None` is the divider, which is the one row with nothing to say.
    const fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Divider => return None,
            Self::Grid => "Canvas Grid",
            Self::Snap => "Snap to Grid",
            Self::Minimap => "Minimap",
            Self::Controls => "Canvas Controls",
            Self::ConnectionStyle => "Connection Style",
            Self::AutoLayout => "Auto-Layout",
            Self::NodeInfo => "Node Info",
            Self::FitGraph => "Fit Graph",
            Self::Maximize => MAXIMIZE_LABEL,
        })
    }

    /// The binding whose key the row shows, read from the table rather than
    /// typed beside the entry.
    const fn action(self) -> Option<Action> {
        match self {
            Self::Grid => Some(Action::CanvasGrid),
            Self::Minimap => Some(Action::CanvasMinimap),
            Self::Controls => Some(Action::CanvasControls),
            Self::AutoLayout => Some(Action::AutoLayout),
            Self::NodeInfo => Some(Action::NodeInfo),
            Self::FitGraph => Some(Action::CanvasFit),
            Self::Maximize => Some(Action::PanelMaximize),
            Self::Divider | Self::Snap | Self::ConnectionStyle => None,
        }
    }

    /// The canvas toggle a checked row writes, for the four that are one.
    const fn toggle(self) -> Option<Toggle> {
        match self {
            Self::Grid => Some(Toggle::Grid),
            Self::Snap => Some(Toggle::Snap),
            Self::Minimap => Some(Toggle::Minimap),
            Self::Controls => Some(Toggle::Controls),
            _ => None,
        }
    }
}

use ViewRow::Divider;

const VIEW_MENU: &[ViewRow] = &[
    ViewRow::Grid,
    ViewRow::Snap,
    ViewRow::Minimap,
    ViewRow::Controls,
    Divider,
    ViewRow::ConnectionStyle,
    Divider,
    ViewRow::AutoLayout,
    Divider,
    ViewRow::NodeInfo,
    Divider,
    ViewRow::FitGraph,
    ViewRow::Maximize,
];

fn draw_view(
    ui: &mut egui::Ui,
    prefs: CanvasPrefs,
    has_selection: bool,
    intents: &mut Intents,
    request: &mut BarRequest,
) {
    for row in VIEW_MENU {
        let Some(label) = row.label() else {
            ui.separator();
            continue;
        };
        let action = row.action();
        if let Some(toggle) = row.toggle() {
            let on = match toggle {
                Toggle::Grid => prefs.grid,
                Toggle::Snap => prefs.snap,
                Toggle::Minimap => prefs.minimap,
                Toggle::Controls => prefs.controls,
            };
            if check_entry(ui, on, label, action).clicked() {
                request.chrome.toggled = Some(toggle);
                ui.close();
            }
            continue;
        }
        match row {
            ViewRow::ConnectionStyle => {
                ui.menu_button(label, |ui| {
                    for routing in WireRouting::ALL {
                        if check_entry(ui, prefs.routing == routing, routing.label(), None)
                            .clicked()
                        {
                            request.routing = Some(routing);
                            ui.close();
                        }
                    }
                });
            }
            ViewRow::AutoLayout => {
                if entry(ui, label, action).clicked() {
                    request.chrome.layout = true;
                    ui.close();
                }
            }
            ViewRow::NodeInfo => {
                if entry_if(ui, has_selection, label, action, "Select a node first").clicked() {
                    request.chrome.info = true;
                    ui.close();
                }
            }
            ViewRow::FitGraph => {
                if entry(ui, label, action).clicked() {
                    request.chrome.fit = true;
                    ui.close();
                }
            }
            ViewRow::Maximize => maximize_entry(ui, SolarxyTab::Nodes, intents),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::keymap::hint;

    fn registry() -> Registry {
        solarxy_graph::nodes::builtin_registry().expect("builtin registry")
    }

    /// An entry the browser has under this menu and this shell does not,
    /// with the reason. Checked in reverse as well, so a row that stops
    /// applying fails rather than lingering.
    const BROWSER_ONLY: &[(&str, &str)] = &[(
        "Auto-Layout (ELK)",
        "the browser offers two layout engines and lazily imports the larger; this shell has no payload budget and therefore no reason for two",
    )];

    /// An entry both have under different words: here, there, and why.
    const WORDED_DIFFERENTLY: &[(&str, &str, &str)] = &[(
        "Auto-Layout",
        "Auto-Layout (Dagre)",
        "the browser names the engine because it offers two; with one there is nothing to tell apart",
    )];

    fn browser_source(file: &str) -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/menu").join(file))
            .unwrap_or_else(|_| panic!("the browser's {file}"))
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

    /// The browser's entry list from one of its menu components. Its submenu
    /// rows are built from tables rather than written as quoted labels, so
    /// what is read is exactly its top-level entries.
    fn browser_entries(source: &str, decl: &str) -> Vec<String> {
        let start = source
            .find(decl)
            .unwrap_or_else(|| panic!("no entry list declared {decl}"));
        let block = &source[start..];
        let block = &block[..block.find("\n  ];").expect("the list's end")];
        labels(block)
    }

    /// The browser's entries as `(label, binding id)` pairs, the id being
    /// the binding it asks for the key beside the entry, or `None` where it
    /// shows none. Each entry runs from its own label to the next one, so an
    /// id is attributed to the entry it was written under.
    ///
    /// It asks its own table by id rather than spelling a key, so what is
    /// compared is which binding each shell names. Comparing the rendered
    /// key would compare two platform formatters instead.
    fn browser_entries_with_keys(source: &str, decl: &str) -> Vec<(String, Option<String>)> {
        let start = source
            .find(decl)
            .unwrap_or_else(|| panic!("no entry list declared {decl}"));
        let block = &source[start..];
        let block = &block[..block.find("\n  ];").expect("the list's end")];
        block
            .split("label: \"")
            .skip(1)
            .filter_map(|rest| {
                let label = rest.split('"').next()?.to_string();
                let body = rest.split("label: \"").next().unwrap_or(rest);
                let key = body
                    .split("shortcut: menuHint(\"")
                    .nth(1)
                    .and_then(|k| k.split('"').next())
                    .map(str::to_string);
                Some((label, key))
            })
            .collect()
    }

    /// The same entries in the same order as the browser's View menu, once
    /// the named differences are taken out and put back in its words.
    #[test]
    fn the_view_menu_lists_the_browsers_entries_in_its_order() {
        let source = browser_source("NodePaneViewMenu.tsx");
        let browser: Vec<String> = browser_entries(&source, "const entries: MenuEntry[] = [")
            .into_iter()
            .filter(|label| !BROWSER_ONLY.iter().any(|(only, _)| only == label))
            .collect();
        assert!(
            browser.len() >= 8,
            "read {} entries from the browser, so the reader is broken",
            browser.len()
        );

        let here: Vec<String> = VIEW_MENU
            .iter()
            .filter_map(|row| row.label())
            .map(|label| {
                let label = label.replace('\u{2026}', "...");
                WORDED_DIFFERENTLY
                    .iter()
                    .find(|(here, _, _)| *here == label)
                    .map_or(label, |(_, there, _)| (*there).to_string())
            })
            .collect();
        assert_eq!(here, browser);
    }

    /// Every row of the two lists still applies: the browser really has the
    /// entry this shell does not, and the pair worded differently is really
    /// spelled each way on its own side.
    #[test]
    fn every_named_difference_is_still_a_difference() {
        let source = browser_source("NodePaneViewMenu.tsx");
        let there = browser_entries(&source, "const entries: MenuEntry[] = [");
        let here: Vec<String> = VIEW_MENU
            .iter()
            .filter_map(|row| row.label())
            .map(|label| label.replace('\u{2026}', "..."))
            .collect();
        for (label, _) in BROWSER_ONLY {
            assert!(
                there.iter().any(|l| l == label),
                "{label} is not in the browser"
            );
            assert!(
                !here.iter().any(|l| l == label),
                "{label} is in this menu too"
            );
        }
        for (ours, theirs, _) in WORDED_DIFFERENTLY {
            assert!(here.iter().any(|l| l == ours), "{ours} is not in this menu");
            assert!(
                there.iter().any(|l| l == theirs),
                "{theirs} is not in the browser"
            );
        }
    }

    /// The connection styles are the browser's, by label and in its order,
    /// read from the table its submenu is drawn from.
    #[test]
    fn the_connection_styles_are_the_browsers() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        let source =
            std::fs::read_to_string(root.join("web/src/store/ui.ts")).expect("the browser's store");
        let start = source
            .find("EDGE_STYLE_LABELS: Record<EdgeStyle, string> = {")
            .expect("the browser's edge style labels");
        let block = &source[start..];
        let block = &block[..block.find("};").expect("the table's end")];
        let browser: Vec<String> = block
            .split(": \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(str::to_string)
            .collect();
        assert_eq!(browser.len(), 4, "the reader found the browser's four");

        let here: Vec<&str> = WireRouting::ALL.iter().map(|r| r.label()).collect();
        assert_eq!(here, browser);
    }

    /// The Add menu leads with the browser's search entry, showing the key
    /// the binding table gives it, which is the browser's too.
    #[test]
    fn the_add_menu_leads_with_the_browsers_search_entry() {
        let source = browser_source("NodesMenu.tsx");
        let entries = browser_entries(&source, "const entries: MenuEntry[] = [");
        let first = entries.first().expect("the browser's first Add entry");
        assert_eq!(SEARCH_NODES.replace('\u{2026}', "..."), *first);

        let named = source
            .split("shortcut: menuHint(\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the browser names a binding for it");
        assert_eq!(Action::OpenNodePalette.id(), named);
        assert!(
            hint(Action::OpenNodePalette).is_some(),
            "and it is bound here"
        );
    }

    /// Every row that names a binding shows the key the table gives it, so a
    /// hint here cannot name a key that does nothing.
    #[test]
    fn every_view_row_that_names_a_binding_shows_the_tables_key() {
        for row in VIEW_MENU {
            if let Some(action) = row.action() {
                assert!(
                    hint(action).is_some(),
                    "{:?} names an unbound action",
                    row.label()
                );
            }
        }
    }

    /// Each row shows the same key the browser shows beside the same entry,
    /// with the one disagreement named and explained. This shell reads its
    /// binding table and the browser types each hint by hand, which is how
    /// the two come apart; the check is what notices when they do.
    #[test]
    fn the_view_menu_shows_the_browsers_keys() {
        let source = browser_source("NodePaneViewMenu.tsx");
        let browser = browser_entries_with_keys(&source, "const entries: MenuEntry[] = [");
        assert!(
            browser.iter().filter(|(_, key)| key.is_some()).count() >= 5,
            "the reader found no keys, so it is broken"
        );

        for row in VIEW_MENU {
            let Some(label) = row.label() else { continue };
            let spelled = WORDED_DIFFERENTLY
                .iter()
                .find(|(here, _, _)| *here == label)
                .map_or(label.to_string(), |(_, there, _)| (*there).to_string());
            let Some((_, theirs)) = browser.iter().find(|(l, _)| *l == spelled) else {
                continue;
            };
            let ours = row.action().map(Action::id);
            assert_eq!(ours, theirs.as_deref(), "the binding named beside {label}");
        }
    }

    /// The Add menu is the palette's two functions and nothing else: every
    /// category the context offers, in the shared order, each holding
    /// exactly the types the palette offers under it.
    #[test]
    fn add_lists_what_the_palette_offers_in_the_shared_order() {
        let registry = registry();
        for kind in [
            ContextKind::Obj,
            ContextKind::Sop,
            ContextKind::Mat,
            ContextKind::Cop,
        ] {
            let groups = add_groups(&registry, kind);
            let categories: Vec<Category> = groups.iter().map(|g| g.category).collect();
            assert_eq!(categories, palette::categories(&registry, kind));
            for group in &groups {
                assert!(!group.types.is_empty(), "no empty submenu");
                assert!(
                    group
                        .types
                        .iter()
                        .all(|d| d.category == group.category && d.contexts.contains(kind)),
                    "a type under the wrong category or in the wrong context"
                );
            }
            let listed: usize = groups.iter().map(|g| g.types.len()).sum();
            assert_eq!(
                listed,
                palette::candidates(&registry, kind, None, "").len(),
                "every type the context takes is listed once"
            );
        }
    }

    /// A node added from the menu lands where the user is looking, and a
    /// run of them fans out and then starts over.
    ///
    /// Exact comparison on purpose: the placement is a whole-pixel step
    /// added to a whole-pixel centre, so any drift is a real change.
    #[test]
    #[allow(clippy::float_cmp)]
    fn a_menu_add_lands_in_view_and_fans_out() {
        let center = [400.0, -120.0];
        assert_eq!(menu_placement(center, 0), center);
        assert_eq!(menu_placement(center, 1), [424.0, -96.0]);
        assert_eq!(menu_placement(center, 4), [496.0, -24.0]);
        assert_eq!(menu_placement(center, 5), center, "the cascade starts over");
    }
}
