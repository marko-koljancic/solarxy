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

pub mod bounds;
pub mod distributions;
pub mod geometry;
pub mod health;
pub mod materials;
pub mod meshes;
pub mod silhouette;
pub mod textures;
pub mod uv;
pub mod validation;

/// Everything a panel is allowed to read.
///
/// Handed in rather than held, so a panel owns only its own view state and
/// cannot accumulate a private copy of the model that drifts from the report.
///
/// `C` is what the surface is *about*: the analysis for the analyze surface, a
/// render's progress for the dashboard. Everything else here is chrome and is
/// the same whatever the subject, which is the whole reason the split is worth
/// a type parameter.
pub struct Ctx<'a, C: ?Sized> {
    /// What this surface is about.
    pub subject: &'a C,
    pub theme: &'a Slots,
    pub glyphs: &'a Glyphs,
    pub caps: Capabilities,
    pub focused: bool,
}

/// What the analyze surface's panels read.
pub struct Analysis<'a> {
    pub report: &'a AnalysisReport,
    /// The raw arrays the plots project, borrowed from the analyzer rather
    /// than copied into the report.
    pub model: &'a ModelView<'a>,
}

/// The context an analyze panel is handed.
///
/// One lifetime rather than two, so a panel returning rows borrowed from the
/// report says so in the ordinary way: `fn rows<'a>(&self, ctx: &AnalyzeCtx<'a>)
/// -> Vec<&'a Row>`. The surface holds its subject for longer than any one
/// frame's borrow, and the shorter of the two is the one that reaches here.
pub type AnalyzeCtx<'a> = Ctx<'a, Analysis<'a>>;

impl<C: ?Sized> Ctx<'_, C> {
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
///
/// Two parameters rather than one and an associated type. An associated type
/// would have to be named at every boxed panel, which is where the surfaces
/// keep theirs, and naming it there is exactly what a second parameter does
/// with less ceremony. `A` is what a key press asks the surface to do, and it
/// is the surface's own vocabulary: the analyze surface can jump to a subject,
/// and a surface with nothing to ask uses the unit type.
pub trait Panel<C: ?Sized, A: Default> {
    /// The words this panel puts in its own top border.
    fn menu(&self) -> &'static [&'static str] {
        &[]
    }

    /// Draw into the area inside the border.
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_, C>);

    /// Handle a key while focused.
    fn handle(&mut self, _key: KeyEvent, _ctx: &Ctx<'_, C>) -> A {
        A::default()
    }

    /// The right-hand end of the panel's bottom border, when it has something
    /// to count.
    fn status(&self, _ctx: &Ctx<'_, C>) -> Option<String> {
        None
    }

    /// Select a row on behalf of a jump from somewhere else.
    ///
    /// Returns whether it could. A panel that holds no rows says no, which is
    /// how the app knows to tell the reader rather than move focus to
    /// something that will not show them what they asked for.
    fn reveal(&mut self, _row: usize) -> bool {
        false
    }

    /// Take a filter query typed into this panel's own border.
    ///
    /// `None` clears it. A panel with nothing to filter ignores this, and its
    /// border shows no filter word, so a reader is never offered a control
    /// that does nothing.
    fn set_filter(&mut self, _query: Option<String>) {}

    /// How many rows the filter admits, out of how many there are.
    ///
    /// Shown live in the border while a query is being typed, which is what
    /// makes filtering feel like narrowing rather than guessing.
    fn filter_counts(&self, _ctx: &Ctx<'_, C>) -> Option<(usize, usize)> {
        None
    }
}

/// One analyze panel, boxed for a leaf to hold.
pub type BoxedAnalyzePanel<'a> = Box<dyn Panel<Analysis<'a>, Action> + 'a>;

/// Build the panel for a type.
pub fn make<'a>(kind: PanelType) -> BoxedAnalyzePanel<'a> {
    match kind {
        PanelType::Geometry => Box::new(geometry::Geometry),
        PanelType::Health => Box::new(health::Health::default()),
        PanelType::Meshes => Box::new(meshes::Meshes::default()),
        PanelType::Materials => Box::new(materials::Materials::default()),
        PanelType::Silhouette => Box::new(silhouette::Silhouette::default()),
        PanelType::Uv => Box::new(uv::Uv::default()),
        PanelType::Distributions => Box::new(distributions::Distributions::default()),
        PanelType::Validation => Box::new(validation::Validation::default()),
        PanelType::Textures => Box::new(textures::Textures::default()),
        PanelType::Bounds => Box::new(bounds::Bounds),
        PanelType::Catalogue => Box::new(Catalogue),
    }
}

/// A leaf that exists but has not been given a panel yet.
///
/// A split creates one of these rather than guessing what the reader wanted,
/// so it says what it is waiting for instead of sitting blank.
pub struct Catalogue;

/// The empty leaf says the same thing to any surface, so it is written for
/// any subject and any action rather than for the analyze pair.
impl<C: ?Sized, A: Default> Panel<C, A> for Catalogue {
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_, C>) {
        let (line, rect) = super::widgets::empty_state("pick a panel", area, ctx.theme);
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
