//! Four overlays, and the one thing that is deliberately not one.
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
    Catalogue(Catalogue),
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

/// The panel pick, which is arrange mode's `a`.
///
/// A split leaves a leaf whose only content is "pick a panel", and this is
/// the pick: every choosable type in catalogue order, one selected, return
/// gives it to the focused leaf. It works on a leaf that already holds a
/// panel too, because "give the focused leaf a panel type" is the whole
/// operation and refusing the second case would invent a distinction the
/// tree does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    /// Index into the name list the surface supplies through
    /// [`Chrome::catalogue`], which is in the same order as the panels a
    /// reader can choose.
    pub selected: usize,
}

impl Overlay {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Help => "Keyboard",
            Self::Export(_) => "Export",
            Self::Confirm(_) => "Close panel",
            Self::Catalogue(_) => "Add panel",
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

/// Everything an overlay draws with that is not the overlay itself.
///
/// A bundle rather than five arguments, and the two name lists are why: this
/// module used to read the analyze panel enum directly, which is the one thing
/// that made it analyze's. Handed the words instead, it belongs to no surface,
/// and a second surface's overlays cost nothing here.
#[derive(Clone, Copy)]
pub struct Chrome<'a> {
    pub theme: &'a Slots,
    pub glyphs: &'a Glyphs,
    pub caps: Capabilities,
    /// The focused panel's own border words, which the help overlay repeats.
    pub panel_menu: &'a [&'static str],
    /// The panels the pick offers, in the order a selection indexes them.
    pub catalogue: &'a [&'static str],
}

/// Draw an overlay centred over the dimmed grid: the window, its double
/// border, and the body the variant supplies.
pub fn draw(frame: &mut Frame, area: Rect, overlay: &Overlay, chrome: &Chrome<'_>) {
    let Chrome {
        theme,
        glyphs,
        caps,
        panel_menu,
        catalogue: catalogue_names,
    } = *chrome;
    dim(frame, area, theme);

    let body = match overlay {
        Overlay::Help => help_lines(theme, glyphs, panel_menu),
        Overlay::Export(export) => export_lines(export, theme, glyphs),
        Overlay::Confirm(confirm) => confirm_lines(confirm, theme),
        Overlay::Catalogue(catalogue) => catalogue_lines(catalogue, theme, catalogue_names),
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

/// The pick list: every choosable type, the selected one marked the way the
/// export overlay marks its chosen format, so the language is already
/// learned and survives monochrome.
///
/// The names arrive rather than being read off a panel enum, because this
/// module has no business knowing which surface is asking. What it costs is
/// that nothing here can check the list is the whole catalogue; the surface
/// that assembles it asserts that instead.
fn catalogue_lines(
    catalogue: &Catalogue,
    theme: &Slots,
    names: &[&'static str],
) -> Vec<Line<'static>> {
    let radio = |on: bool| if on { "(\u{2022})" } else { "( )" };
    let mut lines = vec![Line::raw("")];
    for (i, name) in names.iter().enumerate() {
        let chosen = i == catalogue.selected;
        let style = if chosen {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.ink)
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {}", radio(chosen), name),
            style,
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  \u{21b5}",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  add    ", Style::default().fg(theme.ink)),
        Span::styled(
            "j k",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  move", Style::default().fg(theme.ink)),
    ]));
    lines
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

    /// The pick lists everything it was handed and marks the selection.
    ///
    /// That the list *is* the whole catalogue is asserted where the list is
    /// now assembled, in the surface, since this module no longer knows which
    /// surface is asking. Driven with the analyze names anyway, so the shape a
    /// reader actually sees is the shape under test.
    #[test]
    fn the_catalogue_lists_everything_it_is_given_and_marks_the_pick() {
        let theme = slots();
        // Invented rather than borrowed from a surface: this module no longer
        // knows any, and a test that reached for one would put the coupling
        // back.
        let names = ["first", "second", "third", "fourth"];
        let text: String = catalogue_lines(&Catalogue { selected: 2 }, &theme, &names)
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.to_string())
            .collect();
        for name in &names {
            assert!(text.contains(name), "missing {name}");
        }
        assert!(text.contains(&format!("(\u{2022}) {}", names[2])), "{text}");
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
