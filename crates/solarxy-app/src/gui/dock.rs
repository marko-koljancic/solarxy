//! `egui_dock` integration — the unified panel + viewport docking layer.
//!
//! The user-facing panels (Sidebar, Review Panel, Properties, Tree, Nodes
//! and the rest) plus the 3D Viewport live as tabs
//! inside a single [`egui_dock::DockState`], and one more variant,
//! `Retired`, is what a saved layout's name for a panel that no longer
//! exists deserializes to, so the arrangement survives with that tab gone.
//! Users drag tab titles between leaves to dock left/right/bottom/top; drag
//! outside the dock area to tear out into a floating window. The Viewport tab is **pinned:
//! not closeable, not floatable, and transparent** — `egui_dock` never paints
//! over the wgpu surface, and as in the browser there is no toggle for it
//! because there is nothing to bring back.
//!
//! ## Viewport rect plumbing (one-frame latency)
//!
//! The wgpu `compute_panes` math runs **before** egui this frame, so it
//! reads the Viewport tab's rect from the **previous** frame's render
//! (stored on `EguiRenderer::last_viewport_rect`). The Viewport
//! tab's `ui()` callback records the current rect for the next frame.
//! Latency is invisible at steady state; a one-frame stale rect during
//! resize / dock-rearrangement transients is acceptable.
//!
//! ## Toggling tabs from a menu
//!
//! [`tab_present`] / [`toggle_tab`] are the canonical add-or-remove
//! helpers. The Desks menu's panel rows and the Review menu ask
//! `tab_present` through a closure each frame for their ticks, so the dock tree is the one source of a
//! panel's open state and nothing mirrors it.

use egui_dock::{DockState, TabViewer};

use super::intent::Intents;
use super::pass::{PanelSources, PanelState};
use super::theme::Theme;

/// The tab variants in the Solarxy dock. The `Viewport` variant is
/// special-cased throughout: it never floats, never closes and never paints
/// a background (so the wgpu surface shows through).
///
/// **The variant names are serialized into every user's saved arrangement**,
/// as bare strings, so adding one is safe and renaming or removing one is
/// not, on its own: a blob naming a variant serde cannot match fails as a
/// whole and the silent fallback is the default layout. Two rules close
/// that. A renamed variant keeps its wire name (`Tree` is written as
/// `NodeTree`, the name it had when the blobs were saved), and an unknown
/// name deserializes to `Retired`, which [`sweep_retired`] removes after the
/// parse, so a layout that named a panel this build no longer has restores
/// with only that panel gone. `layout_saved_before_the_tree_still_restores`
/// pins both against a real blob written by 0.8.1, which names `Outliner`,
/// and `a_layout_naming_the_material_inspector_restores_without_it` holds
/// the same for the panel withdrawn in 0.10.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum SolarxyTab {
    Viewport,
    Sidebar,
    ReviewPanel,
    /// Hosts the parameter panel, under the name the browser's panel has.
    Properties,
    /// The scene tree. Written as `NodeTree`, the name every saved
    /// arrangement knows it by.
    #[serde(rename = "NodeTree")]
    Tree,
    Nodes,
    /// The staged assets, one tile each.
    Assets,
    /// One asset, previewed. Opened by a double-click in Assets rather
    /// than from any menu, as the browser's is.
    AssetPreview,
    /// The image network's published output.
    Texture,
    /// The watched geometry's attributes, paged.
    Attributes,
    /// Every text snippet in the document, with an editor.
    Text,
    /// A tab a saved arrangement named that this build does not have.
    /// Never drawn: [`sweep_retired`] removes every one after a restore.
    #[serde(other)]
    Retired,
}

impl SolarxyTab {
    /// Stable kebab-case slug used for menu wiring + serde tags.
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Viewport => "viewport",
            Self::Sidebar => "sidebar",
            Self::ReviewPanel => "review-panel",
            Self::Properties => "properties",
            Self::Tree => "tree",
            Self::Nodes => "nodes",
            Self::Assets => "assets",
            Self::AssetPreview => "asset-preview",
            Self::Texture => "texture",
            Self::Attributes => "attributes",
            Self::Text => "text",
            Self::Retired => "retired",
        }
    }
}

/// The layout a new installation opens in, and what a layout that cannot be
/// restored falls back to: the `Default` arrangement, which is the
/// browser's. Three panels and the Sidebar; every other panel is one toggle
/// away and reopens beside its natural neighbour (see [`toggle_tab`]).
///
/// Until 0.10.0 this mounted every panel, on the principle that the layout
/// itself was how a panel got discovered. A new desktop now opens looking
/// like the browser, which is most of what makes moving between them free.
pub(super) fn default_dock_state() -> DockState<SolarxyTab> {
    super::arrangement::default_arrangement().recipe.build()
}

/// Per-frame `TabViewer`, constructed fresh inside the interface pass.
///
/// **Ten fields where there were seventeen**, and the difference is grouping
/// rather than removal: a panel that needs a new source adds a field to
/// [`PanelSources`], and one that needs its own interface state adds a field to
/// [`PanelState`]. Neither this struct nor the entry point's signature moves.
pub(super) struct SolarxyTabViewer<'a> {
    pub sources: PanelSources<'a>,
    pub panels: PanelState<'a>,
    pub review: &'a mut crate::state::review::ReviewState,
    pub intents: &'a mut Intents,
    pub toolbars: &'a super::chrome::pane_toolbar::PaneToolbarData<'a>,
    pub viewport_rect_out: &'a mut Option<egui::Rect>,
    /// Where the node canvas drew, recorded for the one key claim that
    /// asks where the pointer is rather than what is focused.
    pub canvas_rect_out: &'a mut Option<egui::Rect>,
    /// The size the preview tab drew its model at, in physical pixels, so
    /// the state layer can render the preview at that size.
    pub preview_size_out: &'a mut Option<(u32, u32)>,
    /// The panel the pointer is over, which is what the maximize key acts
    /// on. Only a leaf's front tab is drawn, so the tab names its leaf.
    pub hovered_tab_out: &'a mut Option<SolarxyTab>,
    /// The rects of the furniture drawn over the viewport that takes
    /// clicks, recorded so the pointer routing keeps those clicks from the
    /// camera and the pick.
    pub chrome_rects_out: &'a mut Vec<egui::Rect>,
    /// Whether the floating parameter panel is up, for the docked panel's
    /// View menu to tick.
    pub floating_props_open: bool,
    pub theme: Theme,
}

impl TabViewer for SolarxyTabViewer<'_> {
    type Tab = SolarxyTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            SolarxyTab::Viewport => "Viewport".into(),
            SolarxyTab::Sidebar => "Sidebar".into(),
            SolarxyTab::ReviewPanel => format!("Review ({})", self.review.annotations.len()).into(),
            SolarxyTab::Properties => "Properties".into(),
            SolarxyTab::Tree => "Tree".into(),
            SolarxyTab::Nodes => "Nodes".into(),
            SolarxyTab::Assets => "Assets".into(),
            SolarxyTab::Texture => "Texture".into(),
            SolarxyTab::Attributes => "Attributes".into(),
            SolarxyTab::Text => "Text".into(),
            SolarxyTab::AssetPreview => self
                .panels
                .asset_preview
                .map_or_else(
                    || "Preview".to_string(),
                    |(_, name)| format!("Preview: {name}"),
                )
                .into(),
            SolarxyTab::Retired => String::new().into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        if ui.rect_contains_pointer(ui.max_rect()) {
            *self.hovered_tab_out = Some(*tab);
        }
        match tab {
            SolarxyTab::Viewport => {
                super::chrome::viewport_bar::draw(
                    ui,
                    self.sources.settings,
                    self.intents,
                    self.theme,
                );
                // What is left under the bar is what the scene renders
                // into. Recording the whole tab would put the top of the
                // render behind the bar.
                // The playbar takes the bottom of the tab when it is shown,
                // and the render gets what is left: one clock, so one strip
                // under the whole region rather than one per pane.
                let mut viewport = ui.available_rect_before_wrap();
                if self.sources.settings.transport_bar {
                    let strip = egui::Rect::from_min_max(
                        egui::pos2(
                            viewport.left(),
                            viewport.bottom() - super::chrome::transport_bar::TRANSPORT_BAR_HEIGHT,
                        ),
                        viewport.max,
                    );
                    viewport.max.y = strip.top();
                    super::chrome::transport_bar::draw_transport_bar(
                        ui,
                        strip,
                        self.sources.settings.transport,
                        self.intents,
                        self.theme,
                    );
                }
                *self.viewport_rect_out = Some(viewport);
                super::chrome::pane_toolbar::draw_pane_toolbars(
                    ui,
                    self.toolbars,
                    self.sources.settings,
                    self.intents,
                    self.theme,
                );
                // The furniture over the render: the tool column at the left
                // edge and the attribute strip at the right, which take
                // clicks and so record their rects, and the drag readout at
                // the bottom, which takes none.
                let column = super::chrome::tool_column::draw_tool_column(
                    ui,
                    viewport,
                    self.sources.settings,
                    self.intents,
                    self.theme,
                );
                self.chrome_rects_out.push(column);
                let strip = super::chrome::attr_column::draw_attr_column(
                    ui,
                    viewport,
                    self.sources.attr,
                    self.intents,
                    self.theme,
                );
                self.chrome_rects_out.push(strip);
                super::chrome::gizmo_readout::draw_gizmo_readout(
                    ui,
                    viewport,
                    self.sources.settings.tools.readout,
                    self.theme,
                );
                ui.allocate_space(ui.available_size());
            }
            SolarxyTab::Sidebar => {
                super::panels::sidebar::draw_sidebar_content(
                    ui,
                    self.sources.settings,
                    self.intents,
                );
            }
            SolarxyTab::ReviewPanel => {
                super::panels::review::panel::draw_review_panel_content(
                    ui,
                    self.review,
                    self.intents,
                    self.theme,
                );
            }
            // Properties hosts the parameter panel, under the name the
            // browser's panel has and the name every saved arrangement
            // knows this tab by.
            SolarxyTab::Properties => {
                super::panels::params::draw_params_content(
                    ui,
                    self.sources.params,
                    self.panels.params,
                    super::panels::params::Surface::Docked {
                        floating_open: self.floating_props_open,
                    },
                    self.intents,
                    self.theme,
                );
            }
            SolarxyTab::Tree => {
                super::panels::tree::draw_tree_content(
                    ui,
                    self.sources.tree,
                    self.panels.tree,
                    self.panels.graph_ctx,
                    self.intents,
                    self.theme,
                );
            }
            SolarxyTab::Assets => {
                super::panels::assets::draw_assets_content(
                    ui,
                    self.sources.assets,
                    self.panels.assets,
                    self.intents,
                    self.theme,
                );
            }
            SolarxyTab::AssetPreview => {
                super::panels::asset_preview::draw_asset_preview_content(
                    ui,
                    self.panels.asset_preview,
                    self.sources.assets,
                    self.sources.preview,
                    self.panels.preview,
                    self.preview_size_out,
                    self.intents,
                    self.theme,
                );
            }
            SolarxyTab::Texture => {
                super::panels::texture::draw_texture_content(
                    ui,
                    self.sources.texture,
                    self.panels.texture,
                    self.theme,
                );
            }
            SolarxyTab::Attributes => {
                super::panels::attributes::draw_attributes_content(
                    ui,
                    self.sources.attributes,
                    self.panels.attributes,
                    self.theme,
                );
            }
            SolarxyTab::Text => {
                super::panels::text::draw_text_content(
                    ui,
                    self.sources.text,
                    self.panels.text,
                    self.intents,
                    self.theme,
                );
            }
            // Never present after a restore; drawn as nothing if one is.
            SolarxyTab::Retired => {}
            SolarxyTab::Nodes => {
                *self.canvas_rect_out = Some(ui.max_rect());
                super::panels::nodes::draw_nodes_content(
                    ui,
                    self.sources.canvas,
                    self.panels.canvas,
                    self.panels.graph_ctx,
                    self.sources.settings.canvas,
                    self.intents,
                    self.theme,
                );
            }
        }
    }

    fn closeable(&mut self, tab: &mut Self::Tab) -> bool {
        can_close(*tab)
    }

    fn allowed_in_windows(&self, tab: &mut Self::Tab) -> bool {
        !matches!(tab, SolarxyTab::Viewport)
    }

    fn clear_background(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, SolarxyTab::Viewport)
    }

    /// The tab-body margin every panel puts around its content is wrong
    /// for the one tab that paints with a GPU surface instead of with
    /// egui: nothing paints the leftover ring, so the composite's black
    /// clear showed through on all four sides. Zeroed for the Viewport
    /// alone; every other panel keeps its padding.
    fn tab_style_override(
        &self,
        tab: &Self::Tab,
        global_style: &egui_dock::TabStyle,
    ) -> Option<egui_dock::TabStyle> {
        matches!(tab, SolarxyTab::Viewport).then(|| {
            let mut style = global_style.clone();
            style.tab_body.inner_margin = egui::Margin::ZERO;
            style
        })
    }

    /// The wgpu surface shows through the Viewport tab, so it must never
    /// scroll. `egui_dock` wraps every tab body in a `ScrollArea` whose
    /// `scroll_bars` default to `[true, true]`; in a narrow quad pane the
    /// per-pane toolbar overflows and grows a spurious horizontal
    /// scrollbar that shifts the viewport. Other panels keep scrolling.
    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        if matches!(tab, SolarxyTab::Viewport) {
            [false, false]
        } else {
            [true, true]
        }
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("solarxy_tab", tab.slug()))
    }
}

/// The dock layout, and the one leaf that may be covering it.
///
/// `egui_dock` has no maximize, so it is built here: the leaf a panel sits
/// in is copied into a layout of its own and drawn instead, while the full
/// layout waits underneath. Keeping the two in one type is what makes that
/// safe. **Every question about the arrangement and every change to it goes
/// to the full layout, and a change restores first**, so nothing can be
/// toggled into the maximized view by accident, an arrangement applied while
/// maximized replaces the real layout rather than the copy, and what is
/// saved on the way out is the arrangement rather than the one panel that
/// happened to be covering it. Only the draw call sees the copy.
pub(super) struct Dock {
    layout: DockState<SolarxyTab>,
    maximized: Option<Maximized>,
}

struct Maximized {
    view: DockState<SolarxyTab>,
    /// What the leaf held when it was maximized, so a tab the user closes
    /// while maximized can be closed in the full layout on the way back.
    tabs: Vec<SolarxyTab>,
}

impl Dock {
    pub(super) fn new(layout: DockState<SolarxyTab>) -> Self {
        Self {
            layout,
            maximized: None,
        }
    }

    /// The full arrangement, whatever is drawn over it.
    pub(super) fn layout(&self) -> &DockState<SolarxyTab> {
        &self.layout
    }

    /// The full arrangement, to change. Restores first.
    pub(super) fn layout_mut(&mut self) -> &mut DockState<SolarxyTab> {
        self.restore();
        &mut self.layout
    }

    /// Replace the arrangement outright. Whatever was maximized belonged to
    /// the old one and goes with it.
    pub(super) fn replace(&mut self, layout: DockState<SolarxyTab>) {
        self.maximized = None;
        self.layout = layout;
    }

    /// What is on screen: the maximized leaf, or the arrangement.
    pub(super) fn drawn(&self) -> &DockState<SolarxyTab> {
        self.maximized.as_ref().map_or(&self.layout, |m| &m.view)
    }

    pub(super) fn drawn_mut(&mut self) -> &mut DockState<SolarxyTab> {
        match &mut self.maximized {
            Some(maximized) => &mut maximized.view,
            None => &mut self.layout,
        }
    }

    pub(super) fn is_maximized(&self) -> bool {
        self.maximized.is_some()
    }

    /// Maximize the leaf `tab` sits in, or restore when anything already is:
    /// while maximized there is one panel on screen, so the way back needs
    /// no target.
    pub(super) fn toggle_maximize(&mut self, tab: SolarxyTab) {
        if self.restore() {
            return;
        }
        let Some((surface, node, _)) = self.layout.find_tab(&tab) else {
            return;
        };
        let Some(leaf) = self.layout[surface][node].get_leaf() else {
            return;
        };
        let tabs = leaf.tabs.clone();
        let active = leaf.active;
        let mut view = DockState::new(tabs.clone());
        if let Some((surface, node, _)) = view.find_tab(&tab) {
            view.set_active_tab((surface, node, active));
        }
        self.maximized = Some(Maximized { view, tabs });
    }

    /// Put the arrangement back, and say whether anything was maximized.
    ///
    /// A tab closed while maximized is closed in the arrangement too:
    /// otherwise closing a panel and pressing Escape would bring it back.
    pub(super) fn restore(&mut self) -> bool {
        let Some(maximized) = self.maximized.take() else {
            return false;
        };
        for tab in maximized.tabs {
            if !tab_present(&maximized.view, tab)
                && let Some(locator) = self.layout.find_tab(&tab)
            {
                self.layout.remove_tab(locator);
            }
        }
        true
    }

    /// Restore when the last tab of the maximized leaf has been closed,
    /// rather than leave an empty window. Run once a frame, after the draw.
    pub(super) fn settle(&mut self) {
        if self
            .maximized
            .as_ref()
            .is_some_and(|m| m.view.iter_all_tabs().next().is_none())
        {
            self.restore();
        }
    }
}

/// Whether a tab has a close button. Every panel but the viewport, which is
/// pinned as the browser's is: it has no toggle to bring it back, so it has
/// no button to close it.
pub(super) const fn can_close(tab: SolarxyTab) -> bool {
    !matches!(tab, SolarxyTab::Viewport)
}

/// Return `true` if `tab` is currently mounted anywhere in the dock
/// (main surface or a floating window).
pub(super) fn tab_present(dock: &DockState<SolarxyTab>, tab: SolarxyTab) -> bool {
    dock.iter_all_tabs().any(|(_, t)| *t == tab)
}

/// Remove every tab a saved arrangement named that this build does not
/// have, and say how many went. Run once after a restore, so a user whose
/// blob names a retired panel keeps the rest of their arrangement.
pub(super) fn sweep_retired(dock: &mut DockState<SolarxyTab>) -> usize {
    let mut removed = 0;
    while let Some(locator) = dock.find_tab(&SolarxyTab::Retired) {
        dock.remove_tab(locator);
        removed += 1;
    }
    removed
}

/// Read a saved layout back, and say how many retired panels it named.
///
/// Two ways to fail, and both leave the caller's layout alone. The text may
/// not be a layout at all, after corruption or a docking-library change. Or
/// it may be one whose every panel has since been retired, which parses and
/// then sweeps down to nothing: restoring that would leave an empty window
/// with no panel to reopen anything from.
pub(super) fn restore(json: &str) -> Result<(DockState<SolarxyTab>, usize), String> {
    let mut state: DockState<SolarxyTab> =
        serde_json::from_str(json).map_err(|err| err.to_string())?;
    let dropped = sweep_retired(&mut state);
    if state.iter_all_tabs().next().is_none() {
        return Err("the layout has no panel this build still has".to_string());
    }
    ensure_viewport(&mut state);
    Ok((state, dropped))
}

/// Share of the width a viewport put back into a layout takes.
const RESTORED_VIEWPORT_SHARE: f32 = 0.55;

/// Put the viewport back into a layout that lacks one.
///
/// The viewport could be closed until 0.10.0, so a layout saved with it
/// closed is a real thing in users' configuration files. It cannot be closed
/// now and has no toggle, so such a layout would restore into a window with
/// no scene and no way to get one. It goes back on the left, where every
/// built-in arrangement has it, in a leaf of its own.
fn ensure_viewport(dock: &mut DockState<SolarxyTab>) {
    if tab_present(dock, SolarxyTab::Viewport) {
        return;
    }
    dock.main_surface_mut().split_left(
        egui_dock::NodeIndex::root(),
        RESTORED_VIEWPORT_SHARE,
        vec![SolarxyTab::Viewport],
    );
}

/// Show `tab`, adding it beside `neighbour` when that tab is mounted and
/// to the first leaf otherwise, and make it the active tab of its leaf.
/// The preview opens beside the Assets panel this way, as the browser's
/// opens in the assets panel's group.
pub(super) fn show_tab_beside(
    dock: &mut DockState<SolarxyTab>,
    tab: SolarxyTab,
    neighbour: SolarxyTab,
) {
    if let Some((surface, node, index)) = dock.find_tab(&tab) {
        dock.set_active_tab((surface, node, index));
        return;
    }
    if let Some((surface, node, _)) = dock.find_tab(&neighbour) {
        dock[surface][node].append_tab(tab);
    } else {
        dock.main_surface_mut().push_to_first_leaf(tab);
    }
    if let Some(found) = dock.find_tab(&tab) {
        dock.set_active_tab(found);
    }
}

/// Share of the width a reopened node canvas leaves the viewport, and of
/// the height a reopened parameter panel leaves the canvas.
const REOPEN_VIEWPORT_SHARE: f32 = 0.55;
const REOPEN_CANVAS_SHARE: f32 = 0.5;

/// Add `tab` beside its natural neighbour if absent; remove all occurrences
/// if present. Every panel toggle routes through this.
///
/// **A reopened panel never lands on the viewport's leaf**, where it would
/// cover the scene. The rules are the browser's: the node canvas opens to
/// the right of the viewport, the parameter panel under the canvas, and
/// every other panel tabs in behind the parameter panel.
pub(super) fn toggle_tab(dock: &mut DockState<SolarxyTab>, tab: SolarxyTab) {
    // The viewport is pinned. Nothing asks to toggle it, and a request that
    // did would otherwise remove the one panel that has no way back.
    if tab == SolarxyTab::Viewport && tab_present(dock, tab) {
        return;
    }
    if let Some(locator) = dock.find_tab(&tab) {
        dock.remove_tab(locator);
        // Sweep any duplicate occurrences too.
        while let Some(extra) = dock.find_tab(&tab) {
            dock.remove_tab(extra);
        }
    } else {
        reopen(dock, tab);
    }
}

/// Mount `tab` where a user would look for it.
fn reopen(dock: &mut DockState<SolarxyTab>, tab: SolarxyTab) {
    match tab {
        // The viewport goes back where a layout starts, the first leaf.
        SolarxyTab::Viewport => dock.main_surface_mut().push_to_first_leaf(tab),
        SolarxyTab::Nodes => beside_the_viewport(dock, tab),
        SolarxyTab::Properties => {
            if let Some((surface, node, _)) = dock.find_tab(&SolarxyTab::Nodes) {
                dock[surface].split_below(node, REOPEN_CANVAS_SHARE, vec![tab]);
            } else {
                beside_the_viewport(dock, tab);
            }
        }
        _ => {
            let neighbour = [SolarxyTab::Properties, SolarxyTab::Nodes]
                .into_iter()
                .find_map(|candidate| dock.find_tab(&candidate));
            if let Some((surface, node, _)) = neighbour {
                dock[surface][node].append_tab(tab);
                if let Some(found) = dock.find_tab(&tab) {
                    dock.set_active_tab(found);
                }
            } else {
                beside_the_viewport(dock, tab);
            }
        }
    }
}

/// A new leaf to the right of the viewport, or the first leaf when the
/// viewport itself is closed and there is nothing to stay clear of.
fn beside_the_viewport(dock: &mut DockState<SolarxyTab>, tab: SolarxyTab) {
    if let Some((surface, node, _)) = dock.find_tab(&SolarxyTab::Viewport) {
        dock[surface].split_right(node, REOPEN_VIEWPORT_SHARE, vec![tab]);
    } else {
        dock.main_surface_mut().push_to_first_leaf(tab);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_dock::NodeIndex;
    use std::collections::HashSet;

    fn membership(dock: &DockState<SolarxyTab>) -> HashSet<SolarxyTab> {
        dock.iter_all_tabs().map(|(_, t)| *t).collect()
    }

    /// The default is the `Default` arrangement, exactly: three panels and
    /// the Sidebar. Equality rather than containment, so a panel added to
    /// the fresh-install layout is a decision someone made here.
    #[test]
    fn the_default_layout_is_the_default_arrangement() {
        assert_eq!(
            membership(&default_dock_state()),
            HashSet::from([
                SolarxyTab::Viewport,
                SolarxyTab::Nodes,
                SolarxyTab::Properties,
                SolarxyTab::Sidebar,
            ])
        );
    }

    fn leaf_of(dock: &DockState<SolarxyTab>, tab: SolarxyTab) -> Vec<SolarxyTab> {
        let (surface, node, _) = dock.find_tab(&tab).expect("mounted");
        dock[surface][node].get_leaf().expect("a leaf").tabs.clone()
    }

    /// Every panel a user can reopen, reopened from the default layout,
    /// leaves the viewport alone in its leaf. The first-leaf placement this
    /// replaced put a reopened panel over the scene.
    #[test]
    fn a_reopened_panel_never_covers_the_viewport() {
        for tab in [
            SolarxyTab::Tree,
            SolarxyTab::Assets,
            SolarxyTab::Texture,
            SolarxyTab::Attributes,
            SolarxyTab::Text,
            SolarxyTab::ReviewPanel,
        ] {
            let mut dock = default_dock_state();
            toggle_tab(&mut dock, tab);
            assert_eq!(
                leaf_of(&dock, SolarxyTab::Viewport),
                [SolarxyTab::Viewport],
                "{tab:?} landed on the viewport"
            );
            assert!(
                leaf_of(&dock, tab).contains(&SolarxyTab::Properties),
                "{tab:?} tabs in behind the parameter panel"
            );
        }
    }

    /// The two core panels have places of their own: the canvas beside the
    /// viewport, the parameter panel under the canvas.
    #[test]
    fn the_core_panels_reopen_in_their_own_leaves() {
        let mut dock = default_dock_state();
        toggle_tab(&mut dock, SolarxyTab::Properties);
        toggle_tab(&mut dock, SolarxyTab::Sidebar);
        toggle_tab(&mut dock, SolarxyTab::Nodes);
        assert_eq!(membership(&dock), HashSet::from([SolarxyTab::Viewport]));

        toggle_tab(&mut dock, SolarxyTab::Nodes);
        assert_eq!(leaf_of(&dock, SolarxyTab::Nodes), [SolarxyTab::Nodes]);
        assert_eq!(leaf_of(&dock, SolarxyTab::Viewport), [SolarxyTab::Viewport]);

        toggle_tab(&mut dock, SolarxyTab::Properties);
        assert_eq!(
            leaf_of(&dock, SolarxyTab::Properties),
            [SolarxyTab::Properties]
        );
        assert_eq!(leaf_of(&dock, SolarxyTab::Nodes), [SolarxyTab::Nodes]);
    }

    /// With neither core panel up, an auxiliary panel still stays clear of
    /// the viewport.
    #[test]
    fn an_auxiliary_panel_with_no_neighbour_opens_beside_the_viewport() {
        let mut dock = DockState::new(vec![SolarxyTab::Viewport]);
        toggle_tab(&mut dock, SolarxyTab::Tree);
        assert_eq!(leaf_of(&dock, SolarxyTab::Tree), [SolarxyTab::Tree]);
        assert_eq!(leaf_of(&dock, SolarxyTab::Viewport), [SolarxyTab::Viewport]);
    }

    /// A **real** `last_layout_json`, lifted verbatim from a `config.toml`
    /// written by the shipped app before `SolarxyTab::Tree` existed:
    /// a working arrangement with most panels closed, laid-out rects and
    /// all. Blobs of exactly this shape are sitting in users' configs now.
    ///
    /// A layout serialized from a freshly built `default_dock_state`
    /// deliberately is **not** used here. Its rects are still `NaN`
    /// (nothing has laid them out), which serde writes as `null` and
    /// refuses to read back, so it would have tested the failure path
    /// while appearing to test the success one.
    const LAYOUT_BEFORE_NODE_TREE: &str =
        include_str!("../../tests/fixtures/dock-layout-0.8.1.json");

    /// The persistence half, and it asserts **membership**, not merely that
    /// the parse succeeded.
    ///
    /// `EguiRenderer::apply_layout_json` falls back to the default layout
    /// silently when deserialization fails. So `is_ok()` alone cannot tell
    /// a real restore from a fallback wearing its clothes; the tabs this
    /// fixture actually carries can, because the default layout carries
    /// every panel. What a user would lose if this broke is their whole
    /// arrangement, with no error to explain where it went.
    ///
    /// The fixture names `Outliner` and `Console`, two panels this build no
    /// longer has. It must parse anyway, and the sweep must remove exactly
    /// those tabs and nothing else: that is the whole guarantee a retired
    /// panel makes.
    #[test]
    fn layout_saved_before_the_tree_still_restores() {
        let mut dock: DockState<SolarxyTab> = serde_json::from_str(LAYOUT_BEFORE_NODE_TREE)
            .expect("a blob naming a retired panel must still deserialize");
        assert!(
            tab_present(&dock, SolarxyTab::Retired),
            "Outliner and Console parse as Retired"
        );

        assert_eq!(
            sweep_retired(&mut dock),
            2,
            "two retired tabs, each swept once"
        );
        assert_eq!(
            membership(&dock),
            HashSet::from([SolarxyTab::Viewport]),
            "the restored layout must be the saved tabs minus the retired ones, not the default"
        );
    }

    /// The same guarantee for a name this build has never heard of, so the
    /// mechanism is generic rather than a list of the names retired so far.
    #[test]
    fn a_layout_naming_a_tab_that_never_existed_restores_without_it() {
        let blob = LAYOUT_BEFORE_NODE_TREE.replace("\"Outliner\"", "\"Bogus\"");
        assert_ne!(blob, LAYOUT_BEFORE_NODE_TREE, "the fixture names Outliner");
        let mut dock: DockState<SolarxyTab> =
            serde_json::from_str(&blob).expect("an unknown tab name must not reject the layout");
        assert_eq!(sweep_retired(&mut dock), 2, "Bogus and Console both go");
        assert_eq!(membership(&dock), HashSet::from([SolarxyTab::Viewport]));
        assert_eq!(sweep_retired(&mut dock), 0, "a second sweep finds nothing");
    }

    /// And for the panel withdrawn in 0.10.0, whose name is in every
    /// arrangement saved while it shipped in the default layout. The
    /// variant is gone, so the name has to land on `Retired` like any
    /// other this build does not have; were the variant still here, the
    /// name would parse as itself and only the Console would be swept.
    #[test]
    fn a_layout_naming_the_material_inspector_restores_without_it() {
        let blob = LAYOUT_BEFORE_NODE_TREE.replace("\"Outliner\"", "\"MaterialInspector\"");
        assert_ne!(blob, LAYOUT_BEFORE_NODE_TREE, "the fixture names Outliner");
        let mut dock: DockState<SolarxyTab> = serde_json::from_str(&blob)
            .expect("a blob naming the withdrawn panel must still deserialize");
        assert_eq!(
            sweep_retired(&mut dock),
            2,
            "the Material Inspector and the Console both go"
        );
        assert_eq!(membership(&dock), HashSet::from([SolarxyTab::Viewport]));
    }

    fn technical() -> Dock {
        let arrangement = super::super::arrangement::BUILT_IN
            .iter()
            .find(|a| a.name == "Review")
            .expect("a built-in named Review");
        Dock::new(arrangement.recipe.build())
    }

    /// Maximizing draws the panel's whole leaf and nothing else, leaves the
    /// arrangement untouched underneath, and a second toggle puts it back.
    #[test]
    fn maximize_draws_one_leaf_and_restores_the_arrangement() {
        let mut dock = technical();
        let before = membership(dock.layout());

        dock.toggle_maximize(SolarxyTab::ReviewPanel);
        assert!(dock.is_maximized());
        assert_eq!(
            membership(dock.drawn()),
            HashSet::from([SolarxyTab::Properties, SolarxyTab::ReviewPanel]),
            "the leaf, with every tab it holds"
        );
        assert_eq!(membership(dock.layout()), before, "the arrangement waits");

        dock.toggle_maximize(SolarxyTab::Nodes);
        assert!(!dock.is_maximized(), "any toggle restores while maximized");
        assert_eq!(membership(dock.drawn()), before);
    }

    /// A change to the arrangement restores first, so nothing is toggled
    /// into the maximized copy and lost on the way back.
    #[test]
    fn changing_the_arrangement_restores_first() {
        let mut dock = technical();
        dock.toggle_maximize(SolarxyTab::Nodes);
        toggle_tab(dock.layout_mut(), SolarxyTab::Tree);
        assert!(!dock.is_maximized());
        assert!(tab_present(dock.layout(), SolarxyTab::Tree));
        assert!(tab_present(dock.drawn(), SolarxyTab::Tree));
    }

    /// An arrangement applied while a panel is maximized replaces the real
    /// layout rather than the copy.
    #[test]
    fn replacing_the_arrangement_drops_the_maximized_view() {
        let mut dock = technical();
        dock.toggle_maximize(SolarxyTab::Nodes);
        dock.replace(default_dock_state());
        assert!(!dock.is_maximized());
        assert_eq!(membership(dock.drawn()), membership(&default_dock_state()));
    }

    /// A tab closed while maximized stays closed after the restore, and
    /// closing the last one restores rather than leaving an empty window.
    #[test]
    fn a_tab_closed_while_maximized_stays_closed() {
        let mut dock = technical();
        dock.toggle_maximize(SolarxyTab::ReviewPanel);
        toggle_tab(dock.drawn_mut(), SolarxyTab::ReviewPanel);
        dock.settle();
        assert!(dock.is_maximized(), "Properties is still up");

        toggle_tab(dock.drawn_mut(), SolarxyTab::Properties);
        dock.settle();
        assert!(!dock.is_maximized(), "nothing left to show");
        assert!(!tab_present(dock.layout(), SolarxyTab::ReviewPanel));
        assert!(!tab_present(dock.layout(), SolarxyTab::Properties));
        assert!(tab_present(dock.layout(), SolarxyTab::Viewport));
    }

    /// A panel that is not mounted cannot be maximized.
    #[test]
    fn an_unmounted_panel_is_not_maximized() {
        let mut dock = technical();
        dock.toggle_maximize(SolarxyTab::Text);
        assert!(!dock.is_maximized());
    }

    /// The viewport is pinned: a toggle does not remove it, and it alone
    /// has no close button.
    #[test]
    fn the_viewport_cannot_be_toggled_away() {
        let mut dock = default_dock_state();
        toggle_tab(&mut dock, SolarxyTab::Viewport);
        assert!(tab_present(&dock, SolarxyTab::Viewport));

        assert!(!can_close(SolarxyTab::Viewport));
        for tab in [
            SolarxyTab::Sidebar,
            SolarxyTab::ReviewPanel,
            SolarxyTab::Properties,
            SolarxyTab::Tree,
            SolarxyTab::Nodes,
            SolarxyTab::Assets,
            SolarxyTab::AssetPreview,
            SolarxyTab::Texture,
            SolarxyTab::Attributes,
            SolarxyTab::Text,
        ] {
            assert!(can_close(tab), "{tab:?} closes");
        }
    }

    /// A layout saved with the viewport closed, which was possible until
    /// 0.10.0, restores with the viewport back in a leaf of its own and
    /// every other panel where it was.
    ///
    /// Built from the real saved layout rather than from a fresh state,
    /// whose unlaid-out rects do not survive serialization: its viewport is
    /// renamed to a panel this build has, which leaves a layout with
    /// something in it and no viewport.
    #[test]
    fn a_layout_saved_without_a_viewport_gets_it_back() {
        let saved = LAYOUT_BEFORE_NODE_TREE.replace("\"Viewport\"", "\"Properties\"");
        assert_ne!(saved, LAYOUT_BEFORE_NODE_TREE, "the fixture names Viewport");

        let (dock, dropped) = restore(&saved).expect("restorable");
        assert_eq!(dropped, 2, "the two retired panels still go");
        assert_eq!(
            membership(&dock),
            HashSet::from([SolarxyTab::Properties, SolarxyTab::Viewport])
        );
        assert_eq!(leaf_of(&dock, SolarxyTab::Viewport), [SolarxyTab::Viewport]);
    }

    /// A layout that has its viewport is left exactly as it was.
    #[test]
    fn a_layout_with_its_viewport_is_left_alone() {
        let mut dock = default_dock_state();
        let before = membership(&dock);
        let count = dock.iter_all_tabs().count();
        ensure_viewport(&mut dock);
        assert_eq!(membership(&dock), before);
        assert_eq!(dock.iter_all_tabs().count(), count, "no second viewport");
    }

    /// A layout is restorable when it parses and something is left of it.
    #[test]
    fn a_layout_is_restored_with_its_retired_panels_counted() {
        let (dock, dropped) = restore(LAYOUT_BEFORE_NODE_TREE).expect("restorable");
        assert_eq!(dropped, 2);
        assert_eq!(membership(&dock), HashSet::from([SolarxyTab::Viewport]));
    }

    /// Text that is not a layout, and a layout with nothing left once its
    /// retired panels are swept, are both refused rather than restored into
    /// a broken window.
    #[test]
    fn a_layout_that_cannot_be_restored_is_refused() {
        assert!(restore("not a layout").is_err());
        assert!(restore("{}").is_err());

        let nothing_left = LAYOUT_BEFORE_NODE_TREE.replace("\"Viewport\"", "\"Bogus\"");
        assert_ne!(nothing_left, LAYOUT_BEFORE_NODE_TREE);
        assert!(
            restore(&nothing_left).is_err(),
            "every panel retired leaves nothing to restore"
        );
    }

    /// The wire name of the tree is the one every saved arrangement knows,
    /// so a rename in the code costs nobody their layout.
    #[test]
    fn the_tree_keeps_its_saved_name_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&SolarxyTab::Tree).expect("serializes"),
            "\"NodeTree\""
        );
        let back: SolarxyTab = serde_json::from_str("\"NodeTree\"").expect("deserializes");
        assert_eq!(back, SolarxyTab::Tree);
    }

    /// The recovery path for the layout above: a user whose blob predates
    /// the tab reaches it through its panel toggle, exactly as they would
    /// any panel they had closed.
    #[test]
    fn the_tree_is_reachable_from_a_layout_that_never_had_it() {
        let mut dock: DockState<SolarxyTab> =
            serde_json::from_str(LAYOUT_BEFORE_NODE_TREE).expect("fixture deserializes");
        assert!(!tab_present(&dock, SolarxyTab::Tree));

        toggle_tab(&mut dock, SolarxyTab::Tree);
        assert!(tab_present(&dock, SolarxyTab::Tree));
    }

    /// The preview opens beside the Assets panel, in its leaf and in
    /// front, and a second open brings the same tab forward rather than
    /// adding another.
    #[test]
    fn the_preview_opens_beside_assets_once() {
        // Assets deliberately not in the first leaf, so opening beside it
        // is distinguishable from opening anywhere.
        let mut dock = DockState::new(vec![SolarxyTab::Viewport]);
        dock.main_surface_mut()
            .split_right(NodeIndex::root(), 0.7, vec![SolarxyTab::Assets]);
        assert!(!tab_present(&dock, SolarxyTab::AssetPreview));
        show_tab_beside(&mut dock, SolarxyTab::AssetPreview, SolarxyTab::Assets);
        let (surface, node, _) = dock.find_tab(&SolarxyTab::AssetPreview).expect("mounted");
        let (asset_surface, asset_node, _) = dock.find_tab(&SolarxyTab::Assets).expect("assets");
        assert_eq!(
            (surface, node),
            (asset_surface, asset_node),
            "in the Assets leaf"
        );
        let leaf = dock[surface][node].get_leaf().expect("a leaf");
        assert_eq!(
            leaf.tabs.get(leaf.active.0).copied(),
            Some(SolarxyTab::AssetPreview),
            "and in front"
        );
        show_tab_beside(&mut dock, SolarxyTab::AssetPreview, SolarxyTab::Assets);
        let count = dock
            .iter_all_tabs()
            .filter(|(_, t)| **t == SolarxyTab::AssetPreview)
            .count();
        assert_eq!(count, 1, "a second open is not a second tab");

        // With no Assets tab mounted, it still opens somewhere.
        toggle_tab(&mut dock, SolarxyTab::Assets);
        toggle_tab(&mut dock, SolarxyTab::AssetPreview);
        show_tab_beside(&mut dock, SolarxyTab::AssetPreview, SolarxyTab::Assets);
        assert!(tab_present(&dock, SolarxyTab::AssetPreview));
    }

    #[test]
    fn toggle_tab_is_idempotent() {
        let mut dock = default_dock_state();
        let initial = membership(&dock);
        toggle_tab(&mut dock, SolarxyTab::Sidebar);
        toggle_tab(&mut dock, SolarxyTab::Sidebar);
        assert_eq!(initial, membership(&dock), "two toggles must round-trip");
    }

    #[test]
    fn toggle_tab_removes_duplicates() {
        let mut dock = default_dock_state();
        dock.main_surface_mut()
            .push_to_first_leaf(SolarxyTab::Sidebar);
        let dup_count = dock
            .iter_all_tabs()
            .filter(|(_, t)| **t == SolarxyTab::Sidebar)
            .count();
        assert_eq!(dup_count, 2, "fixture should have 2 Sidebar tabs");

        toggle_tab(&mut dock, SolarxyTab::Sidebar);

        let remaining = dock
            .iter_all_tabs()
            .filter(|(_, t)| **t == SolarxyTab::Sidebar)
            .count();
        assert_eq!(remaining, 0, "toggle must sweep all duplicates");
    }

    #[test]
    fn tab_present_accuracy_after_sequence() {
        let mut dock = default_dock_state();
        // The node canvas is in the default layout, so the round-trip
        // starts from present.
        assert!(tab_present(&dock, SolarxyTab::Nodes));

        toggle_tab(&mut dock, SolarxyTab::Nodes);
        assert!(!tab_present(&dock, SolarxyTab::Nodes));

        toggle_tab(&mut dock, SolarxyTab::Nodes);
        assert!(tab_present(&dock, SolarxyTab::Nodes));
    }
}
