//! The per-session view state both shells own.
//!
//! These were the fields both shells had, against a desktop with no camera
//! nodes. That desktop is gone: it holds an engine, and it tracks which
//! `camera` node each pane looks through exactly as the web shell does, in
//! its own array of its own id type. So the look-through binding is
//! **duplicated rather than shell-specific**, and the only thing keeping it
//! out of here is that the two shells name the node with different types and
//! this crate may not see the engine's.
//!
//! The lock on a bound pane moved here once both shells wrote a pose back to
//! a node; its rule is [`crate::cameras::CameraLocks`]. What stays on each
//! shell is the mid-navigation flag, which exists only to hold the follow
//! off during a gesture and is a fact about that shell's pointer.

use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};
use solarxy_renderer::camera_state::CameraState;

use crate::cameras::CameraLocks;

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
    /// **No constructor sets this**, deliberately. Both shells start unlinked
    /// since 0.10.0 and nothing on either links them, but the field stays a
    /// literal each shell writes rather than a default one inherits, so a
    /// shell that wants the other answer has to say so.
    pub cameras_linked: bool,
    /// Whether each bound pane is locked, so that navigating it writes the
    /// pose back to its camera node. The type carries the rule that a lock
    /// means nothing on a pane that is not bound, so no shell restates it.
    pub camera_locked: CameraLocks,
}
