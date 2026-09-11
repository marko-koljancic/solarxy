//! Undo and redo on the desktop, and the context each step was made in.
//!
//! The engine owns the stack; this shell dispatches into it and reads its
//! depths for the controls. What the engine does not track is *where* a
//! step was made: undoing a change made inside a container while looking
//! at the root would change something the user cannot see. So the shell
//! keeps a mirror of the stack holding one graph context per step, and an
//! undo puts the user back where the change was made.
//!
//! **The mirror is reconciled against the engine's depths once per frame,
//! not hooked into every command site.** Fourteen files apply commands, and
//! a hook forgotten in one of them would leave the mirror short by a step
//! forever. Reconciling attributes every step that appeared since the last
//! frame to the context the user is looking at now, which is where it was
//! made, since one frame's commands come from one gesture.

use solarxy_graph::Command;
use solarxy_graph::document::GraphContext;

use super::State;
use crate::gui::{HistoryReadout, ToastSeverity};

/// One graph context per step on each side of the engine's stack.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct UndoContexts {
    undo: Vec<GraphContext>,
    redo: Vec<GraphContext>,
}

impl UndoContexts {
    /// Bring the mirror in step with the engine, attributing any step that
    /// appeared to `current`. A side that shrank is truncated, which is how
    /// a cleared redo side and a replaced document both land here.
    pub(crate) fn reconcile(
        &mut self,
        undo_depth: usize,
        redo_depth: usize,
        current: GraphContext,
    ) {
        self.undo.truncate(undo_depth);
        self.undo.resize(undo_depth, current);
        self.redo.truncate(redo_depth);
        self.redo.resize(redo_depth, current);
    }

    /// The step undo would take back was made here.
    #[cfg(test)]
    fn next_undo(&self) -> Option<GraphContext> {
        self.undo.last().copied()
    }

    /// The step redo would restore was made here.
    #[cfg(test)]
    fn next_redo(&self) -> Option<GraphContext> {
        self.redo.last().copied()
    }

    /// A step was undone: its context moves to the redo side and is
    /// returned, so the shell can show it.
    pub(crate) fn undone(&mut self) -> Option<GraphContext> {
        let ctx = self.undo.pop()?;
        self.redo.push(ctx);
        Some(ctx)
    }

    /// A step was redone: the reverse.
    pub(crate) fn redone(&mut self) -> Option<GraphContext> {
        let ctx = self.redo.pop()?;
        self.undo.push(ctx);
        Some(ctx)
    }

    #[cfg(test)]
    fn depths(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }
}

impl State {
    /// What the controls show, read from the engine each frame.
    pub(super) fn history_readout(&self) -> HistoryReadout {
        self.engine
            .as_ref()
            .map_or_else(HistoryReadout::default, |engine| HistoryReadout {
                open: true,
                can_undo: engine.undo_depth() > 0,
                can_redo: engine.redo_depth() > 0,
            })
    }

    /// Once per frame: attribute the steps this frame's gestures made to
    /// the context they were made in.
    pub(super) fn reconcile_history(&mut self) {
        let Some(engine) = &self.engine else {
            self.history = UndoContexts::default();
            return;
        };
        let current = self.gui.graph_ctx();
        self.history
            .reconcile(engine.undo_depth(), engine.redo_depth(), current);
    }

    /// Take back the last step and show where it was made.
    pub fn undo(&mut self) {
        self.reconcile_history();
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if engine.undo_depth() == 0 {
            return;
        }
        match engine.apply(Command::Undo) {
            Ok(_) => {
                if let Some(ctx) = self.history.undone() {
                    self.gui.set_graph_ctx(ctx);
                }
            }
            Err(e) => self
                .gui
                .set_toast(&format!("Undo failed: {e}"), ToastSeverity::Error),
        }
    }

    /// Restore the last undone step and show where it was made.
    pub fn redo(&mut self) {
        self.reconcile_history();
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if engine.redo_depth() == 0 {
            return;
        }
        match engine.apply(Command::Redo) {
            Ok(_) => {
                if let Some(ctx) = self.history.redone() {
                    self.gui.set_graph_ctx(ctx);
                }
            }
            Err(e) => self
                .gui
                .set_toast(&format!("Redo failed: {e}"), ToastSeverity::Error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::document::NodeId;
    use solarxy_graph::engine::{Engine, EngineEvent};

    fn add(engine: &mut Engine, ctx: GraphContext, ty: &str) -> NodeId {
        let batch = engine
            .apply(Command::AddNode {
                ctx,
                node_type: ty.to_string(),
                position: [0.0, 0.0],
            })
            .expect("the node adds");
        batch
            .events
            .iter()
            .find_map(|ev| match ev {
                EngineEvent::NodeAdded { node, .. } => Some(node.id),
                _ => None,
            })
            .expect("a node was added")
    }

    /// The mirror follows the engine's depths: steps that appear take the
    /// current context, a shrunk side is cut, and an emptied one is cleared.
    #[test]
    fn the_mirror_follows_the_depths_and_attributes_new_steps_to_the_current_context() {
        let inner = GraphContext::Subflow(NodeId(7));
        let mut m = UndoContexts::default();
        m.reconcile(2, 0, GraphContext::Root);
        m.reconcile(3, 0, inner);
        assert_eq!(m.depths(), (3, 0));
        assert_eq!(m.next_undo(), Some(inner));

        m.reconcile(1, 2, GraphContext::Root);
        assert_eq!(
            m.depths(),
            (1, 2),
            "a shrunk side is cut and a grown one filled"
        );
        assert_eq!(m.next_undo(), Some(GraphContext::Root));

        m.reconcile(0, 0, GraphContext::Root);
        assert_eq!(
            m,
            UndoContexts::default(),
            "a replaced document clears both sides"
        );
    }

    /// An undo moves the step's context to the redo side and hands it back;
    /// a redo moves it home again.
    #[test]
    fn undone_and_redone_move_a_step_between_the_sides() {
        let inner = GraphContext::Subflow(NodeId(3));
        let mut m = UndoContexts::default();
        m.reconcile(1, 0, GraphContext::Root);
        m.reconcile(2, 0, inner);

        assert_eq!(m.undone(), Some(inner));
        assert_eq!(m.depths(), (1, 1));
        assert_eq!(m.next_redo(), Some(inner));
        assert_eq!(m.redone(), Some(inner));
        assert_eq!(m.depths(), (2, 0));
        assert_eq!(m.next_undo(), Some(inner));

        let mut empty = UndoContexts::default();
        assert_eq!(empty.undone(), None);
        assert_eq!(empty.redone(), None);
    }

    /// Driven the way the shell drives it, against a real engine: a change
    /// made inside a container, undone while looking at the root, names
    /// the container as the place to return to, and a new step after an
    /// undo drops the redo contexts with the engine's redo side.
    #[test]
    fn an_undo_across_a_dive_names_the_context_the_change_was_made_in() {
        let mut engine = Engine::new().expect("engine");
        let mut m = UndoContexts::default();

        let container = add(&mut engine, GraphContext::Root, "sopnet");
        m.reconcile(engine.undo_depth(), engine.redo_depth(), GraphContext::Root);
        let inner = GraphContext::Subflow(container);
        add(&mut engine, inner, "box");
        m.reconcile(engine.undo_depth(), engine.redo_depth(), inner);
        assert_eq!(m.depths(), (2, 0));

        // Back at the root, the user undoes.
        m.reconcile(engine.undo_depth(), engine.redo_depth(), GraphContext::Root);
        engine.apply(Command::Undo).expect("undo");
        assert_eq!(
            m.undone(),
            Some(inner),
            "the step was made inside the container"
        );
        m.reconcile(engine.undo_depth(), engine.redo_depth(), GraphContext::Root);
        assert_eq!(m.depths(), (1, 1));

        engine.apply(Command::Redo).expect("redo");
        assert_eq!(m.redone(), Some(inner));
        engine.apply(Command::Undo).expect("undo");
        m.undone();

        add(&mut engine, GraphContext::Root, "sopnet");
        m.reconcile(engine.undo_depth(), engine.redo_depth(), GraphContext::Root);
        assert_eq!(
            m.depths(),
            (2, 0),
            "a new step cleared the redo side on both"
        );
        assert_eq!(m.next_redo(), None);
    }
}
