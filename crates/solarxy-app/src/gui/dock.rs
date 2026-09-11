//! `egui_dock` integration — the unified panel + viewport docking layer.
//!
//! The user-facing panels (Sidebar, Review Panel, Console, Material
//! Inspector, Properties, Tree, Nodes) plus the 3D Viewport live as tabs
//! inside a single [`egui_dock::DockState`], and one more variant,
//! `Retired`, is what a saved layout's name for a panel that no longer
//! exists deserializes to, so the arrangement survives with that tab gone.
//! Users drag tab titles between leaves to dock left/right/bottom/top; drag
//! outside the dock area to tear out into a floating window. The Viewport tab is **closeable but
//! non-floatable and transparent** — `egui_dock` never paints over the
//! wgpu surface, and the user can recover a closed Viewport via the
//! Window menu (`Window → Viewport`).
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
//! ## Toggling tabs from the Window menu
//!
//! [`tab_present`] / [`toggle_tab`] are the canonical add-or-remove
//! helpers. The Window menu asks `tab_present` through a closure each
//! frame for its checkmarks, so the dock tree is the one source of a
//! panel's open state and nothing mirrors it.

use egui_dock::{DockState, NodeIndex, TabViewer};

use super::intent::Intents;
use super::pass::{PanelSources, PanelState};
use super::theme::Theme;

/// The tab variants in the Solarxy dock. The `Viewport` variant is
/// special-cased throughout: it never floats and never paints a background
/// (so the wgpu surface shows through). It *can* be closed; the Window
/// menu restores it via [`toggle_tab`].
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
/// pins both against a real blob written by 0.8.1, which names `Outliner`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum SolarxyTab {
    Viewport,
    Sidebar,
    ReviewPanel,
    Console,
    MaterialInspector,
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
            Self::Console => "console",
            Self::MaterialInspector => "material-inspector",
            Self::Properties => "properties",
            Self::Tree => "tree",
            Self::Nodes => "nodes",
            Self::Assets => "assets",
            Self::AssetPreview => "asset-preview",
            Self::Retired => "retired",
        }
    }
}

/// Build the default dock layout: Viewport central, the Tree top-left with
/// the Sidebar below it, Properties top-right with `ReviewPanel` below it,
/// and the node canvas tabbed with Console and Material Inspector along the
/// bottom, active of the three. Every panel ships in the default tree:
/// discoverability is the layout itself (no panel auto-opens on load).
pub(super) fn default_dock_state() -> DockState<SolarxyTab> {
    let mut state = DockState::new(vec![SolarxyTab::Viewport]);
    let surface = state.main_surface_mut();
    let [center_etc, left] = surface.split_left(
        NodeIndex::root(),
        0.18,
        vec![SolarxyTab::Tree, SolarxyTab::Assets],
    );
    let [_tree, _sidebar] = surface.split_below(left, 0.5, vec![SolarxyTab::Sidebar]);
    let [center, right] = surface.split_right(center_etc, 0.78, vec![SolarxyTab::Properties]);
    let [_props, _review] = surface.split_below(right, 0.5, vec![SolarxyTab::ReviewPanel]);
    let [_main, _bottom] = surface.split_below(
        center,
        0.72,
        vec![
            SolarxyTab::Nodes,
            SolarxyTab::Console,
            SolarxyTab::MaterialInspector,
        ],
    );

    state
}

/// Per-frame `TabViewer`, constructed fresh inside the interface pass.
///
/// **Eight fields where there were seventeen**, and the difference is grouping
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
    pub theme: Theme,
}

impl TabViewer for SolarxyTabViewer<'_> {
    type Tab = SolarxyTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            SolarxyTab::Viewport => "Viewport".into(),
            SolarxyTab::Sidebar => "Sidebar".into(),
            SolarxyTab::ReviewPanel => format!("Review ({})", self.review.annotations.len()).into(),
            SolarxyTab::Console => "Console".into(),
            SolarxyTab::MaterialInspector => "Material Inspector".into(),
            SolarxyTab::Properties => "Properties".into(),
            SolarxyTab::Tree => "Tree".into(),
            SolarxyTab::Nodes => "Nodes".into(),
            SolarxyTab::Assets => "Assets".into(),
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
        match tab {
            SolarxyTab::Viewport => {
                *self.viewport_rect_out = Some(ui.max_rect());
                super::chrome::pane_toolbar::draw_pane_toolbars(
                    ui,
                    self.toolbars,
                    self.sources.settings,
                    self.intents,
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
            SolarxyTab::Console => {
                super::panels::console::draw_console_content(ui, self.panels.console, &self.theme);
            }
            SolarxyTab::MaterialInspector => {
                super::panels::material_inspector::draw_material_inspector_content(
                    ui,
                    self.sources.settings.cook.open,
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
                super::panels::assets::draw_asset_preview_content(
                    ui,
                    self.panels.asset_preview,
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

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
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

/// Add `tab` to the first main-surface leaf if absent; remove all
/// occurrences if present. Window-menu toggles route through this.
pub(super) fn toggle_tab(dock: &mut DockState<SolarxyTab>, tab: SolarxyTab) {
    if let Some(locator) = dock.find_tab(&tab) {
        dock.remove_tab(locator);
        // Sweep any duplicate occurrences too.
        while let Some(extra) = dock.find_tab(&tab) {
            dock.remove_tab(extra);
        }
    } else {
        dock.main_surface_mut().push_to_first_leaf(tab);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn membership(dock: &DockState<SolarxyTab>) -> HashSet<SolarxyTab> {
        dock.iter_all_tabs().map(|(_, t)| *t).collect()
    }

    #[test]
    fn default_dock_state_has_core_tabs() {
        let dock = default_dock_state();
        let present = membership(&dock);
        for tab in [
            SolarxyTab::Viewport,
            SolarxyTab::Sidebar,
            SolarxyTab::ReviewPanel,
            SolarxyTab::Console,
            SolarxyTab::Properties,
            SolarxyTab::MaterialInspector,
            SolarxyTab::Tree,
            SolarxyTab::Nodes,
            SolarxyTab::Assets,
        ] {
            assert!(present.contains(&tab), "default dock missing tab {tab:?}");
        }
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
    /// seven. What a user would lose if this broke is their whole
    /// arrangement, with no error to explain where it went.
    ///
    /// The fixture names `Outliner`, a panel this build no longer has. It
    /// must parse anyway, and the sweep must remove exactly that tab and
    /// nothing else: that is the whole guarantee a retired panel makes.
    #[test]
    fn layout_saved_before_the_tree_still_restores() {
        let mut dock: DockState<SolarxyTab> = serde_json::from_str(LAYOUT_BEFORE_NODE_TREE)
            .expect("a blob naming a retired panel must still deserialize");
        assert!(
            tab_present(&dock, SolarxyTab::Retired),
            "Outliner parses as Retired"
        );

        assert_eq!(sweep_retired(&mut dock), 1, "one retired tab, swept once");
        assert_eq!(
            membership(&dock),
            HashSet::from([SolarxyTab::Viewport, SolarxyTab::Console]),
            "the restored layout must be the saved tabs minus the retired one, not the default"
        );
    }

    /// The same guarantee for a name this build has never heard of, so the
    /// mechanism is generic rather than a list of the names retired so far.
    #[test]
    fn a_layout_naming_a_tab_that_never_existed_restores_without_it() {
        let blob = LAYOUT_BEFORE_NODE_TREE.replace("\"Console\"", "\"Bogus\"");
        assert_ne!(blob, LAYOUT_BEFORE_NODE_TREE, "the fixture names Console");
        let mut dock: DockState<SolarxyTab> =
            serde_json::from_str(&blob).expect("an unknown tab name must not reject the layout");
        assert_eq!(sweep_retired(&mut dock), 2, "Bogus and Outliner both go");
        assert_eq!(membership(&dock), HashSet::from([SolarxyTab::Viewport]));
        assert_eq!(sweep_retired(&mut dock), 0, "a second sweep finds nothing");
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
    /// the tab reaches it through the Window menu, exactly as they would
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
        // Every panel ships in the default tree, so the round-trip starts
        // from present.
        assert!(tab_present(&dock, SolarxyTab::MaterialInspector));

        toggle_tab(&mut dock, SolarxyTab::MaterialInspector);
        assert!(!tab_present(&dock, SolarxyTab::MaterialInspector));

        toggle_tab(&mut dock, SolarxyTab::MaterialInspector);
        assert!(tab_present(&dock, SolarxyTab::MaterialInspector));
    }
}
