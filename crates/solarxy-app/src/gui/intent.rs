//! The typed intent queue: what a panel asks the shell to do, and the order
//! it is done in.
//!
//! A panel draws against borrowed, read-only views of the document and of the
//! shell's settings, and never against a mutable engine or a mutable `State`.
//! That is what keeps the engine the single writer, and it means everything a
//! panel wants has to wait until the interface pass has finished. This module
//! is the vocabulary of that wait: a panel raises an [`Intent`], and
//! `State::drain_intents` applies the whole queue afterwards.
//!
//! ## The queue accumulates across passes, and that is load-bearing
//!
//! `egui::Context::run` re-invokes its closure when something requests a
//! discard, up to `max_passes`, which defaults to two. `egui::Grid` requests
//! one the first time it appears, and three panels here draw a grid, so a
//! twice-run frame is ordinary rather than exotic. On the repeat pass the raw
//! input has been taken, so no widget reports a click and no key is consumed.
//!
//! Two rules follow, and breaking either is silent.
//!
//! - **The queue is never cleared inside the closure.** An intent raised on
//!   the first pass has to survive into the drain, and clearing at the top of
//!   each pass would throw it away on every frame that runs twice.
//! - **An intent is raised only from a widget response or a consumed key**,
//!   never from a condition that is merely true while a panel is open. An
//!   event-driven raise cannot repeat on the second pass, because the events
//!   are gone by then; a state-driven one repeats on every pass and lands in
//!   the queue twice.
//!
//! The flag structs this replaces were immune to both by accident: they were
//! built once outside the closure and every write was idempotent.

use solarxy_core::preferences::ProjectionMode;

use super::node_tree::NodeTreeAction;
use super::outliner::OutlinerAction;
use super::pane_toolbar::LookThroughChange;

/// One thing a panel asked for during an interface pass.
///
/// Grouped by the area a panel belongs to rather than flattened, so a new
/// panel adds a variant to the enum that is already about its area instead of
/// widening one list that every panel shares.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Intent {
    /// A pane's projection was picked from that pane's own toolbar.
    Projection { pane: usize, mode: ProjectionMode },
    /// A pane was bound to a scene camera, or released back to a free view.
    LookThrough {
        pane: usize,
        change: LookThroughChange,
    },
    /// A panel asked for something the shell does on its behalf.
    Panel(PanelIntent),
}

/// What a panel asked for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PanelIntent {
    /// A validation row was clicked: frame the issue it names.
    FlyToIssue(usize),
    /// The Properties panel's Clear HDRI button.
    ClearHdri,
    /// The Properties panel's Load HDRI button, shown when none is loaded.
    LoadHdri,
    /// The Outliner, or the viewport context menu, which raises the same
    /// actions on purpose because they are the same actions.
    Outliner(OutlinerAction),
    /// The Node Tree.
    NodeTree(NodeTreeAction),
}

impl Intent {
    /// Where this intent sits in the drain's order.
    ///
    /// The drain applies by category rather than by whichever thing a user
    /// happened to click first, because that is what the shell did before this
    /// queue existed: a fixed sequence of blocks after the interface pass. A
    /// stable sort on this key keeps the raise order within a category and
    /// reproduces that sequence.
    ///
    /// The values are contiguous and carry no meaning beyond their order, so
    /// inserting a category means renumbering the ones after it and updating
    /// the test that pins the sequence.
    pub(crate) fn order(&self) -> u8 {
        match self {
            Self::Projection { .. } => 0,
            Self::LookThrough { .. } => 1,
            Self::Panel(PanelIntent::FlyToIssue(_)) => 2,
            Self::Panel(PanelIntent::ClearHdri) => 3,
            Self::Panel(PanelIntent::LoadHdri) => 4,
            Self::Panel(PanelIntent::Outliner(_)) => 5,
            Self::Panel(PanelIntent::NodeTree(_)) => 6,
        }
    }
}

/// The queue a panel raises into, and the drain takes from.
#[derive(Debug, Default)]
pub(crate) struct Intents(Vec<Intent>);

impl Intents {
    /// Ask for something. Called from inside the interface pass, under the
    /// two rules in this module's documentation.
    pub(crate) fn raise(&mut self, intent: Intent) {
        self.0.push(intent);
    }

    /// Ask for something a panel wants. Shorthand for the common case, which
    /// is two constructors deep at every call site without it.
    pub(crate) fn panel(&mut self, intent: PanelIntent) {
        self.raise(Intent::Panel(intent));
    }

    /// Take everything raised, ordered for application, leaving the queue
    /// empty.
    pub(crate) fn take_ordered(&mut self) -> Vec<Intent> {
        let mut queued = std::mem::take(&mut self.0);
        // Stable, which is the half of the contract that keeps two intents in
        // one category in the order the user raised them.
        queued.sort_by_key(Intent::order);
        queued
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::document::{GraphContext, NodeId};

    fn keys(intents: &mut Intents) -> Vec<u8> {
        intents.take_ordered().iter().map(Intent::order).collect()
    }

    /// The sequence is the one the shell applied before the queue existed,
    /// and the raise order does not get a vote in it.
    #[test]
    fn the_drain_order_is_the_documented_sequence() {
        let mut intents = Intents::default();
        intents.panel(PanelIntent::NodeTree(NodeTreeAction::Select(
            GraphContext::Root,
            NodeId(1),
        )));
        intents.panel(PanelIntent::Outliner(OutlinerAction::ShowAll));
        intents.panel(PanelIntent::LoadHdri);
        intents.panel(PanelIntent::ClearHdri);
        intents.panel(PanelIntent::FlyToIssue(3));
        intents.raise(Intent::LookThrough {
            pane: 0,
            change: LookThroughChange::Free,
        });
        intents.raise(Intent::Projection {
            pane: 0,
            mode: ProjectionMode::Orthographic,
        });

        assert_eq!(keys(&mut intents), vec![0, 1, 2, 3, 4, 5, 6]);
    }

    /// Two intents in one category keep the order they were raised in, which
    /// is what makes a click sequence inside one panel mean what it looks
    /// like.
    #[test]
    fn two_intents_in_one_category_keep_their_raise_order() {
        let mut intents = Intents::default();
        for mesh in [7_usize, 2, 5] {
            intents.panel(PanelIntent::Outliner(OutlinerAction::HideMesh(mesh)));
        }

        let order: Vec<usize> = intents
            .take_ordered()
            .into_iter()
            .map(|i| match i {
                Intent::Panel(PanelIntent::Outliner(OutlinerAction::HideMesh(m))) => m,
                other => panic!("unexpected intent {other:?}"),
            })
            .collect();
        assert_eq!(order, vec![7, 2, 5]);
    }

    /// Taking the queue empties it, so a frame that raises nothing drains
    /// nothing rather than replaying the frame before it.
    #[test]
    fn taking_the_queue_empties_it() {
        let mut intents = Intents::default();
        intents.panel(PanelIntent::ClearHdri);
        assert_eq!(intents.take_ordered().len(), 1);
        assert!(intents.take_ordered().is_empty());
    }
}
