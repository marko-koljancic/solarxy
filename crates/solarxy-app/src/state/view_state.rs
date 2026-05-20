//! `ViewState` — the per-session bundle of view-related state owned by
//! [`crate::state::State`]. Re-exports [`solarxy_core::view_config`] types
//! (`ViewLayout`, `DisplaySettings`, `PaneDisplaySettings`, `BoundsMode`).

use solarxy_renderer::camera_state::CameraState;

pub(crate) use solarxy_core::view_config::{
    BoundsMode, DisplaySettings, PaneDisplaySettings, ViewLayout,
};

pub(crate) struct ViewState {
    /// Per-pane display settings. Fixed-size — layouts use the first
    /// `layout.pane_count()` slots; the rest are parked defaults.
    pub(super) pane_settings: [PaneDisplaySettings; 4],
    pub(super) display: DisplaySettings,
    /// One camera per pane slot. `None` until `ensure_pane_cameras`
    /// lazily fills the slot (needs a loaded model for bounds). Slot 0
    /// is the Single-layout camera; slots beyond `pane_count()` are
    /// preserved across layout toggles so Quad→Single→Quad is idempotent.
    pub(super) cameras: [Option<CameraState>; 4],
    pub(super) active_pane: usize,
    pub(super) cameras_linked: bool,
}
