//! The scene tree: the whole document folded into one outline, rooted at
//! the scene network and recursing into every container's child network.
//!
//! A fold rather than a component, so the shape and the search are one
//! rule both shells read. The desktop had reimplemented this against the
//! browser's version, down to the depth guard and the choice to store
//! collapsed keys rather than expanded ones, which is the concrete pair
//! this crate exists to remove.

use solarxy_graph::document::{ContextKind, Document, GraphContext, NodeData, NodeId};
use solarxy_graph::registry::Registry;

/// A malformed document, one whose container owned a network containing
/// itself, would recurse forever. Real scenes are a few levels deep.
const MAX_DEPTH: usize = 64;

/// One row of the outline.
///
/// `Serialize` because the browser reads this across the WebAssembly
/// boundary; there is no `Deserialize`, because nothing sends one back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeRow {
    /// Stable across renames, so an expansion survives an edit.
    pub key: String,
    /// The context the node **lives in**, which is where a selection
    /// dispatches. Not the network it opens.
    pub ctx: GraphContext,
    pub node: NodeId,
    pub type_id: String,
    pub label: String,
    /// The child-network kind, for a container. `None` means a leaf, and
    /// is the one question an outline asks to decide whether a row can be
    /// opened or dived into.
    pub opens: Option<ContextKind>,
    /// Whether this node carries its own network's display flag.
    pub is_display: bool,
    /// Whether the node is bypassed, which an outline strikes through.
    pub bypassed: bool,
    pub children: Vec<TreeRow>,
    pub depth: usize,
}

fn context_key(ctx: GraphContext) -> String {
    match ctx {
        GraphContext::Root => "root".to_string(),
        GraphContext::Subflow(owner) => format!("sub:{}", owner.0),
    }
}

fn row(
    doc: &Document,
    registry: &Registry,
    ctx: GraphContext,
    data: &NodeData,
    is_display: bool,
    depth: usize,
) -> TreeRow {
    let opens = registry.opens(&data.type_id);
    let children = if opens.is_some() {
        build(doc, registry, GraphContext::Subflow(data.id), depth + 1)
    } else {
        Vec::new()
    };
    TreeRow {
        key: format!("{}:{}", context_key(ctx), data.id.0),
        ctx,
        node: data.id,
        type_id: data.type_id.clone(),
        label: solarxy_graph::naming::node_name(data, registry),
        opens,
        is_display,
        bypassed: data.bypassed,
        children,
        depth,
    }
}

fn build(doc: &Document, registry: &Registry, ctx: GraphContext, depth: usize) -> Vec<TreeRow> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    // A container whose network is not there renders as a leaf rather than
    // vanishing, because a missing network is a defect worth seeing.
    let Ok(graph) = doc.graph(ctx) else {
        return Vec::new();
    };
    let active = graph.active_output;
    graph
        .nodes()
        .map(|data| row(doc, registry, ctx, data, active == Some(data.id), depth))
        .collect()
}

/// Folds the document into the outline, rooted at the scene network.
#[must_use]
pub fn scene_tree(doc: &Document, registry: &Registry) -> Vec<TreeRow> {
    build(doc, registry, GraphContext::Root, 0)
}

/// The keys of every row that has children.
///
/// This is the collapse-all set. **A pane stores collapsed keys, not
/// expanded ones**, so that its empty default reads as a fully expanded
/// tree and a newly added container appears open rather than hidden.
#[must_use]
pub fn branch_keys(rows: &[TreeRow]) -> Vec<String> {
    let mut out = Vec::new();
    for row in rows {
        walk_branches(row, &mut out);
    }
    out
}

fn walk_branches(row: &TreeRow, out: &mut Vec<String>) {
    if row.children.is_empty() {
        return;
    }
    out.push(row.key.clone());
    for child in &row.children {
        walk_branches(child, out);
    }
}

/// What a search turns up: the rows that matched, and the ancestors that
/// have to be forced open for every match to be reachable.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeSearch {
    pub matches: Vec<String>,
    pub expand: Vec<String>,
}

fn walk_search(row: &TreeRow, needle: &str, ancestors: &[String], found: &mut TreeSearch) {
    if row.label.to_lowercase().contains(needle) || row.type_id.to_lowercase().contains(needle) {
        found.matches.push(row.key.clone());
        for ancestor in ancestors {
            if !found.expand.contains(ancestor) {
                found.expand.push(ancestor.clone());
            }
        }
    }
    let mut next = ancestors.to_vec();
    next.push(row.key.clone());
    for child in &row.children {
        walk_search(child, needle, &next, found);
    }
}

/// Case-insensitive substring search over labels and type ids.
///
/// An empty or blank query matches nothing and expands nothing, which
/// leaves the pane on whatever the reader had opened by hand rather than
/// snapping it about.
#[must_use]
pub fn search_tree(rows: &[TreeRow], query: &str) -> TreeSearch {
    let needle = query.trim().to_lowercase();
    let mut found = TreeSearch::default();
    if needle.is_empty() {
        return found;
    }
    for row in rows {
        walk_search(row, &needle, &[], &mut found);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::params::{ParamSource, ParamValue};

    fn registry() -> Registry {
        solarxy_graph::nodes::builtin_registry().expect("builtin registry")
    }

    fn named(id: u64, type_id: &str, name: Option<&str>) -> NodeData {
        let mut node = NodeData::new(NodeId(id), type_id, 1);
        if let Some(name) = name {
            node.params.insert(
                "name".to_string(),
                ParamSource::Literal(ParamValue::Text(name.to_string())),
            );
        }
        node
    }

    fn put(doc: &mut Document, ctx: GraphContext, node: NodeData) {
        doc.graph_mut(ctx)
            .expect("the context exists")
            .add_node(node);
    }

    /// A scene network holding a geometry container, an image container
    /// and a note; the geometry container holds a box and a nested image
    /// container, which holds a box of its own.
    fn document() -> Document {
        let mut doc = Document::new();
        put(
            &mut doc,
            GraphContext::Root,
            named(1, "sopnet", Some("terrain")),
        );
        put(
            &mut doc,
            GraphContext::Root,
            named(2, "copnet", Some("maps")),
        );
        put(&mut doc, GraphContext::Root, named(3, "note", None));

        doc.create_subflow(NodeId(1), ContextKind::Sop);
        put(
            &mut doc,
            GraphContext::Subflow(NodeId(1)),
            named(4, "box", None),
        );
        put(
            &mut doc,
            GraphContext::Subflow(NodeId(1)),
            named(5, "copnet", Some("inner")),
        );
        doc.graph_mut(GraphContext::Subflow(NodeId(1)))
            .expect("the geometry network")
            .active_output = Some(NodeId(4));

        doc.create_subflow(NodeId(5), ContextKind::Cop);
        put(
            &mut doc,
            GraphContext::Subflow(NodeId(5)),
            named(6, "box", Some("deep")),
        );

        // An orphaned network, whose owner is in no graph. Root-down it is
        // unreachable, and it must stay that way.
        doc.create_subflow(NodeId(99), ContextKind::Sop);
        put(
            &mut doc,
            GraphContext::Subflow(NodeId(99)),
            named(7, "box", None),
        );
        doc
    }

    fn collect(row: &TreeRow, out: &mut Vec<String>) {
        out.push(row.key.clone());
        for child in &row.children {
            collect(child, out);
        }
    }

    fn all_keys(rows: &[TreeRow]) -> Vec<String> {
        let mut out = Vec::new();
        for row in rows {
            collect(row, &mut out);
        }
        out
    }

    #[test]
    fn builds_root_down_preserving_order_with_nested_containers() {
        let rows = scene_tree(&document(), &registry());
        assert_eq!(
            rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["terrain", "maps", "Note"]
        );
        assert_eq!(
            rows[0]
                .children
                .iter()
                .map(|r| r.type_id.as_str())
                .collect::<Vec<_>>(),
            ["box", "copnet"]
        );
        assert_eq!(
            rows[0].children[1]
                .children
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            ["deep"]
        );
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[0].children[0].depth, 1);
        // The orphaned network never appears.
        assert!(!all_keys(&rows).iter().any(|k| k.ends_with(":7")));
    }

    #[test]
    fn marks_containers_leaves_and_the_display_flag() {
        let rows = scene_tree(&document(), &registry());
        assert_eq!(rows[0].opens, Some(ContextKind::Sop));
        assert_eq!(rows[2].opens, None);
        let sub = &rows[0].children;
        assert!(sub[0].is_display);
        assert!(!sub[1].is_display);
        // A row's context is the one it lives in, not the one it opens.
        assert_eq!(sub[0].ctx, GraphContext::Subflow(NodeId(1)));
    }

    #[test]
    fn tolerates_a_container_whose_network_is_absent() {
        let mut doc = Document::new();
        put(
            &mut doc,
            GraphContext::Root,
            named(1, "sopnet", Some("hollow")),
        );
        let rows = scene_tree(&doc, &registry());
        assert_eq!(rows[0].opens, Some(ContextKind::Sop));
        assert!(rows[0].children.is_empty());
    }

    #[test]
    fn terminates_on_a_document_whose_container_holds_itself() {
        // Not reachable from the engine: `Document::mint_node_id` is one
        // counter for the whole document, so an id cannot recur in a
        // network beneath itself. It is constructible by hand, which is
        // what this builds, and the guard is what turns a hang into a
        // truncated tree.
        let mut doc = Document::new();
        put(&mut doc, GraphContext::Root, named(1, "sopnet", None));
        doc.create_subflow(NodeId(1), ContextKind::Sop);
        put(
            &mut doc,
            GraphContext::Subflow(NodeId(1)),
            named(1, "sopnet", None),
        );
        let rows = scene_tree(&doc, &registry());
        assert_eq!(rows.len(), 1);
        let mut depth = 0;
        let mut cursor = &rows[0];
        while let Some(child) = cursor.children.first() {
            depth += 1;
            cursor = child;
        }
        assert!(depth < MAX_DEPTH, "the guard cut the walk at {depth}");
    }

    #[test]
    fn an_empty_document_folds_to_nothing() {
        assert!(scene_tree(&Document::new(), &registry()).is_empty());
    }

    #[test]
    fn matches_case_insensitively_over_label_and_type_id() {
        let rows = scene_tree(&document(), &registry());
        assert_eq!(search_tree(&rows, "DEEP").matches, ["sub:5:6"]);
        assert_eq!(search_tree(&rows, "copnet").matches.len(), 2);
    }

    #[test]
    fn returns_the_ancestor_chain_to_expand() {
        let rows = scene_tree(&document(), &registry());
        assert_eq!(search_tree(&rows, "deep").expand, ["root:1", "sub:1:5"]);
    }

    #[test]
    fn an_empty_query_finds_nothing_and_expands_nothing() {
        let rows = scene_tree(&document(), &registry());
        assert_eq!(search_tree(&rows, "   "), TreeSearch::default());
    }

    #[test]
    fn branch_keys_collect_only_rows_with_children_at_every_depth() {
        // The geometry container and the image container nested inside it
        // are branches; the root image container has an empty network, so
        // it is a leaf, as are the boxes and the note.
        let rows = scene_tree(&document(), &registry());
        assert_eq!(branch_keys(&rows), ["root:1", "sub:1:5"]);
    }

    #[test]
    fn branch_keys_are_empty_for_an_empty_tree() {
        assert!(branch_keys(&[]).is_empty());
    }
}
