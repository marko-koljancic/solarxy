//! The document model: nodes, edges, flat params, subflow containers, and
//! per-context graphs.
//!
//! A document is one root graph plus one subflow graph per `geo` container
//! node, selected by [`GraphContext`]. Each [`Graph`] owns its nodes,
//! edges, variadic port order, active display output, selection, and its
//! [`Topology`] together, kept in lockstep by construction (Minimystix
//! split these across a store and a graph adapter and hand-synced them;
//! that design is deliberately not replicated).
//!
//! Ids are minted from one document-wide monotonic counter, so they are
//! unique across contexts, deterministic (no clock, no randomness: safe
//! for wasm and for replay-style tests), and map 1:1 onto
//! `solarxy_core::scene::SceneObjectId` for displayed nodes.
//!
//! Structural legality enforced here: cycles, single-arity occupancy,
//! variadic `port_order` integrity. Type legality (port existence, the
//! coercion matrix, context masks) is the registry-aware engine layer's
//! job before it calls into this module.

use std::collections::{BTreeMap, BTreeSet};

use crate::GraphError;
use crate::params::ParamSource;
use crate::topology::Topology;

mod file;
mod fragment;

pub use file::{DocumentData, GraphData};
pub use fragment::{GraphFragment, InsertMode, InsertResult, SubflowFragment};

/// Stable identity of one node instance, unique document-wide.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct NodeId(pub u64);

/// Stable identity of one edge, unique document-wide. Variadic
/// `port_order` lists reference these, which is why undo must restore
/// removed edges under their **original** ids.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct EdgeId(pub u64);

/// The network kind a graph is: the vocabulary of typed contexts. The
/// root graph is always [`ContextKind::Obj`]; a child network's kind is
/// whatever its owning container's descriptor `opens` (`geo` opens `Geo`,
/// `matnet` opens `Mat`, `texnet` opens `Tex`). Node placement legality is
/// judged against this kind via the descriptor's `ContextSet`, and adding
/// a kind here plus a container descriptor is the whole cost of a new
/// context. This generalizes the older root/subflow pair, which could only
/// ever describe two kinds.
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    /// The scene/object network (the root canvas): containers, lights,
    /// cameras, render configuration. Nodes placeable here are portless
    /// (the generalized MVP invariant).
    #[default]
    Obj,
    /// A geometry network (a `geo` container's canvas).
    Geo,
    /// A material network (a `matnet` container's canvas).
    Mat,
    /// A texture/image network (a `texnet` container's canvas).
    Tex,
}

impl ContextKind {
    /// Every kind, in declaration order (snapshot and UI vocabularies).
    pub const ALL: [ContextKind; 4] = [
        ContextKind::Obj,
        ContextKind::Geo,
        ContextKind::Mat,
        ContextKind::Tex,
    ];
}

/// Which graph a command or event targets. The serde form is the
/// wasm-boundary shape: `"root"` or `{ "subflow": <nodeId> }`. A
/// "subflow" is any child network regardless of its [`ContextKind`]; the
/// kind lives on the [`Graph`] itself, not in the address.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum GraphContext {
    Root,
    /// The child network owned by this container node.
    Subflow(NodeId),
}

/// One end of a prospective connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortRef {
    pub node: NodeId,
    /// The port key (simultaneously the canvas handle id and the compute
    /// body's input key).
    pub port: String,
}

/// One typed connection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub from: NodeId,
    pub from_port: String,
    pub to: NodeId,
    pub to_port: String,
}

/// One node instance. `name` and `description` are ordinary params in the
/// implicit `general` group, not struct fields; params are stored **flat**
/// by key (the schema-v1 shape; `group` is presentation metadata on the
/// spec, never a storage level).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeData {
    pub id: NodeId,
    pub type_id: String,
    /// The descriptor version this instance was created (or migrated) at.
    pub type_version: u32,
    pub params: BTreeMap<String, ParamSource>,
    /// Canvas position (presentation state, undoable, never cook-relevant).
    pub position: [f32; 2],
    pub bypassed: bool,
    /// Explicit edge order per **variadic** input port:
    /// reordering is a single list rewrite, no renumbering churn. Managed
    /// exclusively by [`Graph::connect`]/[`Graph::disconnect`]/
    /// [`Graph::reorder_variadic`].
    pub port_order: BTreeMap<String, Vec<EdgeId>>,
    /// When `Some`, this node loaded as a non-cooking placeholder (a
    /// too-new `type_version` or an unknown `type_id`): its params and
    /// edges are preserved verbatim, it shows this reason as an error
    /// badge, and it refuses to cook so the document is never destroyed.
    #[serde(default)]
    pub placeholder: Option<String>,
    /// Unix milliseconds when the node was created, and when its behaviour
    /// last changed. `None` on any node saved before 0.8.1 and on any node
    /// created while the host had installed no wall clock (native cooks,
    /// tests): the info card says "unknown" rather than inventing a time.
    ///
    /// `modified` deliberately ignores canvas position. Tidying a graph, or
    /// running auto-layout, would otherwise restamp every node at once and
    /// leave "last modified" answering a question nobody asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<f64>,
}

impl NodeData {
    /// A bare instance; the engine layer fills params from the descriptor
    /// defaults.
    #[must_use]
    pub fn new(id: NodeId, type_id: impl Into<String>, type_version: u32) -> Self {
        Self {
            id,
            type_id: type_id.into(),
            type_version,
            params: BTreeMap::new(),
            position: [0.0, 0.0],
            bypassed: false,
            port_order: BTreeMap::new(),
            placeholder: None,
            created_ms: None,
            modified_ms: None,
        }
    }
}

/// One graph context: the root canvas or one child network.
#[derive(Debug, Default, Clone)]
pub struct Graph {
    nodes: BTreeMap<NodeId, NodeData>,
    edges: BTreeMap<EdgeId, Edge>,
    topology: Topology,
    /// The display node (child networks: exactly one node holds the
    /// display flag; root: unused, root visibility is additive per geo
    /// node).
    pub active_output: Option<NodeId>,
    pub selection: Vec<NodeId>,
    /// The network kind (root: `Obj`; a child network: whatever its
    /// owning container's descriptor opens). Placement legality and the
    /// palette filter judge against this.
    pub kind: ContextKind,
}

impl Graph {
    #[must_use]
    pub fn new(kind: ContextKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&NodeData> {
        self.nodes.get(&id)
    }

    /// Mutable node access for params/position/bypass. `port_order` is
    /// graph-managed; mutate it only through connect/disconnect/reorder.
    #[must_use]
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut NodeData> {
        self.nodes.get_mut(&id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &NodeData> {
        self.nodes.values()
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(&id)
    }

    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn add_node(&mut self, node: NodeData) {
        debug_assert!(
            !self.nodes.contains_key(&node.id),
            "duplicate node id {:?}",
            node.id
        );
        self.topology.add_node(node.id);
        self.nodes.insert(node.id, node);
    }

    /// Removes a node with every incident edge (cleaning `port_order` on
    /// surviving neighbors). Returns the removed state for undo.
    pub fn remove_node(&mut self, id: NodeId) -> Result<(NodeData, Vec<Edge>), GraphError> {
        if !self.nodes.contains_key(&id) {
            return Err(GraphError::UnknownNode(id));
        }
        let incident: Vec<EdgeId> = self
            .edges
            .values()
            .filter(|e| e.from == id || e.to == id)
            .map(|e| e.id)
            .collect();
        let mut removed_edges = Vec::with_capacity(incident.len());
        for edge_id in incident {
            // Cannot fail: the id was just collected.
            if let Ok(edge) = self.disconnect(edge_id) {
                removed_edges.push(edge);
            }
        }
        self.topology.remove_node(id);
        let node = self.nodes.remove(&id).expect("checked contains_key above");
        if self.active_output == Some(id) {
            self.active_output = None;
        }
        self.selection.retain(|&s| s != id);
        Ok((node, removed_edges))
    }

    /// Inserts a fully-specified edge (the caller minted the id and
    /// resolved `to_variadic` from the descriptor). Enforces the
    /// structural rules: existing endpoints, acyclicity, and single-arity
    /// occupancy. On error the graph is untouched.
    pub fn connect(&mut self, edge: Edge, to_variadic: bool) -> Result<(), GraphError> {
        if !self.nodes.contains_key(&edge.from) {
            return Err(GraphError::UnknownNode(edge.from));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(GraphError::UnknownNode(edge.to));
        }
        if self.topology.would_create_cycle(edge.from, edge.to) {
            return Err(GraphError::CycleDetected);
        }
        if !to_variadic
            && self
                .edges
                .values()
                .any(|e| e.to == edge.to && e.to_port == edge.to_port)
        {
            return Err(GraphError::PortOccupied {
                node: edge.to,
                port: edge.to_port.clone(),
            });
        }

        self.topology.add_edge(edge.from, edge.to);
        if to_variadic && let Some(node) = self.nodes.get_mut(&edge.to) {
            let order = node.port_order.entry(edge.to_port.clone()).or_default();
            // Idempotent: a restored node may already carry this edge id in
            // its captured order (undo re-adds an edge whose id is already
            // listed), so do not double-count it.
            if !order.contains(&edge.id) {
                order.push(edge.id);
            }
        }
        self.edges.insert(edge.id, edge);
        Ok(())
    }

    /// Re-adds an edge at a specific position in its target's variadic
    /// `port_order` (the undo of a disconnect).
    ///
    /// [`Graph::connect`] appends, which is right for a new wire and wrong for
    /// a restored one: putting the wire back at the end silently reorders a
    /// variadic port, changing what `merge` concatenates and which branch
    /// `switch` selects by index. `slot` is ignored for a non-variadic target
    /// and clamped if the order has since shrunk.
    ///
    /// # Errors
    /// The same conditions as [`Graph::connect`].
    pub fn connect_at(
        &mut self,
        edge: Edge,
        to_variadic: bool,
        slot: Option<usize>,
    ) -> Result<(), GraphError> {
        let (id, to, to_port) = (edge.id, edge.to, edge.to_port.clone());
        self.connect(edge, to_variadic)?;

        if !to_variadic {
            return Ok(());
        }
        let Some(slot) = slot else { return Ok(()) };
        let Some(order) = self
            .nodes
            .get_mut(&to)
            .and_then(|n| n.port_order.get_mut(&to_port))
        else {
            return Ok(());
        };
        // `connect` just pushed it on the end; move it back to where it was.
        let Some(cur) = order.iter().position(|&e| e == id) else {
            return Ok(());
        };
        let target = slot.min(order.len() - 1);
        if cur != target {
            let e = order.remove(cur);
            order.insert(target, e);
        }
        Ok(())
    }

    /// Removes an edge, cleaning the target's `port_order`. Returns the
    /// removed edge for undo (which must reinsert it under the same id).
    pub fn disconnect(&mut self, id: EdgeId) -> Result<Edge, GraphError> {
        let edge = self.edges.remove(&id).ok_or(GraphError::UnknownEdge(id))?;
        self.topology.remove_edge(edge.from, edge.to);
        if let Some(node) = self.nodes.get_mut(&edge.to)
            && let Some(order) = node.port_order.get_mut(&edge.to_port)
        {
            order.retain(|&e| e != id);
            if order.is_empty() {
                node.port_order.remove(&edge.to_port);
            }
        }
        Ok(edge)
    }

    /// Rewrites a variadic port's edge order. `order` must be a
    /// permutation of the current list. Returns the previous order for
    /// undo.
    pub fn reorder_variadic(
        &mut self,
        node_id: NodeId,
        port: &str,
        order: Vec<EdgeId>,
    ) -> Result<Vec<EdgeId>, GraphError> {
        let node = self
            .nodes
            .get_mut(&node_id)
            .ok_or(GraphError::UnknownNode(node_id))?;
        let current = node
            .port_order
            .get_mut(port)
            .ok_or_else(|| GraphError::NotVariadic {
                node: node_id,
                port: port.to_string(),
            })?;
        let as_set: BTreeSet<EdgeId> = order.iter().copied().collect();
        let current_set: BTreeSet<EdgeId> = current.iter().copied().collect();
        if as_set != current_set || order.len() != current.len() {
            return Err(GraphError::InvalidReorder {
                node: node_id,
                port: port.to_string(),
            });
        }
        Ok(std::mem::replace(current, order))
    }

    /// Edges feeding one input port. For a variadic port the result
    /// follows `port_order` (the gather order); a single port yields at
    /// most one edge.
    #[must_use]
    pub fn incoming_to_port(&self, node: NodeId, port: &str) -> Vec<&Edge> {
        if let Some(order) = self.nodes.get(&node).and_then(|n| n.port_order.get(port)) {
            return order.iter().filter_map(|id| self.edges.get(id)).collect();
        }
        self.edges
            .values()
            .filter(|e| e.to == node && e.to_port == port)
            .collect()
    }

    /// All edges feeding a node (unordered across ports).
    pub fn incoming(&self, node: NodeId) -> impl Iterator<Item = &Edge> {
        self.edges.values().filter(move |e| e.to == node)
    }

    // Topology forwarding (keeps `Topology` mutation crate-private).

    #[must_use]
    pub fn would_create_cycle(&self, from: NodeId, to: NodeId) -> bool {
        self.topology.would_create_cycle(from, to)
    }

    pub fn topological_order(&mut self) -> &[NodeId] {
        self.topology.topological_order()
    }

    pub fn topological_filter(&mut self, subset: &BTreeSet<NodeId>) -> Vec<NodeId> {
        self.topology.topological_filter(subset)
    }

    #[must_use]
    pub fn predecessor_cone(&self, target: NodeId) -> BTreeSet<NodeId> {
        self.topology.predecessor_cone(target)
    }

    #[must_use]
    pub fn downstream(&self, start: NodeId) -> BTreeSet<NodeId> {
        self.topology.downstream(start)
    }
}

/// The whole document: the root graph (kind `Obj`) plus one child network
/// per container node (each stamped with the kind its owner opens), the
/// review annotations, and the id mint.
#[derive(Debug, Default, Clone)]
pub struct Document {
    root: Graph,
    subflows: BTreeMap<NodeId, Graph>,
    review: crate::review::ReviewStore,
    next_id: u64,
}

impl Document {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mint_node_id(&mut self) -> NodeId {
        self.next_id += 1;
        NodeId(self.next_id)
    }

    pub fn mint_edge_id(&mut self) -> EdgeId {
        self.next_id += 1;
        EdgeId(self.next_id)
    }

    pub fn mint_annotation_id(&mut self) -> crate::review::AnnotationId {
        self.next_id += 1;
        crate::review::AnnotationId(self.next_id)
    }

    #[must_use]
    pub fn review(&self) -> &crate::review::ReviewStore {
        &self.review
    }

    pub fn review_mut(&mut self) -> &mut crate::review::ReviewStore {
        &mut self.review
    }

    /// The set of staged asset ids referenced by an `AssetRef` param on any
    /// node across every context (root + all subflows). This is the
    /// reachable set the `.slxy` writer embeds, so staged bytes no longer
    /// referenced by any node are dropped at save time (GC-at-save).
    #[must_use]
    pub fn referenced_assets(&self) -> std::collections::BTreeSet<crate::params::AssetId> {
        use crate::params::{ParamSource, ParamValue};
        let mut ids = std::collections::BTreeSet::new();
        for graph in std::iter::once(&self.root).chain(self.subflows.values()) {
            for node in graph.nodes() {
                for src in node.params.values() {
                    if let ParamSource::Literal(ParamValue::Asset(id)) = src
                        && !id.0.is_empty()
                    {
                        ids.insert(id.clone());
                    }
                }
            }
        }
        ids
    }

    pub fn graph(&self, ctx: GraphContext) -> Result<&Graph, GraphError> {
        match ctx {
            GraphContext::Root => Ok(&self.root),
            GraphContext::Subflow(geo) => self.subflows.get(&geo).ok_or(GraphError::UnknownContext),
        }
    }

    pub fn graph_mut(&mut self, ctx: GraphContext) -> Result<&mut Graph, GraphError> {
        match ctx {
            GraphContext::Root => Ok(&mut self.root),
            GraphContext::Subflow(geo) => self
                .subflows
                .get_mut(&geo)
                .ok_or(GraphError::UnknownContext),
        }
    }

    /// Creates the (empty) child network for a newly added container node,
    /// stamped with the kind the container's descriptor opens.
    pub fn create_subflow(&mut self, owner: NodeId, kind: ContextKind) {
        self.subflows
            .entry(owner)
            .or_insert_with(|| Graph::new(kind));
    }

    /// Detaches a container node's child network (returned whole for undo).
    pub fn remove_subflow(&mut self, owner: NodeId) -> Option<Graph> {
        self.subflows.remove(&owner)
    }

    /// Reattaches a child network (the undo path of
    /// [`Self::remove_subflow`]).
    pub fn restore_subflow(&mut self, owner: NodeId, graph: Graph) {
        self.subflows.insert(owner, graph);
    }

    /// The container node ids that own child networks, in id order.
    pub fn subflow_owners(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.subflows.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(g: &mut Graph, id: u64) -> NodeId {
        let nid = NodeId(id);
        g.add_node(NodeData::new(nid, "test", 1));
        nid
    }

    fn edge(id: u64, from: NodeId, to: NodeId, to_port: &str) -> Edge {
        Edge {
            id: EdgeId(id),
            from,
            from_port: "geometry".to_string(),
            to,
            to_port: to_port.to_string(),
        }
    }

    #[test]
    fn single_arity_occupancy_is_enforced() {
        let mut g = Graph::new(ContextKind::Geo);
        let a = node(&mut g, 1);
        let b = node(&mut g, 2);
        let c = node(&mut g, 3);
        g.connect(edge(10, a, c, "geometry"), false).unwrap();
        let err = g.connect(edge(11, b, c, "geometry"), false).unwrap_err();
        assert!(matches!(err, GraphError::PortOccupied { .. }));
        assert_eq!(g.edge_count(), 1);
        // A different port on the same node is fine.
        g.connect(edge(12, b, c, "other"), false).unwrap();
    }

    #[test]
    fn variadic_connect_appends_port_order() {
        let mut g = Graph::new(ContextKind::Geo);
        let a = node(&mut g, 1);
        let b = node(&mut g, 2);
        let m = node(&mut g, 3);
        g.connect(edge(10, a, m, "inputs"), true).unwrap();
        g.connect(edge(11, b, m, "inputs"), true).unwrap();
        let order = &g.node(m).unwrap().port_order["inputs"];
        assert_eq!(order, &vec![EdgeId(10), EdgeId(11)]);
        // Gather order follows port_order.
        let incoming = g.incoming_to_port(m, "inputs");
        assert_eq!(incoming[0].id, EdgeId(10));
        assert_eq!(incoming[1].id, EdgeId(11));
    }

    #[test]
    fn cycle_refused_and_graph_intact() {
        let mut g = Graph::new(ContextKind::Geo);
        let a = node(&mut g, 1);
        let b = node(&mut g, 2);
        g.connect(edge(10, a, b, "geometry"), false).unwrap();
        let err = g.connect(edge(11, b, a, "geometry"), false).unwrap_err();
        assert!(matches!(err, GraphError::CycleDetected));
        assert_eq!(g.edge_count(), 1);
        assert!(g.node(a).unwrap().port_order.is_empty());
        // Self-edge refused too.
        let err = g.connect(edge(12, a, a, "x"), false).unwrap_err();
        assert!(matches!(err, GraphError::CycleDetected));
    }

    #[test]
    fn disconnect_cleans_port_order() {
        let mut g = Graph::new(ContextKind::Geo);
        let a = node(&mut g, 1);
        let b = node(&mut g, 2);
        let m = node(&mut g, 3);
        g.connect(edge(10, a, m, "inputs"), true).unwrap();
        g.connect(edge(11, b, m, "inputs"), true).unwrap();
        let removed = g.disconnect(EdgeId(10)).unwrap();
        assert_eq!(removed.from, a);
        assert_eq!(g.node(m).unwrap().port_order["inputs"], vec![EdgeId(11)]);
        // Removing the last clears the entry entirely.
        g.disconnect(EdgeId(11)).unwrap();
        assert!(g.node(m).unwrap().port_order.is_empty());
    }

    #[test]
    fn reorder_requires_a_permutation_and_returns_previous() {
        let mut g = Graph::new(ContextKind::Geo);
        let a = node(&mut g, 1);
        let b = node(&mut g, 2);
        let m = node(&mut g, 3);
        g.connect(edge(10, a, m, "inputs"), true).unwrap();
        g.connect(edge(11, b, m, "inputs"), true).unwrap();

        let err = g
            .reorder_variadic(m, "inputs", vec![EdgeId(10)])
            .unwrap_err();
        assert!(matches!(err, GraphError::InvalidReorder { .. }));
        let err = g
            .reorder_variadic(m, "inputs", vec![EdgeId(10), EdgeId(99)])
            .unwrap_err();
        assert!(matches!(err, GraphError::InvalidReorder { .. }));

        let prev = g
            .reorder_variadic(m, "inputs", vec![EdgeId(11), EdgeId(10)])
            .unwrap();
        assert_eq!(prev, vec![EdgeId(10), EdgeId(11)]);
        let incoming = g.incoming_to_port(m, "inputs");
        assert_eq!(incoming[0].id, EdgeId(11));

        // Not a variadic port on this node.
        let err = g.reorder_variadic(a, "inputs", vec![]).unwrap_err();
        assert!(matches!(err, GraphError::NotVariadic { .. }));
    }

    #[test]
    fn remove_node_returns_state_and_cleans_neighbors() {
        let mut g = Graph::new(ContextKind::Geo);
        let a = node(&mut g, 1);
        let b = node(&mut g, 2);
        let m = node(&mut g, 3);
        g.connect(edge(10, a, m, "inputs"), true).unwrap();
        g.connect(edge(11, b, m, "inputs"), true).unwrap();
        g.connect(edge(12, a, b, "geometry"), false).unwrap();
        g.active_output = Some(a);
        g.selection = vec![a, m];

        let (removed, edges) = g.remove_node(a).unwrap();
        assert_eq!(removed.id, a);
        // Both incident edges (to m and to b) came back for undo.
        let mut ids: Vec<EdgeId> = edges.iter().map(|e| e.id).collect();
        ids.sort();
        assert_eq!(ids, vec![EdgeId(10), EdgeId(12)]);
        // The surviving variadic order dropped a's edge.
        assert_eq!(g.node(m).unwrap().port_order["inputs"], vec![EdgeId(11)]);
        // Display/selection references cleared.
        assert_eq!(g.active_output, None);
        assert_eq!(g.selection, vec![m]);
        // b still feeds m, but a is gone from b's upstream cone.
        assert_eq!(g.downstream(b), BTreeSet::from([m]));
        assert_eq!(g.predecessor_cone(b), BTreeSet::from([b]));
    }

    #[test]
    fn document_mints_unique_ids_and_routes_contexts() {
        let mut doc = Document::new();
        let n1 = doc.mint_node_id();
        let e1 = doc.mint_edge_id();
        let n2 = doc.mint_node_id();
        assert_ne!(n1.0, n2.0);
        assert_ne!(n1.0, e1.0);

        assert!(doc.graph(GraphContext::Root).is_ok());
        let geo = n1;
        assert!(matches!(
            doc.graph(GraphContext::Subflow(geo)),
            Err(GraphError::UnknownContext)
        ));
        doc.create_subflow(geo, ContextKind::Geo);
        assert!(doc.graph(GraphContext::Subflow(geo)).is_ok());

        let sub = doc.remove_subflow(geo).unwrap();
        assert!(doc.graph(GraphContext::Subflow(geo)).is_err());
        doc.restore_subflow(geo, sub);
        assert!(doc.graph(GraphContext::Subflow(geo)).is_ok());
        assert_eq!(doc.subflow_owners().collect::<Vec<_>>(), vec![geo]);
    }
}
