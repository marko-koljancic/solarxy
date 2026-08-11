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
#[cfg(feature = "render")]
pub mod render_sink;
/// The render dashboard. Needs both halves: the stream to report, and the
/// terminal machinery to draw it on.
#[cfg(all(feature = "render", feature = "tui"))]
pub mod render_tui;
#[cfg(feature = "tui")]
pub mod tui;
mod validators;

// Re-export the validation orchestration library so existing call sites
// that referenced `solarxy_cli::validate::…` keep working after the
// extraction. New code should depend on `solarxy-validate` directly.
#[cfg(feature = "analyzer")]
pub use solarxy_validate as validate;
