use crate::state::view_state::ViewLayout;

/// The draggable divider's hit zone and the current layout in one
/// parameter. Painting is not here: every gap strip (this one included)
/// is painted from `pane_gaps`, and this bundle only carries the drag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DividerInfo {
    pub hit: egui::Rect,
    pub layout: ViewLayout,
}
