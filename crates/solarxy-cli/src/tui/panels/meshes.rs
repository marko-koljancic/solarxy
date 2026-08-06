//! The mesh table: a real table with a selected row, a sort and a filter.
//!
//! Mesh names are the recovered fact here. Loaders have always carried them
//! and the analyzer dropped them during conversion, so before 0.8.2 every
//! surface could only ever say `Mesh [2]`. A table that names its rows is the
//! difference between a report you can act on and one you have to cross
//! reference against the file.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row, Table, TableState};
use solarxy_core::format_number;
use solarxy_core::report::MeshSummary;

use super::super::widgets;
use super::{Action, Ctx, Panel, Sort};

/// The table's column headers, in sort-cycle order.
pub const COLUMNS: [&str; 6] = ["name", "tris", "verts", "nrm", "uv", "material"];

#[derive(Default)]
pub struct Meshes {
    pub sort: Sort,
    pub filter: Option<String>,
    state: TableState,
}

impl Meshes {
    /// The rows this panel is currently showing, in order.
    ///
    /// Filtering and sorting are resolved together and on every draw rather
    /// than cached, because the alternative is a cache that has to be
    /// invalidated from two places and eventually is not.
    fn rows<'a>(&self, ctx: &Ctx<'a>) -> Vec<&'a MeshSummary> {
        let needle = self.filter.as_deref().unwrap_or("").to_lowercase();
        let mut rows: Vec<&MeshSummary> = ctx
            .report
            .meshes
            .iter()
            .filter(|mesh| needle.is_empty() || display_name(mesh).to_lowercase().contains(&needle))
            .collect();

        match self.sort.column {
            1 => rows.sort_by_key(|m| m.triangle_count),
            2 => rows.sort_by_key(|m| m.vertex_count),
            3 => rows.sort_by_key(|m| m.normal_count),
            4 => rows.sort_by_key(|m| m.texcoord_count),
            5 => rows.sort_by(|a, b| {
                a.material_name
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.material_name.as_deref().unwrap_or(""))
            }),
            _ => rows.sort_by_key(|m| display_name(m)),
        }
        if self.sort.descending {
            rows.reverse();
        }
        rows
    }

    /// The triangle share series, largest first.
    ///
    /// Computed here and read by the distributions panel too, so one
    /// computation serves both the sparkline below and the histogram beside
    /// it.
    pub fn triangle_series(report: &solarxy_core::report::AnalysisReport) -> Vec<u64> {
        let mut series: Vec<u64> = report
            .meshes
            .iter()
            .map(|m| m.triangle_count as u64)
            .collect();
        series.sort_unstable_by(|a, b| b.cmp(a));
        series
    }
}

impl Panel for Meshes {
    fn menu(&self) -> &'static [&'static str] {
        &["sort", "filter"]
    }

    fn handle(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> Action {
        let count = self.rows(ctx).len();
        match key.code {
            KeyCode::Char('s') | KeyCode::Char('S') => self.sort = self.sort.cycle(COLUMNS.len()),
            KeyCode::Down | KeyCode::Char('j') => move_selection(&mut self.state, 1, count),
            KeyCode::Up | KeyCode::Char('k') => move_selection(&mut self.state, -1, count),
            KeyCode::Char('g') => self.state.select(if count == 0 { None } else { Some(0) }),
            KeyCode::Char('G') => self.state.select(count.checked_sub(1)),
            _ => {}
        }
        Action::None
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        let rows = self.rows(ctx);
        if rows.is_empty() {
            let what = if ctx.report.meshes.is_empty() {
                "no meshes in this model"
            } else {
                "no mesh matches the filter"
            };
            let (line, rect) = widgets::empty_state(what, area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }

        // Keep a selection valid across a filter that shrank the table under
        // it, rather than leaving it pointing past the end.
        if self.state.selected().is_none_or(|i| i >= rows.len()) {
            self.state.select(Some(0));
        }

        // One row held back for the sparkline strip along the bottom.
        let table_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));

        let header = Row::new(COLUMNS.iter().enumerate().map(|(i, name)| {
            let mut style = Style::default().fg(ctx.theme.ink_dim);
            if i == self.sort.column {
                style = style.fg(ctx.chrome()).add_modifier(Modifier::BOLD);
            }
            Span::styled((*name).to_string(), style)
        }));

        let table_rows: Vec<Row> = rows
            .iter()
            .map(|mesh| {
                let has = |count: usize| {
                    if count > 0 {
                        Span::styled(
                            ctx.glyphs.check.to_string(),
                            Style::default().fg(ctx.theme.success),
                        )
                    } else {
                        Span::styled(
                            ctx.glyphs.cross.to_string(),
                            Style::default().fg(ctx.theme.error),
                        )
                    }
                };
                Row::new(vec![
                    Span::styled(display_name(mesh), Style::default().fg(ctx.theme.ink)),
                    Span::styled(
                        format_number(mesh.triangle_count),
                        Style::default().fg(ctx.theme.ink),
                    ),
                    Span::styled(
                        format_number(mesh.vertex_count),
                        Style::default().fg(ctx.theme.ink),
                    ),
                    has(mesh.normal_count),
                    has(mesh.texcoord_count),
                    Span::styled(
                        mesh.material_name.clone().unwrap_or_else(|| "-".to_owned()),
                        Style::default().fg(ctx.theme.ink_dim),
                    ),
                ])
            })
            .collect();

        let widths = [
            Constraint::Min(10),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(4),
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
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            )
            // The marker is the selection signal that survives monochrome,
            // where the highlight background is not painted at all.
            .highlight_symbol("");
        frame.render_stateful_widget(table, table_area, &mut self.state);

        let series = Self::triangle_series(ctx.report);
        let spark = widgets::sparkline(&series, ctx.glyphs);
        if area.height >= 2 {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    spark,
                    Style::default().fg(ctx
                        .theme
                        .chart
                        .first()
                        .copied()
                        .unwrap_or(ctx.theme.accent)),
                )))
                .right_aligned(),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
    }

    fn set_filter(&mut self, query: Option<String>) {
        self.filter = query.filter(|q| !q.is_empty());
    }

    fn filter_counts(&self, ctx: &Ctx<'_>) -> Option<(usize, usize)> {
        Some((self.rows(ctx).len(), ctx.report.meshes.len()))
    }

    fn reveal(&mut self, row: usize) -> bool {
        self.state.select(Some(row));
        true
    }

    fn status(&self, ctx: &Ctx<'_>) -> Option<String> {
        Some(format!(
            "{} meshes \u{b7} {} tris",
            format_number(ctx.report.mesh_count),
            format_number(ctx.report.total_triangles)
        ))
    }
}

/// A mesh with no name in the file still needs a row label.
fn display_name(mesh: &MeshSummary) -> String {
    if mesh.name.is_empty() {
        format!("mesh {}", mesh.index)
    } else {
        mesh.name.clone()
    }
}

/// Move a selection without wrapping.
///
/// The ends are walls. Wrapping from the last row to the first in a table
/// someone is reading down is disorienting in a way it is not in a menu.
fn move_selection(state: &mut TableState, delta: i32, count: usize) {
    if count == 0 {
        state.select(None);
        return;
    }
    let current = state.selected().unwrap_or(0) as i32;
    let next = (current + delta).clamp(0, count as i32 - 1);
    state.select(Some(next as usize));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_with_no_name_still_gets_a_label() {
        let mut mesh = MeshSummary {
            index: 2,
            name: String::new(),
            vertex_count: 0,
            index_count: 0,
            triangle_count: 0,
            normal_count: 0,
            texcoord_count: 0,
            material_name: None,
            material_id: None,
            degenerate_faces: Vec::new(),
        };
        assert_eq!(display_name(&mesh), "mesh 2");
        mesh.name = "body".to_owned();
        assert_eq!(display_name(&mesh), "body");
    }

    /// Both ends are walls. A reader scanning down a table does not expect to
    /// arrive back at the top.
    #[test]
    fn selection_stops_at_both_ends_rather_than_wrapping() {
        let mut state = TableState::default();
        state.select(Some(0));
        move_selection(&mut state, -1, 3);
        assert_eq!(state.selected(), Some(0));

        move_selection(&mut state, 1, 3);
        move_selection(&mut state, 1, 3);
        move_selection(&mut state, 1, 3);
        assert_eq!(state.selected(), Some(2));
    }

    #[test]
    fn an_empty_table_selects_nothing() {
        let mut state = TableState::default();
        state.select(Some(1));
        move_selection(&mut state, 1, 0);
        assert_eq!(state.selected(), None);
    }

    /// One computation, two surfaces: the sparkline under this table and the
    /// histogram in the distributions panel read the same series.
    #[test]
    fn the_triangle_series_is_sorted_largest_first() {
        let report = solarxy_core::report::AnalysisReport {
            model_name: "t".into(),
            mesh_count: 3,
            material_count: 0,
            total_vertices: 0,
            total_indices: 0,
            total_triangles: 0,
            bounds: None,
            meshes: [400usize, 8_204, 3_864]
                .into_iter()
                .enumerate()
                .map(|(index, triangle_count)| MeshSummary {
                    index,
                    name: format!("m{index}"),
                    vertex_count: 0,
                    index_count: 0,
                    triangle_count,
                    normal_count: 0,
                    texcoord_count: 0,
                    material_name: None,
                    material_id: None,
                    degenerate_faces: Vec::new(),
                })
                .collect(),
            materials: Vec::new(),
            validation: solarxy_core::validation::ValidationReport::default(),
            source_format: "obj".into(),
            file_size_bytes: None,
            asset_category: None,
            triangle_budget: None,
        };
        assert_eq!(Meshes::triangle_series(&report), vec![8_204, 3_864, 400]);
    }
}
