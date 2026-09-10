//! The parameter panel: what a node is, and every knob it declares.
//!
//! **A pure interpreter, like the palette beside it.** Which tabs exist,
//! which parameters are visible, how they group and what a reset writes
//! are all answered by `solarxy_studio::params` and by the registry's own
//! visibility evaluator. Nothing here branches on a type identifier, and
//! a node type added in Rust gets a working panel with no change to this
//! file.
//!
//! ## Why this is not the Properties panel
//!
//! `panels/properties.rs` reports on the open file: its statistics, its
//! HDRI, its validation. It also carries an Actions section, whose own
//! header calls itself the seed of this panel, because it already
//! interprets the registry and branches on no node type. This is that
//! section generalized to every parameter. The two live side by side
//! until the panel work that collapses them, and Properties keeps its
//! name and its slug throughout, because those names are serialized into
//! a user's saved dock arrangement.

mod frame;

pub(crate) use frame::{ParamPanelSource, ParamPanelState, ParamScene, draw_params_content};
