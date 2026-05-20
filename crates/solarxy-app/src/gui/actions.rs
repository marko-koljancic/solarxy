use solarxy_core::preferences::ProjectionMode;
use crate::state::view_state::ViewLayout;

#[derive(Debug, Default)]
pub(crate) struct MenuActions {
    pub open_model: bool,
    pub open_hdri: bool,
    pub close_model: bool,
    pub quit: bool,
    pub save_screenshot: bool,
    pub save_preferences: bool,
    pub save_view_defaults: bool,
    pub open_recent: Option<String>,
    pub open_config_file: bool,
    pub open_preferences: bool,
    pub open_shortcuts_modal: bool,
    pub set_layout: Option<ViewLayout>,
    pub set_projection: Option<ProjectionMode>,
    pub open_wiki: bool,
    pub open_about: bool,
    pub check_for_updates: bool,
    pub set_split_ratio: Option<f32>,
    pub cancel_reanchor: bool,
    pub exit_review_mode: bool,
    pub save_dock_layout: bool,
    pub restore_saved_layout: bool,
    pub reset_dock_layout: bool,
}

/// Bundle the visible divider rect, its wider hit zone, and the current
/// layout in one parameter — keeps `EguiRenderer::render_ui` argument count
/// stable when adding the draggable-divider plumbing.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DividerInfo {
    pub visible: egui::Rect,
    pub hit: egui::Rect,
    pub layout: ViewLayout,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MenuBarVisibility {
    pub sidebar_visible: bool,
    pub menu_bar_visible: bool,
    pub stats_visible: bool,
    pub status_bar_visible: bool,
    pub console_visible: bool,
    pub review_panel_visible: bool,
    pub material_inspector_visible: bool,
    pub viewport_visible: bool,
    pub has_saved_layout: bool,
}
