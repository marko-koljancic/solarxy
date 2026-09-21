//! The parameter panel: what a node is, and every knob it declares.
//!
//! **A pure interpreter, like the palette beside it.** Which tabs exist,
//! which parameters are visible, how they group and what a reset writes
//! are all answered by `solarxy_studio::params` and by the registry's own
//! visibility evaluator. Nothing here branches on a type identifier, and
//! a node type added in Rust gets a working panel with no change to this
//! file.
//!
//! ## Why the tab is still called Properties
//!
//! This panel is what the Properties tab draws, and the tab keeps that name
//! and its slug because both are serialized into a user's saved dock
//! arrangement, and because it is the name the browser's panel has. The
//! panel that carried the name before reported on an imported file: its
//! statistics, its HDRI, its validation. That content has no source with
//! one document root, and what it answered is read elsewhere now: cook
//! statistics in this panel's header, validation on its Validation tab,
//! and the HDRI in the Environment modal.

mod controls;
mod draft;
mod drag;
mod expression;
mod frame;
mod menus;

pub(crate) use expression::Resolved as ResolvedParams;
pub(in crate::gui) use draft::{Draft, shown_text, step as draft_step};
/// Re-exported for the interpretability twin. See `nodes` for why it does
/// not live inside either panel.
#[cfg(test)]
pub(in crate::gui::panels) use controls::{ControlKind, control_kind};
#[cfg(test)]
pub(in crate::gui::panels) use expression::offers_toggle;
#[cfg(test)]
pub(in crate::gui::panels) use frame::driven;
pub(in crate::gui) use frame::Surface;
pub(crate) use frame::{ParamPanelSource, ParamPanelState, ParamScene, draw_params_content};
