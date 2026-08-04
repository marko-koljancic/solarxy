//! The tiled surface: an arrangement of panels over one report.
//!
//! # Reachable, but not yet the default
//!
//! `SOLARXY_TUI=next` opens this instead of the shipped tabs. It exists
//! because the two tasks after this one have verification no test can perform:
//! whether a braille silhouette of a dense mesh reads as a form or as a solid
//! rectangle is, in the design's own words, the one thing only rendering can
//! settle. Building three panels on a rasteriser nobody has looked at is the
//! avoidable version of that risk.
//!
//! The shipped tabs stay the default until every panel exists, because cutting
//! over before then would ship a surface missing something readers have today.
//! Both this switch and the shell it opens are removed in the same stage.
//!
//! # What lives here and what does not
//!
//! The app owns the arrangement, which panel holds each leaf, and the per-leaf
//! state. It does not own what a panel says: that is the panel's, and the app
//! hands it a borrowed view of the report rather than a copy.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;
use solarxy_core::report::AnalysisReport;

use super::caps::{Capabilities, Glyphs};
use super::layout::{Layout, LeafId, Preset};
use super::panels::{self, Ctx, Panel};
use super::shell::{Flow, Input, Surface};
use super::theme::Theme;

/// Opts into the tiled surface ahead of the cutover.
pub const OPT_IN_ENV_VAR: &str = "SOLARXY_TUI";

/// Whether the reader asked for the tiled surface.
pub fn opted_in(lookup: impl Fn(&str) -> Option<String>) -> bool {
    matches!(
        lookup(OPT_IN_ENV_VAR)
            .map(|raw| raw.trim().to_ascii_lowercase())
            .as_deref(),
        Some("next")
    )
}

pub struct App<'a> {
    report: &'a AnalysisReport,
    layout: Layout,
    maximized: Option<LeafId>,
    /// One panel per leaf, so two of a type are two panels with their own
    /// selection and sort rather than one drawn twice.
    panels: HashMap<LeafId, Box<dyn Panel>>,
    preset: Preset,
    theme: Theme,
    glyphs: Glyphs,
    caps: Capabilities,
    /// A refusal, shown in the focused panel's border until the next key.
    notice: Option<String>,
    exit: bool,
}

impl<'a> App<'a> {
    pub fn new(
        report: &'a AnalysisReport,
        layout: Layout,
        theme: Theme,
        caps: Capabilities,
    ) -> Self {
        let mut app = Self {
            report,
            layout,
            maximized: None,
            panels: HashMap::new(),
            preset: Preset::Survey,
            theme,
            glyphs: caps.glyphs(),
            caps,
            notice: None,
            exit: false,
        };
        app.sync_panels();
        app
    }

    /// Give every leaf a panel and forget the ones whose leaves are gone.
    ///
    /// Called after anything that changes the tree. Keying on the leaf id is
    /// what makes a panel's state survive its neighbours being closed.
    fn sync_panels(&mut self) {
        let leaves = self.layout.leaves();
        for (id, kind) in &leaves {
            self.panels
                .entry(*id)
                .or_insert_with(|| panels::make(*kind));
        }
        self.panels
            .retain(|id, _| leaves.iter().any(|(l, _)| l == id));
    }

    fn context(&self, focused: bool) -> Ctx<'_> {
        Ctx {
            report: self.report,
            theme: &self.theme.slots,
            glyphs: &self.glyphs,
            caps: self.caps,
            focused,
        }
    }

    /// Write the arrangement and the theme back for next time.
    ///
    /// What is persisted is the reader's own tree, never the elided view and
    /// never the maximized one, so a narrow terminal or a moment spent reading
    /// one panel full-screen cannot quietly become the saved layout.
    ///
    /// A file that will not write is not worth interrupting a quit over: the
    /// reader asked to leave and the arrangement is a convenience.
    fn remember(&self) {
        let (mut prefs, _) = super::prefs::TuiPrefs::load();
        prefs.last_layout = Some(self.layout.encode());
        prefs.theme = Some(self.theme.name.clone());
        if let Err(error) = prefs.save() {
            tracing::warn!("could not save the terminal layout: {error}");
        }
    }

    /// The pane the arrangement gets, which is everything but the footer.
    fn pane(area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1))
    }
}

impl Surface for App<'_> {
    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let pane = Self::pane(area);

        // The elision is a view: what the reader arranged is untouched, and a
        // wider terminal brings the dropped panels straight back.
        let shown = self.layout.elided(pane.width);
        let placements = shown.solve(pane, self.maximized);

        for placement in &placements {
            let ctx = self.context(placement.focused);
            let border = if placement.focused {
                self.glyphs.border_focused
            } else {
                self.glyphs.border
            };
            let ink = ctx.chrome();

            let mut title = vec![
                Span::styled(
                    self.glyphs.address(placement.address),
                    Style::default().fg(ink),
                ),
                Span::styled(
                    placement.panel.name(),
                    Style::default().fg(ink).add_modifier(Modifier::BOLD),
                ),
            ];
            if placement.focused {
                let menu = self
                    .panels
                    .get(&placement.id)
                    .map_or(&[][..], |panel| panel.menu());
                for word in menu {
                    title.push(Span::styled(
                        format!(" {word} "),
                        Style::default()
                            .fg(self.theme.slots.accent)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
            }

            let mut block = Block::bordered()
                .title(Line::from(title))
                .border_set(border)
                .border_style(Style::default().fg(ink));

            // A refusal belongs on the panel it was refused for, not in an
            // overlay: the reader asked this panel to do something and this
            // panel is what could not.
            if placement.focused {
                if let Some(notice) = &self.notice {
                    block = block.title_bottom(
                        Line::from(Span::styled(
                            format!(" {notice} "),
                            Style::default().fg(self.theme.slots.error),
                        ))
                        .centered(),
                    );
                } else if let Some(status) = self
                    .panels
                    .get(&placement.id)
                    .and_then(|panel| panel.status(&self.context(true)))
                {
                    block = block.title_bottom(
                        Line::from(Span::styled(
                            format!(" {status} "),
                            Style::default().fg(self.theme.slots.ink_dim),
                        ))
                        .right_aligned(),
                    );
                }
            }

            let inner = block.inner(placement.rect);
            frame.render_widget(block, placement.rect);
            if let Some(panel) = self.panels.get_mut(&placement.id) {
                let ctx = Ctx {
                    report: self.report,
                    theme: &self.theme.slots,
                    glyphs: &self.glyphs,
                    caps: self.caps,
                    focused: placement.focused,
                };
                panel.draw(frame, inner, &ctx);
            }
        }

        draw_footer(
            frame,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            self.report,
            &self.theme,
            &self.glyphs,
        );
    }

    fn handle(&mut self, input: Input) -> Flow {
        let Input::Key(key) = input else {
            return Flow::Continue;
        };
        self.notice = None;

        // Global keys first, then anything left goes to the focused panel.
        // The full table and its contexts arrive with the chrome; this is the
        // subset the panels need to be usable.
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) => {
                self.exit = true;
                self.remember();
                return Flow::Quit;
            }
            (KeyCode::Char(c @ '1'..='9'), _) => {
                let address = c as u8 - b'0';
                self.layout = self
                    .layout
                    .with_focus_on_address(Self::pane(current_size()), address);
                return Flow::Continue;
            }
            (KeyCode::Char('p'), _) => {
                self.preset = self.preset.next();
                self.layout = self.preset.layout();
                self.maximized = None;
                self.sync_panels();
                return Flow::Continue;
            }
            (KeyCode::Enter, _) => {
                self.maximized = Some(self.layout.focus());
                return Flow::Continue;
            }
            (KeyCode::Esc, _) => {
                self.maximized = None;
                return Flow::Continue;
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                self.notice = Some("arrange mode arrives with the chrome".to_owned());
                return Flow::Continue;
            }
            _ => {}
        }

        let focus = self.layout.focus();
        let ctx = Ctx {
            report: self.report,
            theme: &self.theme.slots,
            glyphs: &self.glyphs,
            caps: self.caps,
            focused: true,
        };
        if let Some(panel) = self.panels.get_mut(&focus) {
            let _ = panel.handle(key, &ctx);
        }
        Flow::Continue
    }
}

/// The terminal's size, for the one key that needs it between frames.
///
/// A jump address is resolved against the arrangement as last solved, and the
/// solve needs a pane. Asking the terminal is cheaper and more honest than
/// caching a size that a resize could have invalidated.
fn current_size() -> Rect {
    let (width, height) = crossterm::terminal::size().unwrap_or((140, 45));
    Rect::new(0, 0, width, height)
}

fn draw_footer(
    frame: &mut Frame,
    area: Rect,
    report: &AnalysisReport,
    theme: &Theme,
    glyphs: &Glyphs,
) {
    let key = |k: &str| {
        Span::styled(
            k.to_owned(),
            Style::default()
                .fg(theme.slots.accent)
                .add_modifier(Modifier::BOLD),
        )
    };
    let word = |w: &str| Span::styled(w.to_owned(), Style::default().fg(theme.slots.ink));

    // The keys are named in the repertoire the terminal has, not in the one
    // the design was drawn in. When the keymap table generates this strip it
    // takes the same rule with it.
    let select = format!("{}{}", glyphs.scroll_up, glyphs.scroll_down);
    let enter = match glyphs.tier {
        crate::tui::caps::GlyphTier::Unicode => "\u{21b5}",
        crate::tui::caps::GlyphTier::Ascii => "ent",
    };

    let mut spans = Vec::new();
    for (k, w) in [
        ("1-9", " panel  "),
        (select.as_str(), " select  "),
        (enter, " max  "),
        ("p", " preset  "),
        ("q", " quit"),
    ] {
        spans.push(key(k));
        spans.push(word(w));
    }
    frame.render_widget(ratatui::widgets::Paragraph::new(Line::from(spans)), area);

    let errors = report.validation.error_count();
    let warnings = report.validation.warning_count();
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} {errors}  ", glyphs.cross),
                Style::default().fg(theme.slots.error),
            ),
            Span::styled(
                format!("{} {warnings} ", glyphs.warn),
                Style::default().fg(theme.slots.warning),
            ),
        ]))
        .right_aligned(),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::caps::{ColorTier, GlyphTier};
    use crate::tui::layout::PanelType;
    use crate::tui::theme::{DEFAULT_THEME, ThemeSet};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    const CAPS: Capabilities = Capabilities {
        color: ColorTier::TrueColor,
        glyphs: GlyphTier::Unicode,
    };

    fn report() -> AnalysisReport {
        AnalysisReport {
            model_name: "frog.obj".into(),
            mesh_count: 2,
            material_count: 1,
            total_vertices: 5_350,
            total_indices: 25_848,
            total_triangles: 8_616,
            bounds: None,
            meshes: vec![
                solarxy_core::report::MeshSummary {
                    index: 0,
                    name: "body".into(),
                    vertex_count: 5_102,
                    index_count: 24_612,
                    triangle_count: 8_204,
                    normal_count: 5_102,
                    texcoord_count: 5_102,
                    material_name: Some("skin".into()),
                    material_id: Some(0),
                    degenerate_faces: Vec::new(),
                },
                solarxy_core::report::MeshSummary {
                    index: 1,
                    name: "eyes".into(),
                    vertex_count: 248,
                    index_count: 1_236,
                    triangle_count: 412,
                    normal_count: 248,
                    texcoord_count: 0,
                    material_name: None,
                    material_id: None,
                    degenerate_faces: Vec::new(),
                },
            ],
            materials: vec![solarxy_core::report::MaterialSummary {
                index: 0,
                name: "skin".into(),
                ambient: [0.1, 0.1, 0.1],
                diffuse: [0.82, 0.64, 0.55],
                specular: [0.0; 3],
                shininess: None,
                dissolve: None,
                optical_density: None,
                textures: Vec::new(),
            }],
            validation: solarxy_core::validation::ValidationReport::default(),
            source_format: "obj".into(),
            file_size_bytes: Some(2_516_582),
            asset_category: None,
            triangle_budget: Some(20_000),
        }
    }

    fn render(caps: Capabilities, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let report = report();
        let slots = ThemeSet::bundled()
            .slots_for(DEFAULT_THEME)
            .expect("the default loads");
        let theme = Theme::resolve(caps, DEFAULT_THEME, &slots);
        let mut app = App::new(&report, Preset::Survey.layout(), theme, caps);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        terminal.backend().buffer().clone()
    }

    fn screen(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) -> String {
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Not an assertion: a way to look at the surface without a terminal.
    #[test]
    #[ignore]
    fn preview() {
        let buffer = render(CAPS, 140, 45);
        println!("{}", screen(&buffer, 140, 45));
    }

    /// The opt-in reads like every other override this shell has.
    #[test]
    fn the_switch_is_off_unless_it_says_next() {
        let env = |value: Option<&str>| {
            let owned = value.map(str::to_owned);
            move |key: &str| (key == OPT_IN_ENV_VAR).then(|| owned.clone()).flatten()
        };
        assert!(opted_in(env(Some("next"))));
        assert!(opted_in(env(Some(" NEXT "))));
        assert!(!opted_in(env(Some("1"))));
        assert!(!opted_in(env(Some(""))));
        assert!(!opted_in(env(None)));
    }

    /// Every fact the four panels are responsible for reaches the screen at
    /// the target size.
    #[test]
    fn the_four_panels_put_their_facts_on_the_screen() {
        let buffer = render(CAPS, 140, 45);
        let text = screen(&buffer, 140, 45);
        for expected in [
            "geometry",
            "health",
            "meshes",
            "materials", // panel names
            "OBJ",
            "2.4 MB", // geometry identity
            "8,616",  // triangle total
            "body",
            "eyes", // recovered mesh names
            "skin", // material and its use
            "43%",  // the budget meter, 8,616 against 20,000
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} is missing from:\n{text}"
            );
        }
    }

    /// The recovered mesh names are the whole point of the mesh table. Before
    /// this release no surface could say anything but an index.
    #[test]
    fn the_mesh_table_names_its_rows() {
        let text = screen(&render(CAPS, 140, 45), 140, 45);
        assert!(text.contains("body"), "{text}");
        assert!(!text.contains("Mesh [0]"), "an index leaked into a row");
    }

    #[test]
    fn every_tier_and_glyph_pair_draws_the_whole_surface() {
        for caps in crate::tui::harness::every_pair() {
            let buffer = render(caps, 140, 45);
            let painted = buffer
                .content()
                .iter()
                .filter(|cell| !cell.symbol().trim().is_empty())
                .count();
            assert!(painted > 400, "{caps:?} painted {painted} cells");
            if caps.glyphs == GlyphTier::Ascii {
                for cell in buffer.content() {
                    assert!(
                        cell.symbol().is_ascii(),
                        "{:?} reached an ASCII terminal at {caps:?}",
                        cell.symbol()
                    );
                }
            }
        }
    }

    /// The floor size has to hold a usable surface, not just render.
    #[test]
    fn the_surface_works_at_the_floor_size() {
        let text = screen(&render(CAPS, 100, 30), 100, 30);
        // Below the narrow threshold the elision drops the picture panels and
        // keeps the ones that answer whether the asset passes.
        assert!(text.contains("geometry"), "{text}");
        assert!(text.contains("meshes"), "{text}");
        assert!(
            !text.contains("silhouette"),
            "silhouette survived at 100 columns"
        );
    }

    /// Focus is exactly one panel, and its border is the heavier set.
    #[test]
    fn exactly_one_panel_is_drawn_focused() {
        let buffer = render(CAPS, 140, 45);
        let focused = Glyphs::for_tier(GlyphTier::Unicode).border_focused.top_left;
        let count = buffer
            .content()
            .iter()
            .filter(|cell| cell.symbol() == focused)
            .count();
        assert_eq!(count, 1, "expected one focused frame, saw {count}");
    }

    /// Only the focused panel shows its menu words, which is what stops six
    /// borders becoming six menus competing for attention.
    #[test]
    fn only_the_focused_panel_shows_its_menu() {
        let report = report();
        let slots = ThemeSet::bundled().slots_for(DEFAULT_THEME).expect("loads");
        let theme = Theme::resolve(CAPS, DEFAULT_THEME, &slots);

        // Survey focuses silhouette, which has no menu of its own yet; move
        // focus to the mesh table, which has two words.
        let layout = Preset::Survey.layout();
        let meshes = layout
            .leaves()
            .into_iter()
            .find(|(_, kind)| *kind == PanelType::Meshes)
            .expect("survey holds a mesh panel")
            .0;
        let mut app = App::new(&report, layout.with_focus(meshes), theme, CAPS);

        let mut terminal = Terminal::new(TestBackend::new(140, 45)).expect("terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        let text = screen(terminal.backend().buffer(), 140, 45);
        assert!(text.contains("sort"), "the focused panel hid its menu");
    }
}
