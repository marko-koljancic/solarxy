//! `egui_dock` integration — the unified panel + viewport docking layer.
//!
//! All eight user-facing panels (Sidebar, Review Panel, Console, Material
//! Inspector, Properties, Outliner, Node Tree, Nodes) plus the 3D Viewport
//! live as tabs inside a single [`egui_dock::DockState`], nine variants in
//! all.
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
use super::pass::{OpenFile, PanelSources, PanelState};
use super::theme::Theme;

/// The nine tab variants in the Solarxy dock. The `Viewport` variant is
/// special-cased throughout: it never floats and never paints a background
/// (so the wgpu surface shows through). It *can* be closed — the Window
/// menu restores it via [`toggle_tab`].
///
/// **Adding a variant is safe for persisted layouts, but only because
/// these are unit variants**, which serde writes as bare strings: a blob
/// saved before the variant existed names only tabs that still exist and
/// parses unchanged. Renaming one is the dangerous edit, since the silent
/// fallback is the default layout and the user loses their arrangement
/// without being told. `layout_saved_before_the_node_tree_still_restores`
/// pins this against a real pre-`NodeTree` blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum SolarxyTab {
    Viewport,
    Sidebar,
    ReviewPanel,
    Console,
    MaterialInspector,
    Properties,
    Outliner,
    NodeTree,
    Nodes,
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
            Self::Outliner => "outliner",
            Self::NodeTree => "node-tree",
            Self::Nodes => "nodes",
        }
    }
}

/// Build the default dock layout: Viewport central, Outliner (tabbed with
/// Node Tree) top-left with Sidebar below it, Properties top-right with
/// `ReviewPanel` below it, and the node canvas tabbed with Console and
/// Material Inspector along the bottom, active of the three. Every panel ships in the default tree —
/// discoverability is the layout itself (no panel auto-opens on model
/// load).
///
/// Node Tree shares the Outliner's leaf because they answer the same
/// question from the two sides of the document: what the scene *contains*
/// versus what *produced* it.
pub(super) fn default_dock_state() -> DockState<SolarxyTab> {
    let mut state = DockState::new(vec![SolarxyTab::Viewport]);
    let surface = state.main_surface_mut();
    let [center_etc, left] = surface.split_left(
        NodeIndex::root(),
        0.18,
        vec![SolarxyTab::Outliner, SolarxyTab::NodeTree],
    );
    let [_outliner, _sidebar] = surface.split_below(left, 0.5, vec![SolarxyTab::Sidebar]);
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
    pub open_file: OpenFile<'a>,
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
            SolarxyTab::Outliner => "Outliner".into(),
            SolarxyTab::NodeTree => "Node Tree".into(),
            SolarxyTab::Nodes => "Nodes".into(),
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
                    matches!(
                        self.sources.outliner,
                        super::panels::outliner::OutlinerSource::Scene { .. }
                    ),
                );
            }
            SolarxyTab::Properties => {
                super::panels::properties::draw_properties_content(
                    ui,
                    self.open_file.model_info,
                    self.open_file.hdri_info,
                    self.sources.validation,
                    self.sources.actions,
                    self.sources.settings,
                    self.intents,
                );
            }
            SolarxyTab::Outliner => {
                super::panels::outliner::draw_outliner_content(
                    ui,
                    self.sources.outliner,
                    self.intents,
                );
            }
            SolarxyTab::NodeTree => {
                super::panels::node_tree::draw_node_tree_content(
                    ui,
                    self.sources.node_tree,
                    self.panels.node_tree,
                    self.panels.graph_ctx,
                    self.intents,
                    self.theme,
                );
            }
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
            SolarxyTab::Outliner,
            SolarxyTab::MaterialInspector,
            SolarxyTab::NodeTree,
            SolarxyTab::Nodes,
        ] {
            assert!(present.contains(&tab), "default dock missing tab {tab:?}");
        }
    }

    /// A **real** `last_layout_json`, lifted verbatim from a `config.toml`
    /// written by the shipped app before `SolarxyTab::NodeTree` existed:
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

    /// The persistence half of the Node Tree work, and it asserts
    /// **membership**, not merely that the parse succeeded.
    ///
    /// `EguiRenderer::apply_layout_json` falls back to the default layout
    /// silently when deserialization fails. So `is_ok()` alone cannot tell
    /// a real restore from a fallback wearing its clothes; the three tabs
    /// this fixture actually carries can, because the default layout
    /// carries nine. What a user would lose if this broke is their whole
    /// arrangement, with no error to explain where it went.
    #[test]
    fn layout_saved_before_the_node_tree_still_restores() {
        let dock: DockState<SolarxyTab> = serde_json::from_str(LAYOUT_BEFORE_NODE_TREE)
            .expect("a pre-NodeTree blob must still deserialize");

        assert_eq!(
            membership(&dock),
            HashSet::from([
                SolarxyTab::Viewport,
                SolarxyTab::Console,
                SolarxyTab::Outliner,
            ]),
            "the restored layout must be the three saved tabs, not the default"
        );
    }

    /// The recovery path for the layout above: a user whose blob predates
    /// the tab reaches it through the Window menu, exactly as they would
    /// any panel they had closed.
    #[test]
    fn the_node_tree_is_reachable_from_a_layout_that_never_had_it() {
        let mut dock: DockState<SolarxyTab> =
            serde_json::from_str(LAYOUT_BEFORE_NODE_TREE).expect("fixture deserializes");
        assert!(!tab_present(&dock, SolarxyTab::NodeTree));

        toggle_tab(&mut dock, SolarxyTab::NodeTree);
        assert!(tab_present(&dock, SolarxyTab::NodeTree));
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
