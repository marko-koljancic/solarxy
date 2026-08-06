//! The per-session view state both shells own.
//!
//! Five fields, and only five: the ones the desktop and the web shell both
//! have. The web shell additionally tracks which `camera` node each pane looks
//! through, whether that pane is locked, whether it is mid-navigation, and each
//! pane's own look. Those are camera-node concerns, and the desktop has no
//! camera nodes until it gains an engine, so they stay on the web shell rather
//! than sitting here as four fields one caller sets and the other never reads.

use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};
use solarxy_renderer::camera_state::CameraState;

/// The view state a shell hands to the shared pane orchestration.
pub struct HostViewState {
    /// Per-pane display settings. Fixed-size: a layout uses the first
    /// `layout.pane_count()` slots and the rest are parked defaults, so
    /// Quad to Single and back is idempotent.
    pub pane_settings: [PaneDisplaySettings; 4],
    /// Settings that are scene-wide rather than per pane, so a change does not
    /// have to fan out across four slots.
    pub display: DisplaySettings,
    /// One camera per pane slot. `None` until the slot is lazily filled, which
    /// needs bounds to frame against. Slot 0 is the Single-layout camera;
    /// slots past `pane_count()` are preserved across layout toggles.
    pub cameras: [Option<CameraState>; 4],
    /// The pane the pointer is over, which is what per-pane commands act on.
    pub active_pane: usize,
    /// Whether navigating one pane navigates them all.
    ///
    /// **No constructor sets this**, deliberately. The desktop shell starts
    /// linked and the web shell starts unlinked, so a `new()` that picked one
    /// would quietly change the other the day it was called. Both shells build
    /// the struct literally and say what they mean.
    pub cameras_linked: bool,
}
