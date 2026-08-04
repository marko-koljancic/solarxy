//! Every texture slot every material references, in one place.
//!
//! Missing textures are already raised as errors, one per slot, scattered
//! through a validation list. This is where someone goes to see all of them at
//! once, which is the question actually being asked when an asset comes back
//! from a vendor.
//!
//! The resolution comes from the file's header rather than from a decode, and
//! is absent rather than guessed when the file is missing, is not an image, or
//! is in a format this build cannot read.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Row, Table, TableState};
use solarxy_core::report::TextureEntry;

use super::super::widgets;
use super::{Action, Ctx, Panel, Sort};

pub const COLUMNS: [&str; 4] = ["slot", "file", "res", "ok"];

#[derive(Default)]
pub struct Textures {
    pub sort: Sort,
    state: TableState,
}

impl Textures {
    fn rows<'a>(&self, ctx: &Ctx<'a>) -> Vec<&'a TextureEntry> {
        let mut rows: Vec<&TextureEntry> = ctx
            .report
            .materials
            .iter()
            .flat_map(|material| &material.textures)
            .collect();
        match self.sort.column {
            1 => rows.sort_by(|a, b| a.path.cmp(&b.path)),
            2 => rows.sort_by_key(|t| t.dimensions.map_or(0, |(w, h)| u64::from(w) * u64::from(h))),
            3 => rows.sort_by_key(|t| t.exists),
            _ => rows.sort_by(|a, b| a.slot.cmp(&b.slot)),
        }
        if self.sort.descending {
            rows.reverse();
        }
        rows
    }
}

impl Panel for Textures {
    fn menu(&self) -> &'static [&'static str] {
        &["sort"]
    }

    fn handle(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> Action {
        let count = self.rows(ctx).len();
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
        let rows = self.rows(ctx);
        if rows.is_empty() {
            let (line, rect) = widgets::empty_state("no textures referenced", area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }
        if self.state.selected().is_none_or(|i| i >= rows.len()) {
            self.state.select(Some(0));
        }

        let header = Row::new(COLUMNS.iter().enumerate().map(|(i, name)| {
            let mut style = Style::default().fg(ctx.theme.ink_dim);
            if i == self.sort.column {
                style = style.fg(ctx.chrome()).add_modifier(Modifier::BOLD);
            }
            Span::styled((*name).to_string(), style)
        }));

        let table_rows: Vec<Row> = rows
            .iter()
            .map(|entry| {
                let (mark, ink) = if entry.exists {
                    (ctx.glyphs.check, ctx.theme.success)
                } else {
                    (ctx.glyphs.cross, ctx.theme.error)
                };
                Row::new(vec![
                    Span::styled(entry.slot.clone(), Style::default().fg(ctx.theme.ink)),
                    Span::styled(file_name(&entry.path), Style::default().fg(ctx.theme.ink)),
                    Span::styled(resolution(entry), Style::default().fg(ctx.theme.ink_dim)),
                    Span::styled(mark.to_string(), Style::default().fg(ink)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(12),
            Constraint::Min(12),
            Constraint::Length(11),
            Constraint::Length(3),
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
            );
        frame.render_stateful_widget(table, area, &mut self.state);
    }

    fn reveal(&mut self, row: usize) -> bool {
        self.state.select(Some(row));
        true
    }

    fn status(&self, ctx: &Ctx<'_>) -> Option<String> {
        let all: Vec<&TextureEntry> = ctx
            .report
            .materials
            .iter()
            .flat_map(|m| &m.textures)
            .collect();
        let missing = all.iter().filter(|t| !t.exists).count();
        Some(format!("{} slots \u{b7} {missing} missing", all.len()))
    }
}

/// The file's own name, because the directory is the same for all of them and
/// spending the column on it would push the resolution off a narrow panel.
fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

/// The resolution as a reader would say it.
///
/// Square textures are the common case and get the shorter form, which is what
/// the design draws and what leaves room in a narrow column.
fn resolution(entry: &TextureEntry) -> String {
    match entry.dimensions {
        Some((w, h)) if w == h => format!("{w}\u{b2}"),
        Some((w, h)) => format!("{w}x{h}"),
        None => "-".to_owned(),
    }
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

    fn entry(slot: &str, path: &str, exists: bool, dims: Option<(u32, u32)>) -> TextureEntry {
        TextureEntry {
            slot: slot.to_owned(),
            path: path.to_owned(),
            exists,
            dimensions: dims,
        }
    }

    #[test]
    fn a_square_texture_reads_shorter_than_a_rectangular_one() {
        assert_eq!(
            resolution(&entry("d", "a.png", true, Some((2048, 2048)))),
            "2048\u{b2}"
        );
        assert_eq!(
            resolution(&entry("d", "a.png", true, Some((2048, 1024)))),
            "2048x1024"
        );
    }

    /// A texture that could not be measured says so rather than claiming a
    /// size, which is the honest answer for a missing file and for a format
    /// this build cannot read alike.
    #[test]
    fn an_unmeasurable_texture_shows_no_resolution() {
        assert_eq!(resolution(&entry("n", "n.png", false, None)), "-");
    }

    /// The directory is the same for every row, so spending the column on it
    /// would push the resolution off a narrow panel.
    #[test]
    fn only_the_file_name_takes_the_column() {
        assert_eq!(file_name("textures/skin_bc.png"), "skin_bc.png");
        assert_eq!(file_name("skin_bc.png"), "skin_bc.png");
        assert_eq!(file_name("a\\b\\c.png"), "c.png");
    }
}
