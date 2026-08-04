#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::ignored_unit_patterns,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::semicolon_if_nothing_returned,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unnecessary_semicolon,
    clippy::unnested_or_patterns,
    clippy::wildcard_imports
)]

#[cfg(feature = "analyzer")]
pub mod calc;
pub mod parser;
#[cfg(feature = "tui")]
pub(crate) mod tui;
#[cfg(feature = "tui")]
pub mod tui_analysis;

/// Print every terminal theme this build can find, with a swatch of each.
///
/// The one entry point the binary needs into the terminal module tree, which
/// is otherwise crate-private so the panels behind it stay free to move.
#[cfg(feature = "tui")]
pub fn print_theme_listing() {
    tui::theme::print_listing();
}
#[cfg(feature = "tui")]
pub mod tui_theme;
mod validators;

// Re-export the validation orchestration library so existing call sites
// that referenced `solarxy_cli::validate::…` keep working after the
// extraction. New code should depend on `solarxy-validate` directly.
#[cfg(feature = "analyzer")]
pub use solarxy_validate as validate;
