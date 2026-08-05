//! The analyze surface: a tiled workspace over one model's report.
//!
//! This file is the tree's root and nothing else. The two line builders that
//! used to live here served the tabbed shell that this module replaced, and
//! they left with it.
//!
//! # What is public, and why
//!
//! The binary assembles the surface itself: it detects the terminal, loads
//! preferences, resolves a theme, builds the app and runs it. That is a
//! deliberate shape rather than a convenience. Everything above `panels`
//! knows nothing about [`solarxy_core::report::AnalysisReport`], so a second
//! consumer takes the capability model, the theme system, the split tree, the
//! keymap and the rasteriser, and supplies its own panels. Wrapping that in
//! one opaque entry point would hide the seam that makes the reuse cheap.
//!
//! The cost is that these modules are part of this crate's public interface
//! and cannot be reshaped without a version that says so.

pub mod app;
pub mod arrange;
pub mod caps;
pub mod contrast;
pub mod geometry;
pub mod keymap;
pub mod layout;
pub mod overlay;
pub mod panels;
pub mod prefs;
pub mod raster;
pub mod scroll;
pub mod shell;
pub mod theme;
pub mod uv;
pub mod widgets;

/// The reference panel and the shared render-test machinery.
///
/// Test-only, and in the library rather than under `tests/` because an
/// integration test compiles against the crate from outside and would see
/// none of the state these helpers assert against.
#[cfg(test)]
pub(crate) mod harness;
