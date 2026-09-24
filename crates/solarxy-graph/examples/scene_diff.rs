//! Compare two scene files as documents.
//!
//!     cargo run -p solarxy-graph --example scene_diff -- a.slxy b.slxy [--view]
//!
//! The parity checklist's tool: the same input sequence run on the two
//! shells, or one document crossing between them, has to produce the same
//! document, and the eye cannot check that. This loads both files, cooks
//! them to quiescence, and compares what the engine would save.
//!
//! **Semantically, not byte for byte.** Two shells that agree on the
//! document are allowed to disagree on where the boxes sit and on which
//! node happens to be selected, and an add followed by a delete on one
//! shell shifts every identifier it mints afterwards. So nodes are matched
//! by name within their network, wires by the names of the nodes they
//! join, the order of a variadic input by the same names, and positions,
//! identifiers, the selection and the timestamps are dropped. What is left
//! is the document: the types, the parameters, the bypasses, the wiring,
//! the display flags, the cook outcome of every node, and the size of what
//! each displayed node produced.
//!
//! `--view` compares the saved view as well, the pane cameras and display
//! settings. Off by default because the pane count is host state.
//!
//! One line per difference, and a non-zero exit when there is any.

use std::collections::BTreeMap;
use std::process::ExitCode;

use serde_json::{Map, Value, json};
use solarxy_graph::document::{Edge, EdgeId, NodeData, NodeId};
use solarxy_graph::engine::{Engine, EngineEvent};
use solarxy_graph::naming::node_name;
use solarxy_graph::registry::Registry;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let with_view = args.iter().any(|a| a == "--view");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [a, b] = paths.as_slice() else {
        eprintln!("usage: scene_diff <a.slxy> <b.slxy> [--view]");
        return ExitCode::from(2);
    };
    let left = match describe(a, with_view) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("{a}: {e}");
            return ExitCode::from(2);
        }
    };
    let right = match describe(b, with_view) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("{b}: {e}");
            return ExitCode::from(2);
        }
    };
    let mut differences = Vec::new();
    diff("", &left, &right, &mut differences);
    for line in &differences {
        println!("{line}");
    }
    if differences.is_empty() {
        ExitCode::SUCCESS
    } else {
        eprintln!("{} difference(s)", differences.len());
        ExitCode::FAILURE
    }
}

/// Load, cook and describe one scene file.
fn describe(path: &str, with_view: bool) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read: {e}"))?;
    let mut engine = Engine::new().map_err(|e| format!("engine: {e}"))?;
    let loaded = engine
        .load_slxy(&bytes)
        .map_err(|e| format!("cannot open: {e}"))?;
    for warning in &loaded.warnings {
        eprintln!("{path}: opened with a warning: {warning}");
    }

    // Cook to quiescence, keeping the last status the engine reported for
    // each node. The samples cook in a handful of rounds; a bound keeps a
    // scene that never settles from hanging the tool.
    let mut statuses: BTreeMap<NodeId, String> = BTreeMap::new();
    for _ in 0..64 {
        let events = engine.cook(&mut || true);
        if events.is_empty() {
            break;
        }
        for event in events {
            if let EngineEvent::CookStatus { node, status } = event {
                statuses.insert(node, status_word(&status));
            }
        }
    }

    let file = engine.save_document();
    let registry = engine.registry();
    let doc = &file.document;

    // Every node's name, looked up across the whole document, so a subflow
    // can be keyed by the name of the node that owns it wherever that node
    // lives.
    let mut owners: BTreeMap<NodeId, String> = BTreeMap::new();
    for (id, name) in names_in(&doc.root.nodes, registry) {
        owners.insert(id, name);
    }
    for (_, graph) in &doc.subflows {
        for (id, name) in names_in(&graph.nodes, registry) {
            owners.insert(id, name);
        }
    }

    let mut networks = Map::new();
    networks.insert(
        "root".to_string(),
        network(
            &doc.root.nodes,
            &doc.root.edges,
            doc.root.active_output,
            registry,
            &statuses,
        ),
    );
    for (owner, graph) in &doc.subflows {
        let key = owners
            .get(owner)
            .cloned()
            .unwrap_or_else(|| format!("node {}", owner.0));
        networks.insert(
            key,
            network(
                &graph.nodes,
                &graph.edges,
                graph.active_output,
                registry,
                &statuses,
            ),
        );
    }

    let mut displayed = Map::new();
    for (node, set, _) in engine.display_geometries() {
        let key = owners
            .get(&node)
            .cloned()
            .unwrap_or_else(|| format!("node {}", node.0));
        let points: usize = set.meshes.iter().map(|m| m.positions.len()).sum();
        let indices: usize = set.meshes.iter().map(|m| m.indices.len()).sum();
        displayed.insert(
            key,
            json!({ "meshes": set.meshes.len(), "points": points, "indices": indices }),
        );
    }

    let mut out = Map::new();
    out.insert(
        "cookMode".to_string(),
        serde_json::to_value(file.cook_mode).map_err(|e| e.to_string())?,
    );
    out.insert("networks".to_string(), Value::Object(networks));
    out.insert("annotations".to_string(), json!(doc.annotations.len()));
    out.insert("displayed".to_string(), Value::Object(displayed));
    if with_view {
        out.insert(
            "view".to_string(),
            serde_json::to_value(&loaded.sidecar.view).map_err(|e| e.to_string())?,
        );
    }
    Ok(Value::Object(out))
}

/// One network as the tool sees it: nodes by name, wires by name.
fn network(
    nodes: &[NodeData],
    edges: &[Edge],
    active_output: Option<NodeId>,
    registry: &Registry,
    statuses: &BTreeMap<NodeId, String>,
) -> Value {
    let names: BTreeMap<NodeId, String> = names_in(nodes, registry).into_iter().collect();
    let by_edge: BTreeMap<EdgeId, &Edge> = edges.iter().map(|e| (e.id, e)).collect();
    let name_of = |id: NodeId| {
        names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("node {}", id.0))
    };
    let end = |id: NodeId, port: &str| format!("{}.{port}", name_of(id));

    let mut described = Map::new();
    for node in nodes {
        let mut inputs = Map::new();
        for (port, order) in &node.port_order {
            let sources: Vec<Value> = order
                .iter()
                .filter_map(|edge| by_edge.get(edge))
                .map(|edge| Value::String(end(edge.from, &edge.from_port)))
                .collect();
            inputs.insert(port.clone(), Value::Array(sources));
        }
        described.insert(
            name_of(node.id),
            json!({
                "type": node.type_id,
                "version": node.type_version,
                "params": node.params,
                "bypassed": node.bypassed,
                "inputs": inputs,
                "cook": statuses.get(&node.id).cloned().unwrap_or_else(|| "never".to_string()),
            }),
        );
    }

    let mut wires: Vec<String> = edges
        .iter()
        .map(|e| format!("{} -> {}", end(e.from, &e.from_port), end(e.to, &e.to_port)))
        .collect();
    wires.sort();

    json!({
        "nodes": described,
        "wires": wires,
        "activeOutput": active_output.map(name_of),
    })
}

/// The names of a network's nodes, made unique in order so two nodes that
/// share a name still get one entry each.
fn names_in(nodes: &[NodeData], registry: &Registry) -> Vec<(NodeId, String)> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    nodes
        .iter()
        .map(|node| {
            let base = node_name(node, registry);
            let count = seen.entry(base.clone()).or_insert(0);
            *count += 1;
            let name = if *count == 1 {
                base
            } else {
                format!("{base}#{count}")
            };
            (node.id, name)
        })
        .collect()
}

/// A cook status as one word, or the error it carries.
fn status_word(status: &solarxy_graph::cook::state::CookStatus) -> String {
    use solarxy_graph::cook::state::CookStatus;
    match status {
        CookStatus::Pending => "pending".to_string(),
        CookStatus::Cooking => "cooking".to_string(),
        CookStatus::Ok { .. } => "ok".to_string(),
        CookStatus::Error { message } => format!("error: {message}"),
    }
}

/// Walk two values and record every place they part.
fn diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for key in keys {
                let here = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (x.get(key), y.get(key)) {
                    (Some(l), Some(r)) => diff(&here, l, r, out),
                    (Some(l), None) => out.push(format!("{here}: {} != (absent)", short(l))),
                    (None, Some(r)) => out.push(format!("{here}: (absent) != {}", short(r))),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (l, r)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}[{i}]"), l, r, out);
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: {} != {}", short(a), short(b))),
    }
}

/// A value on one line, cut so a long parameter does not swamp the report.
fn short(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 80 {
        let cut: String = text.chars().take(77).collect();
        format!("{cut}...")
    } else {
        text
    }
}
