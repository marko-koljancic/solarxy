//! Graph fragments: a detachable slice of a document (nodes, their edges,
//! and any owned subflows) used by two features that must reinsert graph
//! structure verbatim:
//!
//! - **Undo of a remove**: restore the exact removed nodes and edges under
//!   their **original** ids ([`InsertMode::PreserveIds`]), because variadic
//!   `port_order` references edge ids.
//! - **Clipboard paste**: reinsert with **fresh** ids
//!   ([`InsertMode::Remap`]), so pasted copies do not collide.
//!
//! The two share this type so the paths cannot drift.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::document::{ContextKind, Document, Edge, EdgeId, Graph, GraphContext, NodeData, NodeId};

/// A serializable snapshot of one child network's structure (rebuilt into
/// a live `Graph` on insert). Keyed by owner id in
/// [`GraphFragment::subflows`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubflowFragment {
    pub nodes: Vec<NodeData>,
    pub edges: Vec<Edge>,
    pub active_output: Option<NodeId>,
    /// The captured network's kind. `None` on pre-context clipboard
    /// payloads, which could only ever hold geo subflows.
    #[serde(default)]
    pub kind: Option<ContextKind>,
}

impl SubflowFragment {
    fn from_graph(graph: &Graph) -> Self {
        Self {
            nodes: graph.nodes().cloned().collect(),
            edges: graph.edges().cloned().collect(),
            active_output: graph.active_output,
            kind: Some(graph.kind),
        }
    }

    /// Rebuilds a live `Graph` (topology re-derived from the edges).
    fn to_graph(&self) -> Graph {
        let mut graph = Graph::new(self.kind.unwrap_or(ContextKind::Geo));
        for node in &self.nodes {
            graph.add_node(node.clone());
        }
        for edge in &self.edges {
            let variadic = self
                .nodes
                .iter()
                .find(|n| n.id == edge.to)
                .is_some_and(|n| n.port_order.contains_key(&edge.to_port));
            let _ = graph.connect(edge.clone(), variadic);
        }
        graph.active_output = self.active_output;
        graph
    }
}

/// A self-contained graph slice. Serializable: the clipboard places it on
/// the system clipboard as JSON, and Phase 5's `.slxy` format reuses these
/// same schema types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphFragment {
    /// The captured nodes, full state.
    pub nodes: Vec<NodeData>,
    /// Edges whose endpoints are both inside `nodes` (internal edges are
    /// preserved; boundary edges to outside nodes are dropped on paste and
    /// separately restored on undo).
    pub edges: Vec<Edge>,
    /// Snapshots of subflows owned by captured `geo` nodes, keyed by owner.
    pub subflows: BTreeMap<NodeId, SubflowFragment>,
}

/// How [`GraphFragment::insert_into`] assigns ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertMode {
    /// Reuse the fragment's original ids (undo).
    PreserveIds,
    /// Mint fresh ids, remapping internal edges (paste / duplicate).
    Remap,
}

impl GraphFragment {
    /// Captures a set of nodes from a context, plus the edges internal to
    /// that set and any subflows the captured `geo` nodes own.
    #[must_use]
    pub fn capture(doc: &Document, ctx: GraphContext, ids: &[NodeId]) -> Self {
        let set: std::collections::BTreeSet<NodeId> = ids.iter().copied().collect();
        let Ok(graph) = doc.graph(ctx) else {
            return Self::default();
        };
        let nodes: Vec<NodeData> = ids
            .iter()
            .filter_map(|id| graph.node(*id).cloned())
            .collect();
        let edges: Vec<Edge> = graph
            .edges()
            .filter(|e| set.contains(&e.from) && set.contains(&e.to))
            .cloned()
            .collect();
        // Capture the child network of ANY captured node that owns one,
        // TRANSITIVELY: a nested container's network rides along too.
        // Owner ids are document-unique, so the map stays flat
        // (registry-independent: ownership itself is the container test).
        let mut subflows = BTreeMap::new();
        let mut stack: Vec<NodeId> = nodes.iter().map(|n| n.id).collect();
        while let Some(owner) = stack.pop() {
            if subflows.contains_key(&owner) {
                continue;
            }
            if let Ok(g) = doc.graph(GraphContext::Subflow(owner)) {
                stack.extend(g.nodes().map(|n| n.id));
                subflows.insert(owner, SubflowFragment::from_graph(g));
            }
        }
        Self {
            nodes,
            edges,
            subflows,
        }
    }

    /// Inserts this fragment into `ctx`, returning the ids of the inserted
    /// nodes (in fragment order). `PreserveIds` reuses ids verbatim;
    /// `Remap` mints fresh node and edge ids from the document and rewires
    /// internal edges. Positions may be offset by the caller afterward.
    ///
    /// Skips nodes whose type is illegal in the target context, reporting
    /// them; boundary edges (to nodes not in the fragment) are only
    /// restorable in `PreserveIds` mode and are the caller's concern.
    ///
    /// `opens` supplies the registry's container knowledge (which node
    /// types own a child network, and of what kind) without this module
    /// depending on the registry: a container pasted WITHOUT a captured
    /// child network still gets a fresh empty one of the right kind.
    pub fn insert_into(
        &self,
        doc: &mut Document,
        ctx: GraphContext,
        mode: InsertMode,
        context_ok: &dyn Fn(&str) -> bool,
        opens: &dyn Fn(&str) -> Option<ContextKind>,
    ) -> InsertResult {
        let mut node_map: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut inserted = Vec::new();
        let mut skipped = Vec::new();

        // (original node id, port) pairs that are variadic, recovered from
        // the captured nodes' own `port_order` (registry-independent: a
        // captured target always carries its port_order).
        let variadic: std::collections::BTreeSet<(NodeId, &str)> = self
            .nodes
            .iter()
            .flat_map(|n| n.port_order.keys().map(move |p| (n.id, p.as_str())))
            .collect();

        for node in &self.nodes {
            if !context_ok(&node.type_id) {
                skipped.push((node.id, node.type_id.clone()));
                continue;
            }
            let new_id = match mode {
                InsertMode::PreserveIds => node.id,
                InsertMode::Remap => doc.mint_node_id(),
            };
            node_map.insert(node.id, new_id);
            let mut clone = node.clone();
            clone.id = new_id;
            // Port order references edge ids; on remap it is rebuilt as
            // edges are re-added, so clear it here.
            if mode == InsertMode::Remap {
                clone.port_order.clear();
            }
            // Restore an owned child network (container nodes). Remap of a
            // subflow's inner ids is a Phase-4+ concern; on remap the paste
            // currently reuses inner ids, which is correct for undo
            // (PreserveIds) and acceptable for a single paste (the ids stay
            // document-unique because they were minted before). A future
            // refinement remaps inner ids too.
            if let Some(subflow) = self.subflows.get(&node.id) {
                doc.restore_subflow(new_id, subflow.to_graph());
                // Restore nested networks transitively. Inner node ids are
                // preserved in both modes (remap keeps inner ids, see the
                // note above), so the flat owner-keyed map still matches.
                let mut stack: Vec<NodeId> = subflow.nodes.iter().map(|n| n.id).collect();
                while let Some(inner) = stack.pop() {
                    if let Some(nested) = self.subflows.get(&inner) {
                        stack.extend(nested.nodes.iter().map(|n| n.id));
                        doc.restore_subflow(inner, nested.to_graph());
                    }
                }
            } else if let Some(kind) = opens(&node.type_id) {
                doc.create_subflow(new_id, kind);
            }
            if let Ok(graph) = doc.graph_mut(ctx) {
                graph.add_node(clone);
                inserted.push(new_id);
            }
        }

        // Re-add internal edges (both endpoints mapped).
        let mut edge_map: BTreeMap<EdgeId, EdgeId> = BTreeMap::new();
        for edge in &self.edges {
            let (Some(&from), Some(&to)) = (node_map.get(&edge.from), node_map.get(&edge.to))
            else {
                continue;
            };
            let new_edge_id = match mode {
                InsertMode::PreserveIds => edge.id,
                InsertMode::Remap => doc.mint_edge_id(),
            };
            edge_map.insert(edge.id, new_edge_id);
            let to_variadic = variadic.contains(&(edge.to, edge.to_port.as_str()));
            if let Ok(graph) = doc.graph_mut(ctx) {
                let _ = graph.connect(
                    Edge {
                        id: new_edge_id,
                        from,
                        from_port: edge.from_port.clone(),
                        to,
                        to_port: edge.to_port.clone(),
                    },
                    to_variadic,
                );
            }
        }

        // On PreserveIds the captured port_order already holds the right
        // edge ids; on Remap connect() rebuilt it in edge iteration order.
        // For PreserveIds, re-apply the captured port_order so variadic
        // ordering is exact (connect appends, which may differ).
        if mode == InsertMode::PreserveIds {
            for node in &self.nodes {
                if let Some(&new_id) = node_map.get(&node.id) {
                    for (port, order) in &node.port_order {
                        if let Ok(graph) = doc.graph_mut(ctx)
                            && let Some(n) = graph.node_mut(new_id)
                        {
                            n.port_order.insert(port.clone(), order.clone());
                        }
                    }
                }
            }
        }

        InsertResult {
            node_map,
            edge_map,
            inserted,
            skipped,
        }
    }
}

/// The id mapping produced by an insert (empty-remap for `PreserveIds`).
#[derive(Debug, Clone, Default)]
pub struct InsertResult {
    pub node_map: BTreeMap<NodeId, NodeId>,
    pub edge_map: BTreeMap<EdgeId, EdgeId>,
    pub inserted: Vec<NodeId>,
    pub skipped: Vec<(NodeId, String)>,
}
