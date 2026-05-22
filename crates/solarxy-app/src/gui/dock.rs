//! `egui_dock` integration — the unified panel + viewport docking layer.
//!
//! All five user-facing panels (Sidebar, Review Panel, Console, Material
//! Inspector, Properties) plus the 3D Viewport live as tabs inside a
//! single [`egui_dock::DockState`]. Users drag tab titles between leaves
//! to dock left/right/bottom/top; drag outside the dock area to tear out
//! into a floating window. The Viewport tab is **closeable but
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
//! helpers. `gui/renderer.rs` projects `MenuBarVisibility.*_visible`
//! flags from these each frame so the Window menu's checkmark state
//! stays in sync without a duplicate source of truth.

use egui_dock::{DockState, NodeIndex, TabViewer};

use solarxy_core::validation::ValidationReport;

use crate::console::ConsoleState;
use crate::state::hdri_info::HdriInfo;

use super::material_inspector::MaterialInspectorState;
use super::outliner::OutlinerEvents;
use super::properties::{ModelInfo, PropertiesEvents};
use super::snapshot::GuiSnapshot;
use super::theme::Theme;

/// The seven tab variants in the Solarxy dock. The `Viewport` variant is
/// special-cased throughout: it never floats and never paints a background
/// (so the wgpu surface shows through). It *can* be closed — the Window
/// menu restores it via [`toggle_tab`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) enum SolarxyTab {
    Viewport,
    Sidebar,
    ReviewPanel,
    Console,
    MaterialInspector,
    Properties,
    Outliner,
}

impl SolarxyTab {
    /// Stable kebab-case slug used for menu wiring + serde tags.
    pub(super) fn slug(self) -> &'static str {
        match self {
            Self::Viewport => "viewport",
            Self::Sidebar => "sidebar",
            Self::ReviewPanel => "review-panel",
            Self::Console => "console",
            Self::MaterialInspector => "material-inspector",
            Self::Properties => "properties",
            Self::Outliner => "outliner",
        }
    }
}

/// Build the default dock layout: Viewport central, Outliner top-left
/// with Sidebar below it, Properties top-right with `ReviewPanel` below
/// it, Console and Material Inspector tabbed together along the bottom.
/// Every panel ships in the default tree — discoverability is the layout
/// itself (no panel auto-opens on model load).
pub(super) fn default_dock_state() -> DockState<SolarxyTab> {
    let mut state = DockState::new(vec![SolarxyTab::Viewport]);
    let surface = state.main_surface_mut();
    let [center_etc, left] =
        surface.split_left(NodeIndex::root(), 0.18, vec![SolarxyTab::Outliner]);
    let [_outliner, _sidebar] = surface.split_below(left, 0.5, vec![SolarxyTab::Sidebar]);
    let [center, right] = surface.split_right(center_etc, 0.78, vec![SolarxyTab::Properties]);
    let [_props, _review] = surface.split_below(right, 0.5, vec![SolarxyTab::ReviewPanel]);
    let [_main, _bottom] = surface.split_below(
        center,
        0.72,
        vec![SolarxyTab::Console, SolarxyTab::MaterialInspector],
    );

    state
}

/// Per-frame `TabViewer` carrying mutable borrows into every tab's state.
/// Constructed fresh inside `render_ui`'s egui closure each frame.
pub(super) struct SolarxyTabViewer<'a> {
    pub snap: &'a mut GuiSnapshot,
    pub review: &'a mut crate::state::review::ReviewState,
    pub console: &'a mut ConsoleState,
    pub model: Option<&'a solarxy_renderer::model::Model>,
    pub model_info: Option<&'a ModelInfo>,
    pub hdri_info: Option<&'a HdriInfo>,
    pub validation_report: Option<&'a ValidationReport>,
    pub properties_events: &'a mut PropertiesEvents,
    pub outliner_events: &'a mut OutlinerEvents,
    pub material_inspector: &'a mut MaterialInspectorState,
    pub viewport_rect_out: &'a mut Option<egui::Rect>,
    pub theme: Theme,
    pub pane_toolbar: super::pane_toolbar::PaneToolbarData<'a>,
}

impl TabViewer for SolarxyTabViewer<'_> {
    type Tab = SolarxyTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            SolarxyTab::Viewport => "Viewport".into(),
            SolarxyTab::Sidebar => "Sidebar".into(),
            SolarxyTab::ReviewPanel => format!("Review ({})", self.review.annotations.len()).into(),
            SolarxyTab::Console => "Console".into(),
            SolarxyTab::MaterialInspector => format!(
                "Material Inspector ({})",
                self.model.map_or(0, |m| m.materials.len())
            )
            .into(),
            SolarxyTab::Properties => "Properties".into(),
            SolarxyTab::Outliner => "Outliner".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            SolarxyTab::Viewport => {
                *self.viewport_rect_out = Some(ui.max_rect());
                super::pane_toolbar::draw_pane_toolbars(
                    ui,
                    &mut self.pane_toolbar,
                    self.snap,
                    self.theme,
                );
                ui.allocate_space(ui.available_size());
            }
            SolarxyTab::Sidebar => {
                super::sidebar::draw_sidebar_content(ui, self.snap);
            }
            SolarxyTab::ReviewPanel => {
                let mut visible = true;
                super::review_panel::draw_review_panel_content(
                    ui,
                    self.review,
                    &mut visible,
                    self.theme,
                );
            }
            SolarxyTab::Console => {
                super::console_view::draw_console_content(ui, self.console);
            }
            SolarxyTab::MaterialInspector => {
                if let Some(model) = self.model {
                    super::material_inspector::draw_material_inspector_content(
                        ui,
                        model,
                        self.material_inspector,
                        &self.theme,
                    );
                } else {
                    draw_no_model_placeholder(ui);
                }
            }
            SolarxyTab::Properties => {
                super::properties::draw_properties_content(
                    ui,
                    self.model_info,
                    self.hdri_info,
                    self.validation_report,
                    self.snap,
                    self.properties_events,
                );
            }
            SolarxyTab::Outliner => {
                super::outliner::draw_outliner_content(ui, self.model, self.outliner_events);
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

fn draw_no_model_placeholder(ui: &mut egui::Ui) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("No model loaded").weak());
    });
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
        ] {
            assert!(present.contains(&tab), "default dock missing tab {tab:?}");
        }
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
