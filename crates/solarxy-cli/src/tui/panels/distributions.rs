//! Which meshes the model's weight is actually in.
//!
//! Sorted descending, so the worst offender is the top row and the question
//! "what is making this asset heavy" is answered by looking rather than by
//! reading. The same series feeds the sparkline under the mesh table, so one
//! computation serves two surfaces rather than two computations drifting.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use solarxy_core::format_number;

use super::super::widgets;
use super::{Action, Ctx, Panel};

/// Which count the bars are of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum By {
    #[default]
    Triangles,
    Vertices,
}

impl By {
    fn next(self) -> Self {
        match self {
            Self::Triangles => Self::Vertices,
            Self::Vertices => Self::Triangles,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Triangles => "triangles per mesh",
            Self::Vertices => "vertices per mesh",
        }
    }
}

#[derive(Default)]
pub struct Distributions {
    pub by: By,
}

const NAME_WIDTH: u16 = 14;

impl Distributions {
    /// Every mesh's share, largest first.
    fn shares(&self, ctx: &Ctx<'_>) -> Vec<(String, u64)> {
        let mut rows: Vec<(String, u64)> = ctx
            .report
            .meshes
            .iter()
            .map(|mesh| {
                let name = if mesh.name.is_empty() {
                    format!("mesh {}", mesh.index)
                } else {
                    mesh.name.clone()
                };
                let count = match self.by {
                    By::Triangles => mesh.triangle_count,
                    By::Vertices => mesh.vertex_count,
                };
                (name, count as u64)
            })
            .collect();
        rows.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        rows
    }
}

impl Panel for Distributions {
    fn menu(&self) -> &'static [&'static str] {
        &["by"]
    }

    fn handle(&mut self, key: KeyEvent, _ctx: &Ctx<'_>) -> Action {
        if matches!(key.code, KeyCode::Char('b') | KeyCode::Char('B')) {
            self.by = self.by.next();
        }
        Action::None
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        let rows = self.shares(ctx);
        if rows.is_empty() {
            let (line, rect) = widgets::empty_state("no meshes to compare", area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }

        let total: u64 = rows.iter().map(|(_, count)| count).sum();
        let largest = rows.first().map_or(0, |(_, count)| *count);
        let bar_cells = area.width.saturating_sub(NAME_WIDTH + 10).clamp(3, 24);
        let series: Vec<u64> = rows.iter().map(|(_, count)| *count).collect();

        // Three rows are spent on the sparkline and its readout, so the bars
        // take what is left. A model with more meshes than that shows the
        // heaviest ones, which are the ones the panel exists to name, and the
        // status line says how many there are in total.
        let reserved = 4u16;
        let visible = usize::from(area.height.saturating_sub(reserved)).max(1);

        let mut lines: Vec<Line> = rows
            .iter()
            .take(visible)
            .map(|(name, count)| {
                let share = if total == 0 {
                    0.0
                } else {
                    *count as f64 / total as f64
                };
                let mut line = widgets::bar_row(
                    name,
                    *count,
                    largest,
                    NAME_WIDTH,
                    bar_cells,
                    widgets::Paint {
                        ink: ctx.theme.chart.first().copied().unwrap_or(ctx.theme.accent),
                        theme: ctx.theme,
                        glyphs: ctx.glyphs,
                    },
                );
                line.spans.push(Span::styled(
                    format!("  {:.1}%", share * 100.0),
                    Style::default().fg(ctx.theme.ink_dim),
                ));
                line
            })
            .collect();

        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            self.by.label().to_owned(),
            Style::default().fg(ctx.theme.ink_dim),
        )));
        lines.push(Line::from(Span::styled(
            widgets::sparkline(&series, ctx.glyphs),
            Style::default().fg(ctx.theme.chart.get(1).copied().unwrap_or(ctx.theme.accent)),
        )));

        let median = median(&series);
        lines.push(Line::from(Span::styled(
            format!(
                "median {}  max {}",
                format_number(median as usize),
                format_number(largest as usize)
            ),
            Style::default().fg(ctx.theme.ink),
        )));

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn status(&self, ctx: &Ctx<'_>) -> Option<String> {
        Some(format!("{} meshes", ctx.report.mesh_count))
    }
}

/// The middle value of a series already sorted descending.
///
/// The median rather than the mean, because one runaway mesh is the case this
/// panel exists for and it drags a mean somewhere no mesh actually is.
fn median(sorted: &[u64]) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        u64::midpoint(sorted[middle - 1], sorted[middle])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_metric_toggles_and_returns() {
        assert_eq!(By::Triangles.next(), By::Vertices);
        assert_eq!(By::Vertices.next(), By::Triangles);
    }

    /// One runaway mesh is exactly what this panel is for, and it is exactly
    /// what drags a mean away from anything real.
    #[test]
    fn the_median_is_not_dragged_by_one_runaway_mesh() {
        let series = [1_000_000u64, 10, 8, 6, 4];
        assert_eq!(median(&series), 8);
        let mean = series.iter().sum::<u64>() / series.len() as u64;
        assert!(mean > 100_000, "the fixture has a runaway");
    }

    #[test]
    fn an_even_series_takes_the_middle_pair() {
        assert_eq!(median(&[10, 8, 6, 4]), 7);
    }

    #[test]
    fn an_empty_series_has_no_median_rather_than_panicking() {
        assert_eq!(median(&[]), 0);
    }
}
