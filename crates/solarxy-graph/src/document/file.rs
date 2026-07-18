//! The document persistence form: a fully round-tripping serde image of a
//! whole [`Document`] (every context's nodes, edges, display flag, and
//! selection, plus the review annotations and the id mint).
//!
//! This is the autosave substrate. The web host serializes a
//! [`DocumentData`] (wrapped by the engine's `DocumentFile` with the cook
//! mode) to OPFS as JSON; the `.slxy` ZIP reuses these same schema
//! types as its `document.json` entry, adding the asset payloads around it.
//! Topology is not stored: it is re-derived from the edges on load, exactly
//! as [`super::fragment::SubflowFragment`] does.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ContextKind, Document, Edge, Graph, NodeData, NodeId};

/// One graph context's serializable contents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<NodeData>,
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub active_output: Option<NodeId>,
    #[serde(default)]
    pub selection: Vec<NodeId>,
    /// The network kind. `None` in pre-context documents, which had
    /// exactly two shapes: the root (always `Obj`) and geo subflows
    /// (always `Geo`), so the loader resolves `None` per position.
    #[serde(default)]
    pub kind: Option<ContextKind>,
}

/// The whole document as data: the root graph, one entry per subflow (keyed
/// by owning `geo` node id), the review annotations, and the id counter so
/// freshly minted ids after a load never collide with loaded ones.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentData {
    pub root: GraphData,
    pub subflows: Vec<(NodeId, GraphData)>,
    #[serde(default)]
    pub annotations: Vec<crate::review::Annotation>,
    pub next_id: u64,
}

/// Captures one live `Graph` as data.
fn graph_to_data(graph: &Graph) -> GraphData {
    GraphData {
        nodes: graph.nodes().cloned().collect(),
        edges: graph.edges().cloned().collect(),
        active_output: graph.active_output,
        selection: graph.selection.clone(),
        kind: Some(graph.kind),
    }
}

/// Rebuilds a live `Graph` from data (topology re-derived from the edges;
/// a target port is variadic exactly when the restored node carries it in
/// `port_order`, registry-independently). `fallback_kind` resolves a
/// pre-context document's missing kind (root: `Obj`; subflows: `Geo`).
fn graph_from_data(data: &GraphData, fallback_kind: ContextKind) -> Graph {
    let mut graph = Graph::new(data.kind.unwrap_or(fallback_kind));
    for node in &data.nodes {
        graph.add_node(node.clone());
    }
    for edge in &data.edges {
        let variadic = data
            .nodes
            .iter()
            .find(|n| n.id == edge.to)
            .is_some_and(|n| n.port_order.contains_key(&edge.to_port));
        let _ = graph.connect(edge.clone(), variadic);
    }
    graph.active_output = data.active_output;
    graph.selection.clone_from(&data.selection);
    graph
}

impl Document {
    /// Serializable image of the whole document.
    #[must_use]
    pub fn to_data(&self) -> DocumentData {
        DocumentData {
            root: graph_to_data(&self.root),
            subflows: self
                .subflows
                .iter()
                .map(|(owner, g)| (*owner, graph_to_data(g)))
                .collect(),
            annotations: self.review.iter().cloned().collect(),
            next_id: self.next_id,
        }
    }

    /// Rebuilds a whole document from data (the load path). Cook state and
    /// undo are the engine's concern to reset around this.
    #[must_use]
    pub fn from_data(data: &DocumentData) -> Self {
        let mut review = crate::review::ReviewStore::new();
        for annotation in &data.annotations {
            review.insert(annotation.clone());
        }
        let subflows: BTreeMap<NodeId, Graph> = data
            .subflows
            .iter()
            .map(|(owner, gd)| (*owner, graph_from_data(gd, ContextKind::Geo)))
            .collect();
        Self {
            root: graph_from_data(&data.root, ContextKind::Obj),
            subflows,
            review,
            next_id: data.next_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::GraphContext;
    use crate::params::{ParamSource, ParamValue};

    #[test]
    fn document_round_trips_through_data_and_json() {
        // A root geo with a subflow holding two connected nodes, a display
        // flag, params, and a variadic edge order.
        let mut doc = Document::new();
        let geo = doc.mint_node_id();
        doc.create_subflow(geo, ContextKind::Geo);
        doc.graph_mut(GraphContext::Root)
            .unwrap()
            .add_node(NodeData::new(geo, "geo", 1));

        let sub = GraphContext::Subflow(geo);
        let a = doc.mint_node_id();
        let b = doc.mint_node_id();
        let m = doc.mint_node_id();
        {
            let g = doc.graph_mut(sub).unwrap();
            g.add_node(NodeData::new(a, "box", 1));
            g.add_node(NodeData::new(b, "box", 1));
            let mut merge = NodeData::new(m, "merge", 1);
            merge.params.insert(
                "name".into(),
                ParamSource::Literal(ParamValue::Text("mm".into())),
            );
            g.add_node(merge);
            g.active_output = Some(m);
        }
        let e1 = doc.mint_edge_id();
        let e2 = doc.mint_edge_id();
        {
            let g = doc.graph_mut(sub).unwrap();
            g.connect(
                Edge {
                    id: e1,
                    from: a,
                    from_port: "geometry".into(),
                    to: m,
                    to_port: "inputs".into(),
                },
                true,
            )
            .unwrap();
            g.connect(
                Edge {
                    id: e2,
                    from: b,
                    from_port: "geometry".into(),
                    to: m,
                    to_port: "inputs".into(),
                },
                true,
            )
            .unwrap();
        }

        let data = doc.to_data();
        let json = serde_json::to_string(&data).unwrap();
        let back: DocumentData = serde_json::from_str(&json).unwrap();
        let rebuilt = Document::from_data(&back);

        // Root geo preserved.
        assert!(
            rebuilt
                .graph(GraphContext::Root)
                .unwrap()
                .node(geo)
                .is_some()
        );
        // Subflow structure, display flag, params, and variadic order exact.
        let rg = rebuilt.graph(sub).unwrap();
        assert_eq!(rg.node_count(), 3);
        assert_eq!(rg.edge_count(), 2);
        assert_eq!(rg.active_output, Some(m));
        assert_eq!(
            rg.node(m).unwrap().params["name"],
            ParamSource::Literal(ParamValue::Text("mm".into()))
        );
        assert_eq!(rg.node(m).unwrap().port_order["inputs"], vec![e1, e2]);
        // The id mint resumes past every loaded id.
        assert_eq!(rebuilt.to_data().next_id, doc.to_data().next_id);
    }
}
