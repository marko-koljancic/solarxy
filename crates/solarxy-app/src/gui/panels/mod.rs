//! The dock's panels, one module each.
//!
//! A panel reads through [`PanelSources`] and writes nothing: what it wants
//! travels as an [`Intent`] and is applied after the interface pass. A panel
//! that is several files gets a folder, which review is.
//!
//! [`PanelSources`]: crate::gui::pass::PanelSources
//! [`Intent`]: crate::gui::intent::Intent

#[cfg(test)]
mod extensibility;

pub(in crate::gui) mod console;
pub(in crate::gui) mod material_inspector;
pub(in crate::gui) mod nodes;
pub(in crate::gui) mod params;
pub(in crate::gui) mod review;
pub(in crate::gui) mod sidebar;
pub(in crate::gui) mod tree;
