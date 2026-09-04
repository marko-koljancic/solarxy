//! Which nodes' cooks are failing, read off the engine's event stream.
//!
//! The desktop drives the engine but has no per-node badges (the editing
//! canvas is a later release), so a cook failure must surface somewhere
//! or a scene silently shows stale or missing geometry. This tracker
//! turns `CookStatus` events into two things: the fresh failures the
//! shell toasts, and a standing map of failing nodes the still render
//! consults before it is willing to report success.
//!
//! "Once per cook rather than once per frame" is inherited from the
//! engine rather than re-implemented here: the cook driver emits a
//! status event only on a state transition (`CookStatus::same_state`),
//! so a node erroring identically across a thousand frames produces one
//! event, and a node re-cooked after an edit and failing again produces
//! exactly one more. Absorbing events is therefore already the right
//! granularity; polling the stored status would not be.

use std::collections::BTreeMap;

use solarxy_graph::EngineEvent;
use solarxy_graph::cook::state::CookStatus;
use solarxy_graph::document::NodeId;

/// The failing-cook ledger. One entry per node whose most recent cook
/// errored, cleared by a clean recook or the node's removal.
#[derive(Default)]
pub(crate) struct CookHealth {
    failures: BTreeMap<NodeId, String>,
}

impl CookHealth {
    /// Absorb one frame's engine events and return the fresh failures to
    /// surface, in event order.
    ///
    /// `Pending` and `Cooking` deliberately do not clear a failure: they
    /// are transit states every recook passes through, and dropping the
    /// entry there would let a still render start over a scene whose
    /// broken node merely had not failed *again* yet.
    pub(crate) fn absorb(&mut self, events: &[EngineEvent]) -> Vec<(NodeId, String)> {
        let mut fresh = Vec::new();
        for event in events {
            match event {
                EngineEvent::CookStatus { node, status } => match status {
                    CookStatus::Error { message } => {
                        self.failures.insert(*node, message.clone());
                        fresh.push((*node, message.clone()));
                    }
                    CookStatus::Ok { .. } => {
                        self.failures.remove(node);
                    }
                    CookStatus::Pending | CookStatus::Cooking => {}
                },
                EngineEvent::NodeRemoved { id, .. } => {
                    self.failures.remove(id);
                }
                _ => {}
            }
        }
        fresh
    }

    /// Whether every node's most recent cook succeeded.
    pub(crate) fn is_healthy(&self) -> bool {
        self.failures.is_empty()
    }

    /// The failing nodes and their reasons, for the still render's
    /// refusal message.
    pub(crate) fn failing(&self) -> &BTreeMap<NodeId, String> {
        &self.failures
    }

    /// Forget everything, for a scene open or close: the ledger describes
    /// one document, and a stale entry would refuse a render over a scene
    /// that never contained the node.
    pub(crate) fn clear(&mut self) {
        self.failures.clear();
    }
}

#[cfg(test)]
mod tests {
    use solarxy_graph::document::GraphContext;

    use super::*;

    fn status(node: u64, status: CookStatus) -> EngineEvent {
        EngineEvent::CookStatus {
            node: NodeId(node),
            status,
        }
    }

    fn error(node: u64, message: &str) -> EngineEvent {
        status(
            node,
            CookStatus::Error {
                message: message.into(),
            },
        )
    }

    #[test]
    fn a_failed_cook_surfaces_once_and_quiet_frames_stay_quiet() {
        let mut health = CookHealth::default();
        let fresh = health.absorb(&[error(7, "no input geometry")]);
        assert_eq!(fresh, vec![(NodeId(7), "no input geometry".to_owned())]);
        assert!(!health.is_healthy());

        // The engine emits nothing while the state stands, so later
        // frames carry no event and nothing resurfaces.
        assert!(health.absorb(&[]).is_empty());
        assert!(!health.is_healthy());
    }

    #[test]
    fn a_clean_recook_clears_the_failure() {
        let mut health = CookHealth::default();
        health.absorb(&[error(7, "no input geometry")]);
        health.absorb(&[status(7, CookStatus::Ok { ms: 0.2 })]);
        assert!(health.is_healthy());
    }

    #[test]
    fn transit_states_keep_the_failure_standing() {
        let mut health = CookHealth::default();
        health.absorb(&[error(7, "no input geometry")]);
        health.absorb(&[
            status(7, CookStatus::Pending),
            status(7, CookStatus::Cooking),
        ]);
        assert!(!health.is_healthy(), "a recook in flight is not a recovery");

        // Failing again after the transit is a fresh cook and surfaces
        // again, which is the once-per-cook contract.
        let fresh = health.absorb(&[error(7, "no input geometry")]);
        assert_eq!(fresh.len(), 1);
    }

    #[test]
    fn a_removed_node_stops_counting_as_failed() {
        let mut health = CookHealth::default();
        health.absorb(&[error(7, "no input geometry")]);
        health.absorb(&[EngineEvent::NodeRemoved {
            ctx: GraphContext::Root,
            id: NodeId(7),
        }]);
        assert!(health.is_healthy());
    }

    #[test]
    fn failures_accumulate_per_node() {
        let mut health = CookHealth::default();
        health.absorb(&[error(7, "no input geometry"), error(9, "bad expression")]);
        assert_eq!(health.failing().len(), 2);
        health.absorb(&[status(7, CookStatus::Ok { ms: 0.1 })]);
        assert_eq!(health.failing().len(), 1);
        assert!(health.failing().contains_key(&NodeId(9)));
    }
}
