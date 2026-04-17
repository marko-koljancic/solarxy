use solarxy_renderer::camera_state::CameraState;

pub(crate) use solarxy_core::view_config::{
    BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout,
};

pub(crate) struct ViewState {
    pub(super) pane_settings: [PaneDisplaySettings; 2],
    pub(super) display: DisplaySettings,
    pub(super) secondary_cam: Option<CameraState>,
    pub(super) active_pane: usize,
    pub(super) cameras_linked: bool,
}
