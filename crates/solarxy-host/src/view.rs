//! The per-session view state both shells own.
//!
//! Five fields, and only five, but the reason has changed and the shape has
//! not yet caught up.
//!
//! These were the fields both shells had, against a desktop with no camera
//! nodes. That desktop is gone: it holds an engine, and it tracks which
//! `camera` node each pane looks through exactly as the web shell does, in
//! its own array of its own id type. So the look-through binding is now
//! **duplicated rather than shell-specific**, and the only thing keeping it
//! out of here is that the two shells name the node with different types and
//! this crate may not see the engine's.
//!
//! What genuinely does stay on the web shell: whether a bound pane is locked,
//! whether it is mid-navigation, and each pane's own look. The lock's *rule*
//! is shared, as [`crate::cameras::CameraLocks`]; the flags are not, because
//! the desktop cannot act on one until it can write a pose back to a node.
//! The mid-navigation flag exists only to serve that write-back and has
//! nothing to suppress without it.

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
