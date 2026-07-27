//! Node naming: unique, auto-numbered, and resolvable by path.
//!
//! Before 0.8.1 `name` was an ordinary Text param whose default was the
//! descriptor's display name, so every sphere in a document answered to
//! "Sphere" and nothing could reference one unambiguously. Expressions
//! need a name to resolve against (`ch("../sphere1/radius")`), so a node
//! now mints a graph-unique `<type_id><n>` when it is created, and a
//! rename that would collide is suffixed rather than refused.
//!
//! Two deliberate scoping choices:
//!
//! - **Uniqueness is per graph, not per document.** Two networks may each
//!   hold a `sphere1`, exactly as two directories may each hold a
//!   `readme`. Paths are resolved relative to a context, so that is the
//!   only scope that has to be unambiguous.
//! - **Existing documents are never rewritten.** A pre-0.8.1 file keeps
//!   whatever it saved, duplicates included; an ambiguous `ch()` against
//!   it is a cook error naming the collision, and the user renames. The
//!   alternative, silently editing titles the user chose, is worse than
//!   an error message.
//!
//! The name a node answers to is not simply its stored param: an unset or
//! blank `name` falls back to the descriptor's display name. That rule
//! lived only in `web/src/flow/nodeLabel.ts` before this module existed,
//! which is why nothing Rust-side could resolve a path.

use crate::document::{Graph, NodeData, NodeId};
use crate::params::{ParamSource, ParamValue};
use crate::registry::Registry;

/// The name a node answers to: its stored `name` when that is a non-empty
/// literal, else the descriptor's display name.
///
/// Mirrors `nodeLabel` in `web/src/flow/nodeLabel.ts`. An expression-valued
/// `name` falls back to the display name, matching the frontend, though
/// `SetParam` refuses one on a Text param.
#[must_use]
pub fn node_name(node: &NodeData, registry: &Registry) -> String {
    if let Some(ParamSource::Literal(ParamValue::Text(text))) = node.params.get("name") {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    registry
        .get(&node.type_id)
        .map_or_else(|| node.type_id.clone(), |d| d.display_name.to_string())
}

/// Every name in use in one graph, optionally ignoring a single node (the
/// one being renamed, which must not collide with itself).
fn names_in_use(graph: &Graph, registry: &Registry, exclude: Option<NodeId>) -> Vec<String> {
    graph
        .nodes()
        .filter(|n| Some(n.id) != exclude)
        .map(|n| node_name(n, registry))
        .collect()
}

/// A graph-unique name for a new node of this type: `<type_id><n>` with
/// the smallest `n >= 1` that is free.
///
/// Gap-filling rather than monotonic (deleting `box2` of three lets the
/// next box reclaim the name), which matches how the numbering reads to a
/// user and keeps names short in a long editing session.
#[must_use]
pub fn mint_name(graph: &Graph, registry: &Registry, type_id: &str) -> String {
    let taken = names_in_use(graph, registry, None);
    for n in 1.. {
        let candidate = format!("{type_id}{n}");
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
    }
    // `1..` is unbounded; the loop always returns.
    unreachable!()
}

/// `desired` if it is free in this graph, else `desired` with the smallest
/// numeric suffix that is.
///
/// Used by the rename path, so a collision is resolved silently instead of
/// rejecting the edit. `exclude` is the node being renamed: a node may
/// always keep the name it already has.
///
/// A blank `desired` is not a name at all (it resolves to the display
/// name), so it is returned untouched and the node falls back.
#[must_use]
pub fn uniquify(graph: &Graph, registry: &Registry, desired: &str, exclude: NodeId) -> String {
    let trimmed = desired.trim();
    if trimmed.is_empty() {
        return desired.to_string();
    }
    let taken = names_in_use(graph, registry, Some(exclude));
    if !taken.iter().any(|t| t == trimmed) {
        return trimmed.to_string();
    }
    // Reuse any trailing digits as the starting point, so renaming a
    // second node to "body2" when "body2" is taken yields "body3" rather
    // than "body22".
    let stem = trimmed.trim_end_matches(|c: char| c.is_ascii_digit());
    let stem = if stem.is_empty() { trimmed } else { stem };
    for n in 2.. {
        let candidate = format!("{stem}{n}");
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_registry;
    use crate::document::{ContextKind, Graph, NodeData, NodeId};

    fn graph_with(names: &[(&str, Option<&str>)]) -> Graph {
        let mut g = Graph::new(ContextKind::Geo);
        for (i, (type_id, name)) in names.iter().enumerate() {
            let mut node = NodeData::new(NodeId(i as u64 + 1), *type_id, 1);
            if let Some(n) = name {
                node.params.insert(
                    "name".to_string(),
                    ParamSource::Literal(ParamValue::Text((*n).to_string())),
                );
            }
            g.add_node(node);
        }
        g
    }

    #[test]
    fn an_unset_name_falls_back_to_the_display_name() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", None)]);
        let node = g.nodes().next().unwrap();
        assert_eq!(node_name(node, &reg), "Box");
    }

    #[test]
    fn a_blank_name_falls_back_too() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("   "))]);
        let node = g.nodes().next().unwrap();
        assert_eq!(node_name(node, &reg), "Box");
    }

    #[test]
    fn minting_starts_at_one_and_fills_gaps() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("box1")), ("box", Some("box3"))]);
        // box2 is free, so it is reclaimed rather than jumping to box3.
        assert_eq!(mint_name(&g, &reg, "box"), "box2");
    }

    #[test]
    fn minting_ignores_names_of_other_types() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("box1")), ("sphere", Some("sphere1"))]);
        assert_eq!(mint_name(&g, &reg, "sphere"), "sphere2");
        assert_eq!(mint_name(&g, &reg, "transform"), "transform1");
    }

    #[test]
    fn minting_avoids_a_display_name_default() {
        let reg = builtin_registry().unwrap();
        // An unnamed box answers to "Box", which does not collide with the
        // lowercase minted form, so the first mint is still box1.
        let g = graph_with(&[("box", None)]);
        assert_eq!(mint_name(&g, &reg, "box"), "box1");
    }

    #[test]
    fn a_rename_to_a_free_name_is_untouched() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("box1")), ("box", Some("box2"))]);
        assert_eq!(uniquify(&g, &reg, "body", NodeId(1)), "body");
    }

    #[test]
    fn a_node_may_keep_its_own_name() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("body")), ("box", Some("box2"))]);
        assert_eq!(uniquify(&g, &reg, "body", NodeId(1)), "body");
    }

    #[test]
    fn a_colliding_rename_is_suffixed() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("body")), ("box", Some("box2"))]);
        assert_eq!(uniquify(&g, &reg, "body", NodeId(2)), "body2");
    }

    #[test]
    fn a_colliding_rename_reuses_trailing_digits_as_the_stem() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[
            ("box", Some("body2")),
            ("box", Some("body3")),
            ("box", Some("other")),
        ]);
        // "body2" is taken, so the stem is "body" and the next free is body4.
        assert_eq!(uniquify(&g, &reg, "body2", NodeId(3)), "body4");
    }

    #[test]
    fn a_blank_rename_is_returned_untouched() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("box1"))]);
        assert_eq!(uniquify(&g, &reg, "", NodeId(1)), "");
    }

    #[test]
    fn a_purely_numeric_name_keeps_its_whole_stem() {
        let reg = builtin_registry().unwrap();
        let g = graph_with(&[("box", Some("12")), ("box", Some("other"))]);
        // Trimming digits would leave an empty stem, so "12" is the stem.
        assert_eq!(uniquify(&g, &reg, "12", NodeId(2)), "122");
    }
}
