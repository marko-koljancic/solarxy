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
//! deliberate shape rather than a convenience. The substrate is report-free
//! and reusable as is: `shell`, `caps`, `contrast`, `theme`, `raster`,
//! `scroll`, `widgets`, the `arrange` grammar and the `keymap` machinery
//! know nothing about [`solarxy_core::report::AnalysisReport`]. The analyze
//! half is `app` and `panels`, which hold the report, and `layout`, whose
//! `PanelType` names this surface's ten panels, so a second surface supplies
//! its own panel contract and vocabulary rather than merely its own panels;
//! that genericization is the extraction a later release performs. Wrapping
//! all of it in one opaque entry point would hide the seam that makes the
//! reuse cheap.
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
