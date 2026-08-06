//! Materials, with the base colour shown as a colour.
//!
//! This is the clearest case for the whole tier model. The shipped shell
//! prints `0.820 0.640 0.550` where a terminal that has the colour could
//! simply show it, and a terminal that does not still gets the numbers. So the
//! swatch is drawn where it can be and the triple survives everywhere.
//!
//! A swatch paints scene data rather than a theme token, which is why it is
//! correctly outside the rule that the palette owns every colour: this is the
//! model's colour, not Solarxy's.
//!
//! Used-by resolves each material back to the meshes that reference it,
//! something the report has always known and no surface has shown.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Paragraph, Row, Table, TableState};
use solarxy_core::report::{AnalysisReport, MaterialSummary};

use super::super::widgets;
use super::{Action, Ctx, Panel, Sort};

/// The table's column headers, in sort-cycle order.
pub const COLUMNS: [&str; 4] = ["name", "base colour", "tex", "used by"];

#[derive(Default)]
pub struct Materials {
    pub sort: Sort,
    state: TableState,
}

impl Materials {
    fn rows<'a>(&self, report: &'a AnalysisReport) -> Vec<&'a MaterialSummary> {
        let mut rows: Vec<&MaterialSummary> = report.materials.iter().collect();
        match self.sort.column {
            1 => rows.sort_by(|a, b| luminance(a.diffuse).total_cmp(&luminance(b.diffuse))),
            2 => rows.sort_by_key(|m| m.textures.len()),
            3 => rows.sort_by_key(|m| users(report, m.index).len()),
            _ => rows.sort_by(|a, b| a.name.cmp(&b.name)),
        }
        if self.sort.descending {
            rows.reverse();
        }
        rows
    }
}

impl Panel for Materials {
    fn menu(&self) -> &'static [&'static str] {
        &["sort"]
    }

    fn handle(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> Action {
        let count = self.rows(ctx.report).len();
        match key.code {
            KeyCode::Char('s') | KeyCode::Char('S') => self.sort = self.sort.cycle(COLUMNS.len()),
            KeyCode::Down | KeyCode::Char('j') => step(&mut self.state, 1, count),
            KeyCode::Up | KeyCode::Char('k') => step(&mut self.state, -1, count),
            KeyCode::Char('g') => self.state.select((count > 0).then_some(0)),
            KeyCode::Char('G') => self.state.select(count.checked_sub(1)),
            _ => {}
        }
        Action::None
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        let rows = self.rows(ctx.report);
        if rows.is_empty() {
            let (line, rect) = widgets::empty_state("no materials in this model", area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }
        if self.state.selected().is_none_or(|i| i >= rows.len()) {
            self.state.select(Some(0));
        }

        let swatch_paints = ctx.caps.color.reads_a_theme();
        let header = Row::new(COLUMNS.iter().enumerate().map(|(i, name)| {
            let mut style = Style::default().fg(ctx.theme.ink_dim);
            if i == self.sort.column {
                style = style.fg(ctx.chrome()).add_modifier(Modifier::BOLD);
            }
            ratatui::text::Span::styled((*name).to_string(), style)
        }));

        let table_rows: Vec<Row> = rows
            .iter()
            .map(|material| {
                let triple = format!(
                    "{:.3} {:.3} {:.3}",
                    material.diffuse[0], material.diffuse[1], material.diffuse[2]
                );
                // The swatch is an addition where the terminal can show it,
                // never a replacement: the numbers are the part a reader can
                // act on and they stay at every tier.
                let colour = if swatch_paints {
                    // The swatch is filled with whatever block the glyph tier
                    // has. A terminal can have every colour and still not have
                    // the block element, and those are separate questions.
                    let (fill, _) = widgets::meter_glyphs(ctx.glyphs);
                    ratatui::text::Line::from(vec![
                        ratatui::text::Span::styled(
                            format!("{} ", fill.repeat(3)),
                            Style::default().fg(swatch(material.diffuse)),
                        ),
                        ratatui::text::Span::styled(
                            triple.clone(),
                            Style::default().fg(ctx.theme.ink),
                        ),
                    ])
                } else {
                    ratatui::text::Line::from(ratatui::text::Span::styled(
                        triple.clone(),
                        Style::default().fg(ctx.theme.ink),
                    ))
                };

                let used = users(ctx.report, material.index);
                Row::new(vec![
                    ratatui::text::Line::from(ratatui::text::Span::styled(
                        material.name.clone(),
                        Style::default().fg(ctx.theme.ink),
                    )),
                    colour,
                    ratatui::text::Line::from(ratatui::text::Span::styled(
                        material.textures.len().to_string(),
                        Style::default().fg(ctx.theme.ink),
                    )),
                    ratatui::text::Line::from(ratatui::text::Span::styled(
                        if used.is_empty() {
                            "-".to_owned()
                        } else {
                            used.join(", ")
                        },
                        Style::default().fg(ctx.theme.ink_dim),
                    )),
                ])
            })
            .collect();

        let widths = [
            Constraint::Min(8),
            Constraint::Length(if swatch_paints { 24 } else { 20 }),
            Constraint::Length(4),
            Constraint::Min(8),
        ];
        let table = Table::new(table_rows, widths)
            .header(header)
            .row_highlight_style(
                Style::default()
                    .fg(ctx.theme.ink)
                    .bg(if ctx.caps.color.paints_a_ground() {
                        ctx.theme.selection
                    } else {
                        Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_stateful_widget(table, area, &mut self.state);
    }

    fn reveal(&mut self, row: usize) -> bool {
        self.state.select(Some(row));
        true
    }

    fn status(&self, ctx: &Ctx<'_>) -> Option<String> {
        let slots: usize = ctx.report.materials.iter().map(|m| m.textures.len()).sum();
        let missing = ctx
            .report
            .materials
            .iter()
            .flat_map(|m| &m.textures)
            .filter(|t| !t.exists)
            .count();
        Some(format!(
            "{} materials \u{b7} {slots} texture slots \u{b7} {missing} missing",
            ctx.report.material_count
        ))
    }
}

/// The meshes that reference a material, by name.
fn users(report: &AnalysisReport, index: usize) -> Vec<String> {
    report
        .meshes
        .iter()
        .filter(|mesh| mesh.material_id == Some(index))
        .map(|mesh| {
            if mesh.name.is_empty() {
                format!("mesh {}", mesh.index)
            } else {
                mesh.name.clone()
            }
        })
        .collect()
}

/// A linear colour triple as a terminal colour.
fn swatch(rgb: [f32; 3]) -> Color {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::Rgb(channel(rgb[0]), channel(rgb[1]), channel(rgb[2]))
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

fn step(state: &mut TableState, delta: i32, count: usize) {
    if count == 0 {
        state.select(None);
        return;
    }
    let current = state.selected().unwrap_or(0) as i32;
    state.select(Some((current + delta).clamp(0, count as i32 - 1) as usize));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_triple_becomes_the_colour_it_names() {
        assert_eq!(swatch([1.0, 0.0, 0.5]), Color::Rgb(255, 0, 128));
        assert_eq!(swatch([0.0, 0.0, 0.0]), Color::Rgb(0, 0, 0));
    }

    /// Out-of-range values are real: some formats carry emissive or
    /// unnormalised colours, and they must not wrap to a wrong hue.
    #[test]
    fn an_out_of_range_triple_is_clamped_rather_than_wrapped() {
        assert_eq!(swatch([2.5, -1.0, 0.5]), Color::Rgb(255, 0, 128));
    }
}
