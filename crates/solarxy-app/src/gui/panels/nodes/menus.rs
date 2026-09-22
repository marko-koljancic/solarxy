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
use crate::gui::chrome::panel_bar::{maximize_entry, panel_bar};
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
    if entry(ui, "Search Nodes\u{2026}", Some(Action::OpenNodePalette)).clicked() {
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

fn draw_view(
    ui: &mut egui::Ui,
    prefs: CanvasPrefs,
    has_selection: bool,
    intents: &mut Intents,
    request: &mut BarRequest,
) {
    for (on, label, action, toggle) in [
        (
            prefs.grid,
            "Canvas Grid",
            Some(Action::CanvasGrid),
            Toggle::Grid,
        ),
        (prefs.snap, "Snap to Grid", None, Toggle::Snap),
        (
            prefs.minimap,
            "Minimap",
            Some(Action::CanvasMinimap),
            Toggle::Minimap,
        ),
        (
            prefs.controls,
            "Canvas Controls",
            Some(Action::CanvasControls),
            Toggle::Controls,
        ),
    ] {
        if check_entry(ui, on, label, action).clicked() {
            request.chrome.toggled = Some(toggle);
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button("Connection Style", |ui| {
        for routing in WireRouting::ALL {
            if check_entry(ui, prefs.routing == routing, routing.label(), None).clicked() {
                request.routing = Some(routing);
                ui.close();
            }
        }
    });
    ui.separator();
    if entry(ui, "Auto-Layout", Some(Action::AutoLayout)).clicked() {
        request.chrome.layout = true;
        ui.close();
    }
    ui.separator();
    if entry_if(
        ui,
        has_selection,
        "Node Info",
        Some(Action::NodeInfo),
        "Select a node first",
    )
    .clicked()
    {
        request.chrome.info = true;
        ui.close();
    }
    ui.separator();
    if entry(ui, "Fit Graph", Some(Action::CanvasFit)).clicked() {
        request.chrome.fit = true;
        ui.close();
    }
    maximize_entry(ui, SolarxyTab::Nodes, intents);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        solarxy_graph::nodes::builtin_registry().expect("builtin registry")
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
