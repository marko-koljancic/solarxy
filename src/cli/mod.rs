#[cfg(feature = "tui")]
pub mod help;
pub mod parser;
#[cfg(feature = "tui")]
pub mod tui;
#[cfg(all(feature = "tui", feature = "analyzer"))]
pub mod tui_analysis;
#[cfg(feature = "tui")]
pub mod tui_docs;
#[cfg(feature = "tui")]
pub mod tui_preferences;
mod validators;
