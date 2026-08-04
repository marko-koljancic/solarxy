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

/// Run the tiled analyze surface over a report.
///
/// Reachable only through `SOLARXY_TUI=next` until the cutover, which is what
/// lets the panels be looked at as they are built rather than all at once at
/// the end. Both the switch and this entry point are temporary.
#[cfg(all(feature = "tui", feature = "analyzer"))]
pub fn run_tiled_analyze(
    report: &solarxy_core::report::AnalysisReport,
    requested_theme: Option<&str>,
) -> std::io::Result<()> {
    use tui::app::App;
    use tui::caps::Capabilities;
    use tui::layout::Preset;
    use tui::theme::{Theme, ThemeSet};

    let caps = Capabilities::detect();
    let (prefs, notices) = tui::prefs::TuiPrefs::load();
    let wanted = requested_theme.or(prefs.theme.as_deref());
    let (resolved, theme_notices) = ThemeSet::load().resolve(wanted, caps);
    for notice in notices {
        tracing::warn!("{notice}");
    }
    for notice in theme_notices {
        tracing::warn!("{notice}");
    }
    let _ = Theme::resolve(caps, &resolved.name, &resolved.slots);

    let layout = prefs
        .opening_layout()
        .unwrap_or_else(|| Preset::Survey.layout());
    let mut app = App::new(report, layout, resolved, caps);
    tui::shell::run(&mut app)
}

/// Whether the reader asked for the tiled surface.
#[cfg(feature = "tui")]
pub fn tiled_analyze_requested() -> bool {
    tui::app::opted_in(|key| std::env::var(key).ok())
}

/// Why this terminal is too small for the analyze surface, if it is.
///
/// The second entry point the binary needs into the terminal module tree,
/// asked before the screen is taken so the notice reaches the reader's own
/// terminal rather than an alternate one about to disappear.
#[cfg(feature = "tui")]
pub fn terminal_floor_notice(width: u16, height: u16) -> Option<String> {
    tui::shell::below_floor(width, height)
}
#[cfg(feature = "tui")]
pub mod tui_theme;
mod validators;

// Re-export the validation orchestration library so existing call sites
// that referenced `solarxy_cli::validate::…` keep working after the
// extraction. New code should depend on `solarxy-validate` directly.
#[cfg(feature = "analyzer")]
pub use solarxy_validate as validate;
