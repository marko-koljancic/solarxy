//! `ch()` path resolution: turning `"../box1/size"` into a value.
//!
//! This module is what `expr/` deliberately is not: it knows about
//! documents, contexts and the registry. The evaluator reaches it only
//! through [`crate::expr::ParamRefs`], which keeps the language a leaf.
//!
//! **The design finding that keeps this contained.** `ch()` reads a
//! *parameter*, which is document state, not a cook output. So it needs no
//! change to the wire topology and no virtual edges, and cook order is
//! irrelevant: a referenced expression is evaluated on demand, right here,
//! by recursing. The dependency graph exists only to know what to re-dirty
//! and what to refuse, never to order anything.
//!
//! Paths are resolved against the two-level context tree (root, then one
//! subflow per container), which bounds every form to at most
//! `/container/node/param`.

use crate::document::{Document, GraphContext, NodeId};
use crate::expr::{ParamRefs, SceneTime, Value};
use crate::naming::node_name;
use crate::params::{ParamSource, ParamValue};
use crate::previews::{Previews, effective_params};
use crate::registry::Registry;
use crate::registry::resolve::conform_and_clamp;

/// How deep a chain of `ch()` references may go.
///
/// `SetParam` refuses a cycle at write time, so a loop should be
/// unreachable; this is the backstop for the paths that bypass it (a
/// hand-edited document, a pasted fragment) and it is what stops a cycle
/// from being a stack overflow instead of an error message.
pub const MAX_REF_DEPTH: usize = 32;

/// Where a resolved path points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub ctx: GraphContext,
    pub node: NodeId,
    pub key: String,
}

/// Resolves a `ch()` path relative to the node holding the expression.
///
/// The four forms, all bounded by the two-level tree:
///
/// - `radius` -- a param on this same node.
/// - `sphere1/radius` -- a node in this same network.
/// - `../radius` -- a param on this network's own container.
/// - `../geo2/translate` -- a sibling of the container, in the parent.
/// - `/geo1/sphere1/radius` -- absolute from the root.
///
/// # Errors
/// An unparseable shape, a name that matches no node or several, or a
/// climb above the root.
pub fn resolve_path(
    doc: &Document,
    registry: &Registry,
    from_ctx: GraphContext,
    from_node: NodeId,
    path: &str,
) -> Result<Target, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("an empty path references nothing".to_string());
    }

    // Absolute: always measured from the root, whoever is asking.
    if let Some(rest) = trimmed.strip_prefix('/') {
        let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        return match segs.as_slice() {
            [node, key] => {
                let id = find_named(doc, registry, GraphContext::Root, node)?;
                Ok(Target {
                    ctx: GraphContext::Root,
                    node: id,
                    key: (*key).to_string(),
                })
            }
            [container, node, key] => {
                let owner = find_named(doc, registry, GraphContext::Root, container)?;
                let ctx = GraphContext::Subflow(owner);
                if doc.graph(ctx).is_err() {
                    return Err(format!("`{container}` is not a container network"));
                }
                let id = find_named(doc, registry, ctx, node)?;
                Ok(Target {
                    ctx,
                    node: id,
                    key: (*key).to_string(),
                })
            }
            _ => Err(format!(
                "`{trimmed}` is not a path; absolute paths look like /geo1/sphere1/radius"
            )),
        };
    }

    // Parent-relative. The tree is two levels, so exactly one climb is
    // possible and `../../` has nowhere to go.
    if let Some(rest) = trimmed.strip_prefix("../") {
        if rest.starts_with("../") {
            return Err(
                "`../../` climbs above the root: networks are only one level deep".to_string(),
            );
        }
        let GraphContext::Subflow(owner) = from_ctx else {
            return Err("`../` climbs above the root, which has no parent".to_string());
        };
        let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        return match segs.as_slice() {
            // `../radius` is the container's own param.
            [key] => Ok(Target {
                ctx: GraphContext::Root,
                node: owner,
                key: (*key).to_string(),
            }),
            [node, key] => {
                let id = find_named(doc, registry, GraphContext::Root, node)?;
                Ok(Target {
                    ctx: GraphContext::Root,
                    node: id,
                    key: (*key).to_string(),
                })
            }
            _ => Err(format!("`{trimmed}` has too many segments")),
        };
    }

    let segs: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    match segs.as_slice() {
        // One segment is a param on this node, never a node name: that is
        // what makes `ch("radius")` unambiguous.
        [key] => Ok(Target {
            ctx: from_ctx,
            node: from_node,
            key: (*key).to_string(),
        }),
        [node, key] => {
            let id = find_named(doc, registry, from_ctx, node)?;
            Ok(Target {
                ctx: from_ctx,
                node: id,
                key: (*key).to_string(),
            })
        }
        _ => Err(format!(
            "`{trimmed}` has too many segments; use ../name/param or /geo1/name/param"
        )),
    }
}

/// The one node in `ctx` answering to `name`.
fn find_named(
    doc: &Document,
    registry: &Registry,
    ctx: GraphContext,
    name: &str,
) -> Result<NodeId, String> {
    let graph = doc
        .graph(ctx)
        .map_err(|_| format!("`{name}` is not in a network that exists"))?;
    let mut hits = graph
        .nodes()
        .filter(|n| node_name(n, registry) == name)
        .map(|n| n.id);
    let Some(first) = hits.next() else {
        return Err(format!("no node named `{name}` here"));
    };
    let extra = hits.count();
    if extra > 0 {
        // Only reachable in a pre-0.8.1 document, where names were never
        // unique. Naming the collision is what tells the user to rename.
        return Err(format!(
            "`{name}` is ambiguous: {} nodes share that name; rename one",
            extra + 1
        ));
    }
    Ok(first)
}

/// Reads parameters across the document for the expression evaluator.
///
/// Cheap to construct and to re-root: every field is a shared borrow, so
/// recursing into a referenced expression is a struct copy, not a clone of
/// anything.
#[derive(Clone, Copy)]
pub struct DocRefs<'a> {
    doc: &'a Document,
    registry: &'a Registry,
    previews: &'a Previews,
    ctx: GraphContext,
    node: NodeId,
    time: SceneTime,
    depth: usize,
}

impl<'a> DocRefs<'a> {
    #[must_use]
    pub fn new(
        doc: &'a Document,
        registry: &'a Registry,
        previews: &'a Previews,
        ctx: GraphContext,
        node: NodeId,
        time: SceneTime,
    ) -> Self {
        Self {
            doc,
            registry,
            previews,
            ctx,
            node,
            time,
            depth: 0,
        }
    }
}

impl ParamRefs for DocRefs<'_> {
    fn read(&self, path: &str) -> Result<Value, String> {
        if self.depth >= MAX_REF_DEPTH {
            return Err(format!(
                "reference chain is deeper than {MAX_REF_DEPTH}; is `{path}` part of a cycle?"
            ));
        }
        let target = resolve_path(self.doc, self.registry, self.ctx, self.node, path)?;

        let graph = self
            .doc
            .graph(target.ctx)
            .map_err(|_| format!("`{path}` is not in a network that exists"))?;
        let data = graph
            .node(target.node)
            .ok_or_else(|| format!("`{path}` points at a node that is gone"))?;
        let desc = self
            .registry
            .get(&data.type_id)
            .ok_or_else(|| format!("`{path}` points at an unknown node type"))?;
        let spec = desc.param(&target.key).ok_or_else(|| {
            format!(
                "`{}` has no param `{}`",
                node_name(data, self.registry),
                target.key
            )
        })?;

        // Preview-effective, so an expression follows a drag on the param
        // it references (previews.rs's standing rule: every consumer of a
        // node's params must resolve through this or a drag is invisible
        // in exactly the surface it drives).
        let params = effective_params(self.previews, target.node, &data.params);
        let stored = params.get(&target.key);

        let literal = match stored {
            Some(ParamSource::Expression { expr }) => {
                // Recurse. Cook order is irrelevant because this reads
                // document state, so the referenced expression is simply
                // evaluated here and now.
                let parsed = crate::expr::parse(expr).map_err(|e| {
                    format!("`{path}` holds an expression that does not parse: {e}")
                })?;
                let inner = DocRefs {
                    ctx: target.ctx,
                    node: target.node,
                    depth: self.depth + 1,
                    ..*self
                };
                // No geometry: the target's inputs belong to its own cook,
                // which is not running. Saying so beats inventing a count.
                let eval_ctx = crate::expr::EvalCtx::new(self.time).with_refs(&inner);
                return crate::expr::eval(&parsed.root, &eval_ctx)
                    .map_err(|e| format!("`{path}`: {e}"));
            }
            Some(ParamSource::Literal(v)) => v.clone(),
            None => spec.default.clone(),
        };

        let conformed =
            conform_and_clamp(&literal, spec).map_err(|reason| format!("`{path}`: {reason}"))?;
        param_to_value(&conformed).ok_or_else(|| {
            format!(
                "`{path}` is a {} param, which has no numeric value",
                type_name_of(&conformed)
            )
        })
    }
}

/// A stored param value as an expression value, or `None` for the types
/// that are not numbers (there is no string type in the lattice).
fn param_to_value(v: &ParamValue) -> Option<Value> {
    Some(match v {
        ParamValue::Float(f) => Value::Float(*f),
        ParamValue::Int(i) => Value::Float(*i as f64),
        ParamValue::Bool(b) => Value::Bool(*b),
        ParamValue::Vec2(a) => Value::Vec2(*a),
        ParamValue::Vec3(a) => Value::Vec3(*a),
        ParamValue::Vec4(a) => Value::Vec4(*a),
        ParamValue::Color(c) => Value::Vec4([
            f64::from(c[0]),
            f64::from(c[1]),
            f64::from(c[2]),
            f64::from(c[3]),
        ]),
        ParamValue::Text(_)
        | ParamValue::Enum(_)
        | ParamValue::Asset(_)
        | ParamValue::NodeRef(_) => return None,
    })
}

fn type_name_of(v: &ParamValue) -> &'static str {
    match v {
        ParamValue::Float(_) => "float",
        ParamValue::Int(_) => "int",
        ParamValue::Bool(_) => "bool",
        ParamValue::Text(_) => "text",
        ParamValue::Vec2(_) => "vec2",
        ParamValue::Vec3(_) => "vec3",
        ParamValue::Vec4(_) => "vec4",
        ParamValue::Color(_) => "color",
        ParamValue::Enum(_) => "enum",
        ParamValue::Asset(_) => "asset",
        ParamValue::NodeRef(_) => "reference",
    }
}

/// The expression dependency graph, over `(node, key)` pairs.
///
/// **Why this exists.** The alternative was measured before this was
/// built: `mark_dirty_inner`
/// scans once per dirtied node, so a scan-based reverse lookup is
/// `O(dirtied x document)` and costs 1.68 ms per param write at 210 nodes,
/// 25.5 ms at 840. One control node driving many params is the canonical
/// expression shape, so that is the normal case, not the worst one.
///
/// **Why it is rebuilt rather than maintained.** The scan's one real
/// advantage was correctness: `referrers_of` has "no maintenance-bug
/// surface across undo/paste/load", and a stale index entry is silent
/// wrongness (a node that fails to re-cook). Rebuilding the whole thing
/// from the document after any command that could change it keeps that
/// property exactly, because the index is never a separate source of
/// truth. The cost is one linear pass per user command, against O(1)
/// lookups during propagation, so the measured quadratic term is gone
/// either way.
///
/// Keys are `(node, key)` pairs, not nodes: `width = ch("height")` on one
/// node is legal and useful, and a node-level graph would call it a cycle.
#[derive(Debug, Default, Clone)]
pub struct ExprIndex {
    /// What each param reads.
    forward: std::collections::BTreeMap<(NodeId, String), Vec<(NodeId, String)>>,
    /// Who reads each param.
    reverse: std::collections::BTreeMap<(NodeId, String), Vec<(NodeId, String)>>,
    /// Every node whose effective params read `$T` or `$F`, with the context
    /// to dirty it in.
    ///
    /// This is what the runtime's tick keys on, and why a scene with no time
    /// expression pays nothing per frame: the set is empty, so the tick
    /// dirties nothing and the cook has nothing to do.
    ///
    /// Both kinds of program count. A `ParamSource::Expression` reading `$T`
    /// is the obvious one; a wrangle's Snippet param is the one that is easy
    /// to miss, because it is stored as plain Text and would otherwise look
    /// like any other string.
    time_dependent: std::collections::BTreeSet<(GraphContext, NodeId)>,
}

/// Whether any Snippet param on `node` holds a program that reads the clock.
///
/// Reads the EFFECTIVE value (stored, else the descriptor default) rather
/// than the stored one. A node using its default program stores nothing, and
/// a default that read `$T` would otherwise animate while being invisible to
/// the tick, which is the sort of bug that presents as "playback does
/// nothing" and takes an hour to find.
fn node_program_uses_time(registry: &Registry, node: &crate::document::NodeData) -> bool {
    let Some(desc) = registry.get(&node.type_id) else {
        return false;
    };
    desc.params
        .iter()
        .filter(|spec| spec.ty == crate::registry::param_spec::ParamType::Snippet)
        .any(|spec| {
            let source = match node.params.get(&spec.key) {
                Some(ParamSource::Literal(ParamValue::Text(t))) => t.as_str(),
                // An expression cannot drive a Snippet (Text-stored types
                // are literal-only), so anything else falls back to the
                // default.
                _ => match &spec.default {
                    ParamValue::Text(t) => t.as_str(),
                    _ => return false,
                },
            };
            crate::expr::parse_program(source).is_ok_and(|p| p.uses_time)
        })
}

impl ExprIndex {
    /// Derives the whole index from the document.
    #[must_use]
    pub fn build(doc: &Document, registry: &Registry) -> Self {
        let mut index = Self::default();
        let mut contexts = vec![GraphContext::Root];
        contexts.extend(doc.subflow_owners().map(GraphContext::Subflow));
        for ctx in contexts {
            let Ok(graph) = doc.graph(ctx) else { continue };
            for node in graph.nodes() {
                if node_program_uses_time(registry, node) {
                    index.time_dependent.insert((ctx, node.id));
                }
                for (key, src) in &node.params {
                    let ParamSource::Expression { expr } = src else {
                        continue;
                    };
                    let Ok(parsed) = crate::expr::parse(expr) else {
                        // An unparseable expression reads nothing. It
                        // badges at cook time; it must not also poison the
                        // index.
                        continue;
                    };
                    if parsed.uses_time {
                        index.time_dependent.insert((ctx, node.id));
                    }
                    let from = (node.id, key.clone());
                    for path in parsed.root.ch_paths() {
                        // A path that does not resolve yet (a dangling
                        // reference) simply has no edge. It badges at cook
                        // time, and the rewrite on rename is what keeps a
                        // live reference live.
                        if let Ok(target) = resolve_path(doc, registry, ctx, node.id, &path) {
                            let to = (target.node, target.key);
                            index
                                .forward
                                .entry(from.clone())
                                .or_default()
                                .push(to.clone());
                            index.reverse.entry(to).or_default().push(from.clone());
                        }
                    }
                }
            }
        }
        index
    }

    /// The params whose expressions read `target`.
    #[must_use]
    pub fn referrers(&self, target: &(NodeId, String)) -> &[(NodeId, String)] {
        self.reverse.get(target).map_or(&[], Vec::as_slice)
    }

    /// Every node a tick has to dirty, with its context.
    #[must_use]
    pub fn time_dependent(&self) -> &std::collections::BTreeSet<(GraphContext, NodeId)> {
        &self.time_dependent
    }

    /// Whether anything in the document reads the clock at all.
    #[must_use]
    pub fn has_time_dependency(&self) -> bool {
        !self.time_dependent.is_empty()
    }

    /// Every node holding an expression that reads `target`, transitively.
    ///
    /// Deduplicated by `(node, key)` rather than by node, so a diamond
    /// (two params on one node reading the same source) is walked once per
    /// param but reported once per node.
    #[must_use]
    pub fn transitive_referrer_nodes(&self, target: &(NodeId, String)) -> Vec<NodeId> {
        let mut seen = std::collections::BTreeSet::new();
        let mut nodes = std::collections::BTreeSet::new();
        let mut stack: Vec<(NodeId, String)> = self.referrers(target).to_vec();
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            nodes.insert(cur.0);
            stack.extend(self.referrers(&cur).iter().cloned());
        }
        nodes.into_iter().collect()
    }

    /// Whether `from` already reaches `to` by following references.
    ///
    /// Used at write time: if the param being assigned is reachable from
    /// what the new expression reads, the assignment would close a loop.
    #[must_use]
    pub fn reaches(&self, from: &(NodeId, String), to: &(NodeId, String)) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        let mut stack = vec![from.clone()];
        while let Some(cur) = stack.pop() {
            if &cur == to {
                return true;
            }
            if !seen.insert(cur.clone()) {
                continue;
            }
            if let Some(next) = self.forward.get(&cur) {
                stack.extend(next.iter().cloned());
            }
        }
        false
    }

    /// The targets a not-yet-stored expression would read, resolved now.
    ///
    /// Separate from [`Self::build`] because cycle refusal has to judge an
    /// expression *before* it is written: the index still describes the
    /// document as it stands.
    #[must_use]
    pub fn targets_of(
        doc: &Document,
        registry: &Registry,
        ctx: GraphContext,
        node: NodeId,
        expr: &str,
    ) -> Vec<(NodeId, String)> {
        let Ok(parsed) = crate::expr::parse(expr) else {
            return Vec::new();
        };
        parsed
            .root
            .ch_paths()
            .into_iter()
            .filter_map(|path| {
                resolve_path(doc, registry, ctx, node, &path)
                    .ok()
                    .map(|t| (t.node, t.key))
            })
            .collect()
    }
}

/// Rewrites the node-name segments of one path that referred to `renamed`.
///
/// Returns `None` when the path does not name that node, so the caller can
/// tell "no change" from "changed to the same text".
///
/// Positional, not textual: `ch("radius")` is a param on this node, so a
/// node that happens to be called `radius` must not rewrite it. Each form
/// declares which of its segments are node names, and each candidate is
/// resolved against the document to confirm it really is the node being
/// renamed.
#[must_use]
pub fn rewrite_path_for_rename(
    doc: &Document,
    registry: &Registry,
    from_ctx: GraphContext,
    path: &str,
    renamed: NodeId,
    new_name: &str,
) -> Option<String> {
    let trimmed = path.trim();
    let names_the_node = |ctx: GraphContext, name: &str| {
        find_named(doc, registry, ctx, name).is_ok_and(|id| id == renamed)
    };
    let split = |rest: &str| -> Vec<String> {
        rest.split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    };

    if let Some(rest) = trimmed.strip_prefix('/') {
        let mut segs = split(rest);
        let mut changed = false;
        match segs.len() {
            // /node/param
            2 => {
                if names_the_node(GraphContext::Root, &segs[0]) {
                    segs[0] = new_name.to_string();
                    changed = true;
                }
            }
            // /container/node/param. The container is resolved from its
            // ORIGINAL name first, so renaming it does not blind the
            // lookup for the segment after it.
            3 => {
                let owner = find_named(doc, registry, GraphContext::Root, &segs[0]).ok();
                if let Some(owner) = owner {
                    if owner == renamed {
                        segs[0] = new_name.to_string();
                        changed = true;
                    }
                    if names_the_node(GraphContext::Subflow(owner), &segs[1]) {
                        segs[1] = new_name.to_string();
                        changed = true;
                    }
                }
            }
            _ => {}
        }
        return changed.then(|| format!("/{}", segs.join("/")));
    }

    if let Some(rest) = trimmed.strip_prefix("../") {
        if !matches!(from_ctx, GraphContext::Subflow(_)) {
            return None;
        }
        let mut segs = split(rest);
        // `../param` is the container's own param and carries no node
        // name, so a rename of the container leaves it correct.
        if segs.len() == 2 && names_the_node(GraphContext::Root, &segs[0]) {
            segs[0] = new_name.to_string();
            return Some(format!("../{}", segs.join("/")));
        }
        return None;
    }

    let mut segs = split(trimmed);
    // One segment is always a param on this node, never a node name.
    if segs.len() == 2 && names_the_node(from_ctx, &segs[0]) {
        segs[0] = new_name.to_string();
        return Some(segs.join("/"));
    }
    None
}

/// Rewrites every `ch()` path in one expression that referred to `renamed`.
///
/// Returns `None` when nothing changed. Replacements are applied
/// right-to-left so each span stays valid as the string shrinks or grows,
/// and only the quoted path inside each call is touched: the rest of the
/// user's text, including their spacing and comments, is preserved
/// byte for byte.
#[must_use]
pub fn rewrite_expression_for_rename(
    source: &str,
    doc: &Document,
    registry: &Registry,
    from_ctx: GraphContext,
    renamed: NodeId,
    new_name: &str,
) -> Option<String> {
    let parsed = crate::expr::parse(source).ok()?;
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (call_span, path) in parsed.root.ch_calls() {
        let Some(new_path) =
            rewrite_path_for_rename(doc, registry, from_ctx, &path, renamed, new_name)
        else {
            continue;
        };
        // Locate the quoted literal inside the call's own span. It is the
        // only string a `ch()` call can contain, so the first quote pair
        // is unambiguous.
        let call = source.get(call_span.clone())?;
        let open = call.find('"')?;
        let close = call[open + 1..].find('"')? + open + 1;
        let start = call_span.start + open + 1;
        let end = call_span.start + close;
        edits.push((start..end, new_path));
    }
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    let mut out = source.to_string();
    for (span, replacement) in edits {
        out.replace_range(span, &replacement);
    }
    Some(out)
}
