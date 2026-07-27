//! W0b: the cost of an expression dependency graph (0.8.1 milestone).
//!
//! The question this answers. `referrers_of` deliberately chose a scan over
//! a maintained index, and documented why: "`NodeRef` literals are
//! self-describing, documents are interactive-sized, and a scan has no
//! maintenance-bug surface across undo/paste/load. Memoize only if
//! profiling ever says so" (`engine/mod.rs:2906-2911`). Expressions make
//! cross-node references far denser than `NodeRef` params ever were, so
//! that stance needs re-testing rather than inheriting. This is the
//! profiling that says so, or does not.
//!
//! ```text
//! cargo run --release -p solarxy-graph --example param_graph_cost
//! ```
//!
//! Honest limitations, both recorded in the W0b amendment:
//!
//! 1. **The reference extractor here is a stub.** `expr/` does not exist
//!    yet (it is W1a), so `ch("...")` targets are pulled out by scanning
//!    for the call and splitting the path. What is being measured is the
//!    *shape* of the reverse lookup, an `O(contexts x nodes x params)`
//!    walk versus one hash probe, not parse cost. A real extractor walks a
//!    cached AST and is strictly cheaper per param than this stub.
//! 2. **The document is synthetic.** Milestone open item 3 (what scene
//!    sizes users actually hit) is still unanswered, so the sizes swept
//!    here are a guess at production reality, chosen to bracket it.

use std::collections::HashMap;
use std::time::Instant;

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::{Command, Engine, EngineEvent};

/// Node counts to sweep. 200 is the milestone's stated target; the others
/// bracket it so the scaling curve is visible rather than a single point.
const SIZES: &[usize] = &[50, 100, 200, 400, 800];

/// Fraction of transform nodes whose `translate` carries an expression.
/// Deliberately high: the worst case for a scan is a document where most
/// params are expressions, which is what "far denser than `NodeRef` ever
/// was" means in practice.
const EXPR_DENSITY: f64 = 0.5;

/// Repetitions per timed measurement (the operations are microseconds, so
/// a single sample is dominated by timer noise).
const REPS: usize = 200;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "W0b: expression dependency-graph cost\n\
         density={:.0}% of transform nodes carry a ch() expression, {REPS} reps per measurement\n",
        EXPR_DENSITY * 100.0
    );

    println!(
        "{:>6}  {:>8}  {:>12}  {:>12}  {:>12}  {:>12}  {:>12}",
        "nodes", "exprs", "setparam", "rebuild", "scan", "index", "index+maint"
    );
    println!("{}", "-".repeat(88));

    for &size in SIZES {
        let row = measure(size)?;
        println!(
            "{:>6}  {:>8}  {:>10.1}us  {:>10.1}us  {:>10.1}us  {:>10.2}us  {:>10.2}us",
            row.nodes,
            row.exprs,
            row.setparam_us,
            row.rebuild_us,
            row.scan_us,
            row.index_us,
            row.index_maint_us
        );
    }

    // The case that actually decides it. `mark_dirty_inner` recurses
    // through referrers with a visited-set guard, so a scan-based reverse
    // lookup runs ONCE PER DIRTIED NODE, not once per SetParam. One
    // control node driving many params (milestone journey J10) is the
    // canonical expression shape, so fan-out is the realistic worst case.
    println!("\nfan-out: one hub param read by every expression in the document");
    println!(
        "{:>6}  {:>8}  {:>14}  {:>14}  {:>10}",
        "nodes", "fanout", "propagate/scan", "propagate/idx", "ratio"
    );
    println!("{}", "-".repeat(62));
    for &size in SIZES {
        let f = measure_fanout(size)?;
        println!(
            "{:>6}  {:>8}  {:>11.1}us  {:>11.2}us  {:>9.0}x",
            f.nodes,
            f.fanout,
            f.scan_us,
            f.index_us,
            if f.index_us > 0.0 {
                f.scan_us / f.index_us
            } else {
                0.0
            }
        );
    }

    println!(
        "\nsetparam    = Engine::apply(SetParam) end to end. Since W1e this INCLUDES\n\
         \x20             rebuilding the expression dependency index from the document,\n\
         \x20             which is why it is tens of microseconds rather than one or two.\n\
         \x20             That is the deliberate trade: one linear rebuild per user\n\
         \x20             command buys O(1) propagation lookups and no stale-index bug\n\
         \x20             class. A drag does not pay it (preview_param does not rebuild).\n\
         \x20             Compare against propagate/scan below, which is what the\n\
         \x20             alternative would have cost on the SAME edit.\n\
         rebuild     = ExprIndex::build alone, the real code path. W1e re-derives\n\
         \x20             the index from the document instead of patching it, so this\n\
         \x20             is what a command pays and it is O(document), not O(refs in\n\
         \x20             the written param). It replaces the hypothetical index+maint\n\
         \x20             column as the honest maintenance figure.\n\
         scan        = expression reverse lookup as an O(contexts x nodes x params)\n\
         \x20             walk, the referrers_of shape extended to expressions.\n\
         index       = the same lookup as one probe into a maintained reverse map.\n\
         index+maint = that probe plus re-extracting the written param's own refs\n\
         \x20             and patching the forward and reverse maps.\n"
    );
    Ok(())
}

struct Row {
    nodes: usize,
    exprs: usize,
    setparam_us: f64,
    rebuild_us: f64,
    scan_us: f64,
    index_us: f64,
    index_maint_us: f64,
}

fn measure(target_nodes: usize) -> Result<Row, Box<dyn std::error::Error>> {
    let mut engine = Engine::new()?;

    // Two-level tree, matching the real context model: `geo` containers at
    // the root (ContextSet::OBJ), geometry nodes inside their subflows.
    // Roughly 20 nodes per container, which is a plausible network size.
    let per_geo = 20usize;
    let geo_count = target_nodes.div_ceil(per_geo).max(1);

    // Only transform nodes are tracked for referencing: they carry the
    // Vec3 params expressions are eligible on (M-3). Boxes are still added
    // so the scan has non-expression params to walk past, which is what a
    // real document looks like.
    let mut transforms: Vec<(GraphContext, NodeId, String)> = Vec::new();
    for _ in 0..geo_count {
        let geo = add_node(&mut engine, GraphContext::Root, "geo")?;
        let ctx = GraphContext::Subflow(geo);
        for ni in 0..per_geo {
            if ni % 4 == 0 {
                add_node(&mut engine, ctx, "box")?;
            } else {
                let id = add_node(&mut engine, ctx, "transform")?;
                let name = format!("transform{}", transforms.len() + 1);
                transforms.push((ctx, id, name));
            }
        }
    }

    // Seed expressions. Each references a sibling by name, which is the
    // form W1e has to resolve and therefore the form the reverse lookup
    // has to recognise.
    let mut exprs = 0usize;
    let step = (1.0 / EXPR_DENSITY).round().max(1.0) as usize;
    for (i, (ctx, id, _)) in transforms.clone().iter().enumerate() {
        if i % step != 0 || i == 0 {
            continue;
        }
        let (_, _, target_name) = &transforms[i - 1];
        let expr = format!("ch(\"{target_name}/translate\") * 2.0 + 1.0");
        if engine
            .apply(Command::SetParam {
                ctx: *ctx,
                node: *id,
                key: "translate".to_string(),
                value: ParamSource::Expression { expr },
            })
            .is_ok()
        {
            exprs += 1;
        }
    }

    let total = engine.document().graph(GraphContext::Root)?.node_count()
        + engine
            .document()
            .subflow_owners()
            .filter_map(|o| engine.document().graph(GraphContext::Subflow(o)).ok())
            .map(solarxy_graph::document::Graph::node_count)
            .sum::<usize>();

    // A node whose param the others reference, so the lookup has real work.
    let (probe_ctx, probe_id, probe_name) = transforms
        .first()
        .cloned()
        .ok_or("synthetic document has no transform nodes")?;

    // 1. Engine::apply(SetParam) as it stands today.
    let t = Instant::now();
    for i in 0..REPS {
        let v = i as f64;
        engine.apply(Command::SetParam {
            ctx: probe_ctx,
            node: probe_id,
            key: "translate".to_string(),
            value: ParamSource::Literal(ParamValue::Vec3([v, v, v])),
        })?;
    }
    let setparam_us = t.elapsed().as_secs_f64() * 1e6 / REPS as f64;

    // 1b. The real `ExprIndex::build`, timed on its own. W1e rebuilds the
    //     index wholesale on every index-affecting command rather than
    //     patching it, so this is the whole of what a `SetParam` pays for
    //     the dependency graph, and it is the number the W0b amendment's
    //     `index+maint` column does NOT describe.
    let t = Instant::now();
    for _ in 0..REPS {
        let built = solarxy_graph::refs::ExprIndex::build(engine.document(), engine.registry());
        std::hint::black_box(&built);
    }
    let rebuild_us = t.elapsed().as_secs_f64() * 1e6 / REPS as f64;

    // 2. The reverse lookup as a scan.
    let t = Instant::now();
    let mut sink = 0usize;
    for _ in 0..REPS {
        sink += scan_referrers(&engine, &probe_name, "translate").len();
    }
    let scan_us = t.elapsed().as_secs_f64() * 1e6 / REPS as f64;

    // 3. The same lookup against a maintained reverse index.
    let mut index = build_index(&engine);
    let key = (probe_name.clone(), "translate".to_string());
    let t = Instant::now();
    for _ in 0..REPS {
        sink += index.get(&key).map_or(0, Vec::len);
    }
    let index_us = t.elapsed().as_secs_f64() * 1e6 / REPS as f64;

    // 4. Index probe plus the maintenance a write actually costs. A write
    //    to (node, key) retracts that param's old edges and inserts its
    //    new ones, which is O(refs in this one param), NOT O(document).
    //    Measured in place: cloning the map here would time HashMap::clone
    //    instead, and would scale with document size for no real reason.
    let owner = (probe_id, "translate".to_string());
    let old_expr = format!("ch(\"{probe_name}/translate\")");
    let new_expr = format!("ch(\"{probe_name}/rotate\")");
    let t = Instant::now();
    for i in 0..REPS {
        let (retract, insert) = if i % 2 == 0 {
            (&old_expr, &new_expr)
        } else {
            (&new_expr, &old_expr)
        };
        for target in extract_refs(retract) {
            if let Some(v) = index.get_mut(&target)
                && let Some(p) = v.iter().position(|e| e == &owner)
            {
                v.swap_remove(p);
            }
        }
        for target in extract_refs(insert) {
            index.entry(target).or_default().push(owner.clone());
        }
        sink += index.get(&key).map_or(0, Vec::len);
    }
    let index_maint_us = t.elapsed().as_secs_f64() * 1e6 / REPS as f64;

    debug_assert!(sink > 0 || exprs == 0);
    let _ = sink;

    Ok(Row {
        nodes: total,
        exprs,
        setparam_us,
        rebuild_us,
        scan_us,
        index_us,
        index_maint_us,
    })
}

struct Fanout {
    nodes: usize,
    fanout: usize,
    scan_us: f64,
    index_us: f64,
}

/// Every expression in the document reads ONE hub param, so dirtying that
/// param has to walk every referrer. Times the whole propagation, which is
/// what a single `SetParam` on the hub actually costs.
fn measure_fanout(target_nodes: usize) -> Result<Fanout, Box<dyn std::error::Error>> {
    let mut engine = Engine::new()?;
    let per_geo = 20usize;
    let geo_count = target_nodes.div_ceil(per_geo).max(1);

    let mut transforms: Vec<(GraphContext, NodeId)> = Vec::new();
    let mut id_to_name: HashMap<NodeId, String> = HashMap::new();
    for _ in 0..geo_count {
        let geo = add_node(&mut engine, GraphContext::Root, "geo")?;
        let ctx = GraphContext::Subflow(geo);
        for ni in 0..per_geo {
            if ni % 4 == 0 {
                add_node(&mut engine, ctx, "box")?;
            } else {
                let id = add_node(&mut engine, ctx, "transform")?;
                id_to_name.insert(id, format!("transform{}", transforms.len() + 1));
                transforms.push((ctx, id));
            }
        }
    }

    // transform1 is the hub; everything else reads it.
    let hub_name = "transform1".to_string();
    let mut fanout = 0usize;
    for (ctx, id) in transforms.iter().skip(1) {
        let expr = format!("ch(\"{hub_name}/translate\") * 2.0");
        if engine
            .apply(Command::SetParam {
                ctx: *ctx,
                node: *id,
                key: "translate".to_string(),
                value: ParamSource::Expression { expr },
            })
            .is_ok()
        {
            fanout += 1;
        }
    }

    let total = engine.document().graph(GraphContext::Root)?.node_count()
        + engine
            .document()
            .subflow_owners()
            .filter_map(|o| engine.document().graph(GraphContext::Subflow(o)).ok())
            .map(solarxy_graph::document::Graph::node_count)
            .sum::<usize>();

    // Scan-based propagation: one full document walk per dirtied node,
    // mirroring how `mark_dirty_inner` recurses through `referrers_of`.
    let reps = 20usize;
    let t = Instant::now();
    let mut sink = 0usize;
    for _ in 0..reps {
        let mut visited = std::collections::BTreeSet::new();
        let mut stack = vec![(hub_name.clone(), "translate".to_string())];
        while let Some((name, key)) = stack.pop() {
            if !visited.insert((name.clone(), key.clone())) {
                continue;
            }
            // Every dirtied node is itself visited and scanned for ITS
            // referrers, which is what makes the scan cost fan-out, not 1.
            for (id, k) in scan_referrers(&engine, &name, &key) {
                sink += 1;
                if let Some(n) = id_to_name.get(&id) {
                    stack.push((n.clone(), k));
                }
            }
        }
    }
    let scan_us = t.elapsed().as_secs_f64() * 1e6 / reps as f64;

    // Index-based propagation: same walk, one hash probe per node.
    let index = build_index(&engine);
    let t = Instant::now();
    for _ in 0..reps {
        let mut visited = std::collections::BTreeSet::new();
        let mut stack = vec![(hub_name.clone(), "translate".to_string())];
        while let Some(k) = stack.pop() {
            if !visited.insert(k.clone()) {
                continue;
            }
            for (id, key) in index.get(&k).into_iter().flatten() {
                sink += 1;
                if let Some(n) = id_to_name.get(id) {
                    stack.push((n.clone(), key.clone()));
                }
            }
        }
    }
    let index_us = t.elapsed().as_secs_f64() * 1e6 / reps as f64;
    let _ = sink;

    Ok(Fanout {
        nodes: total,
        fanout,
        scan_us,
        index_us,
    })
}

/// Adds one node and digs its id out of the emitted batch (`apply` returns
/// events, not the id).
fn add_node(
    engine: &mut Engine,
    ctx: GraphContext,
    node_type: &str,
) -> Result<NodeId, Box<dyn std::error::Error>> {
    let batch = engine.apply(Command::AddNode {
        ctx,
        node_type: node_type.to_string(),
        position: [0.0, 0.0],
    })?;
    batch
        .events
        .iter()
        .find_map(|e| match e {
            EngineEvent::NodeAdded { node, .. } => Some(node.id),
            _ => None,
        })
        .ok_or_else(|| format!("AddNode('{node_type}') emitted no NodeAdded").into())
}

/// The reverse lookup as a scan: every context, every node, every param.
/// This is `referrers_of`'s shape (`engine/mod.rs:2904-2931`) with the
/// `NodeRef` predicate swapped for expression reference extraction.
fn scan_referrers(engine: &Engine, target_name: &str, target_key: &str) -> Vec<(NodeId, String)> {
    let doc = engine.document();
    let mut out = Vec::new();
    let mut contexts = vec![GraphContext::Root];
    contexts.extend(doc.subflow_owners().map(GraphContext::Subflow));
    for ctx in contexts {
        let Ok(graph) = doc.graph(ctx) else { continue };
        for n in graph.nodes() {
            for (key, src) in &n.params {
                let ParamSource::Expression { expr } = src else {
                    continue;
                };
                if extract_refs(expr)
                    .iter()
                    .any(|(name, k)| name == target_name && k == target_key)
                {
                    out.push((n.id, key.clone()));
                }
            }
        }
    }
    out
}

/// The same relation as a maintained map: target (name, key) to the
/// (node, key) pairs whose expressions read it.
fn build_index(engine: &Engine) -> HashMap<(String, String), Vec<(NodeId, String)>> {
    let doc = engine.document();
    let mut index: HashMap<(String, String), Vec<(NodeId, String)>> = HashMap::new();
    let mut contexts = vec![GraphContext::Root];
    contexts.extend(doc.subflow_owners().map(GraphContext::Subflow));
    for ctx in contexts {
        let Ok(graph) = doc.graph(ctx) else { continue };
        for n in graph.nodes() {
            for (key, src) in &n.params {
                let ParamSource::Expression { expr } = src else {
                    continue;
                };
                for target in extract_refs(expr) {
                    index.entry(target).or_default().push((n.id, key.clone()));
                }
            }
        }
    }
    index
}

/// Stub reference extraction: pulls `ch("path/key")` targets out of raw
/// expression text. W1a replaces this with a walk over the cached AST,
/// which is cheaper per param than re-scanning a string.
fn extract_refs(expr: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = expr;
    while let Some(at) = rest.find("ch(\"") {
        rest = &rest[at + 4..];
        let Some(end) = rest.find('"') else { break };
        let path = &rest[..end];
        rest = &rest[end + 1..];
        if let Some((node, key)) = path.rsplit_once('/') {
            out.push((node.to_string(), key.to_string()));
        }
    }
    out
}
