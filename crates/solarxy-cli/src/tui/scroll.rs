//! Scrolling that counts what the terminal draws, not what we wrote.
//!
//! # The defect this type exists to make impossible
//!
//! The shipped analyze shell stores a content height in logical [`Line`]s and
//! then hands that number to three separate consumers: the offset clamp, the
//! position counter, and the scrollbar. A wrapping `Paragraph` turns one long
//! line into several rows, so on a narrow terminal all three under-report by
//! the same unknown amount, and jump-to-bottom stops short of the true last
//! row because the clamp it is measured against is too small.
//!
//! The interesting part is that no arithmetic here is wrong. Every site is
//! individually correct and reads the same wrong number, which is why the
//! defect survived. So the fix is structural: one [`Extent`] describes the
//! content, [`Scroll`] owns nothing but an offset, and the clamp, the counter
//! and the scrollbar are all derived from that single value. They cannot
//! disagree, because there is only one thing for them to agree about.
//!
//! [`rendered_rows`] is what makes the number right. It runs the same word
//! wrapper the renderer runs, so the count and the drawing cannot drift.

use ratatui::text::Text;
use ratatui::widgets::{Paragraph, ScrollbarState, Wrap};

/// How tall the content is and how much of it fits, both in **rendered rows**.
///
/// Never in logical lines. A caller that has a wrapping body measures it with
/// [`rendered_rows`]; a caller whose rows are fixed, such as a table or a
/// list, already has the figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Extent {
    /// Rows the content occupies once drawn.
    pub rendered_rows: u16,
    /// Rows visible at once, inside any border.
    pub viewport_rows: u16,
}

impl Extent {
    pub fn new(rendered_rows: u16, viewport_rows: u16) -> Self {
        Self {
            rendered_rows,
            viewport_rows,
        }
    }

    /// The largest offset that still fills the viewport.
    ///
    /// Content shorter than the viewport pins to zero rather than allowing a
    /// scroll into blank space.
    pub fn max_offset(self) -> u16 {
        self.rendered_rows.saturating_sub(self.viewport_rows)
    }

    /// Whether there is anything to scroll, and so whether a scrollbar earns
    /// its column.
    pub fn overflows(self) -> bool {
        self.rendered_rows > self.viewport_rows
    }
}

/// A scroll position, clamped against an [`Extent`] on every read.
///
/// Clamping on read rather than on write is deliberate: the extent changes
/// when the terminal resizes, and a position that was legal before the resize
/// must not survive it. Storing a pre-clamped value would make the offset
/// depend on the order in which resizes and keystrokes arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scroll {
    offset: u16,
}

impl Scroll {
    /// The first visible row, clamped to what the extent allows.
    pub fn offset(self, extent: Extent) -> u16 {
        self.offset.min(extent.max_offset())
    }

    pub fn up(&mut self, rows: u16) {
        self.offset = self.offset.saturating_sub(rows);
    }

    pub fn down(&mut self, rows: u16, extent: Extent) {
        self.offset = self.offset.saturating_add(rows).min(extent.max_offset());
    }

    pub fn first(&mut self) {
        self.offset = 0;
    }

    /// Jump to the true last row.
    ///
    /// Sets the real maximum rather than a sentinel left for the draw call to
    /// clamp. A sentinel means the offset is wrong between the keystroke and
    /// the next frame, and anything that reads it in between, a counter or an
    /// export, reads the sentinel.
    pub fn last(&mut self, extent: Extent) {
        self.offset = extent.max_offset();
    }

    /// The `[n/total]` readout: the first visible row, one-based, over the
    /// total rendered rows.
    pub fn position(self, extent: Extent) -> (u16, u16) {
        let first = self
            .offset(extent)
            .saturating_add(1)
            .min(extent.rendered_rows);
        (first, extent.rendered_rows)
    }

    pub fn scrollbar(self, extent: Extent) -> ScrollbarState {
        ScrollbarState::new(usize::from(extent.rendered_rows))
            .position(usize::from(self.offset(extent)))
            .viewport_content_length(usize::from(extent.viewport_rows))
    }
}

/// Rows a wrapping body occupies at a given width.
///
/// `width` is the **inner** width, inside any border, and the answer is the
/// **inner** height, because no block is attached here. Measuring with a block
/// set would add its two rows and the caller would subtract them again.
///
/// The measurement runs ratatui's own word wrapper by way of
/// `Paragraph::line_count`, so it is the same code path that draws. A
/// hand-rolled wrap would be a second opinion that has to stay in step
/// forever, which is the original defect wearing different clothes.
///
/// `Wrap { trim: false }` is baked in because that is what every body this
/// shell draws uses; leading indentation is structure here, not noise.
pub fn rendered_rows(text: &Text<'_>, width: u16) -> u16 {
    let count = Paragraph::new(text.clone())
        .wrap(Wrap { trim: false })
        .line_count(width);
    u16::try_from(count).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::text::Line;

    use super::*;

    /// Four short lines and one long one. At width 20 the long line occupies
    /// three rows, so the body is seven rows tall and five lines long, and
    /// every number below has to be the seven.
    fn body() -> Text<'static> {
        Text::from(vec![
            Line::from("alpha"),
            Line::from("bravo"),
            Line::from("charlie delta echo foxtrot golf hotel india"),
            Line::from("juliett"),
            Line::from("kilo"),
        ])
    }

    const WIDTH: u16 = 20;

    #[test]
    fn a_wrapped_line_counts_every_row_it_occupies() {
        let text = body();
        assert_eq!(text.lines.len(), 5, "the body is five logical lines");
        assert_eq!(
            rendered_rows(&text, WIDTH),
            7,
            "the long line wraps into three rows, so the body is seven tall"
        );
    }

    /// Widening the panel until nothing wraps must collapse the two figures
    /// onto each other, or the measurement is counting something else.
    #[test]
    fn an_unwrapped_body_measures_its_own_line_count() {
        let text = body();
        assert_eq!(rendered_rows(&text, 200), 5);
    }

    /// The whole point of the type. Every consumer reads one number, so a
    /// wrapped line moves all of them together or none of them.
    #[test]
    fn the_clamp_the_counter_and_the_scrollbar_read_one_number() {
        let extent = Extent::new(rendered_rows(&body(), WIDTH), 4);
        let mut scroll = Scroll::default();
        scroll.last(extent);

        assert_eq!(extent.max_offset(), 3, "seven rows in a four-row viewport");
        assert_eq!(scroll.offset(extent), 3);
        assert_eq!(
            scroll.position(extent),
            (4, 7),
            "the counter reports rendered rows, not logical lines"
        );

        // Had any of the three read the logical line count instead, the
        // maximum would be one, the counter would end at five, and the
        // scrollbar thumb would sit two rows short of the rail.
        let wrong = Extent::new(5, 4);
        assert_ne!(wrong.max_offset(), extent.max_offset());
        assert_ne!(Scroll::default().position(wrong), scroll.position(extent));
    }

    /// The acceptance criterion, asserted against pixels rather than
    /// arithmetic: after jumping to the end, the bottom row of the viewport
    /// carries the last row of the content.
    #[test]
    fn jumping_to_the_end_reaches_the_last_rendered_row() {
        let text = body();
        let viewport_rows = 4;
        let extent = Extent::new(rendered_rows(&text, WIDTH), viewport_rows);
        let mut scroll = Scroll::default();
        scroll.last(extent);

        let mut terminal =
            Terminal::new(TestBackend::new(WIDTH, viewport_rows)).expect("test terminal");
        terminal
            .draw(|frame| {
                let paragraph = Paragraph::new(text.clone())
                    .wrap(Wrap { trim: false })
                    .scroll((scroll.offset(extent), 0));
                frame.render_widget(paragraph, frame.area());
            })
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        let bottom: String = (0..WIDTH)
            .map(|x| buffer[(x, viewport_rows - 1)].symbol())
            .collect();
        assert_eq!(
            bottom.trim(),
            "kilo",
            "jump-to-bottom stopped short of the true last row"
        );
    }

    /// A viewport taller than the content has nowhere to scroll to, and must
    /// not offer a scrollbar or a non-zero offset.
    #[test]
    fn content_shorter_than_the_viewport_never_scrolls() {
        let extent = Extent::new(3, 10);
        assert!(!extent.overflows());
        assert_eq!(extent.max_offset(), 0);

        let mut scroll = Scroll::default();
        scroll.down(50, extent);
        assert_eq!(scroll.offset(extent), 0);
        scroll.last(extent);
        assert_eq!(scroll.offset(extent), 0);
        assert_eq!(scroll.position(extent), (1, 3));
    }

    /// A terminal shrink changes the extent under a position that was legal
    /// before it. Clamping on read is what keeps the offset honest without
    /// the resize having to reach in and fix it.
    #[test]
    fn a_shrunken_viewport_reclamps_a_position_it_invalidates() {
        let tall = Extent::new(40, 30);
        let mut scroll = Scroll::default();
        scroll.last(tall);
        assert_eq!(scroll.offset(tall), 10);

        let short = Extent::new(12, 10);
        assert_eq!(scroll.offset(short), 2, "the old offset outran the content");
    }

    /// Both ends are walls, not wraps. The shipped shell relied on the draw
    /// call to catch an overrun, which is why an offset could be briefly
    /// nonsense between a keystroke and the next frame.
    #[test]
    fn both_ends_are_walls() {
        let extent = Extent::new(100, 10);
        let mut scroll = Scroll::default();
        scroll.up(20);
        assert_eq!(scroll.offset(extent), 0, "the top is a floor");

        scroll.down(20, extent);
        assert_eq!(scroll.offset(extent), 20);
        scroll.down(500, extent);
        assert_eq!(scroll.offset(extent), 90, "the bottom is a ceiling");
    }

    /// An empty panel and a zero-width one both have to answer rather than
    /// panic, because a resize passes through both on its way to a usable
    /// size.
    #[test]
    fn a_degenerate_extent_still_answers() {
        let empty = Extent::default();
        assert_eq!(empty.max_offset(), 0);
        assert!(!empty.overflows());
        assert_eq!(Scroll::default().position(empty), (0, 0));
        assert_eq!(rendered_rows(&Text::from("anything"), 0), 0);
    }
}
