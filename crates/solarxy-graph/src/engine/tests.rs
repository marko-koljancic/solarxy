//! Engine facade tests: command application, event emission, cook
//! integration, preview non-leakage, and serde round-trips.

use super::*;
use crate::params::ParamValue;

fn engine() -> Engine {
    Engine::new().expect("builtin registry is valid")
}

/// A deterministic host clock for the cook-duration test: each call returns
/// the next integer millisecond, so a single node's two samples (before and
/// after its compute) differ by exactly 1.0. Only the duration test reads
/// it, so the shared counter has one consumer.
static CLOCK_TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn tick_now() -> f64 {
    CLOCK_TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as f64
}

/// Adds a node in a fresh subflow and returns (engine, subflow ctx).
fn subflow_engine() -> (Engine, GraphContext) {
    let mut e = engine();
    // A geo node exists only after N2; for 3b tests we drive a subflow
    // directly by minting one through the document.
    let geo = e.doc.mint_node_id();
    e.doc.create_subflow(geo);
    (e, GraphContext::Subflow(geo))
}

fn add(e: &mut Engine, ctx: GraphContext, ty: &str) -> NodeId {
    let batch = e
        .apply(Command::AddNode {
            ctx,
            node_type: ty.to_string(),
            position: [0.0, 0.0],
        })
        .unwrap();
    match batch.events.iter().find_map(|ev| match ev {
        EngineEvent::NodeAdded { node, .. } => Some(node.id),
        _ => None,
    }) {
        Some(id) => id,
        None => panic!("AddNode emitted no NodeAdded"),
    }
}

#[test]
fn add_node_emits_mirror_and_claims_first_display() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    // First subflow node claims the display flag.
    assert_eq!(e.doc.graph(ctx).unwrap().active_output, Some(box_id));
    // Revision advanced once.
    assert_eq!(e.revision(), 1);
}

#[test]
fn unknown_node_type_and_context_illegal_are_errors() {
    let (mut e, ctx) = subflow_engine();
    assert!(matches!(
        e.apply(Command::AddNode {
            ctx,
            node_type: "nonesuch".to_string(),
            position: [0.0; 2],
        }),
        Err(EngineError::UnknownNodeType(_))
    ));
    // A subflow-only primitive cannot go on the root.
    assert!(matches!(
        e.apply(Command::AddNode {
            ctx: GraphContext::Root,
            node_type: "box".to_string(),
            position: [0.0; 2],
        }),
        Err(EngineError::ContextIllegal { .. })
    ));
}

#[test]
fn connect_type_checks_and_cooks_downstream() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let xform = add(&mut e, ctx, "transform");
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(xform),
    })
    .unwrap();
    let batch = e
        .apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: box_id,
                port: "geometry".to_string(),
            },
            to: PortRefDto {
                node: xform,
                port: "geometry".to_string(),
            },
        })
        .unwrap();
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::EdgeAdded { .. }))
    );

    // Cook: both nodes cook, transform outputs a box.
    let events = e.cook(&mut || true);
    assert!(events.iter().any(|ev| matches!(
        ev,
        EngineEvent::NodeStats { node, points: 24, .. } if *node == xform
    )));
}

#[test]
fn connect_rejects_incompatible_ports() {
    let (mut e, ctx) = subflow_engine();
    // transform.geometry (Geometry in) fed by... there is no non-geometry
    // output in the 3a set, so assert an unknown-port error instead.
    let box_id = add(&mut e, ctx, "box");
    let xform = add(&mut e, ctx, "transform");
    let err = e
        .apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: box_id,
                port: "nonexistent".to_string(),
            },
            to: PortRefDto {
                node: xform,
                port: "geometry".to_string(),
            },
        })
        .unwrap_err();
    assert!(matches!(err, EngineError::UnknownPort { .. }));
}

#[test]
fn set_param_conforms_and_rejects_bad_types() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    // A float into the Int width_segments conforms (rounds).
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width_segments".to_string(),
        value: ParamSource::Literal(ParamValue::Float(3.5)),
    })
    .unwrap();
    let stored = e.doc.graph(ctx).unwrap().node(box_id).unwrap().params["width_segments"].clone();
    assert_eq!(stored, ParamSource::Literal(ParamValue::Int(4)));

    // A nonexistent param is a command error.
    assert!(matches!(
        e.apply(Command::SetParam {
            ctx,
            node: box_id,
            key: "bogus".to_string(),
            value: ParamSource::Literal(ParamValue::Float(1.0)),
        }),
        Err(EngineError::InvalidParam { .. })
    ));
}

#[test]
fn preview_param_does_not_leak_into_document_or_events() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let before = e
        .doc
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params
        .clone();
    let rev_before = e.revision();

    e.preview_param(
        ctx,
        box_id,
        "width",
        ParamSource::Literal(ParamValue::Float(9.0)),
    );

    // No document write, no revision bump (no event batch at all).
    let after = e
        .doc
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params
        .clone();
    assert_eq!(before, after, "preview must not write the document");
    assert_eq!(
        e.revision(),
        rev_before,
        "preview must not advance revision"
    );
    // But it dirtied the node for the next cook.
    assert_eq!(e.cook_state(box_id), CookState::Dirty);

    // The authoritative SetParam clears the overlay and writes.
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".to_string(),
        value: ParamSource::Literal(ParamValue::Float(2.0)),
    })
    .unwrap();
    assert!(e.previews.is_empty());
}

#[test]
fn remove_node_drops_edges_and_forgets_cook_state() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let xform = add(&mut e, ctx, "transform");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: box_id,
            port: "geometry".to_string(),
        },
        to: PortRefDto {
            node: xform,
            port: "geometry".to_string(),
        },
    })
    .unwrap();
    let batch = e
        .apply(Command::RemoveNodes {
            ctx,
            ids: vec![box_id],
        })
        .unwrap();
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::EdgeRemoved { .. }))
    );
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::NodeRemoved { .. }))
    );
    assert_eq!(e.doc.graph(ctx).unwrap().edge_count(), 0);
}

#[test]
fn snapshot_and_registry_snapshot_serialize() {
    let (mut e, ctx) = subflow_engine();
    add(&mut e, ctx, "box");
    let snap = e.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    assert!(json.contains("\"box\""));

    let reg = e.registry_snapshot();
    let json = serde_json::to_string(&reg).unwrap();
    // The palette needs display names and the param schema.
    assert!(json.contains("Box"));
    assert!(json.contains("width_segments"));
    // Degrees unit is surfaced for the transform's rotate param.
    assert!(json.contains("degrees"));
}

#[test]
fn registry_snapshot_carries_the_coercion_matrix() {
    use crate::engine::snapshot::CoercionKind;
    use crate::registry::coerce::DataType;
    let e = engine();
    let reg = e.registry_snapshot();

    let find = |from: DataType, to: DataType| {
        reg.coercions
            .iter()
            .find(|c| c.from == from && c.to == to)
            .map(|c| c.kind)
    };
    // Same, lossless, and lossy cells are all present and labeled.
    assert!(matches!(
        find(DataType::Float, DataType::Float),
        Some(CoercionKind::Same)
    ));
    assert!(matches!(
        find(DataType::Int, DataType::Float),
        Some(CoercionKind::Lossless)
    ));
    assert!(matches!(
        find(DataType::Float, DataType::Int),
        Some(CoercionKind::Lossy)
    ));
    // A forbidden cell is simply absent (frontend treats missing as reject).
    assert!(find(DataType::Geometry, DataType::Float).is_none());

    // It serializes with camelCase kinds and snake_case data types.
    let json = serde_json::to_string(&reg).unwrap();
    assert!(json.contains("\"lossy\""));
    assert!(json.contains("\"geometry\""));
}

#[test]
fn command_round_trips_through_serde() {
    let cmd = Command::SetParam {
        ctx: GraphContext::Root,
        node: NodeId(7),
        key: "radius".to_string(),
        value: ParamSource::Literal(ParamValue::Float(1.5)),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let back: Command = serde_json::from_str(&json).unwrap();
    match back {
        Command::SetParam {
            node, key, value, ..
        } => {
            assert_eq!(node, NodeId(7));
            assert_eq!(key, "radius");
            assert_eq!(value, ParamSource::Literal(ParamValue::Float(1.5)));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn command_boundary_json_shape_is_camelcase() {
    // The wasm boundary contract the frontend depends on: camelCase variant
    // tags and fields, and the `root` / `{subflow: id}` context shape.
    let cmd = Command::AddNode {
        ctx: GraphContext::Root,
        node_type: "box".into(),
        position: [1.0, 2.0],
    };
    let v = serde_json::to_value(&cmd).unwrap();
    assert_eq!(v["type"], "addNode");
    assert_eq!(v["nodeType"], "box");
    assert_eq!(v["ctx"], "root");

    let cmd2 = Command::AddNode {
        ctx: GraphContext::Subflow(NodeId(5)),
        node_type: "sphere".into(),
        position: [0.0, 0.0],
    };
    let v2 = serde_json::to_value(&cmd2).unwrap();
    assert_eq!(v2["ctx"]["subflow"], 5);

    // And it deserializes back from that shape (JS -> Rust).
    let back: Command = serde_json::from_value(v2).unwrap();
    assert!(matches!(
        back,
        Command::AddNode {
            ctx: GraphContext::Subflow(NodeId(5)),
            ..
        }
    ));

    // The event mirror is camelCase too (typeId, not type_id).
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let batch = serde_json::to_value(
        e.apply(Command::AddNode {
            ctx: GraphContext::Subflow(geo),
            node_type: "box".into(),
            position: [0.0, 0.0],
        })
        .unwrap(),
    )
    .unwrap();
    let node_added = batch["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ev| ev["type"] == "nodeAdded")
        .unwrap();
    assert_eq!(node_added["node"]["typeId"], "box");
}

#[test]
fn cook_mode_toggles_and_reports() {
    let mut e = engine();
    assert_eq!(e.cook_mode(), CookMode::Auto);
    let batch = e
        .apply(Command::SetCookMode {
            mode: CookMode::Manual,
        })
        .unwrap();
    assert!(batch.events.iter().any(|ev| matches!(
        ev,
        EngineEvent::CookModeChanged {
            mode: CookMode::Manual
        }
    )));
    assert_eq!(e.cook_mode(), CookMode::Manual);
}

#[test]
fn manual_mode_freezes_cooking_until_cook_now() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    // Auto: the box cooks on the frame.
    e.cook(&mut || true);
    assert_eq!(e.node_geometry_points(box_id), 24);
    assert!(e.dirty_nodes().is_empty());

    // Switch to manual, then edit: the node goes stale but does not cook.
    e.apply(Command::SetCookMode {
        mode: CookMode::Manual,
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    assert_eq!(
        e.dirty_nodes(),
        vec![box_id],
        "the edit marks the node stale"
    );
    let events = e.cook(&mut || true);
    assert!(
        events.is_empty(),
        "manual mode does not cook without CookNow"
    );
    assert_eq!(e.dirty_nodes(), vec![box_id], "still stale");

    // CookNow arms a cook; the next frame drains the stale set.
    e.apply(Command::CookNow).unwrap();
    e.cook(&mut || true);
    assert!(e.dirty_nodes().is_empty(), "CookNow drains the stale set");

    // Switching back to Auto re-cooks any stale set automatically.
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(4.0)),
    })
    .unwrap();
    assert_eq!(e.dirty_nodes(), vec![box_id]);
    e.apply(Command::SetCookMode {
        mode: CookMode::Auto,
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(e.dirty_nodes().is_empty(), "Auto cooks the stale set");
}

// Undo / redo.

/// A structural fingerprint of a subflow: nodes (id, type, params,
/// bypassed), edges (id, endpoints, ports), each node's variadic
/// `port_order`, and the active output. Undo must restore all of it, not
/// just node/edge sets.
fn fingerprint(e: &Engine, ctx: GraphContext) -> String {
    let g = e.document().graph(ctx).unwrap();
    let mut nodes: Vec<String> = g
        .nodes()
        .map(|n| {
            let params = serde_json::to_string(&n.params).unwrap();
            let order = serde_json::to_string(&n.port_order).unwrap();
            format!(
                "N{}:{}:{}:{}:{}",
                n.id.0, n.type_id, n.bypassed, params, order
            )
        })
        .collect();
    nodes.sort();
    let mut edges: Vec<String> = g
        .edges()
        .map(|e| {
            format!(
                "E{}:{}:{}->{}:{}",
                e.id.0, e.from.0, e.from_port, e.to.0, e.to_port
            )
        })
        .collect();
    edges.sort();
    format!(
        "active={:?} nodes=[{}] edges=[{}]",
        g.active_output.map(|n| n.0),
        nodes.join(","),
        edges.join(",")
    )
}

#[test]
fn undo_restores_a_removed_node_with_its_edges_and_order() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let m = add(&mut e, ctx, "merge");
    // Two variadic edges into merge, in a specific order.
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: a,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: m,
            port: "inputs".into(),
        },
    })
    .unwrap();
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: b,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: m,
            port: "inputs".into(),
        },
    })
    .unwrap();
    let before = fingerprint(&e, ctx);

    // Remove the merge node (drops both edges + its port_order).
    e.apply(Command::RemoveNodes { ctx, ids: vec![m] }).unwrap();
    assert_ne!(fingerprint(&e, ctx), before);

    // Undo restores it exactly, including edge ids and port_order.
    e.apply(Command::Undo).unwrap();
    assert_eq!(
        fingerprint(&e, ctx),
        before,
        "undo must restore edge ids and port_order, not just topology"
    );

    // Redo removes it again; undo brings it back (do/undo/redo/undo cycle).
    e.apply(Command::Redo).unwrap();
    e.apply(Command::Undo).unwrap();
    assert_eq!(fingerprint(&e, ctx), before);
}

#[test]
fn undo_all_returns_to_the_initial_document() {
    // A pseudo-random but deterministic command sequence, then undo all,
    // must reproduce the empty subflow.
    let (mut e, ctx) = subflow_engine();
    let initial = fingerprint(&e, ctx);

    let mut applied = 0;
    let types = ["box", "sphere", "transform", "merge"];
    let mut ids = Vec::new();
    for i in 0..12u64 {
        // Deterministic pseudo-choice.
        let choice = (i.wrapping_mul(2654435761) >> 5) % 4;
        match choice {
            0 => ids.push(add(&mut e, ctx, types[(i as usize) % types.len()])),
            1 if ids.len() >= 2 => {
                let from = ids[(i as usize) % ids.len()];
                let to = ids[(i as usize + 1) % ids.len()];
                // May legitimately fail (cycle, occupied, self); ignore.
                let _ = e.apply(Command::Connect {
                    ctx,
                    from: PortRefDto {
                        node: from,
                        port: "geometry".into(),
                    },
                    to: PortRefDto {
                        node: to,
                        port: "geometry".into(),
                    },
                });
                applied += 1;
            }
            2 if !ids.is_empty() => {
                let n = ids[(i as usize) % ids.len()];
                let _ = e.apply(Command::SetParam {
                    ctx,
                    node: n,
                    key: "name".into(),
                    value: ParamSource::Literal(ParamValue::Text(format!("n{i}"))),
                });
                applied += 1;
            }
            3 if !ids.is_empty() => {
                let n = ids[(i as usize) % ids.len()];
                let _ = e.apply(Command::SetBypass {
                    ctx,
                    node: n,
                    bypassed: i % 2 == 0,
                });
                applied += 1;
            }
            _ => {}
        }
    }
    assert!(applied > 0 || !ids.is_empty());
    assert_ne!(fingerprint(&e, ctx), initial);

    // Undo everything.
    for _ in 0..200 {
        e.apply(Command::Undo).unwrap();
    }
    assert_eq!(
        fingerprint(&e, ctx),
        initial,
        "undoing every command must reproduce the initial document"
    );
}

#[test]
fn param_edits_coalesce_within_a_transaction() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let before = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params
        .get("width")
        .cloned();

    // A drag: many SetParams on the same key inside one transaction.
    e.apply(Command::BeginTransaction {
        label: "drag".into(),
    })
    .unwrap();
    for w in [2.0, 3.0, 4.0, 5.0] {
        e.apply(Command::SetParam {
            ctx,
            node: box_id,
            key: "width".into(),
            value: ParamSource::Literal(ParamValue::Float(w)),
        })
        .unwrap();
    }
    e.apply(Command::EndTransaction).unwrap();

    // One undo reverts the whole drag to the pre-drag value.
    e.apply(Command::Undo).unwrap();
    let after = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params
        .get("width")
        .cloned();
    assert_eq!(after, before, "one drag is one undo step");
}

#[test]
fn bypass_times_undo_round_trips() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    assert!(
        !e.document()
            .graph(ctx)
            .unwrap()
            .node(box_id)
            .unwrap()
            .bypassed
    );
    e.apply(Command::SetBypass {
        ctx,
        node: box_id,
        bypassed: true,
    })
    .unwrap();
    assert!(
        e.document()
            .graph(ctx)
            .unwrap()
            .node(box_id)
            .unwrap()
            .bypassed
    );
    e.apply(Command::Undo).unwrap();
    assert!(
        !e.document()
            .graph(ctx)
            .unwrap()
            .node(box_id)
            .unwrap()
            .bypassed
    );
    e.apply(Command::Redo).unwrap();
    assert!(
        e.document()
            .graph(ctx)
            .unwrap()
            .node(box_id)
            .unwrap()
            .bypassed
    );
}

// Clipboard.

#[test]
fn copy_paste_remaps_ids_and_preserves_internal_edges() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let xform = add(&mut e, ctx, "transform");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: box_id,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: xform,
            port: "geometry".into(),
        },
    })
    .unwrap();

    let before_nodes = e.document().graph(ctx).unwrap().node_count();
    let before_edges = e.document().graph(ctx).unwrap().edge_count();

    // Copy both, paste with an offset.
    let fragment = e.copy_nodes(ctx, &[box_id, xform]);
    e.apply(Command::PasteNodes {
        ctx,
        fragment,
        position: [100.0, 0.0],
    })
    .unwrap();

    let g = e.document().graph(ctx).unwrap();
    // Two new nodes, one new internal edge.
    assert_eq!(g.node_count(), before_nodes + 2);
    assert_eq!(g.edge_count(), before_edges + 1);
    // The pasted nodes have fresh ids (not the originals).
    let pasted: Vec<NodeId> = g
        .nodes()
        .map(|n| n.id)
        .filter(|id| *id != box_id && *id != xform)
        .collect();
    assert_eq!(pasted.len(), 2);
    // The pasted internal edge connects two pasted nodes (remapped).
    let pasted_set: std::collections::BTreeSet<NodeId> = pasted.iter().copied().collect();
    let new_edge = g
        .edges()
        .find(|edge| pasted_set.contains(&edge.from) && pasted_set.contains(&edge.to));
    assert!(
        new_edge.is_some(),
        "internal edge must be remapped between pasted nodes"
    );
}

#[test]
fn fragment_round_trips_through_json() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let m = add(&mut e, ctx, "merge");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: box_id,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: m,
            port: "inputs".into(),
        },
    })
    .unwrap();

    let fragment = e.copy_nodes(ctx, &[box_id, m]);
    // The clipboard serializes the fragment to JSON and back.
    let json = serde_json::to_string(&fragment).unwrap();
    let restored: crate::document::GraphFragment = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.nodes.len(), 2);
    assert_eq!(restored.edges.len(), 1);

    // Pasting the round-tripped fragment reconstructs the structure.
    let before = e.document().graph(ctx).unwrap().node_count();
    e.apply(Command::PasteNodes {
        ctx,
        fragment: restored,
        position: [0.0, 0.0],
    })
    .unwrap();
    assert_eq!(e.document().graph(ctx).unwrap().node_count(), before + 2);
}

#[test]
fn duplicate_then_undo_removes_the_copies() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let before = e.document().graph(ctx).unwrap().node_count();

    e.apply(Command::DuplicateNodes {
        ctx,
        ids: vec![box_id],
    })
    .unwrap();
    assert_eq!(e.document().graph(ctx).unwrap().node_count(), before + 1);

    // Undo removes the duplicate.
    e.apply(Command::Undo).unwrap();
    assert_eq!(e.document().graph(ctx).unwrap().node_count(), before);
}

#[test]
fn paste_skips_context_illegal_nodes() {
    // A subflow primitive cannot paste onto the root.
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let fragment = e.copy_nodes(ctx, &[box_id]);
    let before = e.document().graph(GraphContext::Root).unwrap().node_count();
    e.apply(Command::PasteNodes {
        ctx: GraphContext::Root,
        fragment,
        position: [0.0, 0.0],
    })
    .unwrap();
    // Nothing pasted (box is subflow-only).
    assert_eq!(
        e.document().graph(GraphContext::Root).unwrap().node_count(),
        before
    );
}

// Review annotations.

#[test]
fn annotation_crud_and_undo() {
    use crate::review::{ReviewAnchor, ReviewCategory};
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let anchor = ReviewAnchor {
        ctx,
        node: box_id,
        face: None,
        barycentric: None,
    };

    // Add.
    let batch = e
        .apply(Command::AddAnnotation {
            anchor: anchor.clone(),
            text: "check this face".into(),
            category: ReviewCategory::Issue,
        })
        .unwrap();
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::ReviewChanged))
    );
    assert_eq!(e.document().review().len(), 1);
    let id = e.document().review().iter().next().unwrap().id;

    // Resolve, then undo the resolve.
    e.apply(Command::ResolveAnnotation { id, resolved: true })
        .unwrap();
    assert!(e.document().review().get(id).unwrap().resolved);
    e.apply(Command::Undo).unwrap();
    assert!(!e.document().review().get(id).unwrap().resolved);

    // Delete, then undo restores it verbatim.
    e.apply(Command::DeleteAnnotation { id }).unwrap();
    assert!(e.document().review().is_empty());
    e.apply(Command::Undo).unwrap();
    assert_eq!(e.document().review().len(), 1);
    assert_eq!(
        e.document().review().get(id).unwrap().text,
        "check this face"
    );
}

#[test]
fn take_scene_delta_is_empty_without_geo_or_lights() {
    // Root holds no geo/light nodes yet, so only the light list op (empty)
    // is emitted.
    let mut e = engine();
    let delta = e.take_scene_delta();
    assert_eq!(delta.ops.len(), 1);
    assert!(matches!(
        delta.ops[0],
        solarxy_core::scene::SceneOp::SetLights { ref lights } if lights.is_empty()
    ));
}

#[test]
fn scene_delta_maps_a_geo_container_and_lights() {
    use solarxy_core::scene::{LightKind, SceneObjectId, SceneOp};
    let mut e = engine();

    // A geo container at root, with a box in its subflow.
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box_id = add(&mut e, sub, "box");
    // The subflow's first node claimed display automatically.
    assert_eq!(e.document().graph(sub).unwrap().active_output, Some(box_id));

    // A point light at root.
    let _light = add(&mut e, GraphContext::Root, "point_light");

    // Cook (root + subflow), then build the delta.
    e.cook(&mut || true);
    let delta = e.take_scene_delta();

    // The geo maps to an UpsertGeometry (the box) + SetTransform under a
    // scene id derived from the geo node.
    let object_id = SceneObjectId(geo.0);
    let upsert = delta.ops.iter().find_map(|op| match op {
        SceneOp::UpsertGeometry { id, geometry } if *id == object_id => Some(geometry),
        _ => None,
    });
    assert!(upsert.is_some(), "geo should upsert its display geometry");
    assert_eq!(upsert.unwrap().meshes.len(), 1); // the box
    assert!(delta.ops.iter().any(|op| matches!(
        op,
        SceneOp::SetTransform { id, .. } if *id == object_id
    )));

    // The point light lands in the SetLights op.
    let lights = delta.ops.iter().find_map(|op| match op {
        SceneOp::SetLights { lights } => Some(lights),
        _ => None,
    });
    let lights = lights.expect("SetLights op present");
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].kind, LightKind::Point);
    // Default point-light position (10, 10, 5).
    assert!((lights[0].position[0] - 10.0).abs() < 1e-5);
}

// Async import job protocol (deferred-drain, generation guard).

const TRI_STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";

/// Sets up an engine in async mode with one staged STL and an import node
/// in a subflow, cooked once (so the import returns Pending). Returns the
/// engine, ctx, node id, and the spawned (job, request).
fn async_import_fixture() -> (Engine, GraphContext, NodeId, JobId, crate::cook::JobRequest) {
    let mut e = engine();
    e.set_async_jobs(true);
    let asset = e.stage_asset("tri.stl", TRI_STL.as_bytes().to_vec());

    let geo = e.doc.mint_node_id();
    e.doc.create_subflow(geo);
    let ctx = GraphContext::Subflow(geo);
    let node = add(&mut e, ctx, "import_stl");
    e.apply(Command::SetParam {
        ctx,
        node,
        key: "file".into(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();

    // First cook: the import spawns a job and parks Pending.
    e.cook(&mut || true);
    let jobs = e.take_jobs();
    assert_eq!(jobs.len(), 1, "the import should spawn exactly one job");
    let (job_ctx, job, request) = jobs.into_iter().next().unwrap();
    assert_eq!(job_ctx, ctx);
    (e, ctx, node, job, request)
}

#[test]
fn fresh_job_result_commits_geometry() {
    let (mut e, ctx, node, job, request) = async_import_fixture();
    // Resolve and submit the fresh result.
    let result = e.resolve_job(&request);
    let events = e.submit_job_result(ctx, job, result);
    assert!(events.iter().any(|ev| matches!(
        ev,
        EngineEvent::NodeStats { node: n, prims: 1, .. } if *n == node
    )));
    assert_eq!(e.node_geometry_points(node), 3); // one triangle
}

#[test]
fn stale_job_result_is_dropped_by_the_generation_guard() {
    let (mut e, ctx, node, stale_job, stale_request) = async_import_fixture();

    // Re-dirty the import node (a param edit) before its result lands: this
    // bumps the node generation, superseding the in-flight job.
    e.apply(Command::SetParam {
        ctx,
        node,
        key: "scale".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    // Re-cook: a NEW job spawns for the superseded node.
    e.cook(&mut || true);
    let fresh_jobs = e.take_jobs();
    assert_eq!(fresh_jobs.len(), 1, "the re-dirtied node respawns one job");
    let (_, fresh_job, fresh_request) = fresh_jobs.into_iter().next().unwrap();
    assert_ne!(fresh_job.0, stale_job.0, "a new job id is minted");

    // Submit the STALE result first: it must be dropped (no commit).
    let stale_result = e.resolve_job(&stale_request);
    let events = e.submit_job_result(ctx, stale_job, stale_result);
    assert!(events.is_empty(), "a stale result produces no events");
    assert_eq!(
        e.node_geometry_points(node),
        0,
        "stale result must not commit"
    );

    // Submit the FRESH result: it commits (scaled by 3, still one triangle).
    let fresh_result = e.resolve_job(&fresh_request);
    e.submit_job_result(ctx, fresh_job, fresh_result);
    assert_eq!(e.node_geometry_points(node), 3);
}

// Phase-4 additions: picking, document save/load, host-clocked durations.

#[test]
fn pick_returns_the_geo_container_under_the_ray() {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    add(&mut e, sub, "box"); // origin-centered, claims display
    e.cook(&mut || true);

    // A ray down the +Z axis toward the origin hits the box's geo.
    assert_eq!(e.pick([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]), Some(geo));
    // The same ray reversed points away: no hit.
    assert_eq!(e.pick([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]), None);
    // A ray that misses the box entirely.
    assert_eq!(e.pick([100.0, 100.0, 100.0], [1.0, 0.0, 0.0]), None);
    // A degenerate (zero-length) direction is rejected.
    assert_eq!(e.pick([0.0, 0.0, 5.0], [0.0, 0.0, 0.0]), None);
}

#[test]
fn save_and_load_document_round_trips_and_emits_replaced() {
    // Build a root geo with a box in its subflow, plus a param edit.
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box_id = add(&mut e, sub, "box");
    e.apply(Command::SetParam {
        ctx: sub,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    e.apply(Command::SetCookMode {
        mode: CookMode::Manual,
    })
    .unwrap();

    // Save, JSON round-trip, load into a fresh engine.
    let file = e.save_document();
    let json = serde_json::to_string(&file).unwrap();
    let restored: DocumentFile = serde_json::from_str(&json).unwrap();

    let mut e2 = engine();
    let rev_before = e2.revision();
    let batch = e2.load_document(&restored);
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::DocumentReplaced)),
        "load emits a single DocumentReplaced"
    );
    assert!(e2.revision() > rev_before, "load advances the revision");

    // Structure, params, display flag, and cook mode restored.
    {
        assert!(
            e2.document()
                .graph(GraphContext::Root)
                .unwrap()
                .node(geo)
                .is_some()
        );
        let g = e2.document().graph(sub).unwrap();
        assert_eq!(g.active_output, Some(box_id));
        assert_eq!(
            g.node(box_id).unwrap().params["width"],
            ParamSource::Literal(ParamValue::Float(3.0))
        );
    }
    assert_eq!(e2.cook_mode(), CookMode::Manual);

    // The loaded document cooks and picks like the original.
    e2.cook(&mut || true);
    assert_eq!(e2.pick([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]), Some(geo));

    // A freshly minted node gets a brand-new id (the mint resumed past the
    // loaded ids), so it is distinct from the loaded box.
    let extra = add(&mut e2, sub, "sphere");
    assert_ne!(extra, box_id);
    assert_ne!(extra, geo);
}

#[test]
fn cook_durations_use_the_installed_clock() {
    let (mut e, ctx) = subflow_engine();
    e.set_clock(tick_now);
    let box_id = add(&mut e, ctx, "box");
    let events = e.cook(&mut || true);
    // The box's success status carries a real (non-zero) millisecond time.
    let ms = events.iter().find_map(|ev| match ev {
        EngineEvent::CookStatus {
            node,
            status: CookStatus::Ok { ms },
        } if *node == box_id => Some(*ms),
        _ => None,
    });
    assert!(
        matches!(ms, Some(m) if m > 0.0),
        "installed clock yields a non-zero cook duration, got {ms:?}"
    );
}

#[test]
fn cook_status_ms_stays_zero_without_a_clock() {
    // The native/test default (no clock) preserves the Phase-3 behavior.
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let events = e.cook(&mut || true);
    let ms = events.iter().find_map(|ev| match ev {
        EngineEvent::CookStatus {
            node,
            status: CookStatus::Ok { ms },
        } if *node == box_id => Some(*ms),
        _ => None,
    });
    assert_eq!(ms, Some(0.0));
}

#[test]
fn spot_light_angle_resolves_to_radians_in_the_scene() {
    use solarxy_core::scene::{LightKind, SceneOp};
    let mut e = engine();
    add(&mut e, GraphContext::Root, "spot_light");
    e.cook(&mut || true);
    let delta = e.take_scene_delta();
    let lights = delta
        .ops
        .iter()
        .find_map(|op| match op {
            SceneOp::SetLights { lights } => Some(lights),
            _ => None,
        })
        .unwrap();
    assert_eq!(lights[0].kind, LightKind::Spot);
    // Default angle 45 degrees -> the outer cone in radians.
    assert!((lights[0].outer_cone - 45.0_f32.to_radians()).abs() < 1e-4);
}
