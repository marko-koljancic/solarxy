//! The bounding box, plus the one thing five decimal triples never show.
//!
//! A reader can stare at `2.086 1.884 1.424` for a while before noticing the
//! asset is fine, and at `2.086 1.884 0.004` for just as long before noticing
//! it is flat. Three bars against the longest axis answer that at a glance,
//! which is the whole reason this panel is not simply the numbers the report
//! already prints.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::super::widgets;
use super::{Action, AnalyzeCtx, Analysis, Panel};

/// The axis-aligned bounds panel: extents, centre, and the axis spans.
pub struct Bounds;

const LABEL: u16 = 9;

impl Panel<Analysis<'_>, Action> for Bounds {
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &AnalyzeCtx<'_>) {
        let Some(bounds) = &ctx.subject.report.bounds else {
            let (line, rect) = widgets::empty_state("no bounding box", area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        };

        let triple = |v: &[f32; 3]| format!("{:.3}  {:.3}  {:.3}", v[0], v[1], v[2]);
        let mut lines: Vec<Line> = [
            ("min", triple(&bounds.min)),
            ("max", triple(&bounds.max)),
            ("size", triple(&bounds.size)),
            ("centre", triple(&bounds.center)),
            ("diagonal", format!("{:.3}", bounds.diagonal)),
        ]
        .into_iter()
        .map(|(label, value)| Line::from(widgets::field(label, &value, LABEL, ctx.theme)))
        .collect();

        lines.push(Line::raw(""));

        // Scaled against the longest axis rather than against an absolute
        // unit, because the question is proportion: a model is flat or it is
        // not, whatever units it was built in.
        let longest = bounds
            .size
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(f32::EPSILON);
        let bar_cells = area.width.saturating_sub(LABEL + 12).clamp(3, 24);
        for (axis, extent) in ["x", "y", "z"].into_iter().zip(bounds.size) {
            lines.push(widgets::meter(
                f64::from(extent / longest),
                bar_cells,
                &format!("{extent:.3}"),
                widgets::Paint {
                    ink: ctx.theme.chart.first().copied().unwrap_or(ctx.theme.accent),
                    theme: ctx.theme,
                    glyphs: ctx.glyphs,
                },
            ));
            // The axis letter goes in front of the bar it belongs to, which
            // the shared meter does not carry and should not have to.
            if let Some(line) = lines.last_mut() {
                line.spans.insert(
                    0,
                    ratatui::text::Span::styled(
                        format!("{axis} "),
                        ratatui::style::Style::default().fg(ctx.theme.ink_dim),
                    ),
                );
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn status(&self, ctx: &AnalyzeCtx<'_>) -> Option<String> {
        let bounds = ctx.subject.report.bounds.as_ref()?;
        // Naming the shape is the panel's judgement, not the reader's
        // arithmetic. A ratio past a hundred is not a proportion anyone
        // designed on purpose.
        let longest = bounds.size.iter().copied().fold(0.0_f32, f32::max);
        let shortest = bounds.size.iter().copied().fold(f32::MAX, f32::min);
        let shape = if shortest <= 0.0 || longest / shortest > 100.0 {
            "flat"
        } else {
            "solid"
        };
        Some(format!("diagonal {:.3} \u{b7} {shape}", bounds.diagonal))
    }
}

#[cfg(test)]
mod tests {
    use solarxy_core::report::BoundsSummary;

    fn shape_of(size: [f32; 3]) -> &'static str {
        let longest = size.iter().copied().fold(0.0_f32, f32::max);
        let shortest = size.iter().copied().fold(f32::MAX, f32::min);
        if shortest <= 0.0 || longest / shortest > 100.0 {
            "flat"
        } else {
            "solid"
        }
    }

    #[test]
    fn a_normal_model_is_not_called_flat() {
        assert_eq!(shape_of([2.086, 1.884, 1.424]), "solid");
        assert_eq!(shape_of([10.0, 1.0, 5.0]), "solid");
    }

    /// The case the bars exist for: a model whose thickness vanished, which
    /// three decimal triples state without making obvious.
    #[test]
    fn a_model_with_no_thickness_is_called_flat() {
        assert_eq!(shape_of([2.086, 1.884, 0.0]), "flat");
        assert_eq!(shape_of([2.086, 1.884, 0.004]), "flat");
    }

    #[test]
    fn a_bounds_summary_carries_everything_the_panel_prints() {
        let bounds = BoundsSummary {
            min: [-1.043, 0.0, -0.712],
            max: [1.043, 1.884, 0.712],
            size: [2.086, 1.884, 1.424],
            center: [0.0, 0.942, 0.0],
            diagonal: 3.128,
        };
        assert_eq!(shape_of(bounds.size), "solid");
        assert!(bounds.diagonal > 0.0);
    }
}
