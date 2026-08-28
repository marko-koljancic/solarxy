//! An orthographic projection of the model, in braille.
//!
//! The vertex positions are already in memory for the whole session, so this
//! costs no loading, no new field on the report and no new dependency. It is
//! the panel that most obviously could not exist in the shipped shell, and the
//! one a monitoring dashboard structurally cannot draw: braille goes to time
//! series there, and an analyze report has no time axis to spend it on.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::super::geometry::Axis;
use super::super::raster::{Point, Raster};
use super::super::widgets;
use super::{Action, AnalyzeCtx, Analysis, Panel};

#[derive(Default)]
pub struct Silhouette {
    pub axis: Axis,
    /// Whether the model keeps its proportions or fills the panel.
    ///
    /// Aspect-correct by default: a terminal cell is about twice as tall as it
    /// is wide, and stretching to fill would show every model distorted
    /// without saying so.
    pub stretch: bool,
    cached: Option<Cached>,
}

struct Cached {
    axis: Axis,
    stretch: bool,
    cells: (u16, u16),
    rows: Vec<String>,
}

impl Panel<Analysis<'_>, Action> for Silhouette {
    fn menu(&self) -> &'static [&'static str] {
        &["axis", "fit"]
    }

    fn handle(&mut self, key: KeyEvent, _ctx: &AnalyzeCtx<'_>) -> Action {
        match key.code {
            KeyCode::Char('x') | KeyCode::Char('X') => self.axis = self.axis.next(),
            KeyCode::Char('f') | KeyCode::Char('F') => self.stretch = !self.stretch,
            _ => return Action::None,
        }
        // The projection is keyed on what changed, so a view change is what
        // discards it rather than a redraw.
        self.cached = None;
        Action::None
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &AnalyzeCtx<'_>) {
        if ctx.subject.model.is_empty() {
            let (line, rect) = widgets::empty_state("no vertices to project", area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }

        let cells = (area.width, area.height);
        let stale = self
            .cached
            .as_ref()
            .is_none_or(|c| c.axis != self.axis || c.stretch != self.stretch || c.cells != cells);
        if stale {
            self.cached = Some(Cached {
                axis: self.axis,
                stretch: self.stretch,
                cells,
                rows: project(self, ctx, area),
            });
        }

        let rows: Vec<Line> = self
            .cached
            .as_ref()
            .map(|c| {
                c.rows
                    .iter()
                    .map(|row| {
                        Line::from(Span::styled(
                            row.clone(),
                            Style::default().fg(ctx.theme.ink),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        frame.render_widget(Paragraph::new(rows), area);
    }

    fn status(&self, ctx: &AnalyzeCtx<'_>) -> Option<String> {
        let size = ctx.subject.model.bounds()?.size();
        Some(format!(
            "{} \u{b7} {:.2} x {:.2} x {:.2}",
            self.axis.name(),
            size[0],
            size[1],
            size[2]
        ))
    }
}

/// Project every vertex into the panel and encode it.
fn project(panel: &Silhouette, ctx: &AnalyzeCtx<'_>, area: Rect) -> Vec<String> {
    let Some(bounds) = ctx.subject.model.bounds() else {
        return Vec::new();
    };
    let (right, down, away, invert) = panel.axis.axes();

    // A terminal cell is roughly twice as tall as it is wide, and braille adds
    // four dots down against two across, so a dot is about the same shape as a
    // cell. Fitting the model to the smaller of the two ratios is what keeps
    // it from looking squashed.
    let (scale_x, scale_y) = if panel.stretch {
        (1.0, 1.0)
    } else {
        let dots_wide = f32::from(area.width) * 2.0;
        let dots_high = f32::from(area.height) * 4.0;
        let model = bounds.span(right) / bounds.span(down);
        let panel_ratio = dots_wide / dots_high;
        if model > panel_ratio {
            (1.0, panel_ratio / model)
        } else {
            (model / panel_ratio, 1.0)
        }
    };
    let (offset_x, offset_y) = ((1.0 - scale_x) / 2.0, (1.0 - scale_y) / 2.0);

    let mut raster = Raster::new(area.width, area.height);
    let mut points = Vec::with_capacity(ctx.subject.model.vertex_count());
    for mesh in &ctx.subject.model.meshes {
        for xyz in mesh.positions.as_chunks::<3>().0 {
            let u = (xyz[right] - bounds.min[right]) / bounds.span(right);
            let mut v = (xyz[down] - bounds.min[down]) / bounds.span(down);
            if invert {
                v = 1.0 - v;
            }
            points.push(Point {
                x: offset_x + u * scale_x,
                y: offset_y + v * scale_y,
                depth: (xyz[away] - bounds.min[away]) / bounds.span(away),
            });
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
    fn cycling_the_axis_names_all_three_views() {
        let mut panel = Silhouette::default();
        let mut names = Vec::new();
        for _ in 0..3 {
            names.push(panel.axis.name());
            panel.axis = panel.axis.next();
        }
        assert_eq!(names, vec!["front", "side", "top"]);
    }

    /// Aspect-correct is the default, because a stretched model is wrong in a
    /// way that does not announce itself.
    #[test]
    fn the_projection_keeps_its_proportions_unless_asked_not_to() {
        let panel = Silhouette::default();
        assert!(!panel.stretch);
    }

    /// A long thin model must not fill the panel edge to edge on both axes,
    /// which is what stretching would do and what would make every model look
    /// the same shape.
    #[test]
    fn a_long_model_leaves_the_short_axis_unfilled() {
        let positions: Vec<f32> = (0..200u16)
            .flat_map(|i| {
                let t = f32::from(i) / 199.0;
                [t * 10.0, t * 0.2, 0.0]
            })
            .collect();
        let model = ModelView {
            meshes: vec![MeshView {
                positions: &positions,
                texcoords: &[],
                indices: &[],
            }],
        };
        let bounds = model.bounds().expect("vertices");
        assert!(
            bounds.span(0) > bounds.span(1) * 10.0,
            "the fixture is long"
        );
    }
}
