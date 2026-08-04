//! Shared line builders for the analyze TUI.
//!
//! Every color arrives as a [`TuiTheme`] rather than being named here, so
//! this shell stays a pure consumer of the palette in `solarxy_core::theme`.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::tui_theme::TuiTheme;

pub(crate) mod caps;

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
