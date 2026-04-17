use solarxy_core::preferences::ProjectionMode;
use crate::state::view_state::ViewLayout;

#[derive(Default)]
pub(crate) struct MenuActions {
    pub open_model: bool,
    pub open_hdri: bool,
    pub close_model: bool,
    pub quit: bool,
    pub save_screenshot: bool,
    pub save_preferences: bool,
    pub open_recent: Option<String>,
    pub open_config_file: bool,
    pub set_layout: Option<ViewLayout>,
    pub set_projection: Option<ProjectionMode>,
    pub open_wiki: bool,
    pub open_about: bool,
    pub check_for_updates: bool,
}

#[derive(Clone, Copy)]
pub(super) struct MenuBarVisibility {
    pub sidebar_visible: bool,
    pub menu_bar_visible: bool,
    pub stats_visible: bool,
    pub hints_visible: bool,
    pub fps_hud_visible: bool,
    pub console_visible: bool,
}
