//! The shell's own furniture, as against the panels it hosts: the menu bar,
//! the status bar, the per-pane toolbars, the floating overlays, the split
//! divider and the viewport context menu.
//!
//! What separates this from `panels` is ownership of a dock tab. Everything
//! here draws outside the dock, or on top of it.

pub(in crate::gui) mod divider;
pub(in crate::gui) mod menu;
pub(in crate::gui) mod overlays;
pub(in crate::gui) mod pane_toolbar;
pub(in crate::gui) mod status_bar;
pub(in crate::gui) mod viewport_context_menu;
