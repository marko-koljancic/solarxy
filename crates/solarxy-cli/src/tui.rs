//! Shared line builders for the analyze TUI.
//!
//! Every color arrives as a [`TuiTheme`] rather than being named here, so
//! this shell stays a pure consumer of the palette in `solarxy_core::theme`.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::tui_theme::TuiTheme;

pub(crate) mod app;
pub(crate) mod caps;
pub(crate) mod contrast;
// Arrange mode's grammar waits for the keymap that binds it; the tiled
// surface consumes the tree already.
#[allow(dead_code)]
pub(crate) mod arrange;
pub(crate) mod geometry;
pub(crate) mod keymap;
pub(crate) mod layout;
// Writing the file, and reading the arrangement back out of it, belong to
// the quit path and the loop that owns it. Reading the theme already has a
// caller. Removed with the allow above when the loop lands.
#[allow(dead_code)]
pub(crate) mod overlay;
pub(crate) mod panels;
pub(crate) mod prefs;
pub(crate) mod raster;
pub(crate) mod scroll;
pub(crate) mod shell;
pub(crate) mod theme;
pub(crate) mod uv;
pub(crate) mod widgets;

/// The reference panel and the shared render-test machinery.
///
/// Test-only, and in the library rather than under `tests/` because this
/// module tree is `pub(crate)`: an integration test cannot see it.
#[cfg(test)]
pub(crate) mod harness;

pub(crate) fn section_header(text: &str, theme: &TuiTheme) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default()
            .fg(theme.heading)
            .add_modifier(Modifier::BOLD),
    ))
}

pub(crate) fn kv_line(label: &str, value: &str, theme: &TuiTheme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}:", label),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(value.to_string(), Style::default().fg(theme.text)),
    ])
}
