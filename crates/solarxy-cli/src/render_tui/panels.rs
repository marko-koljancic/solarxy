//! The six readouts, and the vocabulary the split tree arranges them by.
//!
//! # One struct, six panels
//!
//! The analyze surface gives each panel type its own struct because each holds
//! state: a selection, a sort, a scroll. None of these do. A render lasts a
//! minute and there is nothing in one of these panels to select, so a single
//! struct carrying which readout it is says the truth, and six empty structs
//! would say that there is state here to keep apart.
//!
//! # What the elision order means here
//!
//! At the narrow width the picture goes first, then the throughput, then what
//! was asked for. Read it the way the analyze surface's own order reads: the
//! question at that width is whether the render is progressing, and the three
//! that answer it are the tiles, the bar and the timings.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::layout::PanelKind;
use crate::tui::panels::{Ctx, Panel};
use crate::tui::widgets::{self, Paint};

use super::state::{RenderView, Stage, seconds};

/// The context a render panel is handed.
pub type RenderCtx<'a> = Ctx<'a, RenderView>;

/// The six readouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashPanel {
    /// The grid of tiles, filled as each converges.
    Tiles,
    /// The bar, the counts, the elapsed time and the estimate.
    #[default]
    Progress,
    /// What each stage took.
    Stages,
    /// What was asked for.
    Subject,
    /// Samples a second over time.
    Throughput,
    /// The picture so far.
    Picture,
}

impl PanelKind for DashPanel {
    fn choosable() -> &'static [Self] {
        &[
            Self::Progress,
            Self::Tiles,
            Self::Stages,
            Self::Picture,
            Self::Throughput,
            Self::Subject,
        ]
    }

    fn elision_order() -> &'static [Self] {
        &[Self::Picture, Self::Throughput, Self::Subject]
    }

    fn name(self) -> &'static str {
        match self {
            Self::Tiles => "tiles",
            Self::Progress => "progress",
            Self::Stages => "stages",
            Self::Subject => "render",
            Self::Throughput => "throughput",
            Self::Picture => "picture",
        }
    }

    fn fallback() -> Self {
        Self::Progress
    }
}

/// A readout. Which one it is, and nothing else.
pub struct Readout(pub DashPanel);

impl Panel<RenderView, ()> for Readout {
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderCtx<'_>) {
        let lines = match self.0 {
            DashPanel::Tiles => tiles(area, ctx),
            DashPanel::Progress => progress(area, ctx),
            DashPanel::Stages => stages(ctx),
            DashPanel::Subject => subject(ctx),
            DashPanel::Throughput => throughput(area, ctx),
            DashPanel::Picture => picture(area, ctx),
        };
        frame.render_widget(Paragraph::new(lines), area);
    }
}

/// The grid, one cell a tile, shaded by how far that tile has come.
///
/// The shading rather than a done-or-not mark, because the tile being drawn is
/// the only one moving and a grid that showed it as pending would look stalled
/// for as long as that tile took.
fn tiles(area: Rect, ctx: &RenderCtx<'_>) -> Vec<Line<'static>> {
    let view = ctx.subject;
    if view.tiles == 0 || view.columns == 0 {
        return vec![Line::from(Span::styled(
            "waiting for the plan",
            Style::default().fg(ctx.theme.ink_dim),
        ))];
    }

    // Two cells a tile, so the grid is nearer square than a terminal cell is.
    let width = usize::from(area.width).max(1);
    let per_row = usize::try_from(view.columns).unwrap_or(1);
    let scale = (width / per_row.max(1)).clamp(1, 3);

    let mut lines = Vec::new();
    for row in 0..view.rows {
        let mut cells = String::new();
        for column in 0..view.columns {
            let index = row * view.columns + column;
            if index >= view.tiles {
                break;
            }
            let fill = match index.cmp(&view.tile) {
                std::cmp::Ordering::Less => 1.0,
                std::cmp::Ordering::Equal => view.tile_fraction(),
                std::cmp::Ordering::Greater => 0.0,
            };
            for _ in 0..scale {
                cells.push_str(ctx.glyphs.shade(fill));
            }
        }
        lines.push(Line::from(Span::styled(
            cells,
            Style::default().fg(ctx.theme.ink),
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("tile {} of {}", view.tile + 1, view.tiles),
        Style::default().fg(ctx.theme.ink_dim),
    )));
    lines
}

/// The bar, and the four numbers a reader watching it wants.
fn progress(area: Rect, ctx: &RenderCtx<'_>) -> Vec<Line<'static>> {
    let view = ctx.subject;
    let paint = Paint {
        ink: ctx.theme.accent,
        theme: ctx.theme,
        glyphs: ctx.glyphs,
    };
    // Room for the bar and a percentage beside it.
    let cells = area.width.saturating_sub(8).clamp(4, 48);
    let percent = format!("{:>3.0}%", view.fraction() * 100.0);

    let mut lines = vec![
        widgets::meter(view.fraction(), cells, &percent, paint),
        Line::raw(""),
    ];

    if view.cancelling {
        lines.push(Line::from(Span::styled(
            "stopping",
            Style::default()
                .fg(ctx.theme.warning)
                .add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            view.stage.name(),
            Style::default()
                .fg(match view.stage {
                    Stage::Done => ctx.theme.success,
                    Stage::Failed => ctx.theme.error,
                    _ => ctx.theme.ink,
                })
                .add_modifier(Modifier::BOLD),
        )));
    }

    if view.samples_total() > 0 {
        lines.push(Line::from(widgets::field(
            "samples",
            &format!("{} of {}", view.samples_drawn(), view.samples_total()),
            9,
            ctx.theme,
        )));
    }
    if let (Stage::Cooking, Some((pass, passes))) = (view.stage, view.cook) {
        lines.push(Line::from(widgets::field(
            "pass",
            &format!("{pass} of {passes}"),
            9,
            ctx.theme,
        )));
    }
    lines.push(Line::from(widgets::field(
        "elapsed",
        &seconds(view.elapsed),
        9,
        ctx.theme,
    )));
    lines.push(Line::from(widgets::field(
        "remaining",
        &view.remaining().map_or_else(|| "-".to_owned(), seconds),
        9,
        ctx.theme,
    )));
    lines
}

/// What each finished stage took, and what the one running has taken so far.
fn stages(ctx: &RenderCtx<'_>) -> Vec<Line<'static>> {
    let view = ctx.subject;
    let mut lines: Vec<Line<'static>> = view
        .timings
        .iter()
        .map(|(stage, span)| {
            Line::from(widgets::field(stage.name(), &seconds(*span), 10, ctx.theme))
        })
        .collect();
    // The stage still running, in the accent so it is plainly the live row and
    // not one more finished measurement. Its own elapsed rather than a mark,
    // because the number is what a reader wondering whether it has stalled is
    // actually after.
    if !matches!(view.stage, Stage::Done | Stage::Failed) {
        lines.push(Line::from(Span::styled(
            format!("{:<10} {}", view.stage.name(), seconds(view.stage_elapsed)),
            Style::default().fg(ctx.theme.accent),
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "nothing has finished yet",
            Style::default().fg(ctx.theme.ink_dim),
        )));
    }
    lines
}

/// What was asked for, so a screenshot of this says what it is a render of.
fn subject(ctx: &RenderCtx<'_>) -> Vec<Line<'static>> {
    let r = &ctx.subject.request;
    let mut lines = vec![
        Line::from(widgets::field("input", &r.input, 9, ctx.theme)),
        Line::from(widgets::field("output", &r.output, 9, ctx.theme)),
        Line::from(widgets::field(
            "engine",
            r.engine.as_deref().unwrap_or("from the scene"),
            9,
            ctx.theme,
        )),
        Line::from(widgets::field(
            "size",
            &r.size
                .map_or_else(|| "from the scene".to_owned(), |(w, h)| format!("{w}x{h}")),
            9,
            ctx.theme,
        )),
    ];
    if let Some(samples) = r.samples {
        lines.push(Line::from(widgets::field(
            "samples",
            &samples.to_string(),
            9,
            ctx.theme,
        )));
    }
    if let Some(bounces) = r.bounces {
        lines.push(Line::from(widgets::field(
            "bounces",
            &bounces.to_string(),
            9,
            ctx.theme,
        )));
    }
    if let Some(seed) = r.seed {
        lines.push(Line::from(widgets::field(
            "seed",
            &seed.to_string(),
            9,
            ctx.theme,
        )));
    }
    if let Some(triangles) = ctx.subject.triangles {
        lines.push(Line::from(widgets::field(
            "triangles",
            &triangles.to_string(),
            9,
            ctx.theme,
        )));
    }
    lines
}

/// Samples a second, over time.
fn throughput(area: Rect, ctx: &RenderCtx<'_>) -> Vec<Line<'static>> {
    let series = &ctx.subject.throughput;
    if series.is_empty() {
        return vec![Line::from(Span::styled(
            "no readings yet",
            Style::default().fg(ctx.theme.ink_dim),
        ))];
    }
    let width = usize::from(area.width).max(1);
    let tail = &series[series.len().saturating_sub(width)..];
    let latest = tail.last().copied().unwrap_or(0);
    vec![
        Line::from(Span::styled(
            widgets::sparkline(tail, ctx.glyphs),
            Style::default().fg(ctx.theme.ink),
        )),
        Line::raw(""),
        Line::from(widgets::field(
            "samples/s",
            &latest.to_string(),
            10,
            ctx.theme,
        )),
    ]
}

/// The picture, in shading, centred.
fn picture(area: Rect, ctx: &RenderCtx<'_>) -> Vec<Line<'static>> {
    let Some(picture) = ctx.subject.picture.as_ref() else {
        return vec![Line::from(Span::styled(
            "no tile has finished yet",
            Style::default().fg(ctx.theme.ink_dim),
        ))];
    };
    let (columns, rows) = picture.fit(area.width, area.height);
    picture
        .rows(columns, rows, ctx.glyphs)
        .into_iter()
        .map(|row| Line::from(Span::styled(row, Style::default().fg(ctx.theme.ink))).centered())
        .collect()
}
