//! Tidying a graph: one pass, one command.
//!
//! **Hand-rolled rather than adopted, and deliberately one algorithm.**
//! The browser ships two layout engines and defers loading one of them
//! purely because of its download size; the desktop has no payload budget
//! and so no reason to carry two implementations of one menu item. What it
//! does need is the same *result*, so the spacing constants below are the
//! browser's, and a two-node chain lands in the same place on both shells.
//!
//! ## The three steps, and why each is there
//!
//! **Rank by longest path**, not by shortest, so a node sits below every
//! node that feeds it rather than beside one of them. The engine forbids
//! cycles, which is what makes the longest path finite.
//!
//! **Order within a rank by the average position of what feeds it**, run
//! twice. One pass puts a node under its inputs; the second lets a node
//! that moved pull its own consumers after it. More passes buy very
//! little on graphs this size, and the arrangement stops being
//! predictable.
//!
//! **Place on a fixed pitch.** Nothing here measures a node, because
//! every role occupies the one layout box; a layout that measured would
//! be answering a question the canvas has already settled.

use std::collections::BTreeMap;

use solarxy_graph::document::{Graph, NodeId};
use solarxy_graph::registry::{NodeRole, Registry};

use super::art::NODE_BOX;

/// The gap between two nodes in the same rank, and between ranks. Both
/// are the browser's, so a graph tidied on either shell reads the same.
const NODE_GAP: f32 = 58.0;
const RANK_GAP: f32 = 128.0;

/// Where every node goes, in the order the document holds them.
///
/// A note is left where it is: it annotates a graph rather than
/// participating in one, and a layout that swept notes into the flow
/// would move a label away from the thing it labels. The browser excludes
/// them for the same reason.
#[must_use]
pub(super) fn layered(graph: &Graph, registry: &Registry) -> Vec<(NodeId, [f32; 2])> {
    let laid: Vec<NodeId> = graph
        .nodes()
        .filter(|node| {
            registry
                .get(&node.type_id)
                .is_none_or(|desc| desc.role != NodeRole::Note)
        })
        .map(|node| node.id)
        .collect();
    if laid.is_empty() {
        return Vec::new();
    }

    let ranks = rank(graph, &laid);
    let mut rows: BTreeMap<usize, Vec<NodeId>> = BTreeMap::new();
    for id in &laid {
        rows.entry(ranks[id]).or_default().push(*id);
    }

    // Two ordering passes. The first puts a node under whatever feeds it;
    // the second lets a node that moved pull its own consumers after it.
    let mut placed: BTreeMap<NodeId, f32> = BTreeMap::new();
    for _ in 0..2 {
        for row in rows.values_mut() {
            row.sort_by(|a, b| {
                barycentre(graph, *a, &placed)
                    .partial_cmp(&barycentre(graph, *b, &placed))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    // Ties break on the document's own order, so a tidy
                    // is the same tidy every time it is asked for.
                    .then_with(|| a.0.cmp(&b.0))
            });
            for (index, id) in row.iter().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                placed.insert(*id, index as f32 * (NODE_BOX.x + NODE_GAP));
            }
        }
    }

    let mut out = Vec::with_capacity(laid.len());
    for (rank, row) in &rows {
        #[allow(clippy::cast_precision_loss)]
        let y = *rank as f32 * (NODE_BOX.y + RANK_GAP);
        for id in row {
            out.push((*id, [placed.get(id).copied().unwrap_or_default(), y]));
        }
    }
    out.sort_unstable_by_key(|(id, _)| id.0);
    out
}

/// Every node's rank: one more than the deepest thing that feeds it.
fn rank(graph: &Graph, laid: &[NodeId]) -> BTreeMap<NodeId, usize> {
    let mut feeders: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for edge in graph.edges() {
        feeders.entry(edge.to).or_default().push(edge.from);
    }
    let mut ranks: BTreeMap<NodeId, usize> = laid.iter().map(|id| (*id, 0)).collect();
    // A rank cannot exceed the node count, and the engine forbids cycles,
    // so the sweep settles. The bound is a guard rather than the rule:
    // a document that somehow held a cycle would stop rather than spin.
    for _ in 0..laid.len() {
        let mut moved = false;
        for id in laid {
            let deepest = feeders
                .get(id)
                .map(|from| {
                    from.iter()
                        .filter_map(|f| ranks.get(f).copied())
                        .max()
                        .map_or(0, |max| max + 1)
                })
                .unwrap_or_default();
            if ranks.get(id).copied().unwrap_or_default() < deepest {
                ranks.insert(*id, deepest);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    ranks
}

/// The average horizontal position of whatever feeds a node, or a large
/// number when nothing placed does, so an unfed node sorts after the fed
/// ones rather than before them.
fn barycentre(graph: &Graph, id: NodeId, placed: &BTreeMap<NodeId, f32>) -> f32 {
    let mut total = 0.0;
    let mut count = 0.0;
    for edge in graph.edges().filter(|e| e.to == id) {
        if let Some(x) = placed.get(&edge.from) {
            total += x;
            count += 1.0;
        }
    }
    if count == 0.0 {
        f32::MAX
    } else {
        total / count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::{Command, Engine};
    use solarxy_graph::document::GraphContext;

    fn engine_with(chain: &[&str]) -> (Engine, GraphContext, Vec<NodeId>) {
        let mut engine = Engine::new().expect("registry builds");
        let geo = add(&mut engine, GraphContext::Root, "sopnet");
        let ctx = GraphContext::Subflow(geo);
        let ids: Vec<NodeId> = chain.iter().map(|t| add(&mut engine, ctx, t)).collect();
        (engine, ctx, ids)
    }

    fn add(engine: &mut Engine, ctx: GraphContext, type_id: &str) -> NodeId {
        let before: Vec<NodeId> = engine
            .document()
            .graph(ctx)
            .expect("context")
            .nodes()
            .map(|n| n.id)
            .collect();
        engine
            .apply(Command::AddNode {
                ctx,
                node_type: type_id.to_string(),
                position: [0.0, 0.0],
            })
            .expect("legal here");
        engine
            .document()
            .graph(ctx)
            .expect("context")
            .nodes()
            .map(|n| n.id)
            .find(|id| !before.contains(id))
            .expect("one node added")
    }

    fn wire(engine: &mut Engine, ctx: GraphContext, from: NodeId, to: NodeId, port: &str) {
        engine
            .apply(Command::Connect {
                ctx,
                from: solarxy_graph::engine::PortRefDto {
                    node: from,
                    port: "geometry".to_string(),
                },
                to: solarxy_graph::engine::PortRefDto {
                    node: to,
                    port: port.to_string(),
                },
            })
            .expect("a geometry output feeds a geometry input");
    }

    /// The figure the browser's own layout test pins, so a two-node chain
    /// lands the same distance apart on both shells.
    #[test]
    fn a_chain_stacks_one_rank_apart_in_the_same_column() {
        let (mut engine, ctx, ids) = engine_with(&["box", "subdivide"]);
        wire(&mut engine, ctx, ids[0], ids[1], "geometry");

        let graph = engine.document().graph(ctx).expect("context");
        let placed = layered(graph, engine.registry());
        let at = |id: NodeId| {
            placed
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, p)| *p)
                .expect("every node is placed")
        };

        let (upstream, downstream) = (at(ids[0]), at(ids[1]));
        assert!(
            (upstream[0] - downstream[0]).abs() < 0.01,
            "a chain is one column"
        );
        assert!(
            (downstream[1] - upstream[1] - (NODE_BOX.y + RANK_GAP)).abs() < 0.01,
            "one rank apart, at the browser's spacing"
        );
    }

    /// A node sits below **everything** that feeds it, which is what the
    /// longest path buys over the shortest: with a short rank a merge
    /// would sit beside one of its inputs.
    #[test]
    fn a_node_sits_below_every_node_that_feeds_it() {
        let (mut engine, ctx, ids) = engine_with(&["box", "subdivide", "merge"]);
        wire(&mut engine, ctx, ids[0], ids[2], "inputs");
        wire(&mut engine, ctx, ids[0], ids[1], "geometry");
        wire(&mut engine, ctx, ids[1], ids[2], "inputs");

        let graph = engine.document().graph(ctx).expect("context");
        let placed = layered(graph, engine.registry());
        let at = |id: NodeId| {
            placed
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, p)| p[1])
                .expect("every node is placed")
        };

        assert!(at(ids[2]) > at(ids[1]), "the merge is below the subdivide");
        assert!(
            at(ids[2]) > at(ids[0]),
            "and below the box that also feeds it"
        );
    }

    /// Siblings in a rank do not overlap, since a tidy that stacked two
    /// nodes on one another would be worse than no tidy.
    #[test]
    fn siblings_in_a_rank_are_a_node_and_a_gap_apart() {
        let (engine, ctx, ids) = engine_with(&["box", "sphere", "cone"]);
        let graph = engine.document().graph(ctx).expect("context");
        let placed = layered(graph, engine.registry());

        let mut xs: Vec<f32> = ids
            .iter()
            .map(|id| {
                placed
                    .iter()
                    .find(|(node, _)| node == id)
                    .map(|(_, p)| p[0])
                    .expect("placed")
            })
            .collect();
        xs.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        for window in xs.windows(2) {
            assert!(
                (window[1] - window[0] - (NODE_BOX.x + NODE_GAP)).abs() < 0.01,
                "siblings must be one box and one gap apart"
            );
        }
    }

    /// A node goes under whatever feeds it, not under whatever was added
    /// first.
    ///
    /// **The fixture is built so the two answers disagree.** The sinks are
    /// added in the opposite order to their feeders, so ordering by the
    /// document's own order puts each one under the wrong source and only
    /// the average-position pass puts them right. A determinism test
    /// stood here first and was deleted: this function has no clock, no
    /// randomness and no address-dependent iteration, so it was
    /// deterministic by construction and no mutation could make it fail.
    #[test]
    fn a_node_is_placed_under_what_feeds_it_rather_than_under_its_own_age() {
        let (mut engine, ctx, ids) = engine_with(&["box", "sphere"]);
        let (left, right) = (ids[0], ids[1]);
        // Added in the order that puts the ids against the placement.
        let under_right = add(&mut engine, ctx, "subdivide");
        let under_left = add(&mut engine, ctx, "subdivide");
        wire(&mut engine, ctx, right, under_right, "geometry");
        wire(&mut engine, ctx, left, under_left, "geometry");

        let graph = engine.document().graph(ctx).expect("context");
        let placed = layered(graph, engine.registry());
        let at = |id: NodeId| {
            placed
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, p)| p[0])
                .expect("every node is placed")
        };

        assert!(at(left) < at(right), "the sources sit in document order");
        assert!(
            at(under_left) < at(under_right),
            "each sink must follow its own source rather than its own age"
        );
        assert!(
            (at(under_left) - at(left)).abs() < 0.01,
            "and land directly under it"
        );
    }

    /// A note annotates a graph rather than participating in one, so a
    /// tidy leaves it where its author put it.
    #[test]
    fn a_note_is_left_where_its_author_put_it() {
        let (engine, ctx, ids) = engine_with(&["box", "note"]);
        let graph = engine.document().graph(ctx).expect("context");
        let placed = layered(graph, engine.registry());

        assert!(placed.iter().any(|(id, _)| *id == ids[0]));
        assert!(
            !placed.iter().any(|(id, _)| *id == ids[1]),
            "a note is not moved by a tidy"
        );
    }

    #[test]
    fn an_empty_context_is_left_alone() {
        let (engine, ctx, _) = engine_with(&[]);
        let graph = engine.document().graph(ctx).expect("context");
        assert!(layered(graph, engine.registry()).is_empty());
    }
}
