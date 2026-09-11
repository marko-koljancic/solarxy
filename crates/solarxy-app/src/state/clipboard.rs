//! Copy, paste and duplicate, through the engine's own clipboard commands.
//!
//! **The clipboard is in memory, as the browser's is.** Copy is the pure
//! read `Engine::copy_nodes`, which captures the selected nodes, the edges
//! between them and the networks the containers among them own; paste is
//! `PasteNodes` and duplicate is `DuplicateNodes`, each one command and
//! therefore one undo step. Nothing crosses to the system clipboard, so
//! nothing crosses between two running instances either, which the task
//! leaves out.
//!
//! **A cross-kind paste is refused per node, silently, by the engine.** It
//! skips every node the target network's kind cannot hold and returns no
//! error, so the only way to tell a user that five nodes became two is to
//! count what arrived against what was asked for. That is the browser's
//! rule and its wording.

use solarxy_graph::Command;
use solarxy_graph::document::NodeId;
use solarxy_graph::engine::{EngineEvent, EventBatch};

use super::State;
use crate::gui::{ClipboardReadout, ToastSeverity};

/// Where a pasted fragment lands relative to where it was copied: the
/// browser's offset, so a paste over its own source is visibly a copy.
pub(super) const PASTE_OFFSET: [f32; 2] = [30.0, 30.0];

/// How many of `wanted` nodes a paste did not produce.
pub(super) fn skipped_on_paste(wanted: usize, batch: &EventBatch) -> usize {
    let added = batch
        .events
        .iter()
        .filter(|ev| matches!(ev, EngineEvent::NodeAdded { .. }))
        .count();
    wanted.saturating_sub(added)
}

/// The browser's wording for a paste that left nodes behind.
pub(super) fn skipped_message(skipped: usize) -> String {
    format!("{skipped} node(s) skipped: not allowed in this context")
}

impl State {
    /// The selection in the graph the user is looking at.
    fn current_selection(&self) -> Vec<NodeId> {
        let ctx = self.gui.graph_ctx();
        self.engine
            .as_ref()
            .and_then(|engine| engine.document().graph(ctx).ok())
            .map(|graph| graph.selection.clone())
            .unwrap_or_default()
    }

    /// What the Edit menu enables.
    pub(super) fn clipboard_readout(&self) -> ClipboardReadout {
        ClipboardReadout {
            has_selection: !self.current_selection().is_empty(),
            has_clipboard: self.clipboard.is_some(),
        }
    }

    /// Capture the selection. Nothing selected copies nothing and keeps
    /// what was on the clipboard, as the browser does.
    pub fn copy_selection(&mut self) {
        let ids = self.current_selection();
        if ids.is_empty() {
            return;
        }
        let ctx = self.gui.graph_ctx();
        if let Some(engine) = &self.engine {
            self.clipboard = Some(engine.copy_nodes(ctx, &ids));
        }
    }

    /// Paste the clipboard into the graph the user is looking at, offset
    /// from where it was copied, and say how many nodes were refused.
    pub fn paste_clipboard(&mut self) {
        let Some(fragment) = self.clipboard.clone() else {
            return;
        };
        let wanted = fragment.nodes.len();
        let ctx = self.gui.graph_ctx();
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        match engine.apply(Command::PasteNodes {
            ctx,
            fragment,
            position: PASTE_OFFSET,
        }) {
            Ok(batch) => {
                let skipped = skipped_on_paste(wanted, &batch);
                if skipped > 0 {
                    self.gui
                        .set_toast(&skipped_message(skipped), ToastSeverity::Warning);
                }
            }
            Err(e) => self
                .gui
                .set_toast(&format!("Paste failed: {e}"), ToastSeverity::Error),
        }
    }

    /// Duplicate the selection in place, one step.
    pub fn duplicate_selection(&mut self) {
        let ids = self.current_selection();
        if ids.is_empty() {
            return;
        }
        let ctx = self.gui.graph_ctx();
        self.apply_node_command(Command::DuplicateNodes { ctx, ids });
    }
}

/// What a fragment carries, for a test to reason about.
#[cfg(test)]
fn fragment_shape(fragment: &solarxy_graph::document::GraphFragment) -> (usize, usize) {
    (fragment.nodes.len(), fragment.edges.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::document::GraphContext;
    use solarxy_graph::engine::{Engine, PortRefDto};

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

    fn connect(engine: &mut Engine, ctx: GraphContext, from: NodeId, to: NodeId) {
        engine
            .apply(Command::Connect {
                ctx,
                from: PortRefDto {
                    node: from,
                    port: "geometry".to_string(),
                },
                to: PortRefDto {
                    node: to,
                    port: "geometry".to_string(),
                },
            })
            .expect("the wire connects");
    }

    /// Two wired nodes copied and pasted round-trip within their network
    /// and into a sibling of the same kind, wire included, one undo step
    /// each; pasted into a network of another kind they are refused, and
    /// the refusal is counted rather than reported by the engine.
    #[test]
    fn a_fragment_round_trips_within_and_across_networks_of_one_kind_and_is_refused_by_another() {
        let mut engine = Engine::new().expect("engine");
        let a = GraphContext::Subflow(add(&mut engine, GraphContext::Root, "sopnet"));
        let b = GraphContext::Subflow(add(&mut engine, GraphContext::Root, "sopnet"));
        let images = GraphContext::Subflow(add(&mut engine, GraphContext::Root, "copnet"));
        let source = add(&mut engine, a, "box");
        let sink = add(&mut engine, a, "transform");
        connect(&mut engine, a, source, sink);

        let fragment = engine.copy_nodes(a, &[source, sink]);
        assert_eq!(
            fragment_shape(&fragment),
            (2, 1),
            "both nodes and the wire between them"
        );

        let steps = engine.undo_depth();
        let batch = engine
            .apply(Command::PasteNodes {
                ctx: a,
                fragment: fragment.clone(),
                position: PASTE_OFFSET,
            })
            .expect("paste");
        assert_eq!(skipped_on_paste(fragment.nodes.len(), &batch), 0);
        assert_eq!(engine.document().graph(a).expect("a").nodes().count(), 4);
        assert_eq!(engine.undo_depth(), steps + 1, "a paste is one step");

        let batch = engine
            .apply(Command::PasteNodes {
                ctx: b,
                fragment: fragment.clone(),
                position: PASTE_OFFSET,
            })
            .expect("paste into a sibling");
        assert_eq!(skipped_on_paste(fragment.nodes.len(), &batch), 0);
        let sibling = engine.document().graph(b).expect("b");
        assert_eq!(sibling.nodes().count(), 2);
        assert_eq!(
            sibling.edges().count(),
            1,
            "the wire crossed with the nodes"
        );

        let batch = engine
            .apply(Command::PasteNodes {
                ctx: images,
                fragment: fragment.clone(),
                position: PASTE_OFFSET,
            })
            .expect("the engine refuses per node, not per paste");
        assert_eq!(skipped_on_paste(fragment.nodes.len(), &batch), 2);
        assert_eq!(
            engine
                .document()
                .graph(images)
                .expect("images")
                .nodes()
                .count(),
            0,
            "geometry nodes do not land in an image network"
        );
        assert_eq!(
            skipped_message(2),
            "2 node(s) skipped: not allowed in this context"
        );

        let steps = engine.undo_depth();
        engine
            .apply(Command::DuplicateNodes {
                ctx: a,
                ids: vec![source, sink],
            })
            .expect("duplicate");
        assert_eq!(engine.undo_depth(), steps + 1, "a duplicate is one step");
        assert_eq!(engine.document().graph(a).expect("a").nodes().count(), 6);
    }
}
