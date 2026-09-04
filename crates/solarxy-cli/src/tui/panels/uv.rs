//! The UV layout, drawn rather than merely counted.
//!
//! Coverage, overlap and wasted area are the three numbers a delivery check
//! turns on, and none is reachable in the shipped shell. But a number says an
//! asset has overlap; a picture says where. That difference is the whole
//! reason this panel plots rather than reporting three percentages, and it is
//! why the overlapping texels take the error hue: the panel shows the problem
//! rather than announcing it.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::super::raster::{Point, Raster};
use super::super::uv::{Coverage, Occupancy};
use super::super::widgets;
use super::{Action, AnalyzeCtx, Analysis, Panel};

#[derive(Default)]
pub struct Uv {
    /// Whether the view frames the whole square or just what is used.
    ///
    /// The square by default, because how much of the layout is empty is one
    /// of the three things this panel is for. Zooming is for reading an atlas
    /// that occupies a corner.
    pub zoomed: bool,
    coverage: Option<Coverage>,
    cached: Option<Cached>,
}

struct Cached {
    zoomed: bool,
    cells: (u16, u16),
    covered: Vec<String>,
    overlapping: Vec<String>,
}

impl Panel<Analysis<'_>, Action> for Uv {
    fn menu(&self) -> &'static [&'static str] {
        &["set", "zoom"]
    }

    fn handle(&mut self, key: KeyEvent, _ctx: &AnalyzeCtx<'_>) -> Action {
        if matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z')) {
            self.zoomed = !self.zoomed;
            self.cached = None;
        }
        Action::None
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &AnalyzeCtx<'_>) {
        if !ctx.subject.model.has_uvs() {
            let (line, rect) = widgets::empty_state("no uv coordinates", area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }

        let zoomed = self.zoomed;
        let cells = (area.width, area.height);
        let stale = self
            .cached
            .as_ref()
            .is_none_or(|c| c.zoomed != zoomed || c.cells != cells);
        // The layout cannot change while a report is open, so the grid is
        // rasterised once for the session. A resize or a zoom re-plots it into
        // the new panel, which is cheap; re-rasterising a quarter of a million
        // triangles on every drag of a window edge would not be.
        let coverage = self
            .coverage
            .get_or_insert_with(|| Coverage::rasterise(ctx.subject.model));
        if stale {
            let window = if zoomed {
                coverage.used_bounds().unwrap_or((0.0, 0.0, 1.0, 1.0))
            } else {
                (0.0, 0.0, 1.0, 1.0)
            };
            self.cached = Some(Cached {
                zoomed,
                cells,
                covered: plot(coverage, window, area, ctx, 1),
                overlapping: plot(coverage, window, area, ctx, 2),
            });
        }

        let Some(cached) = &self.cached else { return };
        // Two layers over each other: everything covered in the ink, then the
        // overlapping texels in the error hue on top. Drawing overlap second
        // is what makes it visible where the two coincide, which is every
        // texel it occupies.
        let base: Vec<Line> = cached
            .covered
            .iter()
            .map(|row| {
                Line::from(Span::styled(
                    row.clone(),
                    Style::default().fg(ctx.theme.ink),
                ))
            })
            .collect();
        frame.render_widget(Paragraph::new(base), area);

        let over: Vec<Line> = cached
            .overlapping
            .iter()
            .map(|row| {
                Line::from(Span::styled(
                    // Blanks let the layer beneath show through rather than
                    // erasing it, so the plot is one picture and not two.
                    row.clone(),
                    Style::default().fg(ctx.theme.error),
                ))
            })
            .collect();
        for (index, line) in over.into_iter().enumerate() {
            let row = area.y + u16::try_from(index).unwrap_or(u16::MAX);
            if row >= area.bottom() {
                break;
            }
            render_transparent(frame, Rect::new(area.x, row, area.width, 1), line);
        }
    }

    fn status(&self, ctx: &AnalyzeCtx<'_>) -> Option<String> {
        let occupancy = self.coverage.as_ref().map_or(
            Occupancy {
                coverage: 0.0,
                overlap: 0.0,
            },
            Coverage::occupancy,
        );
        let _ = ctx;
        Some(format!(
            "coverage {:.0}%  overlap {:.1}%  wasted {:.0}%",
            occupancy.coverage * 100.0,
            occupancy.overlap * 100.0,
            occupancy.wasted() * 100.0
        ))
    }
}

/// Draw a line without painting over what a blank cell already covers.
fn render_transparent(frame: &mut Frame, area: Rect, line: Line<'static>) {
    let mut column = area.x;
    for span in line.spans {
        for glyph in span.content.chars() {
            if column >= area.right() {
                return;
            }
            if glyph != ' ' && glyph != '\u{2800}' {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(glyph.to_string(), span.style))),
                    Rect::new(column, area.y, 1, 1),
                );
            }
            column += 1;
        }
    }
}

/// Plot every texel covered at least `least` times.
fn plot(
    coverage: &Coverage,
    window: (f32, f32, f32, f32),
    area: Rect,
    ctx: &AnalyzeCtx<'_>,
    least: u16,
) -> Vec<String> {
    let (lo_x, lo_y, hi_x, hi_y) = window;
    let (span_x, span_y) = (
        (hi_x - lo_x).max(f32::EPSILON),
        (hi_y - lo_y).max(f32::EPSILON),
    );

    let mut raster = Raster::new(area.width, area.height);
    let mut points = Vec::new();
    // One sample per dot rather than per texel: the grid is far finer than any
    // panel, and sampling at the panel's own resolution is both faster and
    // exactly as informative.
    let (dots_wide, dots_high) = (raster.dots_wide(), raster.dots_high());
    for row in 0..dots_high {
        for column in 0..dots_wide {
            let u = (f32::from(column) + 0.5) / f32::from(dots_wide.max(1));
            let v = (f32::from(row) + 0.5) / f32::from(dots_high.max(1));
            if coverage.at(lo_x + u * span_x, lo_y + v * span_y) >= least {
                points.push(Point::flat(u, v));
            }
        }
    }
    raster.plot(&points);
    raster.render(ctx.glyphs.plot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::geometry::{MeshView, ModelView};

    #[test]
    fn the_whole_square_is_the_default_view() {
        assert!(!Uv::default().zoomed);
    }

    /// A quarter-covered layout has to report three quarters wasted, which is
    /// the number a delivery check is actually looking at.
    #[test]
    fn the_status_line_reports_all_three_numbers() {
        let uv = vec![0.0, 0.0, 0.5, 0.0, 0.5, 0.5, 0.0, 0.5];
        let indices = vec![0u32, 1, 2, 0, 2, 3];
        let model = ModelView {
            meshes: vec![MeshView {
                positions: &[],
                texcoords: &uv,
                indices: &indices,
            }],
        };
        let occupancy = Coverage::rasterise(&model).occupancy();
        assert!((occupancy.coverage - 0.25).abs() < 0.005);
        assert!((occupancy.wasted() - 0.75).abs() < 0.005);
    }
}
