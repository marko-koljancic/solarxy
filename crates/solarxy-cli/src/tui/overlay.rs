//! Three overlays, and the one thing that is deliberately not one.
//!
//! # Elevation is a border weight, not a shadow
//!
//! Panels use the rounded single set and overlays the double one. Two weights
//! carry the whole elevation language, with no shadows and no fake depth, and
//! both survive monochrome where a colour-only treatment would not.
//!
//! # The grid dims, it does not vanish
//!
//! An overlay that hides the thing it is about makes a reader close it to
//! remember why they opened it. So the arrangement stays visible underneath at
//! reduced weight rather than being painted over.
//!
//! # Filter is not one of these
//!
//! It lives in the panel's own border, because filtering is about one panel
//! and an overlay would say otherwise while covering the very rows being
//! filtered.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::caps::{Capabilities, Glyphs};
use super::keymap::{self, Context};
use super::theme::Slots;

/// What is open over the grid, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Export(Export),
    Confirm(Confirm),
}

/// The two prompts the shipped shell puts on its footer border, promoted to
/// one overlay with the format chosen inside.
///
/// Folding them together is what keeps the JSON capability alive: it was
/// reachable only through a shift-only key that appeared nowhere, and it now
/// has a visible control instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub json: bool,
    pub path: String,
}

/// The one destructive thing in a read-only report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub panel: String,
    pub address: u8,
}

impl Overlay {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Help => "Keyboard",
            Self::Export(_) => "Export",
            Self::Confirm(_) => "Close panel",
        }
    }
}

/// Reduce everything already drawn to a dim ground.
///
/// Restyled rather than repainted, so the arrangement is still legible and
/// still where it was. A reader who opens help has not stopped looking at
/// their model.
pub fn dim(frame: &mut Frame, area: Rect, theme: &Slots) {
    frame.buffer_mut().set_style(
        area,
        Style::default()
            .fg(theme.ink_dim)
            .remove_modifier(Modifier::BOLD),
    );
}

/// Where an overlay sits: centred, and never larger than the pane.
fn window(area: Rect, want_width: u16, want_height: u16) -> Rect {
    let width = want_width.min(area.width);
    let height = want_height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    overlay: &Overlay,
    theme: &Slots,
    glyphs: &Glyphs,
    caps: Capabilities,
    panel_menu: &[&'static str],
) {
    dim(frame, area, theme);

    let body = match overlay {
        Overlay::Help => help_lines(theme, glyphs, panel_menu),
        Overlay::Export(export) => export_lines(export, theme, glyphs),
        Overlay::Confirm(confirm) => confirm_lines(confirm, theme),
    };
    let want_width = body
        .iter()
        .map(|line| line.width() as u16 + 4)
        .max()
        .unwrap_or(30)
        .max(30);
    let rect = window(area, want_width, body.len() as u16 + 2);

    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Line::from(Span::styled(
            format!(" {} ", overlay.title()),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )))
        .title_bottom(
            Line::from(Span::styled(" esc  close ", Style::default().fg(theme.ink_dim)))
                .right_aligned(),
        )
        // The double set, reserved for elevation, so an overlay reads as one
        // even where no colour is painted at all.
        .border_set(glyphs.border_focused)
        .border_style(Style::default().fg(theme.accent));
    let inner = block.inner(rect);
    frame.render_widget(
        block.style(if caps.color.paints_a_ground() {
            Style::default().bg(theme.panel_ground)
        } else {
            Style::default()
        }),
        rect,
    );
    frame.render_widget(Paragraph::new(body), inner);
}

/// The help overlay, grouped by context in the table's own order.
///
/// Generated, so it cannot drift from the footer or from what the keys do.
/// The shipped shell has no help surface at all.
fn help_lines(theme: &Slots, glyphs: &Glyphs, panel_menu: &[&'static str]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for context in [Context::Global, Context::Focused, Context::Arrange] {
        lines.push(Line::from(Span::styled(
            context.heading().to_owned(),
            Style::default()
                .fg(theme.ink_dim)
                .add_modifier(Modifier::BOLD),
        )));
        for binding in keymap::rows(context) {
            lines.push(entry(
                &keymap::label(binding, glyphs.tier),
                binding.describes,
                theme,
            ));
        }
        lines.push(Line::raw(""));
    }
    if !panel_menu.is_empty() {
        lines.push(Line::from(Span::styled(
            Context::Panel.heading().to_owned(),
            Style::default()
                .fg(theme.ink_dim)
                .add_modifier(Modifier::BOLD),
        )));
        for word in panel_menu {
            lines.push(entry(
                &keymap::panel_key_label(word, glyphs.tier),
                word,
                theme,
            ));
        }
    }
    lines
}

fn entry(key: &str, describes: &str, theme: &Slots) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {key:<10}"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(describes.to_owned(), Style::default().fg(theme.ink)),
    ])
}

fn export_lines(export: &Export, theme: &Slots, glyphs: &Glyphs) -> Vec<Line<'static>> {
    let radio = |on: bool| if on { "(\u{2022})" } else { "( )" };
    vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  format    ", Style::default().fg(theme.ink_dim)),
            Span::styled(
                format!("{} text   ", radio(!export.json)),
                Style::default().fg(if export.json {
                    theme.ink_dim
                } else {
                    theme.ink
                }),
            ),
            Span::styled(
                format!("{} json", radio(export.json)),
                Style::default().fg(if export.json {
                    theme.ink
                } else {
                    theme.ink_dim
                }),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  path      ", Style::default().fg(theme.ink_dim)),
            Span::styled(export.path.clone(), Style::default().fg(theme.ink)),
            Span::styled(glyphs.caret.to_owned(), Style::default().fg(theme.accent)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "  \u{21b5}",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  save    ", Style::default().fg(theme.ink)),
            Span::styled(
                "\u{2192}",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  swap format", Style::default().fg(theme.ink)),
        ]),
    ]
}

fn confirm_lines(confirm: &Confirm, theme: &Slots) -> Vec<Line<'static>> {
    vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Close ", Style::default().fg(theme.ink)),
            Span::styled(
                confirm.panel.clone(),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("?", Style::default().fg(theme.ink)),
        ]),
        Line::from(Span::styled(
            "  Its filter and sort are not saved.",
            Style::default().fg(theme.ink_dim),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "  \u{21b5}",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  close    ", Style::default().fg(theme.ink)),
            Span::styled(
                "esc",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  keep", Style::default().fg(theme.ink)),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{DEFAULT_THEME, ThemeSet};

    fn slots() -> Slots {
        ThemeSet::bundled()
            .slots_for(DEFAULT_THEME)
            .expect("the default loads")
    }

    /// The help overlay is a view of the table and nothing else, so a key
    /// absent from the table cannot appear in it.
    #[test]
    fn help_shows_every_row_of_the_table_and_no_others() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(crate::tui::caps::GlyphTier::Unicode);
        let text: String = help_lines(&theme, &glyphs, &[])
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.to_string())
            .collect();

        for context in [Context::Global, Context::Focused, Context::Arrange] {
            assert!(text.contains(context.heading()), "{}", context.heading());
            for binding in keymap::rows(context) {
                assert!(
                    text.contains(binding.describes),
                    "help omits {:?}",
                    binding.describes
                );
            }
        }
    }

    /// The focused panel's own actions belong in help too, or the border says
    /// a panel can do something and nothing says how.
    #[test]
    fn help_carries_the_focused_panels_own_actions() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(crate::tui::caps::GlyphTier::Unicode);
        let text: String = help_lines(&theme, &glyphs, &["axis", "fit"])
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.to_string())
            .collect();
        assert!(text.contains("THIS PANEL"), "{text}");
        assert!(text.contains("axis"), "{text}");
        assert!(text.contains("fit"), "{text}");
    }

    /// A panel with no menu contributes no section rather than an empty one.
    #[test]
    fn a_panel_with_no_actions_adds_no_section() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(crate::tui::caps::GlyphTier::Unicode);
        let text: String = help_lines(&theme, &glyphs, &[])
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.to_string())
            .collect();
        assert!(!text.contains("THIS PANEL"), "{text}");
    }

    /// An overlay never grows past the pane, however little room there is.
    #[test]
    fn an_overlay_stays_inside_a_small_pane() {
        for (w, h) in [(100u16, 30u16), (40, 10), (24, 6)] {
            let area = Rect::new(0, 0, w, h);
            let rect = window(area, 60, 24);
            assert!(rect.right() <= area.right(), "{w}x{h}");
            assert!(rect.bottom() <= area.bottom(), "{w}x{h}");
        }
    }

    #[test]
    fn the_export_overlay_shows_which_format_is_chosen() {
        let theme = slots();
        let glyphs = Glyphs::for_tier(crate::tui::caps::GlyphTier::Unicode);
        let text = |json: bool| -> String {
            export_lines(
                &Export {
                    json,
                    path: "/models/frog.json".to_owned(),
                },
                &theme,
                &glyphs,
            )
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.to_string())
            .collect()
        };
        assert!(text(true).contains("(\u{2022}) json"), "{}", text(true));
        assert!(text(false).contains("(\u{2022}) text"), "{}", text(false));
    }

    /// Confirm names what it is about to close and why it is asking, because
    /// a prompt that says neither trains a reader to dismiss it.
    #[test]
    fn confirm_names_the_panel_and_what_is_lost() {
        let theme = slots();
        let text: String = confirm_lines(
            &Confirm {
                panel: "meshes".to_owned(),
                address: 3,
            },
            &theme,
        )
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.to_string())
        .collect();
        assert!(text.contains("meshes"), "{text}");
        assert!(text.contains("not saved"), "{text}");
    }
}
