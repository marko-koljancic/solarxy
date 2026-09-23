//! The shell's own furniture, as against the panels it hosts: the menu bar,
//! the viewport's own bar, the per-pane toolbars, the tool column, the
//! attribute strip and the drag readout over the viewport, the playbar under
//! it, the floating overlays, the split divider and the viewport context
//! menu.
//!
//! What separates this from `panels` is ownership of a dock tab. Everything
//! here draws outside the dock, or on top of it. Two modules are furniture a
//! panel borrows rather than furniture the shell draws: `menu_items`, the
//! entry vocabulary every menu is written in, and `panel_bar`, the frame a
//! panel's own menu bar sits in.

pub(in crate::gui) mod attr_column;
pub(in crate::gui) mod divider;
pub(in crate::gui) mod gizmo_readout;
pub(in crate::gui) mod menu;
pub(in crate::gui) mod menu_items;
pub(in crate::gui) mod overlays;
pub(in crate::gui) mod pane_toolbar;
pub(in crate::gui) mod panel_bar;
pub(in crate::gui) mod tool_column;
pub(in crate::gui) mod transport_bar;
pub(in crate::gui) mod viewport_bar;
pub(in crate::gui) mod viewport_context_menu;
pub(in crate::gui) mod viewport_icons;
