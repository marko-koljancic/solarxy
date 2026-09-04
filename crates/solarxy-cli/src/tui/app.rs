//! The tiled surface: an arrangement of panels over one report.
//!
//! # What lives here and what does not
//!
//! The app owns the arrangement, which panel holds each leaf, and the per-leaf
//! state. It does not own what a panel says: that is the panel's, and the app
//! hands it a borrowed view of the report rather than a copy.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;
use solarxy_core::report::AnalysisReport;

use super::caps::{Capabilities, Glyphs};
use super::geometry::ModelView;
use super::arrange::{self, Toward};
use super::keymap::{self, Command, Context};
use super::layout::{Direction, Layout, LeafId, PanelKind, PanelType, Preset};
use super::overlay::{Catalogue, Confirm, Export, Overlay};
use super::panels::{self, Action, Analysis, BoxedAnalyzePanel, Ctx};
use super::shell::{Flow, Input, Surface};
use super::theme::Theme;

/// The analyze surface: the arrangement, one panel per leaf, and whatever
/// modal state (overlay, filter, arrange mode, maximize) sits over it, all
/// over one borrowed report.
pub struct App<'a> {
    /// What this surface is about, in the shape its panels read it.
    subject: Analysis<'a>,
    layout: Layout<PanelType>,
    maximized: Option<LeafId>,
    /// One panel per leaf, so two of a type are two panels with their own
    /// selection and sort rather than one drawn twice.
    panels: HashMap<LeafId, BoxedAnalyzePanel<'a>>,
    preset: Preset,
    theme: Theme,
    glyphs: Glyphs,
    caps: Capabilities,
    /// A refusal, shown in the focused panel's border until the next key.
    notice: Option<String>,
    /// What is open over the grid, if anything.
    overlay: Option<Overlay>,
    /// Whether the reader is arranging rather than reading.
    arranging: bool,
    /// The live filter query, while one is being typed.
    filtering: Option<String>,
    exit: bool,
}

impl<'a> App<'a> {
    /// Build the surface over a report and its borrowed model arrays, with a
    /// panel constructed for every leaf the layout names.
    pub fn new(
        report: &'a AnalysisReport,
        model: &'a ModelView<'a>,
        layout: Layout<PanelType>,
        theme: Theme,
        caps: Capabilities,
    ) -> Self {
        let mut app = Self {
            subject: Analysis { report, model },
            layout,
            maximized: None,
            panels: HashMap::new(),
            preset: Preset::Survey,
            theme,
            glyphs: caps.glyphs(),
            caps,
            notice: None,
            overlay: None,
            arranging: false,
            filtering: None,
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

    /// The context for one panel this frame.
    ///
    /// Two lifetimes and they are genuinely different: the borrow is this
    /// frame's, the subject is the surface's. A boxed panel names the subject
    /// type exactly, and a trait object is invariant in it, so collapsing the
    /// two would hand a panel a subject with a shorter life than the one it
    /// was built for.
    fn context(&self, focused: bool) -> Ctx<'_, Analysis<'a>> {
        Ctx {
            subject: &self.subject,
            theme: &self.theme.slots,
            glyphs: &self.glyphs,
            caps: self.caps,
            focused,
        }
    }

    /// Escape means back, and back is relative to what is open.
    ///
    /// One layer per press, in the design's order, and never a quit. A key
    /// that sometimes leaves a dialog and sometimes ends the session is the
    /// kind a reader stops trusting.
    fn escape(&mut self) {
        if self.overlay.take().is_some() {
            return;
        }
        if self.filtering.take().is_some() {
            self.apply_filter(None);
            return;
        }
        if self.arranging {
            self.arranging = false;
            return;
        }
        if self.maximized.take().is_some() {
            return;
        }
        // Nothing left to go back to, and the tabbed shell this replaced quit
        // on this key. A reader who learned it there gets an answer instead of
        // silence. This is a bridge for readers arriving from 0.8.1 and comes
        // out again once they have arrived.
        self.notice = Some("nothing to go back to; press q to quit".to_owned());
    }

    fn global(&mut self, command: Command) -> Flow {
        match command {
            Command::Quit => {
                self.exit = true;
                self.remember();
                return Flow::Quit;
            }
            Command::FocusAddress(address) => {
                self.layout = self
                    .layout
                    .with_focus_on_address(Self::pane(current_size()), address);
            }
            Command::CyclePreset => {
                self.preset = self.preset.next();
                self.layout = self.preset.layout();
                self.maximized = None;
                self.sync_panels();
            }
            Command::EnterArrange => self.arranging = true,
            Command::SaveLayout => self.save_layout(),
            Command::Help => self.overlay = Some(Overlay::Help),
            _ => return Flow::Continue,
        }
        Flow::Continue
    }

    /// The focused-panel context. Returns whether the command was consumed
    /// here rather than passed down to the panel itself.
    fn focused(&mut self, command: Command) -> bool {
        match command {
            Command::Open if !self.panel_opens_rows() => {
                self.maximized = Some(self.layout.focus());
                true
            }
            Command::Filter => {
                self.filtering = Some(String::new());
                true
            }
            Command::Export => {
                self.overlay = Some(Overlay::Export(Export {
                    json: false,
                    path: default_export_path(self.subject.report, false),
                }));
                true
            }
            // Selection, sort and first-and-last are the panel's own business:
            // only it knows how many rows it has.
            _ => false,
        }
    }

    /// Whether return on this panel opens a row rather than maximizing.
    ///
    /// The one panel with rows a reader can open is validation, whose groups
    /// fold. Everywhere else return means maximize, which is what the table
    /// row says: "maximize, or open the row".
    fn panel_opens_rows(&self) -> bool {
        self.layout.panel_of(self.layout.focus()) == Some(super::layout::PanelType::Validation)
    }

    fn arrange_key(&mut self, key: KeyEvent) {
        let Some(command) = keymap::lookup(Context::Arrange, key) else {
            return;
        };
        let pane = Self::pane(current_size());
        let command = match command {
            Command::LeaveArrange => {
                self.arranging = false;
                return;
            }
            Command::ArrangeLeft => arrange::Command::Focus(Toward::Left),
            Command::ArrangeDown => arrange::Command::Focus(Toward::Down),
            Command::ArrangeUp => arrange::Command::Focus(Toward::Up),
            Command::ArrangeRight => arrange::Command::Focus(Toward::Right),
            Command::SplitHorizontal => arrange::Command::Split(Direction::Horizontal),
            Command::SplitVertical => arrange::Command::Split(Direction::Vertical),
            Command::GrowDivider => arrange::Command::Grow,
            Command::ShrinkDivider => arrange::Command::Shrink,
            Command::Balance => arrange::Command::Balance,
            Command::Close => {
                // The one destructive thing in a read-only report, and the
                // only thing that gets a confirmation.
                if self.focused_panel_has_unsaved_view() {
                    self.confirm_close();
                    return;
                }
                arrange::Command::Close
            }
            Command::Add => {
                self.overlay = Some(Overlay::Catalogue(Catalogue { selected: 0 }));
                return;
            }
            _ => return,
        };
        match arrange::apply(&self.layout, pane, command) {
            arrange::Outcome::Changed(layout) => {
                self.layout = layout;
                self.sync_panels();
            }
            arrange::Outcome::Refused(refusal) => self.notice = Some(refusal.to_string()),
        }
    }

    fn confirm_close(&mut self) {
        let focus = self.layout.focus();
        let name = self
            .layout
            .panel_of(focus)
            .map_or("this panel", super::layout::PanelType::name);
        let address = self
            .layout
            .solve(Self::pane(current_size()), None)
            .into_iter()
            .find(|p| p.id == focus)
            .map_or(0, |p| p.address);
        self.overlay = Some(Overlay::Confirm(Confirm {
            panel: name.to_owned(),
            address,
        }));
    }

    /// Whether closing the focused panel would discard something.
    fn focused_panel_has_unsaved_view(&self) -> bool {
        self.filtering.is_some()
    }

    fn overlay_key(&mut self, key: KeyEvent) -> Flow {
        let Some(overlay) = self.overlay.as_mut() else {
            return Flow::Continue;
        };
        match overlay {
            Overlay::Help => {
                // Any key closes help, because a reader who opened it has read
                // it and pressing something is how they say so.
                self.overlay = None;
            }
            Overlay::Export(export) => match key.code {
                KeyCode::Right | KeyCode::Left | KeyCode::Tab => {
                    export.json = !export.json;
                    export.path = default_export_path(self.subject.report, export.json);
                }
                KeyCode::Char(c) => export.path.push(c),
                KeyCode::Backspace => {
                    export.path.pop();
                }
                KeyCode::Enter => {
                    let done = self.write_export();
                    self.notice = Some(done);
                    self.overlay = None;
                }
                _ => {}
            },
            Overlay::Confirm(_) => {
                if key.code == KeyCode::Enter {
                    self.overlay = None;
                    self.filtering = None;
                    if let arrange::Outcome::Changed(layout) = arrange::apply(
                        &self.layout,
                        Self::pane(current_size()),
                        arrange::Command::Close,
                    ) {
                        self.layout = layout;
                        self.sync_panels();
                    }
                }
            }
            Overlay::Catalogue(catalogue) => match key.code {
                KeyCode::Up | KeyCode::Char('k' | 'K') => {
                    let count = PanelType::choosable().len();
                    catalogue.selected = (catalogue.selected + count - 1) % count;
                }
                KeyCode::Down | KeyCode::Char('j' | 'J') => {
                    catalogue.selected = (catalogue.selected + 1) % PanelType::choosable().len();
                }
                KeyCode::Enter => {
                    let kind = PanelType::choosable()[catalogue.selected];
                    self.overlay = None;
                    self.layout = self.layout.assign(kind);
                    // The leaf keeps its id across the assignment, so the old
                    // panel object has to go or `sync_panels` would keep
                    // serving the previous type's state under the new name.
                    self.panels.remove(&self.layout.focus());
                    self.sync_panels();
                }
                _ => {}
            },
        }
        Flow::Continue
    }

    fn filter_key(&mut self, key: KeyEvent) {
        let Some(query) = self.filtering.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Char(c) => query.push(c),
            KeyCode::Backspace => {
                query.pop();
            }
            KeyCode::Enter => {
                // Return commits the filter and leaves the buffer, so the rows
                // it selected can be moved through with the ordinary keys.
                let query = query.clone();
                self.filtering = None;
                self.apply_filter(Some(query));
                return;
            }
            _ => return,
        }
        let query = query.clone();
        self.apply_filter(Some(query));
    }

    fn apply_filter(&mut self, query: Option<String>) {
        let focus = self.layout.focus();
        if let Some(panel) = self.panels.get_mut(&focus) {
            panel.set_filter(query);
        }
    }

    fn write_export(&mut self) -> String {
        let Some(Overlay::Export(export)) = &self.overlay else {
            return String::new();
        };
        let rendered = if export.json {
            match solarxy_core::json::report_to_json(self.subject.report) {
                Ok(json) => json,
                Err(error) => return format!("could not build json: {error}"),
            }
        } else {
            self.subject.report.to_string()
        };
        match std::fs::write(&export.path, rendered) {
            Ok(()) => format!("saved to {}", export.path),
            Err(error) => format!("could not save: {error}"),
        }
    }

    fn save_layout(&mut self) {
        let (mut prefs, _) = super::prefs::TuiPrefs::load();
        prefs.saved_layout = Some(self.layout.encode());
        self.notice = Some(match prefs.save() {
            Ok(()) => "layout saved".to_owned(),
            Err(error) => format!("could not save the layout: {error}"),
        });
    }

    /// Move focus to the panel holding a subject and select it there.
    ///
    /// The one action that crosses panels, and the reason it exists: the
    /// question a validation issue raises is always about something in another
    /// panel, and making a reader find it by hand is the failure the shipped
    /// shell has.
    ///
    /// It can fail in two honest ways, and both say so rather than moving
    /// focus somewhere useless. A model-wide issue is about the asset rather
    /// than a row, so there is nowhere to go; and the panel that would hold
    /// the subject may simply not be in the arrangement the reader built.
    fn jump(&mut self, scope: &solarxy_core::validation::IssueScope) {
        use super::panels::validation::{home_of, row_of};

        let Some(kind) = home_of(scope) else {
            self.notice = Some("this issue is about the whole model".to_owned());
            return;
        };
        let Some(target) = self
            .layout
            .leaves()
            .into_iter()
            .find(|(_, leaf)| *leaf == kind)
            .map(|(id, _)| id)
        else {
            self.notice = Some(format!("no {} panel to jump to", kind.name()));
            return;
        };

        if let (Some(row), Some(panel)) = (row_of(scope), self.panels.get_mut(&target))
            && panel.reveal(row)
        {
            self.layout = self.layout.with_focus(target);
            self.maximized = None;
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
            let ink = if self.arranging {
                self.theme.slots.warning
            } else {
                ctx.chrome()
            };

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
                // Typing after slash replaces the menu words with a live
                // query. It belongs to the panel and never covers the rows it
                // is filtering, which an overlay would.
                if let Some(query) = &self.filtering {
                    title.push(Span::styled(
                        " / ".to_owned(),
                        Style::default().fg(self.theme.slots.accent),
                    ));
                    title.push(Span::styled(
                        query.clone(),
                        Style::default().fg(self.theme.slots.ink),
                    ));
                    title.push(Span::styled(
                        self.glyphs.caret.to_owned(),
                        Style::default().fg(self.theme.slots.accent),
                    ));
                    title.push(Span::raw(" "));
                } else {
                    let menu = self
                        .panels
                        .get(&placement.id)
                        .map_or(&[][..], |panel| panel.menu());
                    for word in menu {
                        title.push(Span::styled(
                            format!(
                                " {} {word} ",
                                keymap::panel_key_label(word, self.glyphs.tier)
                            ),
                            Style::default()
                                .fg(self.theme.slots.accent)
                                .add_modifier(Modifier::BOLD),
                        ));
                    }
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
                } else if let Some(counts) = self
                    .filtering
                    .as_ref()
                    .and_then(|_| self.panels.get(&placement.id))
                    .and_then(|panel| panel.filter_counts(&self.context(true)))
                {
                    block = block.title_bottom(
                        Line::from(Span::styled(
                            format!(" {} of {} shown  esc clears ", counts.0, counts.1),
                            Style::default().fg(self.theme.slots.ink_dim),
                        ))
                        .right_aligned(),
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
                    subject: &self.subject,
                    theme: &self.theme.slots,
                    glyphs: &self.glyphs,
                    caps: self.caps,
                    focused: placement.focused,
                };
                panel.draw(frame, inner, &ctx);
            }
        }

        let menu = self
            .panels
            .get(&self.layout.focus())
            .map_or(&[][..], |panel| panel.menu());
        draw_footer(
            frame,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            self.subject.report,
            &self.theme,
            &self.glyphs,
            menu,
            self.arranging,
        );

        if let Some(overlay) = &self.overlay {
            super::overlay::draw(
                frame,
                area,
                overlay,
                &super::overlay::Chrome {
                    theme: &self.theme.slots,
                    glyphs: &self.glyphs,
                    caps: self.caps,
                    panel_menu: menu,
                    catalogue: &catalogue_names(),
                },
            );
        }
    }

    fn handle(&mut self, input: Input) -> Flow {
        let Input::Key(key) = input else {
            return Flow::Continue;
        };
        self.notice = None;

        // Escape is answered before anything else, because it means back and
        // back is relative to what is open. The chain is the design's:
        // an overlay, then a filter, then arrange mode, then maximize. It
        // never quits; `q` alone does.
        if key.code == KeyCode::Esc {
            self.escape();
            return Flow::Continue;
        }

        // A text buffer takes the whole keyboard while it is open. Without
        // that, typing a path containing `q` would quit underneath the prompt.
        if self.overlay.is_some() {
            return self.overlay_key(key);
        }
        if self.filtering.is_some() {
            self.filter_key(key);
            return Flow::Continue;
        }
        if self.arranging {
            self.arrange_key(key);
            return Flow::Continue;
        }

        if let Some(command) = keymap::lookup(Context::Global, key) {
            return self.global(command);
        }
        if let Some(command) = keymap::lookup(Context::Focused, key)
            && self.focused(command)
        {
            return Flow::Continue;
        }

        // Anything the table did not claim is the panel's own, which is how
        // its border words reach it.
        let focus = self.layout.focus();
        let ctx = Ctx {
            subject: &self.subject,
            theme: &self.theme.slots,
            glyphs: &self.glyphs,
            caps: self.caps,
            focused: true,
        };
        let action = self
            .panels
            .get_mut(&focus)
            .map_or(Action::None, |panel| panel.handle(key, &ctx));
        if let Action::Jump(scope) = action {
            self.jump(&scope);
        }
        Flow::Continue
    }
}

/// The pick list the catalogue overlay shows.
///
/// Assembled here rather than read off the enum inside the overlay, so that
/// module stays free of any one surface's vocabulary. The coupling does not
/// disappear, it moves: this is now the only place that says the catalogue
/// offers exactly what a reader can choose, which is what the test below
/// asserts.
fn catalogue_names() -> Vec<&'static str> {
    PanelType::choosable().iter().map(|k| k.name()).collect()
}

/// Where an export lands unless the reader says otherwise.
fn default_export_path(report: &AnalysisReport, json: bool) -> String {
    let stem = report
        .model_name
        .rsplit_once('.')
        .map_or(report.model_name.as_str(), |(stem, _)| stem);
    if json {
        format!("{stem}.json")
    } else {
        format!("{stem}_report.txt")
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

/// The strip along the bottom, generated from the table.
///
/// Nothing here is written by hand, which is what stops it drifting from the
/// help overlay and from what the keys actually do. Its right half changes
/// with focus, because a border word without its key is a control a reader
/// cannot use.
fn draw_footer(
    frame: &mut Frame,
    area: Rect,
    report: &AnalysisReport,
    theme: &Theme,
    glyphs: &Glyphs,
    panel_menu: &[&'static str],
    arranging: bool,
) {
    let accent = |text: String| {
        Span::styled(
            text,
            Style::default()
                .fg(theme.slots.accent)
                .add_modifier(Modifier::BOLD),
        )
    };
    let plain = |text: String| Span::styled(text, Style::default().fg(theme.slots.ink));

    let mut spans = Vec::new();
    if arranging {
        // The mode says so rather than leaving a reader to infer it from the
        // borders alone.
        spans.push(accent("ARRANGE  ".to_owned()));
        for binding in keymap::rows(Context::Arrange) {
            spans.push(accent(keymap::label(binding, glyphs.tier)));
            spans.push(plain(format!(" {}  ", short(binding.describes))));
        }
    } else {
        for (label, describes) in keymap::footer(panel_menu, glyphs.tier) {
            spans.push(accent(label));
            spans.push(plain(format!(" {}  ", short(&describes))));
        }
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

/// The first word or two of a description, because the footer has one row and
/// the help overlay has the whole sentence.
fn short(describes: &str) -> String {
    describes
        .split([',', ' '])
        .find(|word| !word.is_empty())
        .unwrap_or(describes)
        .to_owned()
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
        let model = ModelView::default();
        let mut app = App::new(&report, &model, Preset::Survey.layout(), theme, caps);
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
    #[ignore = "manual preview, not an assertion"]
    fn preview() {
        let buffer = render(CAPS, 140, 45);
        println!("{}", screen(&buffer, 140, 45));
    }

    /// Not an assertion: the help overlay over a dimmed grid.
    #[test]
    #[ignore = "manual preview, not an assertion"]
    fn preview_help() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);
        app.overlay = Some(Overlay::Help);
        let mut terminal = Terminal::new(TestBackend::new(140, 45)).expect("terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        println!("{}", screen(terminal.backend().buffer(), 140, 45));
    }

    /// Not an assertion: the whole surface over a real model, which is the
    /// only way to judge the plots.
    #[test]
    #[ignore = "manual preview over a model on disk, not an assertion"]
    fn preview_a_real_model() {
        const DEFAULT: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../res/models/xyzrgb_dragon.obj"
        );
        let path = std::env::var("SOLARXY_PREVIEW_MODEL").unwrap_or_else(|_| DEFAULT.to_owned());
        let analyzer =
            crate::calc::analyze::ModelAnalyzer::new_with_config(&path, None).expect("loads");
        let report = analyzer.generate_report();
        // The same view the binary builds. A struct literal rather than a
        // shared helper because the analyzer sits behind a feature this
        // module does not have, and both sites break at compile time if a
        // field is ever added.
        let model = ModelView {
            meshes: analyzer
                .meshes
                .iter()
                .map(|mesh| crate::tui::geometry::MeshView {
                    positions: &mesh.positions,
                    texcoords: &mesh.texcoords,
                    indices: &mesh.indices,
                })
                .collect(),
        };

        let slots = ThemeSet::bundled().slots_for(DEFAULT_THEME).expect("loads");
        let theme = Theme::resolve(CAPS, DEFAULT_THEME, &slots);
        let mut app = App::new(&report, &model, Preset::Survey.layout(), theme, CAPS);
        let mut terminal = Terminal::new(TestBackend::new(140, 45)).expect("terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        println!("{}", screen(terminal.backend().buffer(), 140, 45));
    }

    fn key(code: KeyCode) -> Input {
        Input::Key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        ))
    }

    fn app_over<'a>(report: &'a AnalysisReport, model: &'a ModelView<'a>) -> App<'a> {
        let slots = ThemeSet::bundled().slots_for(DEFAULT_THEME).expect("loads");
        let theme = Theme::resolve(CAPS, DEFAULT_THEME, &slots);
        App::new(report, model, Preset::Survey.layout(), theme, CAPS)
    }

    /// The keymap's own words for arrange's `a` are "add a panel from the
    /// catalogue", so a split's empty leaf must be fillable: split, move to
    /// it, pick, and the leaf holds the panel.
    #[test]
    fn arrange_a_fills_the_new_leaf_from_the_catalogue() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);
        app.arranging = true;
        app.layout = Layout::single(PanelType::Meshes);
        app.sync_panels();

        app.handle(key(KeyCode::Char('v')));
        let leaf = app
            .layout
            .leaves()
            .into_iter()
            .find(|(_, kind)| *kind == PanelType::Catalogue)
            .map(|(id, _)| id)
            .expect("the split left a catalogue leaf");
        app.handle(key(KeyCode::Char('l')));
        assert_eq!(
            app.layout.focus(),
            leaf,
            "focus did not reach the catalogue leaf"
        );

        app.handle(key(KeyCode::Char('a')));
        assert!(
            matches!(app.overlay, Some(Overlay::Catalogue(_))),
            "{:?}",
            app.overlay
        );
        app.handle(key(KeyCode::Char('j')));
        app.handle(key(KeyCode::Enter));

        assert!(app.overlay.is_none(), "the pick did not close the overlay");
        assert!(app.arranging, "picking a panel ended arrange mode");
        assert_eq!(
            app.layout.panel_of(leaf),
            Some(PanelType::CHOOSABLE[1]),
            "the pick did not reach the leaf"
        );
        assert!(
            app.panels.contains_key(&leaf),
            "the leaf has no panel object after the pick"
        );
    }

    /// Every choosable panel reaches the pick, on screen, through the real
    /// wiring.
    ///
    /// The overlay renders whatever list it is handed and can no longer check
    /// that the list is the whole catalogue, so the check moves here and is
    /// made against painted cells rather than against the list: a narrowed
    /// list, a wrong list, or a call site that stops passing one all fail the
    /// same way. Without it a panel added to the enum would simply never
    /// appear in the pick and nothing would say so.
    #[test]
    fn every_choosable_panel_reaches_the_pick() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);
        app.overlay = Some(Overlay::Catalogue(Catalogue { selected: 0 }));
        let mut terminal = Terminal::new(TestBackend::new(140, 45)).expect("terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        let text = screen(terminal.backend().buffer(), 140, 45);
        for kind in PanelType::choosable() {
            assert!(
                text.contains(&format!("( ) {}", kind.name()))
                    || text.contains(&format!("(\u{2022}) {}", kind.name())),
                "{} is not offered by the pick",
                kind.name()
            );
        }
    }

    /// The pick draws through the whole surface path, titled, over the
    /// dimmed grid, so a sizing mistake in its body fails here rather than
    /// on a reader's terminal.
    #[test]
    fn the_catalogue_overlay_draws_over_the_grid() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);
        app.overlay = Some(Overlay::Catalogue(Catalogue { selected: 0 }));
        let mut terminal = Terminal::new(TestBackend::new(140, 45)).expect("terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        let text = screen(terminal.backend().buffer(), 140, 45);
        assert!(text.contains("Add panel"), "the overlay title is missing");
        assert!(text.contains("silhouette"), "the list is missing");
    }

    /// Escape keeps meaning back: it closes the pick without assigning, so a
    /// reader who opened it by accident loses nothing.
    #[test]
    fn escape_closes_the_catalogue_without_assigning() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);
        app.arranging = true;
        app.layout = Layout::single(PanelType::Meshes);
        app.sync_panels();

        app.handle(key(KeyCode::Char('v')));
        app.handle(key(KeyCode::Char('l')));
        let leaf = app.layout.focus();

        app.handle(key(KeyCode::Char('a')));
        app.handle(key(KeyCode::Esc));
        assert!(app.overlay.is_none());
        assert_eq!(
            app.layout.panel_of(leaf),
            Some(PanelType::Catalogue),
            "escape assigned a panel"
        );
    }

    /// Escape means back, one layer at a time, in the design's order. It is
    /// the criterion most easily broken by a later change, because every new
    /// mode is a temptation to add another arm somewhere else.
    #[test]
    fn escape_unwinds_one_layer_at_a_time_and_never_quits() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);

        // Build every layer up: maximized, then arranging, then a filter,
        // then an overlay over all of it.
        app.handle(key(KeyCode::Enter));
        assert!(app.maximized.is_some());
        app.arranging = true;
        app.filtering = Some("gr".to_owned());
        app.overlay = Some(Overlay::Help);

        app.handle(key(KeyCode::Esc));
        assert!(app.overlay.is_none(), "the overlay did not close first");
        assert!(app.filtering.is_some(), "the filter closed too early");

        app.handle(key(KeyCode::Esc));
        assert!(app.filtering.is_none(), "the filter did not clear");
        assert!(app.arranging, "arrange mode ended too early");

        app.handle(key(KeyCode::Esc));
        assert!(!app.arranging, "arrange mode did not end");
        assert!(app.maximized.is_some(), "maximize restored too early");

        app.handle(key(KeyCode::Esc));
        assert!(app.maximized.is_none(), "maximize did not restore");

        // And at the bottom of the chain it still does not quit.
        assert_eq!(app.handle(key(KeyCode::Esc)), Flow::Continue);
        assert!(!app.exit, "escape quit the application");
    }

    /// The tabbed shell quit on this key, so at the bottom of the chain the
    /// reader is told which key does now. Only at the bottom: a press that
    /// actually went back has already shown what it did.
    #[test]
    fn escape_with_nothing_left_names_the_key_that_quits() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);

        app.handle(key(KeyCode::Esc));
        assert!(
            app.notice.as_deref().is_some_and(|n| n.contains("press q")),
            "{:?}",
            app.notice
        );

        // A press with a layer to unwind restores it and says nothing.
        app.handle(key(KeyCode::Enter));
        assert!(app.maximized.is_some());
        app.handle(key(KeyCode::Esc));
        assert!(app.maximized.is_none(), "maximize did not restore");
        assert!(app.notice.is_none(), "{:?}", app.notice);

        // The next one has nothing left, so the hint is back.
        app.handle(key(KeyCode::Esc));
        assert!(
            app.notice.as_deref().is_some_and(|n| n.contains("press q")),
            "{:?}",
            app.notice
        );
    }

    /// A text buffer takes the whole keyboard, or typing a path containing
    /// `q` would quit underneath the prompt.
    #[test]
    fn an_open_overlay_swallows_the_global_keys() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);

        app.overlay = Some(Overlay::Export(Export {
            json: false,
            path: String::new(),
        }));
        assert_eq!(app.handle(key(KeyCode::Char('q'))), Flow::Continue);
        assert!(!app.exit, "a typed q quit the application");
        let Some(Overlay::Export(export)) = &app.overlay else {
            panic!("the overlay closed");
        };
        assert_eq!(export.path, "q");

        // The same for the in-border filter.
        let mut app = app_over(&report, &model);
        app.filtering = Some(String::new());
        app.handle(key(KeyCode::Char('q')));
        assert!(!app.exit);
        assert_eq!(app.filtering.as_deref(), Some("q"));
    }

    /// Help opens from the table's own key and closes on anything, because a
    /// reader who pressed something has read it.
    #[test]
    fn help_opens_and_closes() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);

        app.handle(key(KeyCode::Char('?')));
        assert_eq!(app.overlay, Some(Overlay::Help));
        app.handle(key(KeyCode::Char('x')));
        assert!(app.overlay.is_none());
    }

    /// Arrange takes its prefix and every border tints, so the mode is never
    /// ambiguous.
    #[test]
    fn arrange_mode_is_entered_by_its_prefix_and_left_by_escape() {
        let report = report();
        let model = ModelView::default();
        let mut app = app_over(&report, &model);

        app.handle(Input::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('w'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert!(app.arranging);

        // A plain w is not the prefix, so it cannot arrange by accident.
        app.arranging = false;
        app.handle(key(KeyCode::Char('w')));
        assert!(!app.arranging);
    }

    /// Jump is the only action that crosses panels, so it is the only one
    /// whose effect the app rather than the panel has to be tested for.
    #[test]
    fn a_jump_moves_focus_to_the_panel_holding_the_subject() {
        use solarxy_core::validation::IssueScope;

        let report = report();
        let model = ModelView::default();
        let slots = ThemeSet::bundled().slots_for(DEFAULT_THEME).expect("loads");
        let theme = Theme::resolve(CAPS, DEFAULT_THEME, &slots);
        let mut app = App::new(&report, &model, Preset::Survey.layout(), theme, CAPS);

        app.jump(&IssueScope::Mesh(1));
        assert_eq!(
            app.layout.panel_of(app.layout.focus()),
            Some(PanelType::Meshes),
            "a mesh issue did not reach the mesh table"
        );
        assert!(app.notice.is_none(), "{:?}", app.notice);

        app.jump(&IssueScope::Material(0));
        assert_eq!(
            app.layout.panel_of(app.layout.focus()),
            Some(PanelType::Materials)
        );
    }

    /// Two honest failures, and both say so rather than moving focus
    /// somewhere that will not show the reader what they asked for.
    #[test]
    fn a_jump_with_nowhere_to_go_says_so() {
        use solarxy_core::validation::IssueScope;

        let report = report();
        let model = ModelView::default();
        let slots = ThemeSet::bundled().slots_for(DEFAULT_THEME).expect("loads");
        let theme = Theme::resolve(CAPS, DEFAULT_THEME, &slots);

        let mut app = App::new(
            &report,
            &model,
            Preset::Survey.layout(),
            theme.clone(),
            CAPS,
        );
        app.jump(&IssueScope::Model);
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("whole model")),
            "{:?}",
            app.notice
        );

        // The validation preset holds no mesh table, so a mesh issue has no
        // panel to land in and the reader is told rather than moved.
        let mut app = App::new(&report, &model, Preset::Validation.layout(), theme, CAPS);
        let before = app.layout.focus();
        app.jump(&IssueScope::Mesh(0));
        assert_eq!(app.layout.focus(), before, "focus moved with no target");
        assert!(
            app.notice.as_deref().is_some_and(|n| n.contains("meshes")),
            "{:?}",
            app.notice
        );
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
        let model = ModelView::default();
        let mut app = App::new(&report, &model, layout.with_focus(meshes), theme, CAPS);

        let mut terminal = Terminal::new(TestBackend::new(140, 45)).expect("terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw");
        let text = screen(terminal.backend().buffer(), 140, 45);
        assert!(text.contains("sort"), "the focused panel hid its menu");
    }
}
