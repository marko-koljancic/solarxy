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
    /// Review-menu toggle of review mode — applied identically to `Shift+R`.
    pub toggle_review_mode: bool,
    /// Review-menu toggle of the 3D marker overlay (`ReviewState::markers_hidden`).
    pub toggle_review_markers: bool,
    /// Review-menu "Save Review Notes" — writes the sidecar (same as `Cmd/Ctrl+S`).
    pub save_review_notes: bool,
    pub save_dock_layout: bool,
    pub restore_saved_layout: bool,
    pub reset_dock_layout: bool,
    /// View-menu "Show All Meshes" — un-hides every mesh (same as `Alt+H`).
    pub show_all_meshes: bool,
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
    pub outliner_visible: bool,
    pub node_tree_visible: bool,
    pub menu_bar_visible: bool,
    pub properties_visible: bool,
    pub status_bar_visible: bool,
    pub console_visible: bool,
    pub review_panel_visible: bool,
    pub material_inspector_visible: bool,
    pub viewport_visible: bool,
    pub has_saved_layout: bool,
}
