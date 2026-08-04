//! What a panel is, and the four that replace the shipped tabs.
//!
//! # A panel decides what to say, not how it looks
//!
//! Drawing a meter, a bar or a frame belongs to the shared primitives, and
//! resolving a theme belongs to the tier model. What is left for a panel is
//! the only part that is actually about this tool: which facts about a model
//! are worth a row, and in what order.
//!
//! # State belongs to the leaf, not to the type
//!
//! Selection, sort and filter are held per leaf and keyed on its id, so two
//! mesh panels are two panels with their own selected row. Keying on the panel
//! type instead would make them a single panel drawn twice, which is a
//! different and much worse thing.
//!
//! # One action reaches across panels, deliberately
//!
//! Everything a panel does affects only itself, except jump: the question a
//! validation issue raises is always about something in another panel, and
//! making a reader find it by hand is the failure the shipped shell already
//! has. So jump is a value a panel returns rather than a reach into a
//! neighbour, and the app is what knows where the subject lives.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use solarxy_core::report::AnalysisReport;
use solarxy_core::validation::IssueScope;

use super::caps::{Capabilities, Glyphs};
use super::geometry::ModelView;
use super::layout::PanelType;
use super::theme::Slots;

pub mod distributions;
pub mod geometry;
pub mod health;
pub mod materials;
pub mod meshes;
pub mod silhouette;
pub mod uv;

/// Everything a panel is allowed to read.
///
/// Handed in rather than held, so a panel owns only its own view state and
/// cannot accumulate a private copy of the model that drifts from the report.
pub struct Ctx<'a> {
    pub report: &'a AnalysisReport,
    /// The raw arrays the plots project, borrowed from the analyzer rather
    /// than copied into the report.
    pub model: &'a ModelView<'a>,
    pub theme: &'a Slots,
    pub glyphs: &'a Glyphs,
    pub caps: Capabilities,
    pub focused: bool,
}

impl Ctx<'_> {
    /// The ink a panel's own chrome takes: the accent when it holds focus,
    /// dim otherwise. Focus is carried by border weight first; this is the
    /// second signal, not the only one.
    pub fn chrome(&self) -> ratatui::style::Color {
        if self.focused {
            self.theme.accent
        } else {
            self.theme.ink_dim
        }
    }
}

/// What a panel wants the app to do after a key.
#[derive(Debug, Clone, Default)]
pub enum Action {
    #[default]
    None,
    /// Move focus to whichever panel holds this subject and select it.
    Jump(IssueScope),
}

/// Anything that can occupy a leaf.
pub trait Panel {
    /// The words this panel puts in its own top border.
    fn menu(&self) -> &'static [&'static str] {
        &[]
    }

    /// Draw into the area inside the border.
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_>);

    /// Handle a key while focused.
    fn handle(&mut self, _key: KeyEvent, _ctx: &Ctx<'_>) -> Action {
        Action::None
    }

    /// The right-hand end of the panel's bottom border, when it has something
    /// to count.
    fn status(&self, _ctx: &Ctx<'_>) -> Option<String> {
        None
    }
}

/// Build the panel for a type.
///
/// The types with no implementation yet render as a named placeholder rather
/// than as nothing, so an arrangement holding one is still legible and the
/// reader can see what is coming.
pub fn make(kind: PanelType) -> Box<dyn Panel> {
    match kind {
        PanelType::Geometry => Box::new(geometry::Geometry),
        PanelType::Health => Box::new(health::Health::default()),
        PanelType::Meshes => Box::new(meshes::Meshes::default()),
        PanelType::Materials => Box::new(materials::Materials::default()),
        PanelType::Silhouette => Box::new(silhouette::Silhouette::default()),
        PanelType::Uv => Box::new(uv::Uv::default()),
        PanelType::Distributions => Box::new(distributions::Distributions::default()),
        other => Box::new(Pending(other)),
    }
}

/// A panel type that exists in the catalogue but has no body yet.
pub struct Pending(pub PanelType);

impl Panel for Pending {
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        let (line, rect) = super::widgets::empty_state(
            &format!("{} arrives in a later commit", self.0.name()),
            area,
            ctx.theme,
        );
        frame.render_widget(ratatui::widgets::Paragraph::new(line), rect);
    }
}

/// A sortable column set, shared by every table panel.
///
/// Cycling rather than choosing: one key steps through the columns and a
/// second press on the same one reverses it, which is the whole sort
/// interaction and needs no menu of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sort {
    pub column: usize,
    pub descending: bool,
}

impl Sort {
    /// Step to the next column, or reverse the current one.
    ///
    /// The first press on a new column sorts descending, because every numeric
    /// column here is one where the interesting end is the large one: the
    /// biggest mesh, the commonest issue, the heaviest material.
    #[must_use]
    pub fn cycle(self, columns: usize) -> Self {
        if self.descending {
            Self {
                column: (self.column + 1) % columns,
                descending: false,
            }
        } else {
            Self {
                column: self.column,
                descending: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two presses per column: descending, then ascending, then on to the
    /// next. Three columns is therefore a six-press cycle.
    #[test]
    fn sorting_cycles_through_every_column_in_both_directions() {
        let mut sort = Sort::default();
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push((sort.column, sort.descending));
            sort = sort.cycle(3);
        }
        assert_eq!(
            seen,
            vec![
                (0, false),
                (0, true),
                (1, false),
                (1, true),
                (2, false),
                (2, true)
            ]
        );
        assert_eq!(sort, Sort::default(), "the cycle did not close");
    }

    /// The first press on a column shows the large end, which is the end
    /// anyone opening a mesh table is looking for.
    #[test]
    fn a_new_column_sorts_descending_first() {
        let sort = Sort::default().cycle(4);
        assert!(sort.descending);
        assert_eq!(sort.column, 0);
    }
}
