//! Arrange mode: the grammar for changing an arrangement.
//!
//! # Why arranging is a mode and almost nothing else is
//!
//! Sort, filter, export and nine jump addresses have already claimed the
//! single-letter namespace. Making split and close always live would either
//! collide with those or push them onto modifiers, and an accidental layout
//! change costs more than one extra keystroke. So arranging takes a prefix and
//! everything else stays live.
//!
//! # No keys here
//!
//! This module knows commands, not keys. The keymap table that binds them
//! lands with the rest of the chrome, and until it does the grammar is
//! testable on its own terms: every command is a function of a layout and a
//! pane, and every refusal is a value rather than a beep.

use ratatui::layout::Rect;

use super::layout::{Direction, Layout, Refusal};

/// One thing a reader can do to an arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Move focus to the nearest panel in a direction.
    Focus(Toward),
    /// Split the focused panel, putting a catalogue leaf beside it.
    Split(Direction),
    /// Close the focused panel; its sibling takes the room.
    Close,
    /// Move the divider above the focused panel.
    Grow,
    Shrink,
    /// Even every ratio out.
    Balance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
}

/// How much of the axis one resize step moves.
///
/// Small enough that a reader can land on what they wanted, large enough that
/// they are not holding the key down. Two steps is roughly a tenth of the
/// pane, which is about the granularity a terminal's cells support anyway.
pub const RESIZE_STEP: f32 = 0.05;

/// What happened, so the caller knows whether to redraw and what to say.
///
/// Leaving the mode is deliberately not one of these. The keymap owns which
/// key ends arranging and the caller acts on it directly, so this enum stays
/// about what happened to the arrangement rather than to the reader's mode.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The arrangement changed.
    Changed(Layout),
    /// The command was refused; the arrangement is untouched and the reason
    /// belongs in the focused panel's border.
    Refused(Refusal),
}

/// Apply one command.
///
/// Takes the pane because every refusal is a question about cells: whether a
/// split leaves both children readable can only be answered against the size
/// the arrangement is actually being drawn at.
pub fn apply(layout: &Layout, area: Rect, command: Command) -> Outcome {
    match command {
        Command::Focus(toward) => Outcome::Changed(focus_toward(layout, area, toward)),
        Command::Split(dir) => result(layout.split(area, dir)),
        Command::Close => result(layout.close()),
        Command::Grow => result(layout.resize(area, RESIZE_STEP)),
        Command::Shrink => result(layout.resize(area, -RESIZE_STEP)),
        Command::Balance => result(layout.balance(area)),
    }
}

fn result(outcome: Result<Layout, Refusal>) -> Outcome {
    match outcome {
        Ok(layout) => Outcome::Changed(layout),
        Err(refusal) => Outcome::Refused(refusal),
    }
}

/// Move focus to the nearest panel in a direction.
///
/// Spatial rather than structural. A reader pressing right means the panel to
/// the right of the one they are looking at, not the sibling that happens to
/// be next in the tree, and those are different the moment a split nests.
///
/// Nothing in that direction leaves focus where it is, which is the right
/// answer at an edge: silently wrapping to the far side would move the eye
/// somewhere it did not ask to go.
#[must_use]
pub fn focus_toward(layout: &Layout, area: Rect, toward: Toward) -> Layout {
    let placements = layout.solve(area, None);
    let Some(current) = placements.iter().find(|p| p.focused) else {
        return layout.clone();
    };
    let here = current.rect;

    // Two filters, and both are needed. Being beyond the *edge* rather than
    // beyond the centre is what stops a tall neighbour from counting as being
    // in every direction at once. Overlapping on the cross axis is what stops
    // a panel in the row below from winning a rightward move: it is nearer by
    // centre distance than the panel actually to the right, which is exactly
    // the wrong answer.
    let candidates: Vec<_> = placements
        .iter()
        .filter(|p| !p.focused)
        .filter(|p| beyond(here, p.rect, toward))
        .collect();
    let aligned: Vec<_> = candidates
        .iter()
        .filter(|p| overlaps(here, p.rect, toward))
        .copied()
        .collect();

    // Nothing sharing the reader's line means a ragged arrangement rather than
    // an edge, so fall back to anything in that direction rather than refusing
    // to move.
    let pool = if aligned.is_empty() {
        &candidates
    } else {
        &aligned
    };

    let best = pool.iter().min_by_key(|p| {
        let (along, across) = match toward {
            Toward::Left => (
                here.x.saturating_sub(p.rect.right()),
                cross(here, p.rect, toward),
            ),
            Toward::Right => (
                p.rect.x.saturating_sub(here.right()),
                cross(here, p.rect, toward),
            ),
            Toward::Up => (
                here.y.saturating_sub(p.rect.bottom()),
                cross(here, p.rect, toward),
            ),
            Toward::Down => (
                p.rect.y.saturating_sub(here.bottom()),
                cross(here, p.rect, toward),
            ),
        };
        (along, across)
    });

    match best {
        Some(found) => layout.with_focus(found.id),
        None => layout.clone(),
    }
}

/// Whether `other` lies past `here`'s edge in the direction of travel.
fn beyond(here: Rect, other: Rect, toward: Toward) -> bool {
    match toward {
        Toward::Left => other.right() <= here.x,
        Toward::Right => other.x >= here.right(),
        Toward::Up => other.bottom() <= here.y,
        Toward::Down => other.y >= here.bottom(),
    }
}

/// Whether the two share any of the axis perpendicular to the travel.
fn overlaps(here: Rect, other: Rect, toward: Toward) -> bool {
    match toward {
        Toward::Left | Toward::Right => here.y < other.bottom() && other.y < here.bottom(),
        Toward::Up | Toward::Down => here.x < other.right() && other.x < here.right(),
    }
}

/// How far off the reader's current line a candidate sits.
fn cross(here: Rect, other: Rect, toward: Toward) -> u16 {
    let (a, b) = match toward {
        Toward::Left | Toward::Right => (here.y + here.height / 2, other.y + other.height / 2),
        Toward::Up | Toward::Down => (here.x + here.width / 2, other.x + other.width / 2),
    };
    a.abs_diff(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::layout::{PanelType, Preset};

    const PANE: Rect = Rect {
        x: 0,
        y: 0,
        width: 140,
        height: 44,
    };

    fn focused_name(layout: &Layout) -> &'static str {
        layout
            .panel_of(layout.focus())
            .expect("focus points at a leaf")
            .name()
    }

    /// Survey's top row is silhouette, geometry, health left to right, so
    /// stepping right crosses them in that order and stops at the edge.
    #[test]
    fn focus_moves_to_the_neighbour_in_that_direction() {
        let mut layout = Preset::Survey.layout();
        assert_eq!(focused_name(&layout), "silhouette");

        layout = focus_toward(&layout, PANE, Toward::Right);
        assert_eq!(focused_name(&layout), "geometry");
        layout = focus_toward(&layout, PANE, Toward::Right);
        assert_eq!(focused_name(&layout), "health");
    }

    #[test]
    fn focus_at_an_edge_stays_put_rather_than_wrapping() {
        let layout = Preset::Survey.layout();
        let unmoved = focus_toward(&layout, PANE, Toward::Left);
        assert_eq!(unmoved.focus(), layout.focus(), "left from the leftmost");
        let unmoved = focus_toward(&layout, PANE, Toward::Up);
        assert_eq!(unmoved.focus(), layout.focus(), "up from the top row");
    }

    /// Moving down out of the top row must land in the row below rather than
    /// in whichever leaf the tree happens to reach first.
    #[test]
    fn focus_down_crosses_into_the_row_below() {
        let layout = Preset::Survey.layout();
        let down = focus_toward(&layout, PANE, Toward::Down);
        assert_eq!(focused_name(&down), "meshes");

        let further = focus_toward(&down, PANE, Toward::Down);
        assert_eq!(focused_name(&further), "validation");
    }

    /// Nesting is invisible to the reader, so stepping right out of the
    /// left-hand column has to reach the panel that is actually to the right.
    #[test]
    fn focus_ignores_how_the_tree_nests() {
        let layout = Preset::Meshes.layout();
        assert_eq!(focused_name(&layout), "meshes");
        let right = focus_toward(&layout, PANE, Toward::Right);
        assert_eq!(focused_name(&right), "silhouette");
        let down = focus_toward(&right, PANE, Toward::Down);
        assert_eq!(focused_name(&down), "distributions");
    }

    #[test]
    fn splitting_and_closing_change_the_arrangement() {
        let layout = Preset::Survey.layout();
        let Outcome::Changed(split) = apply(&layout, PANE, Command::Split(Direction::Horizontal))
        else {
            panic!("a survey panel has room to split");
        };
        assert_eq!(split.leaves().len(), 7);

        let Outcome::Changed(closed) = apply(&split, PANE, Command::Close) else {
            panic!("seven panels can lose one");
        };
        assert_eq!(closed.leaves().len(), 6);
    }

    /// A refusal is a value, and the arrangement it was asked about is
    /// untouched.
    #[test]
    fn a_refused_command_reports_why_and_changes_nothing() {
        let tight = Rect::new(0, 0, 40, 10);
        let layout = Layout::single(PanelType::Meshes);
        let before = layout.encode();

        let outcome = apply(&layout, tight, Command::Split(Direction::Vertical));
        assert_eq!(outcome, Outcome::Refused(Refusal::TooSmall));
        assert_eq!(layout.encode(), before);

        let outcome = apply(&layout, PANE, Command::Close);
        assert_eq!(outcome, Outcome::Refused(Refusal::LastPanel));
        assert_eq!(layout.encode(), before);
    }

    #[test]
    fn growing_and_shrinking_move_the_divider_opposite_ways() {
        let layout = Preset::Survey.layout();
        let width_of = |l: &Layout| l.solve(PANE, None)[0].rect.width;
        let start = width_of(&layout);

        let Outcome::Changed(grown) = apply(&layout, PANE, Command::Grow) else {
            panic!("room to grow");
        };
        let Outcome::Changed(shrunk) = apply(&layout, PANE, Command::Shrink) else {
            panic!("room to shrink");
        };
        assert!(width_of(&grown) > start, "grow did not widen the panel");
        assert!(width_of(&shrunk) < start, "shrink did not narrow it");
    }

    #[test]
    fn balance_is_accepted_and_keeps_every_panel_usable() {
        let Outcome::Changed(balanced) = apply(&Preset::Survey.layout(), PANE, Command::Balance)
        else {
            panic!("balance should be accepted at the target size");
        };
        for placement in balanced.solve(PANE, None) {
            assert!(super::super::layout::fits(placement.rect));
        }
    }

    /// Whatever sequence a reader runs, the arrangement stays usable and
    /// exactly one panel holds focus. This is the invariant the whole module
    /// exists to keep.
    #[test]
    fn no_sequence_of_commands_reaches_an_unusable_arrangement() {
        let script = [
            Command::Split(Direction::Vertical),
            Command::Grow,
            Command::Focus(Toward::Right),
            Command::Split(Direction::Horizontal),
            Command::Shrink,
            Command::Balance,
            Command::Focus(Toward::Down),
            Command::Close,
            Command::Focus(Toward::Left),
            Command::Split(Direction::Vertical),
            Command::Close,
        ];

        let mut layout = Preset::Survey.layout();
        for (step, command) in script.into_iter().enumerate() {
            if let Outcome::Changed(next) = apply(&layout, PANE, command) {
                layout = next;
            }
            let placements = layout.solve(PANE, None);
            assert_eq!(
                placements.iter().filter(|p| p.focused).count(),
                1,
                "after step {step} ({command:?}) focus was not exactly one panel"
            );
            for placement in &placements {
                assert!(
                    super::super::layout::fits(placement.rect),
                    "after step {step} ({command:?}) {:?} was unusable",
                    placement.rect
                );
            }
        }
    }
}
