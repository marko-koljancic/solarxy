//! The drawing primitives every panel shares.
//!
//! Small on purpose. A panel's job is to decide what to say about a model; how
//! a meter or a bar looks is not its business, and ten panels each inventing a
//! meter is how a surface ends up with ten slightly different ones.
//!
//! # Why these are hand-built rather than ratatui widgets
//!
//! `Gauge` was the obvious choice for the meter and cannot be used: it writes a
//! block element for every filled cell whatever the glyph tier, and under its
//! label it swaps the style's foreground into the cell's background, so it
//! survives neither the ASCII repertoire nor the rule that the terminal owns
//! the ground below 256 colours. The same reasoning rules it out for the bars.
//! `Table` and `List` are used as they come.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::caps::{GlyphTier, Glyphs};
use super::theme::Slots;

/// How to draw, as opposed to what to draw.
///
/// Every primitive here needs the same three things: the hue this particular
/// mark takes, the theme behind it, and the repertoire it may draw from. They
/// travel together because they are always needed together.
#[derive(Clone, Copy)]
pub struct Paint<'a> {
    pub ink: ratatui::style::Color,
    pub theme: &'a Slots,
    pub glyphs: &'a Glyphs,
}

/// Filled and empty cells for a meter, per glyph tier.
///
/// Lose detail, keep meaning: at the ASCII tier a run of hashes against a run
/// of dots still reads as a proportion.
pub fn meter_glyphs(glyphs: &Glyphs) -> (&'static str, &'static str) {
    match glyphs.tier {
        GlyphTier::Unicode => ("\u{2588}", "\u{2591}"),
        GlyphTier::Ascii => ("#", "."),
    }
}

/// A proportion bar of a fixed cell width, with a trailing label.
///
/// `fraction` above one is clamped rather than allowed to overrun, because a
/// model over its budget is exactly when this is read and a bar that ran past
/// its own track would say less than a full one does.
pub fn meter(fraction: f64, cells: u16, label: &str, paint: Paint<'_>) -> Line<'static> {
    let Paint { ink, theme, glyphs } = paint;
    let (filled_glyph, empty_glyph) = meter_glyphs(glyphs);
    let cells = usize::from(cells);
    let filled = (fraction.clamp(0.0, 1.0) * cells as f64).round() as usize;
    Line::from(vec![
        Span::styled(filled_glyph.repeat(filled), Style::default().fg(ink)),
        Span::styled(
            empty_glyph.repeat(cells.saturating_sub(filled)),
            Style::default().fg(theme.border),
        ),
        Span::raw("  "),
        Span::styled(label.to_owned(), Style::default().fg(theme.ink)),
    ])
}

/// One labelled bar in a descending set: name, bar, count.
///
/// The bar is scaled against the largest value in the set rather than against
/// a total, so the shape of the distribution is visible even when one kind
/// dominates.
pub fn bar_row(
    name: &str,
    value: u64,
    largest: u64,
    name_width: u16,
    bar_cells: u16,
    paint: Paint<'_>,
) -> Line<'static> {
    let Paint { ink, theme, glyphs } = paint;
    let (filled_glyph, _) = meter_glyphs(glyphs);
    let share = if largest == 0 {
        0.0
    } else {
        value as f64 / largest as f64
    };
    // At least one cell for any non-zero value: a bar that rounds to nothing
    // reads as an absence rather than as a small number.
    let filled = if value == 0 {
        0
    } else {
        ((share * f64::from(bar_cells)).round() as usize).max(1)
    };
    Line::from(vec![
        Span::styled(
            format!(
                "{:<width$} ",
                fit(name, name_width),
                width = usize::from(name_width)
            ),
            Style::default().fg(theme.ink_dim),
        ),
        Span::styled(filled_glyph.repeat(filled), Style::default().fg(ink)),
        Span::raw(" ".repeat(usize::from(bar_cells).saturating_sub(filled) + 1)),
        Span::styled(
            solarxy_core::format_number(value as usize),
            Style::default().fg(theme.ink),
        ),
    ])
}

/// Cut a label to the column it has, so a long one cannot push the rest of
/// the row off the panel.
///
/// Cut rather than elided: the ellipsis is not in the ASCII repertoire, and a
/// name that has lost its tail is still the name a reader recognises.
pub fn fit(name: &str, width: u16) -> String {
    let width = usize::from(width);
    if name.chars().count() <= width {
        return name.to_owned();
    }
    name.chars().take(width).collect()
}

/// A label and its value on one row.
pub fn field(label: &str, value: &str, label_width: u16, theme: &Slots) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!("{label:<width$}", width = usize::from(label_width)),
            Style::default().fg(theme.ink_dim),
        ),
        Span::raw(" "),
        Span::styled(
            value.to_owned(),
            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
        ),
    ]
}

/// What a panel says when it has nothing to say.
///
/// One dim centred line naming what is absent. No panel is dropped for lack of
/// data, because a missing panel makes a reader wonder what they did wrong and
/// an empty frame makes them wonder whether it is broken.
pub fn empty_state(what: &str, area: Rect, theme: &Slots) -> (Line<'static>, Rect) {
    let line = Line::from(Span::styled(
        what.to_owned(),
        Style::default().fg(theme.ink_dim),
    ))
    .centered();
    let y = area.y + area.height / 2;
    (line, Rect::new(area.x, y, area.width, 1))
}

/// A one-row sparkline over a series, scaled to its own maximum.
///
/// Eight levels at the Unicode tier and five at the ASCII one, which is the
/// same lose-detail-keep-meaning trade every other degradation here makes.
pub fn sparkline(series: &[u64], glyphs: &Glyphs) -> String {
    const LEVELS: [&str; 8] = [
        "\u{2581}", "\u{2582}", "\u{2583}", "\u{2584}", "\u{2585}", "\u{2586}", "\u{2587}",
        "\u{2588}",
    ];
    let largest = series.iter().copied().max().unwrap_or(0);
    if largest == 0 {
        return String::new();
    }
    series
        .iter()
        .map(|value| {
            let share = *value as f64 / largest as f64;
            match glyphs.tier {
                GlyphTier::Unicode => {
                    let step = ((share * (LEVELS.len() - 1) as f64).round() as usize)
                        .min(LEVELS.len() - 1);
                    LEVELS[step]
                }
                GlyphTier::Ascii => {
                    let ramp = Glyphs::ASCII_DENSITY;
                    let step =
                        ((share * (ramp.len() - 1) as f64).round() as usize).min(ramp.len() - 1);
                    ramp[step]
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::caps::GlyphTier;
    use crate::tui::theme::{DEFAULT_THEME, ThemeSet};

    fn slots() -> Slots {
        ThemeSet::bundled()
            .slots_for(DEFAULT_THEME)
            .expect("the default loads")
    }

    fn paint<'a>(theme: &'a Slots, glyphs: &'a Glyphs) -> Paint<'a> {
        Paint {
            ink: ratatui::style::Color::Reset,
            theme,
            glyphs,
        }
    }

    fn width_of(line: &Line<'_>) -> usize {
        line.spans.iter().map(|s| s.content.chars().count()).sum()
    }

    /// A meter is a fixed number of cells whatever the value, or a row of them
    /// would not line up in a column.
    #[test]
    fn a_meter_is_the_same_width_at_every_value() {
        let theme = slots();
        for tier in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let glyphs = Glyphs::for_tier(tier);
            let widths: Vec<usize> = [0.0, 0.25, 0.62, 1.0]
                .into_iter()
                .map(|f| width_of(&meter(f, 20, "", paint(&theme, &glyphs))))
                .collect();
            assert!(
                widths.windows(2).all(|w| w[0] == w[1]),
                "{tier:?} produced ragged meters: {widths:?}"
            );
        }
    }

    /// Over budget is exactly when this is read, so the bar has to stop at its
    /// own track rather than running past it.
    #[test]
    fn a_meter_over_one_is_clamped_rather_than_overrun() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(GlyphTier::Unicode);
        let full = meter(1.0, 20, "", paint(&theme, &glyphs));
        let over = meter(3.4, 20, "", paint(&theme, &glyphs));
        assert_eq!(width_of(&full), width_of(&over));
    }

    /// A small non-zero value must still draw something. Rounding it away
    /// makes one issue look like none.
    #[test]
    fn a_bar_never_rounds_a_real_value_down_to_nothing() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(GlyphTier::Unicode);
        let row = bar_row("rare", 1, 10_000, 12, 20, paint(&theme, &glyphs));
        let bar = &row.spans[1].content;
        assert_eq!(bar.chars().count(), 1, "a single issue drew {bar:?}");

        let none = bar_row("none", 0, 10_000, 12, 20, paint(&theme, &glyphs));
        assert!(none.spans[1].content.is_empty(), "zero drew a bar");
    }

    /// A long label must not push the bar and its count off the panel, which
    /// is what an unbounded name does to a column layout.
    #[test]
    fn a_long_label_is_cut_to_its_column() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(GlyphTier::Unicode);
        let short = bar_row("uv", 3, 9, 12, 10, paint(&theme, &glyphs));
        let long = bar_row(
            "triangle budget exceeded",
            3,
            9,
            12,
            10,
            paint(&theme, &glyphs),
        );
        assert_eq!(
            width_of(&short),
            width_of(&long),
            "a long name changed the row width"
        );
        assert_eq!(fit("uv", 12), "uv");
        assert_eq!(fit("triangle budget exceeded", 12), "triangle bud");
    }

    #[test]
    fn a_sparkline_has_one_cell_per_sample_at_both_tiers() {
        for tier in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let glyphs = Glyphs::for_tier(tier);
            let line = sparkline(&[3, 1, 4, 1, 5, 9, 2, 6], &glyphs);
            assert_eq!(line.chars().count(), 8, "{tier:?}");
        }
    }

    #[test]
    fn a_sparkline_of_nothing_is_nothing_rather_than_a_flat_line() {
        let glyphs = Glyphs::for_tier(GlyphTier::Unicode);
        assert!(sparkline(&[], &glyphs).is_empty());
        assert!(sparkline(&[0, 0, 0], &glyphs).is_empty());
    }

    #[test]
    fn the_ascii_tier_keeps_every_primitive_inside_its_repertoire() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(GlyphTier::Ascii);
        let rendered = [
            meter(0.5, 10, "62%", paint(&theme, &glyphs))
                .spans
                .iter()
                .map(|s| s.content.to_string())
                .collect::<String>(),
            bar_row("kind", 3, 9, 8, 10, paint(&theme, &glyphs))
                .spans
                .iter()
                .map(|s| s.content.to_string())
                .collect::<String>(),
            sparkline(&[1, 2, 3], &glyphs),
        ];
        for text in rendered {
            assert!(text.is_ascii(), "{text:?} left the ASCII repertoire");
        }
    }
}
