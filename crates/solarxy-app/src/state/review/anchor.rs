//! Where a note is placed, and the re-anchor sub-mode.
//!
//! The arithmetic left with the store: the engine resolves an anchor to a
//! world point and derives its staleness after every cook, so what remains
//! here is the one conversion the shell makes, from a detailed pick to the
//! anchor the engine stores, and the two transitions of the re-anchor
//! sub-mode.

use solarxy_graph::document::GraphContext;
use solarxy_graph::engine::PickDetail;
use solarxy_graph::review::{AnnotationId, ReviewAnchor};

use super::ReviewState;

/// The anchor a detailed pick describes, as the browser builds one: the
/// picked node, mesh and face, the weights, and the picked world point as
/// the fallback the marker draws from while the anchor is stale.
///
/// The pick's three weights and the anchor's are one array: element k
/// weights the face's k-th vertex on both sides, and only the letters the
/// two doc comments once used to name them differed. They are copied
/// through unchanged; reordering them would be the bug, not the fix, and
/// the engine's round-trip test is what holds the two to one meaning.
pub(crate) fn anchor_from_pick(pick: &PickDetail) -> ReviewAnchor {
    ReviewAnchor {
        ctx: GraphContext::Root,
        node: pick.node,
        mesh: Some(pick.mesh),
        face: Some(pick.face),
        barycentric: Some(pick.barycentric),
        world_fallback: Some(pick.world_pos),
        // Filled by the engine on add and on re-anchor.
        geometry_hash: None,
    }
}

impl ReviewState {
    /// Enter the re-anchor sub-mode for an annotation: select it so the
    /// panel scrolls to it, and arm the next click on geometry to re-place
    /// it. The engine refuses an id it does not hold at dispatch, so nothing
    /// is checked here.
    pub fn begin_reanchor(&mut self, id: AnnotationId) {
        self.selected = Some(id);
        self.scroll_to_selected = true;
        self.reanchor_target = Some(id);
    }

    /// Leave the re-anchor sub-mode without changing any annotation. Leaves
    /// `selected` intact so the row stays highlighted.
    pub fn cancel_reanchor(&mut self) {
        self.reanchor_target = None;
    }
}

#[cfg(test)]
mod tests {
    use solarxy_graph::document::NodeId;

    use super::*;

    #[test]
    fn anchor_from_pick_copies_the_weights_through_unchanged() {
        let pick = PickDetail {
            node: NodeId(3),
            mesh: 1,
            face: 42,
            barycentric: [0.2, 0.3, 0.5],
            world_pos: [1.0, 2.0, 3.0],
            distance: 9.0,
        };
        let anchor = anchor_from_pick(&pick);
        assert_eq!(anchor.ctx, GraphContext::Root);
        assert_eq!(anchor.node, NodeId(3));
        assert_eq!(anchor.mesh, Some(1));
        assert_eq!(anchor.face, Some(42));
        assert_eq!(
            anchor.barycentric,
            Some([0.2, 0.3, 0.5]),
            "element k weights vertex k on both sides; nothing is reordered"
        );
        assert_eq!(anchor.world_fallback, Some([1.0, 2.0, 3.0]));
        assert!(anchor.geometry_hash.is_none(), "the engine fills the hash");
    }

    #[test]
    fn begin_reanchor_selects_and_arms() {
        let mut state = ReviewState::default();
        state.begin_reanchor(AnnotationId(5));
        assert_eq!(state.selected, Some(AnnotationId(5)));
        assert_eq!(state.reanchor_target, Some(AnnotationId(5)));
        assert!(state.scroll_to_selected);
    }

    #[test]
    fn cancel_reanchor_clears_target_only() {
        let mut state = ReviewState::default();
        state.begin_reanchor(AnnotationId(5));
        state.cancel_reanchor();
        assert!(state.reanchor_target.is_none());
        assert_eq!(
            state.selected,
            Some(AnnotationId(5)),
            "the row stays highlighted"
        );
    }
}
