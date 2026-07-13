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
    // The Title Case category label rides beside the stable snake_case id.
    assert!(json.contains("\"categoryLabel\":\"Primitives\""));
    assert!(json.contains("\"category\":\"primitives\""));
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
        mesh: None,
        face: None,
        barycentric: None,
        world_fallback: None,
        geometry_hash: None,
    };

    // Add.
    let batch = e
        .apply(Command::AddAnnotation {
            anchor: anchor.clone(),
            text: "check this face".into(),
            category: ReviewCategory::Warning,
            author: None,
            created_at: String::new(),
            reply_to: None,
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
    e.apply(Command::ResolveAnnotation {
        id,
        resolved: true,
        updated_at: String::new(),
    })
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

// Phase 7 review: anchoring, threading, staleness, markers, detailed picks.

/// A displayable scene: a root geo whose subflow holds one default box
/// (display flag claimed), cooked. Returns (engine, geo id, subflow ctx,
/// box id).
fn displayed_box() -> (Engine, NodeId, GraphContext, NodeId) {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box_id = add(&mut e, sub, "box");
    e.cook(&mut || true);
    (e, geo, sub, box_id)
}

/// Picks the default box's front face dead-on and returns a full anchor.
fn picked_anchor(e: &Engine) -> crate::review::ReviewAnchor {
    let pd = e
        .pick_detailed([0.0, 0.0, 10.0], [0.0, 0.0, -1.0])
        .expect("the displayed box is hit");
    crate::review::ReviewAnchor {
        ctx: GraphContext::Root,
        node: pd.node,
        mesh: Some(pd.mesh),
        face: Some(pd.face),
        barycentric: Some(pd.barycentric),
        world_fallback: Some(pd.world_pos),
        geometry_hash: None, // engine-filled on add
    }
}

fn add_picked_note(
    e: &mut Engine,
    text: &str,
    reply_to: Option<crate::review::AnnotationId>,
) -> crate::review::AnnotationId {
    let anchor = picked_anchor(e);
    add_note(e, anchor, text, reply_to)
}

fn add_note(
    e: &mut Engine,
    anchor: crate::review::ReviewAnchor,
    text: &str,
    reply_to: Option<crate::review::AnnotationId>,
) -> crate::review::AnnotationId {
    let before: std::collections::BTreeSet<_> =
        e.document().review().iter().map(|a| a.id).collect();
    e.apply(Command::AddAnnotation {
        anchor,
        text: text.into(),
        category: crate::review::ReviewCategory::Question,
        author: Some("Tester".into()),
        created_at: "2026-07-10T09:00:00Z".into(),
        reply_to,
    })
    .unwrap();
    e.document()
        .review()
        .iter()
        .map(|a| a.id)
        .find(|id| !before.contains(id))
        .expect("add minted a new annotation")
}

#[test]
fn add_fills_the_geometry_hash_engine_side() {
    let (mut e, ..) = displayed_box();
    let anchor = picked_anchor(&e);
    let id = add_note(&mut e, anchor, "front face", None);
    let stored = &e.document().review().get(id).unwrap().anchor;
    assert!(
        stored.geometry_hash.is_some(),
        "3D anchors get their staleness reference filled on add"
    );
    assert!(!e.annotation_stale(id), "freshly pinned is never stale");
}

#[test]
fn reply_inherits_the_parent_anchor_and_threading_stays_flat() {
    let (mut e, geo, ..) = displayed_box();
    let parent = add_picked_note(&mut e, "parent", None);

    // The reply sends a deliberately different (node-only) anchor; the
    // engine must ignore it and copy the parent's.
    let decoy = crate::review::ReviewAnchor {
        ctx: GraphContext::Root,
        node: geo,
        mesh: None,
        face: None,
        barycentric: None,
        world_fallback: None,
        geometry_hash: None,
    };
    let reply = add_note(&mut e, decoy.clone(), "reply", Some(parent));
    let store = e.document().review();
    assert_eq!(
        store.get(reply).unwrap().anchor,
        store.get(parent).unwrap().anchor
    );
    assert_eq!(store.get(reply).unwrap().reply_to, Some(parent));

    // Reply-to-reply is rejected (flat threading).
    let err = e
        .apply(Command::AddAnnotation {
            anchor: decoy.clone(),
            text: "nested".into(),
            category: crate::review::ReviewCategory::Info,
            author: None,
            created_at: String::new(),
            reply_to: Some(reply),
        })
        .unwrap_err();
    assert!(matches!(
        err,
        EngineError::Graph(GraphError::InvalidReply(_))
    ));

    // Replying to a missing parent is rejected.
    let err = e
        .apply(Command::AddAnnotation {
            anchor: decoy,
            text: "orphan".into(),
            category: crate::review::ReviewCategory::Info,
            author: None,
            created_at: String::new(),
            reply_to: Some(crate::review::AnnotationId(9999)),
        })
        .unwrap_err();
    assert!(matches!(
        err,
        EngineError::Graph(GraphError::UnknownAnnotation(_))
    ));
}

#[test]
fn delete_cascades_to_replies_and_one_undo_restores_the_thread() {
    let (mut e, ..) = displayed_box();
    let parent = add_picked_note(&mut e, "parent", None);
    add_picked_note(&mut e, "reply 1", Some(parent));
    add_picked_note(&mut e, "reply 2", Some(parent));
    let bystander = add_picked_note(&mut e, "unrelated", None);
    assert_eq!(e.document().review().len(), 4);

    e.apply(Command::DeleteAnnotation { id: parent }).unwrap();
    assert_eq!(e.document().review().len(), 1);
    assert!(e.document().review().get(bystander).is_some());

    // One undo restores the whole thread (whole-store snapshot).
    e.apply(Command::Undo).unwrap();
    assert_eq!(e.document().review().len(), 4);
    e.apply(Command::Redo).unwrap();
    assert_eq!(e.document().review().len(), 1);
}

#[test]
fn recook_that_changes_geometry_flags_stale_and_undo_recook_clears_it() {
    let (mut e, _geo, sub, box_id) = displayed_box();
    let id = add_picked_note(&mut e, "watch this", None);
    assert!(!e.annotation_stale(id));

    // Widen the box: the quantized AABB (and topology-independent hash
    // inputs) change, so the anchor must flag on the recook.
    e.apply(Command::SetParam {
        ctx: sub,
        node: box_id,
        key: "width".to_string(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    let events = e.cook(&mut || true);
    let review_changed = events
        .iter()
        .filter(|ev| matches!(ev, EngineEvent::ReviewChanged))
        .count();
    assert_eq!(review_changed, 1, "exactly one coalesced ReviewChanged");
    assert!(e.annotation_stale(id));

    // The snapshot mirrors the runtime flag.
    let snap = serde_json::to_value(e.snapshot()).unwrap();
    assert_eq!(snap["annotations"][0]["needsReanchor"], true);

    // Undo the param edit and recook: the original hash matches again.
    e.apply(Command::Undo).unwrap();
    let events = e.cook(&mut || true);
    assert!(
        events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::ReviewChanged))
    );
    assert!(!e.annotation_stale(id));
}

#[test]
fn losing_the_display_output_flags_stale_immediately() {
    let (mut e, _geo, sub, _box_id) = displayed_box();
    let id = add_picked_note(&mut e, "pin", None);
    let batch = e
        .apply(Command::SetActiveOutput {
            ctx: sub,
            node: None,
        })
        .unwrap();
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::ReviewChanged)),
        "the flag flips in the same batch, before any cook"
    );
    assert!(e.annotation_stale(id));
}

#[test]
fn reanchor_updates_hash_clears_stale_and_propagates_to_replies() {
    let (mut e, _geo, sub, box_id) = displayed_box();
    let parent = add_picked_note(&mut e, "parent", None);
    let reply = add_picked_note(&mut e, "reply", Some(parent));

    // Invalidate the pin.
    e.apply(Command::SetParam {
        ctx: sub,
        node: box_id,
        key: "width".to_string(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(e.annotation_stale(parent));
    let old_anchor = e.document().review().get(parent).unwrap().anchor.clone();

    // Re-place on the recooked geometry: same batch clears the flag.
    let fresh = picked_anchor(&e);
    let batch = e
        .apply(Command::ReanchorAnnotation {
            id: parent,
            anchor: fresh,
            updated_at: "2026-07-10T10:00:00Z".into(),
        })
        .unwrap();
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::ReviewChanged))
    );
    assert!(!e.annotation_stale(parent));
    let store = e.document().review();
    let new_anchor = store.get(parent).unwrap().anchor.clone();
    assert_ne!(new_anchor, old_anchor);
    assert_eq!(
        store.get(reply).unwrap().anchor,
        new_anchor,
        "replies follow the parent's pin"
    );
    assert_eq!(
        store.get(parent).unwrap().updated_at,
        "2026-07-10T10:00:00Z"
    );

    // Re-anchoring a reply directly is rejected.
    let fresh = picked_anchor(&e);
    assert!(matches!(
        e.apply(Command::ReanchorAnnotation {
            id: reply,
            anchor: fresh,
            updated_at: String::new(),
        }),
        Err(EngineError::Graph(GraphError::InvalidReply(_)))
    ));

    // Undo the re-anchor: the old (stale) anchor returns and the flag
    // flips back in the same batch.
    e.apply(Command::Undo).unwrap();
    assert_eq!(
        e.document().review().get(parent).unwrap().anchor,
        old_anchor
    );
    assert!(e.annotation_stale(parent));
}

#[test]
fn markers_resolve_through_the_geo_transform_without_flagging_stale() {
    let (mut e, geo, ..) = displayed_box();
    let id = add_picked_note(&mut e, "front", None);
    let markers = e.review_markers_world();
    assert_eq!(markers.len(), 1);
    let before = markers[0].world.expect("3D anchor has a pin");
    assert!((before[2] - 0.5).abs() < 1e-4, "front face of the unit box");

    // Moving the geo container translates the marker but must NOT flag it:
    // the transform is applied at lowering, not baked into the geometry.
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: geo,
        key: "translate".to_string(),
        value: ParamSource::Literal(ParamValue::Vec3([5.0, 0.0, 0.0])),
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(
        !e.annotation_stale(id),
        "a rigid transform is not staleness"
    );
    let markers = e.review_markers_world();
    let after = markers[0].world.unwrap();
    assert!((after[0] - (before[0] + 5.0)).abs() < 1e-4);
    assert!(!markers[0].needs_reanchor);

    // Replies never appear as markers (the anchor is inherited from the
    // parent regardless of what the host sends, so a decoy suffices; the
    // pick ray would miss the translated geo anyway).
    let decoy = crate::review::ReviewAnchor {
        ctx: GraphContext::Root,
        node: geo,
        mesh: None,
        face: None,
        barycentric: None,
        world_fallback: None,
        geometry_hash: None,
    };
    add_note(&mut e, decoy, "reply", Some(id));
    assert_eq!(e.review_markers_world().len(), 1);
}

#[test]
fn pick_detailed_reports_the_mesh_within_a_merged_set() {
    // Subflow: box0 at origin, box1 pushed +3x through a transform, both
    // merged (mesh order = connection order); geo translated +5x.
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box0 = add(&mut e, sub, "box");
    let box1 = add(&mut e, sub, "box");
    let xform = add(&mut e, sub, "transform");
    let merge = add(&mut e, sub, "merge");
    let connect = |e: &mut Engine, from: NodeId, to: NodeId, port: &str| {
        e.apply(Command::Connect {
            ctx: sub,
            from: PortRefDto {
                node: from,
                port: "geometry".to_string(),
            },
            to: PortRefDto {
                node: to,
                port: port.to_string(),
            },
        })
        .unwrap();
    };
    connect(&mut e, box1, xform, "geometry");
    connect(&mut e, box0, merge, "inputs");
    connect(&mut e, xform, merge, "inputs");
    e.apply(Command::SetParam {
        ctx: sub,
        node: xform,
        key: "translate".to_string(),
        value: ParamSource::Literal(ParamValue::Vec3([3.0, 0.0, 0.0])),
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: sub,
        node: Some(merge),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: geo,
        key: "translate".to_string(),
        value: ParamSource::Literal(ParamValue::Vec3([5.0, 0.0, 0.0])),
    })
    .unwrap();
    e.cook(&mut || true);

    // Dead-on at the second box: world center [8, 0, 0], front z = +0.5.
    let pd = e
        .pick_detailed([8.0, 0.0, 10.0], [0.0, 0.0, -1.0])
        .expect("hit");
    assert_eq!(pd.node, geo);
    assert_eq!(pd.mesh, 1, "the transformed box is the second merged mesh");
    assert!((pd.world_pos[0] - 8.0).abs() < 1e-3);
    assert!((pd.world_pos[2] - 0.5).abs() < 1e-3);
    assert!((pd.distance - 9.5).abs() < 1e-3);
    let bary_sum: f32 = pd.barycentric.iter().sum();
    assert!((bary_sum - 1.0).abs() < 1e-3);

    // The first box resolves as mesh 0.
    let pd0 = e
        .pick_detailed([5.0, 0.0, 10.0], [0.0, 0.0, -1.0])
        .expect("hit");
    assert_eq!(pd0.mesh, 0);
}

#[test]
fn annotations_round_trip_the_document_file_with_all_fields() {
    let (mut e, ..) = displayed_box();
    let parent = add_picked_note(&mut e, "note", None);
    add_picked_note(&mut e, "reply", Some(parent));
    e.apply(Command::ResolveAnnotation {
        id: parent,
        resolved: true,
        updated_at: "2026-07-10T11:00:00Z".into(),
    })
    .unwrap();

    let json = serde_json::to_string(&e.save_document()).unwrap();
    let file: DocumentFile = serde_json::from_str(&json).unwrap();
    let mut e2 = engine();
    e2.load_document(&file);

    let a: Vec<_> = e.document().review().iter().cloned().collect();
    let b: Vec<_> = e2.document().review().iter().cloned().collect();
    assert_eq!(a, b, "author, timestamps, threading, and anchors survive");
}

#[test]
fn review_command_boundary_shape_is_camelcase() {
    let (mut e, ..) = displayed_box();
    let id = add_picked_note(&mut e, "note", None);

    // Rust -> JS: camelCase tags and fields.
    let cmd = Command::ReanchorAnnotation {
        id,
        anchor: picked_anchor(&e),
        updated_at: "t".into(),
    };
    let v = serde_json::to_value(&cmd).unwrap();
    assert_eq!(v["type"], "reanchorAnnotation");
    assert!(v["anchor"]["worldFallback"].is_array());
    assert_eq!(v["updatedAt"], "t");

    // JS -> Rust: a minimal AddAnnotation payload (no author/createdAt/
    // replyTo, bare anchor) deserializes through the serde defaults.
    let js = serde_json::json!({
        "type": "addAnnotation",
        "anchor": { "ctx": "root", "node": 1 },
        "text": "from js",
        "category": "change"
    });
    let back: Command = serde_json::from_value(js).unwrap();
    assert!(matches!(
        back,
        Command::AddAnnotation {
            reply_to: None,
            author: None,
            ..
        }
    ));

    // The snapshot mirror is camelCase with the runtime flag flattened in.
    let snap = serde_json::to_value(e.snapshot()).unwrap();
    let a = &snap["annotations"][0];
    assert_eq!(a["needsReanchor"], false);
    assert_eq!(a["createdAt"], "2026-07-10T09:00:00Z");
    assert_eq!(a["author"], "Tester");
    assert!(a["anchor"]["geometryHash"].is_u64());
}

#[test]
fn granting_cast_shadow_releases_every_other_root_light_in_one_step() {
    // The exclusive-shadow-caster rule (UX spec J3): the handoff cascades
    // inside the same command, so it is one undo step and the batch names
    // the released lights via their ParamChanged events.
    let mut e = engine();
    let a = add(&mut e, GraphContext::Root, "directional_light");
    let b = add(&mut e, GraphContext::Root, "spot_light");

    let resolved_flag = |e: &Engine, id: NodeId| -> bool {
        let g = e.document().graph(GraphContext::Root).unwrap();
        let n = g.node(id).unwrap();
        let d = e.registry().get(&n.type_id).unwrap();
        crate::registry::resolve::resolve_params(&n.params, &d.params)
            .unwrap()
            .bool("cast_shadow")
    };
    // Shadow-capable lights default the flag on.
    assert!(resolved_flag(&e, a) && resolved_flag(&e, b));

    // Granting the shadow to B releases A in the same batch.
    let batch = e
        .apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: b,
            key: "cast_shadow".to_string(),
            value: ParamSource::Literal(ParamValue::Bool(true)),
        })
        .unwrap();
    assert!(!resolved_flag(&e, a), "A released the shadow");
    assert!(resolved_flag(&e, b));
    let released: Vec<NodeId> = batch
        .events
        .iter()
        .filter_map(|ev| match ev {
            EngineEvent::ParamChanged {
                node,
                key,
                value: ParamSource::Literal(ParamValue::Bool(false)),
                ..
            } if key == "cast_shadow" => Some(*node),
            _ => None,
        })
        .collect();
    assert_eq!(released, vec![a], "the batch names the released light");

    // One undo restores BOTH flags (single step by construction).
    e.apply(Command::Undo).unwrap();
    assert!(resolved_flag(&e, a), "undo restores the released light");

    // Setting the flag FALSE never cascades.
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: b,
        key: "cast_shadow".to_string(),
        value: ParamSource::Literal(ParamValue::Bool(false)),
    })
    .unwrap();
    assert!(
        resolved_flag(&e, a),
        "clearing a flag releases nothing else"
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

// Phase 8 root visibility: hidden-but-cooked, picking and marker gates.

/// Sets a root-level bool param (the visibility / shadow toggles).
fn set_root_bool(e: &mut Engine, node: NodeId, key: &str, value: bool) {
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node,
        key: key.into(),
        value: ParamSource::Literal(ParamValue::Bool(value)),
    })
    .unwrap();
}

#[test]
fn hidden_geo_emits_set_visible_false_and_stays_cooked() {
    use solarxy_core::scene::{SceneObjectId, SceneOp};
    let (mut e, geo, ..) = displayed_box();
    let object_id = SceneObjectId(geo.0);

    // Visible by default: the producer emits the flag every pass.
    let delta = e.take_scene_delta();
    assert!(delta.ops.iter().any(|op| matches!(
        op,
        SceneOp::SetVisible { id, visible: true } if *id == object_id
    )));
    let before = e.display_geometries();
    assert_eq!(before.len(), 1);
    let warm = std::sync::Arc::clone(&before[0].1);

    // Hide: SetVisible false, while the geometry upsert remains
    // (hidden-but-cooked, GPU-resident for instant re-show) and the
    // visualization aggregation drops the object.
    set_root_bool(&mut e, geo, "visible", false);
    e.cook(&mut || true);
    let delta = e.take_scene_delta();
    assert!(delta.ops.iter().any(|op| matches!(
        op,
        SceneOp::SetVisible { id, visible: false } if *id == object_id
    )));
    assert!(delta.ops.iter().any(|op| matches!(
        op,
        SceneOp::UpsertGeometry { id, .. } if *id == object_id
    )));
    assert!(e.display_geometries().is_empty());

    // Re-show and cook: the display output is the same Arc, proving the
    // toggle never invalidated the cook (no staleness cliff).
    set_root_bool(&mut e, geo, "visible", true);
    e.cook(&mut || true);
    let after = e.display_geometries();
    assert_eq!(after.len(), 1);
    assert!(std::sync::Arc::ptr_eq(&warm, &after[0].1));
}

#[test]
fn invisible_root_light_is_flagged_for_the_renderer_gate() {
    use solarxy_core::scene::SceneOp;
    let mut e = engine();
    let light = add(&mut e, GraphContext::Root, "point_light");
    set_root_bool(&mut e, light, "visible", false);
    let delta = e.take_scene_delta();
    let lights = delta
        .ops
        .iter()
        .find_map(|op| match op {
            SceneOp::SetLights { lights } => Some(lights),
            _ => None,
        })
        .expect("SetLights op present");
    // The def still lands in the list; the renderer's light loop filters
    // on the flag (light.rs), so the contribution disappears.
    assert_eq!(lights.len(), 1);
    assert!(!lights[0].visible);
}

#[test]
fn geo_cast_shadow_toggle_reaches_the_delta_and_spares_the_lights() {
    use solarxy_core::scene::{SceneObjectId, SceneOp};
    let (mut e, geo, ..) = displayed_box();
    let light = add(&mut e, GraphContext::Root, "point_light");
    let object_id = SceneObjectId(geo.0);

    // On by default.
    let delta = e.take_scene_delta();
    assert!(delta.ops.iter().any(|op| matches!(
        op,
        SceneOp::SetCastShadow { id, cast_shadow: true } if *id == object_id
    )));

    // Toggling the geo flag reaches the delta and is orthogonal to
    // visibility (the object still renders) and to the light-side
    // exclusive-caster rule (a geo is not a light; nothing is released).
    set_root_bool(&mut e, geo, "cast_shadow", false);
    let delta = e.take_scene_delta();
    assert!(delta.ops.iter().any(|op| matches!(
        op,
        SceneOp::SetCastShadow { id, cast_shadow: false } if *id == object_id
    )));
    assert!(delta.ops.iter().any(|op| matches!(
        op,
        SceneOp::SetVisible { id, visible: true } if *id == object_id
    )));
    let lights = delta
        .ops
        .iter()
        .find_map(|op| match op {
            SceneOp::SetLights { lights } => Some(lights),
            _ => None,
        })
        .expect("SetLights op present");
    assert!(
        lights[0].cast_shadow,
        "the light keeps its shadow; the geo toggle is per-object participation"
    );
    let _ = light;
}

#[test]
fn hidden_geo_is_not_pickable() {
    let (mut e, geo, ..) = displayed_box();
    let (origin, dir) = ([0.0, 0.0, 10.0], [0.0, 0.0, -1.0]);
    assert_eq!(e.pick(origin, dir), Some(geo));
    assert!(e.pick_detailed(origin, dir).is_some());

    set_root_bool(&mut e, geo, "visible", false);
    assert_eq!(e.pick(origin, dir), None);
    assert!(e.pick_detailed(origin, dir).is_none());

    set_root_bool(&mut e, geo, "visible", true);
    assert_eq!(e.pick(origin, dir), Some(geo));
}

#[test]
fn markers_hide_with_their_object_and_return_on_reshow() {
    let (mut e, geo, ..) = displayed_box();
    let id = add_picked_note(&mut e, "on the box", None);
    assert_eq!(e.review_markers_world().len(), 1);

    // Hiding suppresses the pin without flagging staleness (the anchored
    // geometry is untouched; the review panel still lists the note).
    set_root_bool(&mut e, geo, "visible", false);
    assert!(e.review_markers_world().is_empty());
    assert!(!e.annotation_stale(id));

    set_root_bool(&mut e, geo, "visible", true);
    assert_eq!(e.review_markers_world().len(), 1);
    assert!(!e.annotation_stale(id));
}

// Async import job protocol (deferred-drain, generation guard).

const TRI_STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";

/// Sets up an engine in async mode with one staged STL and an import node
/// in a subflow, cooked once (so the import returns Pending). Returns the
/// engine, ctx, node id, and the spawned (job, request).
fn async_import_fixture() -> (Engine, GraphContext, NodeId, JobId, crate::cook::JobRequest) {
    let mut e = engine();
    e.set_async_jobs(true);
    let asset = e.stage_asset("tri.stl", "model/stl", TRI_STL.as_bytes().to_vec());

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

// Phase-5 .slxy round-trip fidelity (graph, params, positions, view/camera,
// assets, bypass, type_version, variadic port_order, cook_mode).

#[test]
fn slxy_round_trip_preserves_full_document_and_assets() {
    let (mut e, ctx) = subflow_engine();
    let obj = b"o cube\nv 0 0 0\n".to_vec();
    let asset = e.stage_asset("cube.obj", "model/obj", obj.clone());

    // box a, box b -> variadic merge; plus an import node referencing the asset.
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let m = add(&mut e, ctx, "merge");
    let imp = add(&mut e, ctx, "import_obj");

    // Connect a then b into merge's variadic `inputs` (order a, b).
    for src in [a, b] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: src,
                port: "geometry".into(),
            },
            to: PortRefDto {
                node: m,
                port: "inputs".into(),
            },
        })
        .unwrap();
    }

    // Params: a merge name, the import file ref + a non-default scale.
    e.apply(Command::SetParam {
        ctx,
        node: m,
        key: "name".into(),
        value: ParamSource::Literal(ParamValue::Text("Combined".into())),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: imp,
        key: "file".into(),
        value: ParamSource::Literal(ParamValue::Asset(asset.clone())),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: imp,
        key: "scale".into(),
        value: ParamSource::Literal(ParamValue::Float(2.5)),
    })
    .unwrap();

    // Presentation: a moved node, a bypassed node, manual cook mode.
    e.apply(Command::MoveNodes {
        ctx,
        moves: vec![(a, [42.0, -7.0])],
    })
    .unwrap();
    e.apply(Command::SetBypass {
        ctx,
        node: b,
        bypassed: true,
    })
    .unwrap();
    e.apply(Command::SetCookMode {
        mode: CookMode::Manual,
    })
    .unwrap();

    // Save with a host sidecar carrying a camera and a document name.
    let mut sidecar = SceneSidecar {
        generator: "solarxy-test 0.0.0".into(),
        ..Default::default()
    };
    sidecar.view.panes[0].camera = solarxy_scenefile::CameraJson {
        target: [1.0, 2.0, 3.0],
        yaw: 0.9,
        pitch: 0.3,
        distance: 12.0,
        fov_y: 0.8,
        ..solarxy_scenefile::CameraJson::default()
    };
    sidecar.meta.name = "My Scene".into();

    let bytes = e.save_slxy(&sidecar).expect("save .slxy");

    // Load into a fresh engine (ids are preserved, so a/b/m/imp stay valid).
    let mut e2 = engine();
    let loaded = e2.load_slxy(&bytes).expect("load .slxy");

    assert!(
        loaded.warnings.is_empty(),
        "clean round-trip has no warnings: {:?}",
        loaded.warnings
    );
    assert!(
        loaded
            .batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::DocumentReplaced))
    );

    let g = e2.doc.graph(ctx).unwrap();
    assert_eq!(g.node_count(), 4);
    assert_eq!(g.edge_count(), 2);

    // Params round-tripped through the plain-literal shape + re-typing.
    assert_eq!(
        g.node(m).unwrap().params["name"],
        ParamSource::Literal(ParamValue::Text("Combined".into()))
    );
    assert_eq!(
        g.node(imp).unwrap().params["file"],
        ParamSource::Literal(ParamValue::Asset(asset.clone()))
    );
    assert_eq!(
        g.node(imp).unwrap().params["scale"],
        ParamSource::Literal(ParamValue::Float(2.5))
    );

    // Position, bypass, type_version.
    assert!((g.node(a).unwrap().position[0] - 42.0).abs() < 1e-6);
    assert!((g.node(a).unwrap().position[1] - (-7.0)).abs() < 1e-6);
    assert!(g.node(b).unwrap().bypassed);
    // Phase 8 bumped the subflow geometry nodes to v2 (rendering-group
    // strip); the version stored and reloaded is the current one.
    assert_eq!(g.node(m).unwrap().type_version, 2);

    // Variadic port order: inputs are [edge from a, edge from b].
    let inputs = &g.node(m).unwrap().port_order["inputs"];
    assert_eq!(inputs.len(), 2);
    assert_eq!(g.edge(inputs[0]).unwrap().from, a);
    assert_eq!(g.edge(inputs[1]).unwrap().from, b);

    // Cook mode.
    assert_eq!(e2.cook_mode(), CookMode::Manual);

    // Asset bytes restaged (content-addressed id matches, bytes identical).
    assert_eq!(e2.asset_count(), 1);
    assert_eq!(e2.asset_bytes(&asset), Some(obj.as_slice()));

    // The host sidecar (camera + meta) came back for the boundary to apply.
    assert_eq!(loaded.sidecar.meta.name, "My Scene");
    let cam = &loaded.sidecar.view.panes[0].camera;
    assert!((cam.distance - 12.0).abs() < 1e-6);
    assert!((cam.target[1] - 2.0).abs() < 1e-6);
}

// Validation systems (phase 6 W3): implicit import validation, the
// validate node's cache + boundary events, the effective-validation
// lowering, and the async validate-job protocol.

/// An OBJ with one degenerate face (all three corners the same vertex) and
/// no UVs, so validation reliably finds issues and a degenerate-face list.
const DIRTY_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 1 1\nf 1 2 3\n";

/// A geo container whose subflow holds one `import_obj` over the dirty
/// OBJ (display auto-claimed by the import). Not yet cooked.
fn dirty_import_fixture() -> (Engine, GraphContext, NodeId, NodeId) {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let import = add(&mut e, sub, "import_obj");
    let asset = e.stage_asset("dirty.obj", "model/obj", DIRTY_OBJ.as_bytes().to_vec());
    e.apply(Command::SetParam {
        ctx: sub,
        node: import,
        key: "file".into(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();
    (e, sub, geo, import)
}

/// The `SetValidation` payload lowered for a scene object, or `None` if
/// the op is absent (the outer level is the op's presence, the inner the
/// attached result).
#[allow(clippy::option_option)]
fn attached_validation(
    delta: &solarxy_core::scene::SceneDelta,
    object: solarxy_core::scene::SceneObjectId,
) -> Option<Option<std::sync::Arc<solarxy_core::validation::ValidationResult>>> {
    delta.ops.iter().find_map(|op| match op {
        solarxy_core::scene::SceneOp::SetValidation { id, validation } if *id == object => {
            Some(validation.clone())
        }
        _ => None,
    })
}

#[test]
fn import_cook_validates_implicitly_and_lowers_set_validation() {
    use std::sync::Arc;
    let (mut e, _sub, geo, import) = dirty_import_fixture();
    let events = e.cook(&mut || true);

    // Summary + full report events for the import node, with real counts.
    let summary = events.iter().find_map(|ev| match ev {
        EngineEvent::ValidationSummary {
            node,
            errors,
            warnings,
        } if *node == import => Some((*errors, *warnings)),
        _ => None,
    });
    let (errors, warnings) = summary.expect("import emits a validation summary");
    assert!(errors + warnings > 0, "the dirty OBJ has issues");
    assert!(events.iter().any(|ev| matches!(
        ev,
        EngineEvent::ValidationReport {
            node,
            truncated: false,
            issues,
            ..
        } if *node == import && !issues.is_empty()
    )));

    // The cached result keeps the degenerate-face list (`f 1 1 1`).
    let cached = Arc::clone(e.validation(import).expect("validation cached"));
    assert!(cached.degenerate_faces.iter().any(|f| !f.is_empty()));

    // The lowering attaches the effective validation to the geo's object.
    let delta = e.take_scene_delta();
    let attached = attached_validation(&delta, solarxy_core::scene::SceneObjectId(geo.0))
        .expect("SetValidation lowered")
        .expect("effective validation present");
    assert!(Arc::ptr_eq(&attached, &cached));
}

#[test]
fn nearest_validate_node_wins_and_bypass_falls_back_to_the_import() {
    use std::sync::Arc;
    let (mut e, sub, geo, import) = dirty_import_fixture();
    let object = solarxy_core::scene::SceneObjectId(geo.0);
    let validate = add(&mut e, sub, "validate");
    e.apply(Command::Connect {
        ctx: sub,
        from: PortRefDto {
            node: import,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: validate,
            port: "geometry".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: sub,
        node: Some(validate),
    })
    .unwrap();
    e.cook(&mut || true);

    // Both nodes cache a result; the displayed chain's nearest (the
    // validate node) wins the object attachment.
    let from_validate = Arc::clone(e.validation(validate).expect("validate caches"));
    let from_import = Arc::clone(e.validation(import).expect("import caches"));
    let delta = e.take_scene_delta();
    let attached = attached_validation(&delta, object)
        .expect("SetValidation lowered")
        .expect("effective validation present");
    assert!(Arc::ptr_eq(&attached, &from_validate));
    assert!(!Arc::ptr_eq(&attached, &from_import));

    // Bypassing the validate node clears its cache (zeroed summary) and
    // the import's implicit validation becomes effective.
    e.apply(Command::SetBypass {
        ctx: sub,
        node: validate,
        bypassed: true,
    })
    .unwrap();
    let events = e.cook(&mut || true);
    assert!(events.iter().any(|ev| matches!(
        ev,
        EngineEvent::ValidationSummary {
            node,
            errors: 0,
            warnings: 0,
        } if *node == validate
    )));
    assert!(e.validation(validate).is_none());
    let delta = e.take_scene_delta();
    let attached = attached_validation(&delta, object)
        .expect("SetValidation lowered")
        .expect("effective validation present");
    assert!(Arc::ptr_eq(&attached, &from_import));
}

#[test]
fn heavy_validate_routes_through_the_job_protocol() {
    let mut e = engine();
    e.set_async_jobs(true);
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let sphere = add(&mut e, sub, "sphere");
    // 512x512 segments is ~522k triangles, over the 250k inline threshold.
    for key in ["width_segments", "height_segments"] {
        e.apply(Command::SetParam {
            ctx: sub,
            node: sphere,
            key: key.into(),
            value: ParamSource::Literal(ParamValue::Int(512)),
        })
        .unwrap();
    }
    let validate = add(&mut e, sub, "validate");
    e.apply(Command::Connect {
        ctx: sub,
        from: PortRefDto {
            node: sphere,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: validate,
            port: "geometry".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: sub,
        node: Some(validate),
    })
    .unwrap();

    // The sphere cooks inline; the validate node parks Pending with a
    // ValidateGeometry job.
    e.cook(&mut || true);
    let jobs = e.take_jobs();
    assert_eq!(jobs.len(), 1, "one validate job spawns");
    let (ctx, job, request) = jobs.into_iter().next().unwrap();
    assert!(matches!(
        request,
        crate::cook::JobRequest::ValidateGeometry { .. }
    ));

    // Resolve natively and submit: the passthrough geometry commits and
    // the validation summary lands.
    let result = e.resolve_job(&request);
    let events = e.submit_job_result(ctx, job, result);
    assert!(e.node_geometry_points(validate) > 0, "passthrough commits");
    assert!(events.iter().any(|ev| matches!(
        ev,
        EngineEvent::ValidationSummary { node, .. } if *node == validate
    )));
    assert!(e.validation(validate).is_some());
}

#[test]
fn validation_result_round_trips_through_json() {
    use solarxy_core::validation::{
        IssueKind, IssueScope, Severity, ValidationIssue, ValidationReport, ValidationResult,
    };
    // The worker boundary shape: `validate_geometry_job` / the implicit
    // import validation serialize a full result to JSON.
    let result = ValidationResult {
        report: ValidationReport {
            issues: vec![ValidationIssue {
                severity: Severity::Error,
                scope: IssueScope::Edge {
                    mesh_index: 2,
                    vertices: [7, 9],
                },
                kind: IssueKind::NonManifoldEdge,
                message: "shared by 3 faces".into(),
            }],
        },
        degenerate_faces: vec![vec![], vec![3, 5]],
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("degenerateFaces"), "camelCase boundary shape");
    let back: ValidationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.report.issues.len(), 1);
    assert_eq!(back.report.error_count(), 1);
    assert_eq!(back.degenerate_faces, result.degenerate_faces);
}

#[test]
fn preview_param_reaches_the_cook() {
    use solarxy_core::scene::{SceneObjectId, SceneOp};
    // The realtime contract (UX spec section 17 item 1): a param drag must reach
    // the viewport, and the mechanism is the preview lane. If previews never
    // reach the cook, a drag shows nothing until the pointer is released.
    let (mut e, geo, sub, box_id) = displayed_box();

    let before = {
        let delta = e.take_scene_delta();
        delta
            .ops
            .iter()
            .find_map(|op| match op {
                SceneOp::UpsertGeometry { id, geometry } if *id == SceneObjectId(geo.0) => {
                    Some(geometry.bounds)
                }
                _ => None,
            })
            .expect("box upserted")
    };

    // Drag the box's width, the preview way (no document write, no undo entry).
    e.preview_param(
        sub,
        box_id,
        "width",
        ParamSource::Literal(ParamValue::Float(8.0)),
    );
    e.cook(&mut || true);

    let after = e
        .take_scene_delta()
        .ops
        .iter()
        .find_map(|op| match op {
            SceneOp::UpsertGeometry { id, geometry } if *id == SceneObjectId(geo.0) => {
                Some(geometry.bounds)
            }
            _ => None,
        })
        .expect("box re-upserted after the preview");

    let before_w = before.max.x - before.min.x;
    let after_w = after.max.x - after.min.x;
    assert!(
        after_w > before_w * 2.0,
        "the previewed width must reach the cooked geometry: {before_w} -> {after_w}"
    );
}

// ---- Phase 11: the gizmo policy ----

/// The engine's own selection is what `gizmo_target` reads, so set it the way
/// the host does.
fn select(e: &mut Engine, ctx: GraphContext, ids: Vec<NodeId>) {
    e.apply(Command::SetSelection { ctx, ids }).unwrap();
}

#[test]
fn gizmo_target_at_root_drives_the_geo_itself() {
    // The cheap path: dragging a geo at root writes its OWN transform, which the
    // renderer applies as the object transform. No cook, no appended node.
    let (mut e, geo, _sub, _box_id) = displayed_box();
    select(&mut e, GraphContext::Root, vec![geo]);

    let t = e
        .gizmo_target(GraphContext::Root)
        .expect("a geo is selected");
    assert_eq!(t.ctx, GraphContext::Root);
    assert_eq!(t.node, geo);
    assert!(!t.append_pending, "root never appends anything");
    assert!(t.translate.iter().all(|v| v.abs() < 1e-6));
    // Identity parent: a world delta maps straight onto the geo's translate.
    assert!((t.parent[0][0] - 1.0).abs() < 1e-6 && t.parent[0][1].abs() < 1e-6);
    assert!((t.parent[3][3] - 1.0).abs() < 1e-6 && t.parent[3][0].abs() < 1e-6);
}

#[test]
fn gizmo_target_at_root_needs_exactly_one_geo() {
    let (mut e, geo, sub, box_id) = displayed_box();

    // Nothing selected.
    assert!(e.gizmo_target(GraphContext::Root).is_none());

    // A light is not a geo.
    let light = add(&mut e, GraphContext::Root, "point_light");
    select(&mut e, GraphContext::Root, vec![light]);
    assert!(e.gizmo_target(GraphContext::Root).is_none());

    // Multi-select is ambiguous in v1.
    select(&mut e, GraphContext::Root, vec![geo, light]);
    assert!(e.gizmo_target(GraphContext::Root).is_none());

    // A subflow node selected in the SUBFLOW context is a different question.
    select(&mut e, sub, vec![box_id]);
    assert!(e.gizmo_target(sub).is_some());
}

#[test]
fn gizmo_target_in_a_subflow_reports_append_pending_over_a_box() {
    // The tail is a `box`, not a transform: the drag will have to append one.
    let (e, _geo, sub, box_id) = displayed_box();
    let t = e.gizmo_target(sub).expect("the subflow has a display node");
    assert!(t.append_pending, "a box tail is not a transform");
    assert_eq!(t.node, box_id, "the DISPLAY node until the drag mints one");
    assert!(t.translate.iter().all(|v| v.abs() < 1e-6));
}

#[test]
fn ensure_transform_target_appends_and_moves_the_display_flag() {
    let (mut e, geo, sub, box_id) = displayed_box();

    let batch = e.apply(Command::EnsureTransformTarget { geo }).unwrap();
    let target = batch
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .expect("the paired event carries the id");

    assert_ne!(target, box_id, "a fresh node was appended");
    let g = e.document().graph(sub).unwrap();
    assert_eq!(g.node(target).unwrap().type_id, "transform");
    // Without the flag moving, the appended transform would be invisible.
    assert_eq!(g.active_output, Some(target), "the display flag followed");
    // And it is wired downstream of the box, not floating.
    assert!(
        g.edges()
            .any(|edge| edge.from == box_id && edge.to == target),
        "the box feeds the new transform"
    );
}

#[test]
fn ensure_transform_target_reuses_a_tail_transform() {
    let (mut e, geo, sub, _box_id) = displayed_box();

    // First drag appends.
    let first = e.apply(Command::EnsureTransformTarget { geo }).unwrap();
    let appended = first
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();

    // A second drag must REUSE it, not stack another transform on top.
    let before = e.document().graph(sub).unwrap().nodes().count();
    let second = e.apply(Command::EnsureTransformTarget { geo }).unwrap();
    let reused = second
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .expect("the reuse path still reports the target");

    assert_eq!(reused, appended, "the same node, dragged again");
    assert_eq!(
        e.document().graph(sub).unwrap().nodes().count(),
        before,
        "reuse mints nothing"
    );
    // The reuse path emits no NodeAdded at all, which is precisely why
    // TransformTargetReady has to exist.
    assert!(
        !second
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::NodeAdded { .. })),
        "nothing was added"
    );
}

#[test]
fn ensure_transform_target_treats_a_bypassed_tail_as_absent() {
    // A bypassed transform passes geometry through unchanged, so dragging it
    // would move nothing the user can see. Append a live one instead.
    let (mut e, geo, sub, _box_id) = displayed_box();
    let first = e.apply(Command::EnsureTransformTarget { geo }).unwrap();
    let appended = first
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();

    e.apply(Command::SetBypass {
        ctx: sub,
        node: appended,
        bypassed: true,
    })
    .unwrap();

    assert!(
        e.gizmo_target(sub).unwrap().append_pending,
        "a bypassed tail is not a usable target"
    );
    let second = e.apply(Command::EnsureTransformTarget { geo }).unwrap();
    let fresh = second
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();
    assert_ne!(
        fresh, appended,
        "a live transform was appended past the bypassed one"
    );
}

#[test]
fn ensure_transform_target_errors_without_a_display_node() {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    // An empty subflow displays nothing, so there is nothing to transform.
    assert!(e.gizmo_target(GraphContext::Subflow(geo)).is_none());
    assert!(matches!(
        e.apply(Command::EnsureTransformTarget { geo }),
        Err(EngineError::NoDisplayNode { .. })
    ));
}

#[test]
fn an_append_drag_is_exactly_one_undo_step() {
    // The whole point of running Ensure inside the drag's transaction: undoing
    // must remove BOTH the appended node and the param change, in one press.
    let (mut e, geo, sub, box_id) = displayed_box();
    let before = fingerprint(&e, sub);

    e.apply(Command::BeginTransaction {
        label: "move".into(),
    })
    .unwrap();
    let batch = e.apply(Command::EnsureTransformTarget { geo }).unwrap();
    let target = batch
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();
    e.apply(Command::SetParam {
        ctx: sub,
        node: target,
        key: "translate".into(),
        value: ParamSource::Literal(ParamValue::Vec3([3.0, 0.0, 0.0])),
    })
    .unwrap();
    e.apply(Command::EndTransaction).unwrap();

    e.apply(Command::Undo).unwrap();

    assert_eq!(
        fingerprint(&e, sub),
        before,
        "one undo restores the graph exactly: the appended node is gone with the drag"
    );
    assert_eq!(
        e.document().graph(sub).unwrap().active_output,
        Some(box_id),
        "the display flag came back too"
    );
}

#[test]
fn cancel_transaction_rolls_back_an_append_without_touching_redo() {
    // Escape mid-drag: the document returns to the start, and REDO must stay
    // empty -- the user cancelled, they did not ask to re-apply anything.
    let (mut e, geo, sub, _box_id) = displayed_box();
    let before = fingerprint(&e, sub);

    e.apply(Command::BeginTransaction {
        label: "move".into(),
    })
    .unwrap();
    e.apply(Command::EnsureTransformTarget { geo }).unwrap();
    e.apply(Command::CancelTransaction).unwrap();

    assert_eq!(
        fingerprint(&e, sub),
        before,
        "the appended node is gone; the document is untouched"
    );

    // Redo must be a no-op: a cancelled drag left nothing to re-apply.
    e.apply(Command::Redo).unwrap();
    assert_eq!(
        fingerprint(&e, sub),
        before,
        "redo did not resurrect the cancelled append"
    );
}

#[test]
fn clear_preview_releases_a_cancelled_drag() {
    // Without this, a cancelled drag would roll the document back while the
    // preview overlay kept asserting the dragged value, and the viewport would
    // disagree with the parameter panel forever.
    let (mut e, geo, _sub, _box_id) = displayed_box();

    e.preview_param(
        GraphContext::Root,
        geo,
        "translate",
        ParamSource::Literal(ParamValue::Vec3([5.0, 0.0, 0.0])),
    );
    select(&mut e, GraphContext::Root, vec![geo]);
    let live = e.gizmo_target(GraphContext::Root).unwrap().translate;
    assert!(
        (live[0] - 5.0).abs() < 1e-6,
        "the preview is live: {live:?}"
    );

    e.clear_preview(GraphContext::Root, geo, "translate");
    let cleared = e.gizmo_target(GraphContext::Root).unwrap().translate;
    assert!(
        cleared.iter().all(|v| v.abs() < 1e-6),
        "the object snapped back to the document: {cleared:?}"
    );
}

#[test]
fn gizmo_command_boundary_json_shape_is_camelcase() {
    // Pins the hand-authored TS mirror (web/src/engine/types.ts) to the Rust
    // serde shapes for the Phase 11 additions, the same way the other boundary
    // guards do for theirs.
    let ensure = serde_json::to_value(Command::EnsureTransformTarget { geo: NodeId(7) }).unwrap();
    assert_eq!(ensure["type"], "ensureTransformTarget");
    assert_eq!(ensure["geo"], 7);

    let cancel = serde_json::to_value(Command::CancelTransaction).unwrap();
    assert_eq!(cancel["type"], "cancelTransaction");

    // Both commands must deserialize back from exactly what the frontend sends.
    let round: Command = serde_json::from_value(ensure).unwrap();
    assert!(matches!(
        round,
        Command::EnsureTransformTarget { geo } if geo == NodeId(7)
    ));
    let round: Command = serde_json::from_value(cancel).unwrap();
    assert!(matches!(round, Command::CancelTransaction));

    // The paired event carries the target id on BOTH policy paths.
    let ev = serde_json::to_value(EngineEvent::TransformTargetReady {
        ctx: GraphContext::Subflow(NodeId(3)),
        node: NodeId(9),
    })
    .unwrap();
    assert_eq!(ev["type"], "transformTargetReady");
    assert_eq!(ev["ctx"]["subflow"], 3);
    assert_eq!(ev["node"], 9);
}

/// The invariant the whole `geo` v3 unification exists to establish: the
/// container's world matrix and the `transform` node's baked matrix are the
/// SAME composition, for every order. Before this, `geo` hardcoded
/// `T * Rz * Ry * Rx * S` (ZYX) while `transform` defaulted to XYZ, so typing
/// the same angles into each gave two different orientations. A rotate gizmo
/// has to decompose back into the target's order, which is what made the
/// divergence load-bearing rather than merely untidy.
#[test]
fn geo_and_transform_compose_rotation_identically() {
    use solarxy_kernel::transform::{RotateOrder, compose_trs};

    let orders = [
        ("xyz", RotateOrder::Xyz),
        ("xzy", RotateOrder::Xzy),
        ("yxz", RotateOrder::Yxz),
        ("yzx", RotateOrder::Yzx),
        ("zxy", RotateOrder::Zxy),
        ("zyx", RotateOrder::Zyx),
    ];
    let degrees = [30.0_f64, 40.0, 50.0];
    let translate = [1.0_f64, 2.0, 3.0];
    let scale = [2.0_f64, 0.5, 1.5];

    for (key, order) in orders {
        let mut e = engine();
        let geo = add(&mut e, GraphContext::Root, "geo");
        for (k, v) in [
            ("translate", ParamValue::Vec3(translate)),
            ("rotate", ParamValue::Vec3(degrees)),
            ("scale", ParamValue::Vec3(scale)),
            ("rotate_order", ParamValue::Enum(key.to_string())),
        ] {
            e.apply(Command::SetParam {
                ctx: GraphContext::Root,
                node: geo,
                key: k.to_string(),
                value: ParamSource::Literal(v),
            })
            .unwrap();
        }

        let got = e.geo_world_matrix(geo).unwrap();
        // The kernel function the `transform` node's cook calls, fed the same
        // params (a geo's pivot is its origin).
        let want = compose_trs(
            [1.0, 2.0, 3.0],
            [
                (degrees[0] as f32).to_radians(),
                (degrees[1] as f32).to_radians(),
                (degrees[2] as f32).to_radians(),
            ],
            order,
            [2.0, 0.5, 1.5],
            [0.0; 3],
        );
        for c in 0..4 {
            for r in 0..4 {
                assert!(
                    (got[c][r] - want[c][r]).abs() < 1e-5,
                    "{key}: [{c}][{r}] {} != {}",
                    got[c][r],
                    want[c][r]
                );
            }
        }
    }
}

/// A fresh geo defaults to XYZ, matching `transform`. Pins the unification
/// against a regression back to the old hardcoded ZYX.
#[test]
fn a_fresh_geo_rotates_xyz_like_a_transform_node() {
    use solarxy_kernel::transform::{RotateOrder, rotation_matrix};

    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: geo,
        key: "rotate".to_string(),
        // Two nonzero lanes, so the order is observable at all.
        value: ParamSource::Literal(ParamValue::Vec3([90.0, 90.0, 0.0])),
    })
    .unwrap();

    let got = e.geo_world_matrix(geo).unwrap();
    let xyz = rotation_matrix(
        [90f32.to_radians(), 90f32.to_radians(), 0.0],
        RotateOrder::Xyz,
    );
    let zyx = rotation_matrix(
        [90f32.to_radians(), 90f32.to_radians(), 0.0],
        RotateOrder::Zyx,
    );

    let close = |m: cgmath::Matrix3<f32>| {
        (0..3).all(|c| (0..3).all(|r| (got[c][r] - m[c][r]).abs() < 1e-5))
    };
    assert!(close(xyz), "a fresh geo must compose XYZ");
    assert!(!close(zyx), "and must no longer compose the old ZYX");
}

/// `resolve_params` hands back RADIANS; a `SetParam` writes DEGREES. The gizmo
/// straddles that conversion, so `GizmoTarget` must speak the units it writes
/// back, or every rotate drag would be off by a factor of 57.
#[test]
fn gizmo_target_reports_rotation_in_degrees_not_radians() {
    let (mut e, geo, _sub, _box_id) = displayed_box();
    select(&mut e, GraphContext::Root, vec![geo]);
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: geo,
        key: "rotate".to_string(),
        value: ParamSource::Literal(ParamValue::Vec3([90.0, 0.0, 0.0])),
    })
    .unwrap();

    let t = e.gizmo_target(GraphContext::Root).unwrap();
    assert!(
        (t.rotate[0] - 90.0).abs() < 1e-3,
        "degrees, got {:?} (radians would read ~1.57)",
        t.rotate
    );
    assert_eq!(t.rotate_order, solarxy_kernel::transform::RotateOrder::Xyz);
}

/// A `transform` rotates and scales about its PIVOT, not its origin: a point at
/// the pivot maps to `translate + pivot`. The rings and cubes have to be drawn
/// around that point, or they would spin about a centre they do not surround.
#[test]
fn the_gizmo_anchors_on_the_pivot_a_transform_actually_rotates_about() {
    let (mut e, geo, sub, _box_id) = displayed_box();
    let node = e
        .apply(Command::EnsureTransformTarget { geo })
        .unwrap()
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();

    for (key, value) in [
        ("translate", ParamValue::Vec3([1.0, 0.0, 0.0])),
        ("pivot", ParamValue::Vec3([0.0, 5.0, 0.0])),
    ] {
        e.apply(Command::SetParam {
            ctx: sub,
            node,
            key: key.to_string(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }

    let t = e.gizmo_target(sub).unwrap();
    assert!((t.pivot[1] - 5.0).abs() < 1e-6, "the pivot round-trips");
    // Column 3 of a column-major matrix is its translation: translate + pivot.
    let origin = [t.anchor[3][0], t.anchor[3][1], t.anchor[3][2]];
    assert!(
        (origin[0] - 1.0).abs() < 1e-5 && (origin[1] - 5.0).abs() < 1e-5,
        "the gizmo must sit at translate + pivot, got {origin:?}"
    );
}

/// Local-orientation handles ride the object's own axes. The basis must carry
/// the rotation and NOT the scale: a scaled basis would stretch the handles
/// with the object, defeating the screen-constant sizing.
#[test]
fn the_gizmo_basis_carries_rotation_but_never_scale() {
    let (mut e, geo, _sub, _box_id) = displayed_box();
    select(&mut e, GraphContext::Root, vec![geo]);
    for (key, value) in [
        ("rotate", ParamValue::Vec3([0.0, 90.0, 0.0])),
        ("scale", ParamValue::Vec3([5.0, 5.0, 5.0])),
    ] {
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: geo,
            key: key.to_string(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }

    let t = e.gizmo_target(GraphContext::Root).unwrap();
    // Under a 90-degree Y rotation the local +X axis points down world -Z.
    let local_x = t.basis[0];
    assert!(
        local_x[0].abs() < 1e-5 && (local_x[2] + 1.0).abs() < 1e-5,
        "local +X should be world -Z, got {local_x:?}"
    );
    // Unit length despite the 5x scale.
    let length = (local_x[0].powi(2) + local_x[1].powi(2) + local_x[2].powi(2)).sqrt();
    assert!(
        (length - 1.0).abs() < 1e-5,
        "basis must be orthonormal, got {length}"
    );
}

/// Inside a subflow the handles must compose the container's rotation with the
/// transform node's own: the node's angles are expressed in the geo's frame.
#[test]
fn a_subflow_basis_composes_the_container_and_the_node() {
    let (mut e, geo, sub, _box_id) = displayed_box();
    let node = e
        .apply(Command::EnsureTransformTarget { geo })
        .unwrap()
        .events
        .iter()
        .find_map(|ev| match ev {
            EngineEvent::TransformTargetReady { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();

    // Container turned 90 about Y; node turned another 90 about Y. Composed,
    // the object faces 180, so its local +X points down world -X.
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: geo,
        key: "rotate".to_string(),
        value: ParamSource::Literal(ParamValue::Vec3([0.0, 90.0, 0.0])),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx: sub,
        node,
        key: "rotate".to_string(),
        value: ParamSource::Literal(ParamValue::Vec3([0.0, 90.0, 0.0])),
    })
    .unwrap();

    let t = e.gizmo_target(sub).unwrap();
    let local_x = t.basis[0];
    assert!(
        (local_x[0] + 1.0).abs() < 1e-4 && local_x[2].abs() < 1e-4,
        "90 + 90 about Y sends local +X to world -X, got {local_x:?}"
    );
}
