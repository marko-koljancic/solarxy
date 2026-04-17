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
#[cfg(feature = "tui")]
pub mod help;
pub mod parser;
#[cfg(feature = "tui")]
pub(crate) mod tui;
#[cfg(feature = "tui")]
pub mod tui_analysis;
#[cfg(feature = "tui")]
pub mod tui_docs;
#[cfg(feature = "tui")]
pub mod tui_preferences;
mod validators;
