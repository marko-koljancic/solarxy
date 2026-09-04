//! Severity counts, and a bar for every issue kind that occurred.
//!
//! Eleven kinds ride on every validation issue and until 0.8.2 no surface read
//! one. This panel is pure recovered information: nothing here needed new
//! analysis, only somewhere to put what was already computed.
//!
//! Bars are sorted descending and scaled against the largest kind rather than
//! against the total, because the question is which problem dominates, and a
//! scale against the total flattens everything when one kind runs away.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use solarxy_core::validation::Severity;

use super::super::widgets;
use super::{Action, AnalyzeCtx, Analysis, Panel};

/// Which severities are shown. Cycles with the panel's one menu word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    ErrorsOnly,
    WarningsOnly,
}

impl Filter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::ErrorsOnly,
            Self::ErrorsOnly => Self::WarningsOnly,
            Self::WarningsOnly => Self::All,
        }
    }

    fn admits(self, severity: Severity) -> bool {
        match self {
            Self::All => true,
            Self::ErrorsOnly => severity == Severity::Error,
            Self::WarningsOnly => severity == Severity::Warning,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::ErrorsOnly => "errors",
            Self::WarningsOnly => "warnings",
        }
    }
}

#[derive(Default)]
pub struct Health {
    pub filter: Filter,
}

const NAME_WIDTH: u16 = 18;

impl Panel<Analysis<'_>, Action> for Health {
    fn menu(&self) -> &'static [&'static str] {
        &["filter"]
    }

    fn handle(&mut self, key: KeyEvent, _ctx: &AnalyzeCtx<'_>) -> Action {
        if matches!(key.code, KeyCode::Char('/')) {
            self.filter = self.filter.next();
        }
        Action::None
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &AnalyzeCtx<'_>) {
        let validation = &ctx.subject.report.validation;
        let errors = validation.error_count();
        let warnings = validation.warning_count();

        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("{} {errors} errors", ctx.glyphs.cross),
                    Style::default()
                        .fg(ctx.theme.error)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("   "),
                Span::styled(
                    format!("{} {warnings} warnings", ctx.glyphs.warn),
                    Style::default()
                        .fg(ctx.theme.warning)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::raw(""),
        ];

        let ranked: Vec<_> = validation
            .ranked_kinds()
            .into_iter()
            .filter(|(kind, _)| {
                self.filter.admits(kind_severity(*kind, validation)) || self.filter == Filter::All
            })
            .collect();

        if ranked.is_empty() {
            let (line, rect) = widgets::empty_state("no issues found", area, ctx.theme);
            frame.render_widget(Paragraph::new(lines), area);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }

        let largest = ranked.first().map_or(0, |(_, count)| *count as u64);
        let bar_cells = area.width.saturating_sub(NAME_WIDTH + 8).clamp(3, 24);
        for (kind, count) in ranked {
            let severity = kind_severity(kind, validation);
            let ink = match severity {
                Severity::Error => ctx.theme.error,
                Severity::Warning => ctx.theme.warning,
            };
            lines.push(widgets::bar_row(
                &kind.to_string(),
                count as u64,
                largest,
                NAME_WIDTH,
                bar_cells,
                widgets::Paint {
                    ink,
                    theme: ctx.theme,
                    glyphs: ctx.glyphs,
                },
            ));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn status(&self, ctx: &AnalyzeCtx<'_>) -> Option<String> {
        let kinds = ctx.subject.report.validation.ranked_kinds().len();
        Some(format!("{} kinds \u{b7} {}", kinds, self.filter.label()))
    }
}

/// The severity a kind actually occurred at in this report.
///
/// Taken from the issues rather than from a table, because the same kind can
/// be raised at either severity depending on the project's rules, and guessing
/// would colour a bar wrong on exactly the assets that matter.
fn kind_severity(
    kind: solarxy_core::validation::IssueKind,
    report: &solarxy_core::validation::ValidationReport,
) -> Severity {
    report
        .issues
        .iter()
        .find(|issue| issue.kind == kind)
        .map_or(Severity::Warning, |issue| issue.severity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_severity_filter_cycles_and_returns() {
        let mut filter = Filter::All;
        for _ in 0..3 {
            filter = filter.next();
        }
        assert_eq!(filter, Filter::All);
    }

    #[test]
    fn each_filter_admits_what_it_says_it_does() {
        assert!(Filter::All.admits(Severity::Error));
        assert!(Filter::All.admits(Severity::Warning));
        assert!(Filter::ErrorsOnly.admits(Severity::Error));
        assert!(!Filter::ErrorsOnly.admits(Severity::Warning));
        assert!(Filter::WarningsOnly.admits(Severity::Warning));
        assert!(!Filter::WarningsOnly.admits(Severity::Error));
    }
}
