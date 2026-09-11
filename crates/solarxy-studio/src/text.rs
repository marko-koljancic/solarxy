//! The document's text snippets, listed for the Text panel.
//!
//! A snippet is a node whose role is text: it holds a body and a language
//! and cooks nothing. The panel lists every one in every context, because
//! a snippet is a note or a program kept with the scene rather than a
//! thing that belongs to one network, and it sorts them by name so the
//! list reads the same on both shells. The browser walks its own mirror
//! for the same rows; this is that walk, over the document.

use solarxy_graph::document::{Document, GraphContext, NodeId};
use solarxy_graph::naming::node_name;
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::registry::{NodeRole, Registry};

/// One snippet: where it lives, what it is called, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetRow {
    pub ctx: GraphContext,
    pub node: NodeId,
    pub label: String,
    /// The language the body is read as, `plain` or `wrangle`.
    pub language: String,
    pub body: String,
}

/// The parameter the body lives in.
pub const BODY_KEY: &str = "body";
/// The parameter the language lives in.
pub const LANGUAGE_KEY: &str = "language";

/// Every snippet in every context, sorted by name without regard to case.
#[must_use]
pub fn snippets(doc: &Document, registry: &Registry) -> Vec<SnippetRow> {
    let contexts =
        std::iter::once(GraphContext::Root).chain(doc.subflow_owners().map(GraphContext::Subflow));
    let mut rows: Vec<SnippetRow> = contexts
        .filter_map(|ctx| doc.graph(ctx).ok().map(|graph| (ctx, graph)))
        .flat_map(|(ctx, graph)| {
            graph
                .nodes()
                .filter(|node| {
                    registry
                        .get(&node.type_id)
                        .is_some_and(|d| d.role == NodeRole::Text)
                })
                .map(move |node| {
                    let desc = registry.get(&node.type_id);
                    SnippetRow {
                        ctx,
                        node: node.id,
                        label: node_name(node, registry),
                        language: text_param(node, LANGUAGE_KEY, desc),
                        body: text_param(node, BODY_KEY, desc),
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    rows.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.node.0.cmp(&b.node.0))
    });
    rows
}

/// A literal text or enum parameter as a string: what the node stores,
/// else what its type declares as the default, since a parameter never
/// written is not stored at all, and nothing for an expression.
fn text_param(
    node: &solarxy_graph::document::NodeData,
    key: &str,
    desc: Option<&solarxy_graph::registry::NodeTypeDescriptor>,
) -> String {
    match node.params.get(key) {
        Some(ParamSource::Literal(ParamValue::Text(s) | ParamValue::Enum(s))) => s.clone(),
        Some(_) => String::new(),
        None => desc
            .and_then(|d| d.params.iter().find(|p| p.key == key))
            .map(|p| match &p.default {
                ParamValue::Text(s) | ParamValue::Enum(s) => s.clone(),
                _ => String::new(),
            })
            .unwrap_or_default(),
    }
}

/// The chip beside a snippet: where it lives, in the browser's words.
#[must_use]
pub fn context_chip(ctx: GraphContext) -> &'static str {
    match ctx {
        GraphContext::Root => "scene",
        GraphContext::Subflow(_) => "in a container",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::Command;
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

    /// Every text node in every context is listed, sorted by name without
    /// regard to case, with its body and language, and nothing else is.
    #[test]
    fn every_text_node_in_every_context_is_listed_sorted_by_name() {
        let mut engine = Engine::new().expect("engine");
        let container = add(&mut engine, GraphContext::Root, "sopnet");
        let inner = GraphContext::Subflow(container);
        let root_note = add(&mut engine, GraphContext::Root, "text");
        let inner_note = add(&mut engine, inner, "text");
        let _box = add(&mut engine, inner, "box");
        for (ctx, node, name) in [
            (GraphContext::Root, root_note, "zeta"),
            (inner, inner_note, "Alpha"),
        ] {
            engine
                .apply(Command::SetParam {
                    ctx,
                    node,
                    key: "name".to_string(),
                    value: ParamSource::Literal(ParamValue::Text(name.to_string())),
                })
                .expect("renames");
        }
        engine
            .apply(Command::SetParam {
                ctx: inner,
                node: inner_note,
                key: BODY_KEY.to_string(),
                value: ParamSource::Literal(ParamValue::Text("@P += 1;".to_string())),
            })
            .expect("body");
        engine
            .apply(Command::SetParam {
                ctx: inner,
                node: inner_note,
                key: LANGUAGE_KEY.to_string(),
                value: ParamSource::Literal(ParamValue::Enum("wrangle".to_string())),
            })
            .expect("language");

        let rows = snippets(engine.document(), engine.registry());
        assert_eq!(rows.len(), 2, "the box is not a snippet");
        assert_eq!(rows[0].label, "Alpha");
        assert_eq!(rows[0].ctx, inner);
        assert_eq!(rows[0].body, "@P += 1;");
        assert_eq!(rows[0].language, "wrangle");
        assert_eq!(rows[1].label, "zeta");
        assert_eq!(rows[1].ctx, GraphContext::Root);
        assert_eq!(rows[1].language, "plain");
        assert_eq!(context_chip(rows[0].ctx), "in a container");
        assert_eq!(context_chip(rows[1].ctx), "scene");
    }
}
