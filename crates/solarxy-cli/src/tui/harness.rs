//! The reference panel, and the machinery every render test shares.
//!
//! # Why a synthetic panel rather than a real one
//!
//! The invariants worth guarding are properties of the *drawing*, not of any
//! particular panel: no colour at the monochrome tier, nothing outside the
//! repertoire at the ASCII tier, no painted ground where the terminal owns it.
//! A real panel proves those only for the facts it happens to hold, and it
//! stops proving them the day someone changes its content for an unrelated
//! reason.
//!
//! So the reference panel is a specimen. It paints exactly one instance of
//! every theme slot, every glyph, both border sets, a table, a meter and a
//! plot, each on its own labelled row, and it never changes for a reason that
//! is not about the drawing. When a tier or a theme breaks something, the row
//! that fails names what broke.
//!
//! It renders at 140 by 45, the target size the specification names, and the
//! invariants are asserted over the whole frame rather than over a region,
//! because a stray cell anywhere is the failure.
//!
//! # Two ratatui widgets that cannot be used as they come
//!
//! Discovered here, which is the point of building this first:
//!
//! - `Gauge` writes `symbols::block::FULL` for every filled cell whatever the
//!   glyph tier, and under its label it swaps the style's foreground into the
//!   cell's **background**. So it survives neither the ASCII repertoire nor
//!   the rule that the terminal owns the ground below 256 colours. The meter
//!   here is built by hand for that reason, and the panels that want one have
//!   to do the same.
//! - No `Canvas` marker is ASCII: the dot is `\u{2022}`, the block and the bar
//!   are block elements. The plot below therefore branches on
//!   [`PlotStyle`], and the rasteriser that replaces it has to keep branching.

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Paragraph, Row, Table};
use ratatui::{Frame, Terminal};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::caps::{Capabilities, ColorTier, GlyphTier, Glyphs, PlotStyle};
use super::layout::{Layout as PanelLayout, PanelKind};
use super::theme::{Slots, Theme, ThemeSet};

/// The target terminal the specification names.
pub(crate) const REFERENCE_WIDTH: u16 = 140;
pub(crate) const REFERENCE_HEIGHT: u16 = 45;

/// Every colour tier crossed with every glyph tier.
pub(crate) fn every_pair() -> [Capabilities; 8] {
    let mut pairs = [Capabilities::default(); 8];
    let mut index = 0;
    for color in [
        ColorTier::Mono,
        ColorTier::Ansi16,
        ColorTier::Ansi256,
        ColorTier::TrueColor,
    ] {
        for glyphs in [GlyphTier::Unicode, GlyphTier::Ascii] {
            pairs[index] = Capabilities { color, glyphs };
            index += 1;
        }
    }
    pairs
}

/// The two tiers at which the terminal owns the ground and the theme is
/// ignored, which is where the colour invariants hold.
///
/// Above them a theme paints its own ink and its own ground by design, so
/// asserting the shipped surface's rules there would assert the opposite of
/// what the richer tiers exist to do.
pub(crate) fn lower_tiers() -> [Capabilities; 4] {
    [
        Capabilities {
            color: ColorTier::Mono,
            glyphs: GlyphTier::Unicode,
        },
        Capabilities {
            color: ColorTier::Mono,
            glyphs: GlyphTier::Ascii,
        },
        Capabilities {
            color: ColorTier::Ansi16,
            glyphs: GlyphTier::Unicode,
        },
        Capabilities {
            color: ColorTier::Ansi16,
            glyphs: GlyphTier::Ascii,
        },
    ]
}

/// Draw the reference panel through a theme's slots and read the cells back.
///
/// Resolution runs through [`Theme::resolve`], the same door the shell uses,
/// so a caller handing this a light theme is asking what that theme would
/// actually paint at that tier rather than what its file holds.
pub(crate) fn render_reference(caps: Capabilities, name: &str, slots: &Slots) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(REFERENCE_WIDTH, REFERENCE_HEIGHT)).expect("test terminal");
    let theme = Theme::resolve(caps, name, slots);
    let glyphs = caps.glyphs();
    terminal
        .draw(|frame| draw_reference(frame, frame.area(), &theme.slots, &glyphs))
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// The reference panel under a named bundled theme.
pub(crate) fn render_bundled(caps: Capabilities, name: &str) -> Buffer {
    let set = ThemeSet::bundled();
    let slots = set
        .slots_for(name)
        .unwrap_or_else(|notice| panic!("{name}: {notice}"));
    render_reference(caps, name, &slots)
}

/// The reference panel under the default theme.
pub(crate) fn render_reference_at(caps: Capabilities) -> Buffer {
    render_bundled(caps, super::theme::DEFAULT_THEME)
}

/// Draw a whole arrangement as empty framed panels.
///
/// The panels have no bodies yet, which is exactly what makes this useful: it
/// asserts the solve against cells rather than against arithmetic, so a border
/// landing one column out shows up here instead of inside the first panel
/// built on top of it.
pub(crate) fn render_layout<P: PanelKind>(
    caps: Capabilities,
    layout: &PanelLayout<P>,
    area: Rect,
) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(area.width, area.height)).expect("test terminal");
    let set = ThemeSet::bundled();
    let slots = set
        .slots_for(super::theme::DEFAULT_THEME)
        .expect("the default loads");
    let theme = Theme::resolve(caps, super::theme::DEFAULT_THEME, &slots);
    let glyphs = caps.glyphs();

    terminal
        .draw(|frame| {
            for placement in layout.solve(area, None) {
                let border = if placement.focused {
                    glyphs.border_focused
                } else {
                    glyphs.border
                };
                let ink = if placement.focused {
                    theme.slots.accent
                } else {
                    theme.slots.ink_dim
                };
                let title = Line::from(vec![
                    Span::styled(glyphs.address(placement.address), Style::default().fg(ink)),
                    Span::styled(
                        placement.panel.name(),
                        Style::default().fg(ink).add_modifier(Modifier::BOLD),
                    ),
                ]);
                frame.render_widget(
                    Block::bordered()
                        .title(title)
                        .border_set(border)
                        .border_style(Style::default().fg(ink)),
                    placement.rect,
                );
            }
        })
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// Every slot the theme carries, named, so a failing cell says which one.
fn slot_rows(slots: &Slots) -> [(&'static str, Color); 9] {
    [
        ("accent", slots.accent),
        ("border_focus", slots.border_focus),
        ("ink", slots.ink),
        ("ink_dim", slots.ink_dim),
        ("border", slots.border),
        ("selection", slots.selection),
        ("success", slots.success),
        ("warning", slots.warning),
        ("error", slots.error),
    ]
}

fn draw_reference(frame: &mut Frame, area: Rect, theme: &Slots, glyphs: &Glyphs) {
    // At the two lower tiers this ground resolves to `Color::Reset`, which is
    // the terminal's own and indistinguishable from painting none. Above them
    // it is the theme's, which is what a theme is for. Either way it is set in
    // one place rather than decided per widget.
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.ground)),
        area,
    );

    let frame_block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(glyphs.sun, Style::default().fg(theme.accent)),
            Span::raw(" "),
            Span::styled(
                "Reference",
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            ),
        ]))
        .border_set(glyphs.border_focused)
        .border_style(Style::default().fg(theme.border));
    let inner = frame_block.inner(area);
    frame.render_widget(frame_block, area);

    let rows = Layout::vertical([
        Constraint::Length(9),
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(4),
    ])
    .split(inner);

    draw_slots(frame, rows[0], theme);
    draw_glyphs(frame, rows[1], theme, glyphs);
    draw_table(frame, rows[2], theme);
    draw_meter(frame, rows[3], theme, glyphs);
    draw_unfocused_specimen(frame, rows[4], theme, glyphs);
    draw_plot(frame, rows[5], theme, glyphs);
}

/// One row per slot, the slot's own name painted in it. Bold rides two of
/// them, because bold is the only emphasis the monochrome tier keeps.
fn draw_slots(frame: &mut Frame, area: Rect, theme: &Slots) {
    let lines: Vec<Line> = slot_rows(theme)
        .into_iter()
        .map(|(name, color)| {
            let emphasised = matches!(name, "accent" | "heading");
            let mut style = Style::default().fg(color);
            if emphasised {
                style = style.add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(format!("{name:<9}"), style),
                Span::raw(" "),
                Span::styled("the quick brown fox", Style::default().fg(theme.ink)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

/// Every glyph, each beside the word it stands for, so a tier that drops one
/// still leaves the meaning readable in the failure message.
fn draw_glyphs(frame: &mut Frame, area: Rect, theme: &Slots, glyphs: &Glyphs) {
    let mark = |glyph: &'static str, word: &'static str, color: Color| {
        vec![
            Span::styled(glyph.to_owned(), Style::default().fg(color)),
            Span::raw(format!(" {word}  ")),
        ]
    };
    let mut severities = Vec::new();
    severities.extend(mark(glyphs.check, "complete", theme.success));
    severities.extend(mark(glyphs.cross, "absent", theme.error));
    severities.extend(mark(glyphs.warn, "partial", theme.warning));

    let chrome = Line::from(vec![
        Span::styled(glyphs.caret.to_owned(), Style::default().fg(theme.accent)),
        Span::raw(" "),
        Span::styled(
            glyphs.divider.to_owned(),
            Style::default().fg(theme.ink_dim),
        ),
        Span::raw(" "),
        Span::styled(
            glyphs.scroll_up.to_owned(),
            Style::default().fg(theme.ink_dim),
        ),
        Span::styled(
            glyphs.scroll_down.to_owned(),
            Style::default().fg(theme.ink_dim),
        ),
        Span::raw("  "),
        Span::styled(glyphs.address(1), Style::default().fg(theme.accent)),
        Span::raw(" "),
        Span::styled(glyphs.address(10), Style::default().fg(theme.accent)),
    ]);

    frame.render_widget(
        Paragraph::new(Text::from(vec![Line::from(severities), chrome])),
        area,
    );
}

fn draw_table(frame: &mut Frame, area: Rect, theme: &Slots) {
    let widths = [
        Constraint::Length(20),
        Constraint::Length(10),
        Constraint::Length(8),
    ];
    let table = Table::new(
        [
            Row::new(["body_low", "1,024", "yes"]),
            Row::new(["body_high", "16,384", "no"]),
        ],
        widths,
    )
    .header(
        Row::new(["name", "triangles", "uv"])
            .style(Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
    )
    .style(Style::default().fg(theme.ink));
    frame.render_widget(table, area);
}

/// A proportion bar, built by hand.
///
/// `Gauge` would be the obvious widget and cannot be used: see the module
/// docs. The two glyphs are chosen here rather than carried on [`Glyphs`]
/// because nothing ships a meter yet; the panel that does should move them
/// there so every meter in the shell agrees.
fn draw_meter(frame: &mut Frame, area: Rect, theme: &Slots, glyphs: &Glyphs) {
    const CELLS: usize = 40;
    let (filled_glyph, empty_glyph) = match glyphs.tier {
        GlyphTier::Unicode => ("\u{2588}", "\u{2591}"),
        GlyphTier::Ascii => ("#", "."),
    };
    let filled = CELLS * 62 / 100;

    let bar = Line::from(vec![
        Span::styled(
            filled_glyph.repeat(filled),
            Style::default().fg(theme.success),
        ),
        Span::styled(
            empty_glyph.repeat(CELLS - filled),
            Style::default().fg(theme.ink_dim),
        ),
        Span::raw("  "),
        Span::styled("62% of budget", Style::default().fg(theme.ink)),
    ]);
    frame.render_widget(Paragraph::new(Text::from(vec![bar])), area);
}

/// The unfocused frame, so both border sets reach the buffer and focus by
/// weight is asserted rather than assumed.
fn draw_unfocused_specimen(frame: &mut Frame, area: Rect, theme: &Slots, glyphs: &Glyphs) {
    let block = Block::bordered()
        .title(Span::styled(
            "unfocused",
            Style::default().fg(theme.ink_dim),
        ))
        .border_set(glyphs.border)
        .border_style(Style::default().fg(theme.border));
    frame.render_widget(block, area);
}

/// Fifteen points on a circle, which is enough shape to tell a drawn plot
/// from a blank one and few enough to stay deterministic.
const PLOT_SAMPLES: [(f64, f64); 15] = [
    (0.50, 0.95),
    (0.68, 0.90),
    (0.82, 0.78),
    (0.92, 0.62),
    (0.95, 0.44),
    (0.88, 0.27),
    (0.75, 0.14),
    (0.58, 0.06),
    (0.40, 0.06),
    (0.24, 0.15),
    (0.12, 0.29),
    (0.06, 0.46),
    (0.10, 0.64),
    (0.22, 0.80),
    (0.36, 0.91),
];

fn draw_plot(frame: &mut Frame, area: Rect, theme: &Slots, glyphs: &Glyphs) {
    match glyphs.plot {
        PlotStyle::Braille => {
            let canvas = Canvas::default()
                .marker(Marker::Braille)
                .x_bounds([0.0, 1.0])
                .y_bounds([0.0, 1.0])
                .paint(|ctx| {
                    ctx.draw(&Points {
                        coords: &PLOT_SAMPLES,
                        color: theme.accent,
                    });
                });
            frame.render_widget(canvas, area);
        }
        PlotStyle::Ascii => {
            let ramp = Glyphs::ASCII_DENSITY;
            let lines: Vec<Line> = (0..area.height)
                .map(|y| {
                    let cells: String = (0..area.width)
                        .map(|x| {
                            let step = (usize::from(x) + usize::from(y) * 2) % ramp.len();
                            ramp[step]
                        })
                        .collect();
                    Line::from(Span::styled(cells, Style::default().fg(theme.accent)))
                })
                .collect();
            frame.render_widget(Paragraph::new(Text::from(lines)), area);
        }
    }
}

/// Nothing but the terminal's own ink may reach a cell.
///
/// Severity is carried by a glyph and a word as well as a hue, which is what
/// makes this tier legible rather than merely safe.
pub(crate) fn assert_no_colour(buffer: &Buffer, context: &str) {
    for cell in buffer.content() {
        assert_eq!(
            cell.fg,
            Color::Reset,
            "{context} painted {:?} into a terminal with no colour to give",
            cell.fg
        );
        assert_eq!(cell.bg, Color::Reset, "{context} painted a ground");
    }
}

/// Ink is the terminal's own or a named ANSI slot; the only colours we choose
/// are the semantic hues from the shared palette.
pub(crate) fn assert_only_terminal_ink_or_palette_hues(
    buffer: &Buffer,
    theme: &Slots,
    context: &str,
) {
    let allowed = [Color::Reset, Color::DarkGray];
    let hues = [theme.accent, theme.success, theme.warning, theme.error];
    for cell in buffer.content() {
        assert!(
            allowed.contains(&cell.fg) || hues.contains(&cell.fg),
            "{context} painted {:?}, which is neither terminal ink nor a palette hue",
            cell.fg
        );
    }
}

/// The bulk of the surface is the terminal's own foreground, which is the one
/// value legible in every colour scheme.
pub(crate) fn assert_body_text_is_terminal_ink(buffer: &Buffer, context: &str) {
    let resets = buffer
        .content()
        .iter()
        .filter(|cell| cell.fg == Color::Reset && !cell.symbol().trim().is_empty())
        .count();
    assert!(
        resets > 20,
        "{context}: expected the bulk of the panel to be terminal ink, saw {resets}"
    );
}

/// The terminal owns the ground here. Painting over it is how ink becomes
/// invisible, which is the regression this whole model exists to prevent.
pub(crate) fn assert_no_background(buffer: &Buffer, context: &str) {
    for cell in buffer.content() {
        assert_eq!(
            cell.bg,
            Color::Reset,
            "{context} painted a background, which fights the user's terminal"
        );
    }
}

/// Lose detail, keep meaning: nothing outside the repertoire reaches a cell.
pub(crate) fn assert_only_ascii(buffer: &Buffer, context: &str) {
    for cell in buffer.content() {
        assert!(
            cell.symbol().is_ascii(),
            "{context}: {:?} reached the screen at the ASCII tier",
            cell.symbol()
        );
    }
}

/// The key event a terminal would deliver for this code.
///
/// Uppercase carries `SHIFT`, because that is what crossterm reports and a
/// dispatcher that reads modifiers must be handed the real thing. The kind is
/// always a press: releases and repeats are filtered before dispatch.
pub(crate) fn key(code: KeyCode) -> KeyEvent {
    let modifiers = match code {
        KeyCode::Char(c) if c.is_ascii_uppercase() => KeyModifiers::SHIFT,
        _ => KeyModifiers::NONE,
    };
    KeyEvent::new(code, modifiers)
}

/// Feed a sequence of key codes to a dispatcher.
///
/// Takes the dispatcher as a closure rather than a concrete shell, so the same
/// helper serves whatever owns the keymap next without either of them knowing
/// about the other.
pub(crate) fn press(mut dispatch: impl FnMut(KeyEvent), codes: &[KeyCode]) {
    for &code in codes {
        dispatch(key(code));
    }
}

#[cfg(test)]
mod tests {
    use solarxy_core::theme::Palette;

    use super::super::layout::Preset;

    use super::*;

    /// Every combination draws something. A tier that panics or paints an
    /// empty frame is worse than one that paints plainly.
    #[test]
    fn every_tier_and_glyph_pair_paints_the_reference_panel() {
        for caps in every_pair() {
            let buffer = render_reference_at(caps);
            let painted = buffer
                .content()
                .iter()
                .filter(|cell| !cell.symbol().trim().is_empty())
                .count();
            assert!(painted > 200, "{caps:?} painted only {painted} cells");
        }
    }

    #[test]
    fn the_monochrome_tier_paints_no_colour() {
        for glyphs in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let caps = Capabilities {
                color: ColorTier::Mono,
                glyphs,
            };
            assert_no_colour(&render_reference_at(caps), &format!("{caps:?}"));
        }
    }

    #[test]
    fn the_ascii_tier_paints_only_ascii() {
        for color in [
            ColorTier::Mono,
            ColorTier::Ansi16,
            ColorTier::Ansi256,
            ColorTier::TrueColor,
        ] {
            let caps = Capabilities {
                color,
                glyphs: GlyphTier::Ascii,
            };
            assert_only_ascii(&render_reference_at(caps), &format!("{caps:?}"));
        }
    }

    /// The three whole-frame invariants, asserted where they are true: at the
    /// two tiers that ignore the theme and leave the ground to the terminal.
    #[test]
    fn the_lower_tiers_keep_the_shipped_colour_rules() {
        let set = ThemeSet::bundled();
        // Every shipped theme, not just the default: the rules hold because
        // the tier refuses to read a file, so which file it refused to read
        // must not matter.
        for name in set.names() {
            let slots = set.slots_for(name).expect("bundled themes load");
            for caps in lower_tiers() {
                let context = format!("{name} at {caps:?}");
                let buffer = render_reference(caps, name, &slots);
                let resolved = Theme::resolve(caps, name, &slots);
                assert_only_terminal_ink_or_palette_hues(&buffer, &resolved.slots, &context);
                assert_body_text_is_terminal_ink(&buffer, &context);
                assert_no_background(&buffer, &context);
            }
        }
    }

    /// Every shipped theme renders at both tiers that read one. The colour
    /// rules above are deliberately not asserted here: painting its own ink on
    /// its own ground is the whole point of a theme.
    #[test]
    fn every_bundled_theme_renders_at_the_upper_tiers() {
        let set = ThemeSet::bundled();
        for name in set.names() {
            let slots = set.slots_for(name).expect("bundled themes load");
            for color in [ColorTier::Ansi256, ColorTier::TrueColor] {
                for glyphs in [GlyphTier::Unicode, GlyphTier::Ascii] {
                    let caps = Capabilities { color, glyphs };
                    let buffer = render_reference(caps, name, &slots);
                    let painted = buffer
                        .content()
                        .iter()
                        .filter(|cell| !cell.symbol().trim().is_empty())
                        .count();
                    assert!(painted > 200, "{name} at {caps:?} painted {painted} cells");
                }
            }
        }
    }

    /// A theme owns the ground at the tiers that read it, and the reference
    /// panel has to actually paint it or nothing downstream is guarding the
    /// slot.
    #[test]
    fn the_upper_tiers_paint_the_themes_ground() {
        let set = ThemeSet::bundled();
        let slots = set.slots_for("solarxy-paper").expect("loads");
        let caps = Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Unicode,
        };
        let buffer = render_reference(caps, "solarxy-paper", &slots);
        assert!(
            buffer.content().iter().any(|cell| cell.bg == slots.ground),
            "the theme's ground never reached a cell"
        );
    }

    /// The regression the maintainer caught in a real terminal: selecting the
    /// GUI's light theme painted near-black ink into a dark terminal, so the
    /// report was invisible.
    ///
    /// Scoped to the two tiers that ignore the theme, and only there. A
    /// shipped light theme derives its ink from this very palette and paints
    /// exactly these values at the richer tiers, on purpose, against a ground
    /// it also paints. Asserting this frame-wide would forbid the feature the
    /// theme system exists to provide.
    #[test]
    fn a_light_themes_ink_never_reaches_the_lower_tiers() {
        let set = ThemeSet::bundled();
        let paper = set.slots_for("solarxy-paper").expect("loads");
        let light = Palette::light();
        let ink = Color::Rgb(
            light.roles.ink_primary.rgb.r,
            light.roles.ink_primary.rgb.g,
            light.roles.ink_primary.rgb.b,
        );
        assert_eq!(paper.ink, ink, "the shipped light theme is the fixture");

        for caps in lower_tiers() {
            let buffer = render_reference(caps, "solarxy-paper", &paper);
            for cell in buffer.content() {
                assert_ne!(
                    cell.fg, ink,
                    "a light theme's ink was painted into a terminal at {caps:?}"
                );
                assert_ne!(cell.bg, paper.ground, "its ground reached {caps:?} too");
            }
        }
    }

    /// The richer tiers render exact palette entries rather than leaving the
    /// terminal to approximate an RGB triple it cannot show.
    #[test]
    fn the_256_tier_paints_indexed_colour_and_never_raw_rgb() {
        for glyphs in [GlyphTier::Unicode, GlyphTier::Ascii] {
            let buffer = render_reference_at(Capabilities {
                color: ColorTier::Ansi256,
                glyphs,
            });
            let mut saw_indexed = false;
            for cell in buffer.content() {
                assert!(
                    !matches!(cell.fg, Color::Rgb(..)),
                    "raw RGB reached a 256-colour terminal"
                );
                saw_indexed |= matches!(cell.fg, Color::Indexed(_));
            }
            assert!(saw_indexed, "nothing was quantised at all at {glyphs:?}");
        }
    }

    /// The accent is the cheapest proof that an authored colour survived the
    /// whole way to a cell.
    #[test]
    fn truecolor_carries_the_authored_accent() {
        let buffer = render_reference_at(Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Unicode,
        });
        let accent = ThemeSet::bundled()
            .slots_for(super::super::theme::DEFAULT_THEME)
            .expect("loads")
            .accent;
        assert!(
            buffer.content().iter().any(|cell| cell.fg == accent),
            "the authored accent never reached a truecolor terminal"
        );
    }

    /// Focus is carried by border weight before colour, so the two frames must
    /// differ on screen and not merely in the glyph table.
    #[test]
    fn both_border_weights_reach_the_screen() {
        for caps in every_pair() {
            let glyphs = caps.glyphs();
            let buffer = render_reference_at(caps);
            let symbols: Vec<&str> = buffer.content().iter().map(|cell| cell.symbol()).collect();
            assert!(
                symbols.contains(&glyphs.border_focused.top_left),
                "the focused frame is absent at {caps:?}"
            );
            assert!(
                symbols.contains(&glyphs.border.top_left),
                "the unfocused frame is absent at {caps:?}"
            );
        }
    }

    /// Severity is never carried by colour alone, so all three marks have to
    /// reach a cell at every tier. At the monochrome tier they are the only
    /// thing left distinguishing an error from a warning.
    #[test]
    fn the_severity_marks_reach_the_screen() {
        for caps in every_pair() {
            let glyphs = caps.glyphs();
            let buffer = render_reference_at(caps);
            let symbols: Vec<&str> = buffer.content().iter().map(|cell| cell.symbol()).collect();
            for (mark, name) in [
                (glyphs.check, "complete"),
                (glyphs.cross, "absent"),
                (glyphs.warn, "partial"),
            ] {
                assert!(
                    symbols.contains(&mark),
                    "the {name} mark {mark:?} never reached the screen at {caps:?}"
                );
            }
        }
    }

    /// The two composed elements, a table and a hand-built meter, are the
    /// shapes the panels are made of. A tier that silently drops one of them
    /// still looks plausible, which is what makes it worth asserting.
    #[test]
    fn the_table_and_the_meter_reach_the_screen() {
        for caps in every_pair() {
            let buffer = render_reference_at(caps);
            let rows: Vec<String> = (0..REFERENCE_HEIGHT)
                .map(|y| {
                    (0..REFERENCE_WIDTH)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect()
                })
                .collect();
            let screen = rows.join("\n");

            assert!(screen.contains("triangles"), "the table header at {caps:?}");
            assert!(screen.contains("16,384"), "a table row at {caps:?}");
            assert!(screen.contains("62% of budget"), "the meter at {caps:?}");

            let filled = match caps.glyphs {
                GlyphTier::Unicode => "\u{2588}\u{2588}",
                GlyphTier::Ascii => "##",
            };
            assert!(screen.contains(filled), "the meter's bar at {caps:?}");
        }
    }

    /// The plot is the one element that has no ASCII form of its own, so it is
    /// the element most likely to be dropped rather than degraded.
    #[test]
    fn the_plot_is_drawn_at_both_glyph_tiers() {
        let braille = render_reference_at(Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Unicode,
        });
        assert!(
            braille
                .content()
                .iter()
                .any(|cell| matches!(cell.symbol().chars().next(), Some('\u{2800}'..='\u{28ff}'))),
            "no braille reached a Unicode terminal"
        );

        let ascii = render_reference_at(Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Ascii,
        });
        let densest = Glyphs::ASCII_DENSITY[Glyphs::ASCII_DENSITY.len() - 1];
        assert!(
            ascii.content().iter().any(|cell| cell.symbol() == densest),
            "the density ramp never reached an ASCII terminal"
        );
    }

    /// Every leaf's frame lands exactly where the solve said, and the corners
    /// of adjacent panels meet without a gap or a doubled column. Arithmetic
    /// alone passes an off-by-one here; cells do not.
    #[test]
    fn every_panel_frames_the_rect_the_solve_predicted() {
        let caps = Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Unicode,
        };
        let area = Rect::new(0, 0, 140, 44);
        let layout = Preset::Survey.layout();
        let buffer = render_layout(caps, &layout, area);
        let glyphs = caps.glyphs();

        for placement in layout.solve(area, None) {
            let rect = placement.rect;
            let set = if placement.focused {
                glyphs.border_focused
            } else {
                glyphs.border
            };
            let corner = |x: u16, y: u16| buffer[(x, y)].symbol().to_owned();
            assert_eq!(
                corner(rect.x, rect.y),
                set.top_left,
                "{} top left at {rect:?}",
                placement.panel.name()
            );
            assert_eq!(
                corner(rect.right() - 1, rect.bottom() - 1),
                set.bottom_right,
                "{} bottom right at {rect:?}",
                placement.panel.name()
            );
        }
    }

    /// The panel name and its jump address reach the border, which is where
    /// the design puts them instead of spending a row on a header.
    #[test]
    fn each_panel_carries_its_address_and_name_in_the_border() {
        let caps = Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Unicode,
        };
        let area = Rect::new(0, 0, 140, 44);
        let layout = Preset::Survey.layout();
        let buffer = render_layout(caps, &layout, area);

        for placement in layout.solve(area, None) {
            let row: String = (placement.rect.x..placement.rect.right())
                .map(|x| buffer[(x, placement.rect.y)].symbol())
                .collect();
            assert!(
                row.contains(placement.panel.name()),
                "{} is missing from its own border: {row}",
                placement.panel.name()
            );
            assert!(
                row.contains(&caps.glyphs().address(placement.address)),
                "{} is missing address {}",
                placement.panel.name(),
                placement.address
            );
        }
    }

    /// Every cell of the pane belongs to exactly one panel, so no arrangement
    /// leaves an unpainted seam.
    #[test]
    fn the_arrangement_leaves_no_unpainted_cell() {
        let caps = Capabilities {
            color: ColorTier::TrueColor,
            glyphs: GlyphTier::Unicode,
        };
        let area = Rect::new(0, 0, 140, 44);
        for preset in Preset::ALL {
            let buffer = render_layout(caps, &preset.layout(), area);
            for y in 0..area.height {
                let row: String = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
                assert!(
                    !row.trim().is_empty(),
                    "{} left row {y} entirely blank",
                    preset.name()
                );
            }
        }
    }

    /// Uppercase arrives with the modifier a real terminal sets, so a keymap
    /// that reads modifiers is handed the truth rather than a convenient
    /// fiction.
    #[test]
    fn synthesised_keys_carry_the_modifiers_a_terminal_sends() {
        assert_eq!(key(KeyCode::Char('j')).modifiers, KeyModifiers::NONE);
        assert_eq!(key(KeyCode::Char('J')).modifiers, KeyModifiers::SHIFT);
        assert_eq!(key(KeyCode::Esc).modifiers, KeyModifiers::NONE);

        let mut seen = Vec::new();
        press(
            |event| seen.push(event.code),
            &[KeyCode::Char('g'), KeyCode::Down, KeyCode::Esc],
        );
        assert_eq!(
            seen,
            vec![KeyCode::Char('g'), KeyCode::Down, KeyCode::Esc],
            "keys must arrive in the order they were pressed"
        );
    }
}
