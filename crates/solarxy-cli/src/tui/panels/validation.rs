//! Issues grouped by what they are, with a way to reach what they are about.
//!
//! # Grouping is by kind, not severity
//!
//! Severity is a filter and kind is the structure. Five errors of one kind are
//! one problem; five of five kinds are five, and a flat list cannot tell those
//! apart. The shipped shell shows the flat list.
//!
//! A second grouping answers the other question a reader has. By kind asks
//! what is wrong with this asset; by subject asks what is wrong with this
//! mesh, and collects everything said about one object under its name.
//!
//! # Jump is the only action that reaches across panels
//!
//! Everything else a panel does affects only itself. Jump deliberately breaks
//! that, because the question a validation issue raises is always about
//! something in another panel, and making a reader find it by hand is a
//! failure the shipped shell already has. It travels on the structured scope
//! the analyzer has always had and always flattened into a string.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListState, Paragraph};
use solarxy_core::validation::{IssueScope, Severity, ValidationIssue};

use super::super::widgets;
use super::{Action, Ctx, Panel};

/// What the rows are collected under.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Group {
    #[default]
    Kind,
    Subject,
}

impl Group {
    fn next(self) -> Self {
        match self {
            Self::Kind => Self::Subject,
            Self::Subject => Self::Kind,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Kind => "by kind",
            Self::Subject => "by subject",
        }
    }
}

/// Which severities are shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    ErrorsOnly,
    WarningsOnly,
}

impl Filter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::ErrorsOnly,
            Self::ErrorsOnly => Self::WarningsOnly,
            Self::WarningsOnly => Self::All,
        }
    }

    fn admits(self, severity: Severity) -> bool {
        match self {
            Self::All => true,
            Self::ErrorsOnly => severity == Severity::Error,
            Self::WarningsOnly => severity == Severity::Warning,
        }
    }
}

/// One drawn row: a group header, or an issue under it.
enum Row<'a> {
    Header {
        label: String,
        severity: Severity,
        count: usize,
        folded: bool,
    },
    Issue(&'a ValidationIssue),
}

#[derive(Default)]
pub struct Validation {
    pub group: Group,
    pub filter: Filter,
    /// Groups the reader has folded away, by label.
    ///
    /// Keyed on the label rather than an index so a collapse survives the
    /// filter changing under it, which would otherwise fold a different group
    /// than the one that was chosen.
    collapsed: Vec<String>,
    state: ListState,
}

impl Validation {
    fn rows<'a>(&self, ctx: &Ctx<'a>) -> Vec<Row<'a>> {
        let issues: Vec<&ValidationIssue> = ctx
            .report
            .validation
            .issues
            .iter()
            .filter(|issue| self.filter.admits(issue.severity))
            .collect();

        // Groups are built in the order their first issue appears, so the
        // ordering is a property of the report rather than of a hash.
        let mut groups: Vec<(String, Severity, Vec<&ValidationIssue>)> = Vec::new();
        for issue in issues {
            let label = match self.group {
                Group::Kind => issue.kind.to_string(),
                Group::Subject => subject(issue, ctx),
            };
            match groups.iter_mut().find(|(name, _, _)| *name == label) {
                Some((_, severity, members)) => {
                    // A group takes the worst severity in it, so a mesh with
                    // one error among warnings does not read as a warning.
                    if issue.severity == Severity::Error {
                        *severity = Severity::Error;
                    }
                    members.push(issue);
                }
                None => groups.push((label, issue.severity, vec![issue])),
            }
        }
        groups.sort_by_key(|(_, _, members)| std::cmp::Reverse(members.len()));

        let mut rows = Vec::new();
        for (label, severity, members) in groups {
            let folded = self.collapsed.contains(&label);
            rows.push(Row::Header {
                label,
                severity,
                count: members.len(),
                folded,
            });
            if !folded {
                rows.extend(members.into_iter().map(Row::Issue));
            }
        }
        rows
    }
}

/// What an issue is about, named the way a reader would say it.
fn subject(issue: &ValidationIssue, ctx: &Ctx<'_>) -> String {
    let mesh = |index: usize| {
        ctx.report
            .meshes
            .iter()
            .find(|m| m.index == index)
            .map_or_else(
                || format!("mesh {index}"),
                |m| {
                    if m.name.is_empty() {
                        format!("mesh {index}")
                    } else {
                        format!("mesh {index} '{}'", m.name)
                    }
                },
            )
    };
    match issue.scope {
        IssueScope::Mesh(index) | IssueScope::Face(index, _) => mesh(index),
        IssueScope::Edge { mesh_index, .. } => mesh(mesh_index),
        IssueScope::Material(index) => ctx
            .report
            .materials
            .iter()
            .find(|m| m.index == index)
            .map_or_else(
                || format!("material {index}"),
                |m| format!("material {index} '{}'", m.name),
            ),
        IssueScope::Model => "the model".to_owned(),
    }
}

impl Panel for Validation {
    fn menu(&self) -> &'static [&'static str] {
        &["group", "severity", "jump"]
    }

    fn handle(&mut self, key: KeyEvent, ctx: &Ctx<'_>) -> Action {
        let rows = self.rows(ctx);
        match key.code {
            KeyCode::Char('g') if key.modifiers.is_empty() => {
                self.state.select((!rows.is_empty()).then_some(0));
            }
            KeyCode::Char('G') => self.state.select(rows.len().checked_sub(1)),
            KeyCode::Char('o') | KeyCode::Char('O') => self.group = self.group.next(),
            KeyCode::Char('/') => self.filter = self.filter.next(),
            KeyCode::Down | KeyCode::Char('j') => step(&mut self.state, 1, rows.len()),
            KeyCode::Up | KeyCode::Char('k') => step(&mut self.state, -1, rows.len()),
            KeyCode::Enter => {
                // Return means the same thing in both places: open what is
                // under the cursor. On an issue that is the object it is
                // about; on a header it is the group itself, which matters
                // because one kind can carry a thousand issues and bury every
                // other kind under them.
                match self.state.selected().and_then(|i| rows.get(i)) {
                    Some(Row::Issue(issue)) => return Action::Jump(issue.scope.clone()),
                    Some(Row::Header { label, .. }) => {
                        let label = label.clone();
                        match self.collapsed.iter().position(|name| *name == label) {
                            Some(index) => {
                                self.collapsed.remove(index);
                            }
                            None => self.collapsed.push(label),
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
        Action::None
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &Ctx<'_>) {
        let rows = self.rows(ctx);
        if rows.is_empty() {
            let what = if ctx.report.validation.is_clean() {
                "no issues found"
            } else {
                "no issue matches this severity"
            };
            let (line, rect) = widgets::empty_state(what, area, ctx.theme);
            frame.render_widget(Paragraph::new(line), rect);
            return;
        }
        if self.state.selected().is_none_or(|i| i >= rows.len()) {
            self.state.select(Some(0));
        }

        let lines: Vec<Line> = rows
            .iter()
            .map(|row| match row {
                Row::Header {
                    label,
                    severity,
                    count,
                    folded,
                } => {
                    let (mark, ink) = match severity {
                        Severity::Error => (ctx.glyphs.cross, ctx.theme.error),
                        Severity::Warning => (ctx.glyphs.warn, ctx.theme.warning),
                    };
                    Line::from(vec![
                        Span::styled(
                            format!("{mark} {}", severity_word(*severity)),
                            Style::default().fg(ink).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            label.clone(),
                            Style::default()
                                .fg(ctx.theme.ink)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(
                                "  {count} {}{}",
                                if *count == 1 { "issue" } else { "issues" },
                                if *folded { ", folded" } else { "" }
                            ),
                            Style::default().fg(ctx.theme.ink_dim),
                        ),
                    ])
                }
                Row::Issue(issue) => Line::from(vec![
                    Span::raw("  "),
                    Span::styled(subject(issue, ctx), Style::default().fg(ctx.theme.ink_dim)),
                    Span::raw("  "),
                    Span::styled(issue.message.clone(), Style::default().fg(ctx.theme.ink)),
                ]),
            })
            .collect();

        let list = List::new(lines).highlight_style(
            Style::default()
                .fg(ctx.theme.ink)
                .bg(if ctx.caps.color.paints_a_ground() {
                    ctx.theme.selection
                } else {
                    ratatui::style::Color::Reset
                })
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(list, area, &mut self.state);
    }

    fn status(&self, ctx: &Ctx<'_>) -> Option<String> {
        let validation = &ctx.report.validation;
        Some(format!(
            "{} errors, {} warnings across {} kinds \u{b7} {}",
            validation.error_count(),
            validation.warning_count(),
            validation.ranked_kinds().len(),
            self.group.label()
        ))
    }
}

fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERROR",
        Severity::Warning => "WARN",
    }
}

fn step(state: &mut ListState, delta: i32, count: usize) {
    if count == 0 {
        state.select(None);
        return;
    }
    let current = state.selected().unwrap_or(0) as i32;
    state.select(Some((current + delta).clamp(0, count as i32 - 1) as usize));
}

/// Which panel type holds the thing a scope names.
///
/// Lives here rather than in the app because it is a statement about what
/// validation scopes mean, not about how panels are arranged.
pub fn home_of(scope: &IssueScope) -> Option<super::super::layout::PanelType> {
    use super::super::layout::PanelType;
    match scope {
        IssueScope::Mesh(_) | IssueScope::Face(..) | IssueScope::Edge { .. } => {
            Some(PanelType::Meshes)
        }
        IssueScope::Material(_) => Some(PanelType::Materials),
        // A model-wide issue is about the asset rather than about a row in
        // some table, so there is nowhere to jump to.
        IssueScope::Model => None,
    }
}

/// The row index a scope selects in its home panel.
pub fn row_of(scope: &IssueScope) -> Option<usize> {
    match scope {
        IssueScope::Mesh(index) | IssueScope::Face(index, _) | IssueScope::Material(index) => {
            Some(*index)
        }
        IssueScope::Edge { mesh_index, .. } => Some(*mesh_index),
        IssueScope::Model => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::layout::PanelType;

    #[test]
    fn a_mesh_scoped_issue_goes_to_the_mesh_table() {
        assert_eq!(home_of(&IssueScope::Mesh(2)), Some(PanelType::Meshes));
        assert_eq!(row_of(&IssueScope::Mesh(2)), Some(2));
    }

    /// A face or an edge is a place on a mesh, so it lands on that mesh's row
    /// rather than nowhere.
    #[test]
    fn a_face_or_edge_issue_lands_on_its_mesh() {
        assert_eq!(home_of(&IssueScope::Face(3, 91)), Some(PanelType::Meshes));
        assert_eq!(row_of(&IssueScope::Face(3, 91)), Some(3));

        let edge = IssueScope::Edge {
            mesh_index: 4,
            vertices: [7, 8],
        };
        assert_eq!(home_of(&edge), Some(PanelType::Meshes));
        assert_eq!(row_of(&edge), Some(4));
    }

    #[test]
    fn a_material_issue_goes_to_the_material_table() {
        assert_eq!(
            home_of(&IssueScope::Material(1)),
            Some(PanelType::Materials)
        );
        assert_eq!(row_of(&IssueScope::Material(1)), Some(1));
    }

    /// A model-wide issue is about the asset, not about a row, so jump has
    /// nowhere to take a reader and must say so rather than guess.
    #[test]
    fn a_model_wide_issue_has_nowhere_to_jump_to() {
        assert_eq!(home_of(&IssueScope::Model), None);
        assert_eq!(row_of(&IssueScope::Model), None);
    }

    #[test]
    fn the_grouping_and_the_filter_each_cycle_and_return() {
        assert_eq!(Group::Kind.next(), Group::Subject);
        assert_eq!(Group::Subject.next(), Group::Kind);

        let mut filter = Filter::All;
        for _ in 0..3 {
            filter = filter.next();
        }
        assert_eq!(filter, Filter::All);
    }

    #[test]
    fn a_filter_admits_only_what_it_names() {
        assert!(Filter::All.admits(Severity::Error));
        assert!(Filter::ErrorsOnly.admits(Severity::Error));
        assert!(!Filter::ErrorsOnly.admits(Severity::Warning));
        assert!(!Filter::WarningsOnly.admits(Severity::Error));
    }

    /// One kind can carry a thousand issues and bury every other kind under
    /// them, which is exactly what a real model does. Folding is what makes a
    /// grouped list usable rather than a longer flat one.
    #[test]
    fn folding_a_group_hides_its_issues_and_keeps_its_header() {
        let mut panel = Validation::default();
        assert!(panel.collapsed.is_empty());

        panel.collapsed.push("Non-manifold edge".to_owned());
        assert_eq!(panel.collapsed.len(), 1);

        // Folding is keyed on the label, so changing the filter underneath
        // cannot fold a different group than the one that was chosen.
        panel.filter = Filter::ErrorsOnly;
        assert_eq!(panel.collapsed, vec!["Non-manifold edge".to_owned()]);
    }

    #[test]
    fn the_severity_word_is_the_one_the_design_shows() {
        assert_eq!(severity_word(Severity::Error), "ERROR");
        assert_eq!(severity_word(Severity::Warning), "WARN");
    }
}
