//! Engine facade tests: command application, event emission, cook
//! integration, preview non-leakage, and serde round-trips.

use super::*;
use crate::document::ContextKind;
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
    e.doc.create_subflow(geo, ContextKind::Geo);
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
fn has_active_previews_tracks_the_preview_lane() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    // No interaction in flight to begin with.
    assert!(!e.has_active_previews());

    // A drag streams a preview: the interaction signal goes true (the host
    // uses this to freeze the grid/floor/shadow refit during a drag).
    e.preview_param(
        ctx,
        box_id,
        "width",
        ParamSource::Literal(ParamValue::Float(9.0)),
    );
    assert!(e.has_active_previews());

    // An explicit cancel clears it.
    e.clear_preview(ctx, box_id, "width");
    assert!(!e.has_active_previews());

    // And the committing SetParam clears it after a real drag, so the refit
    // runs exactly once on release.
    e.preview_param(
        ctx,
        box_id,
        "width",
        ParamSource::Literal(ParamValue::Float(3.0)),
    );
    assert!(e.has_active_previews());
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".to_string(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    assert!(!e.has_active_previews());
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
    assert!(json.contains("\"categoryLabel\":\"Generators\""));
    assert!(json.contains("\"category\":\"generators\""));
    // Node identity for the canvas: the icon key and the silhouette family
    // (merge is the gather-shaped exception).
    assert!(json.contains("\"glyph\":\"box\""));
    assert!(json.contains("\"role\":\"standard\""));
    assert!(json.contains("\"role\":\"gather\""));
    // The attribute-name widget variant (the attribute nodes' Name param).
    assert!(json.contains("\"paramType\":\"attributeName\""));
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

    // resetParams: camelCase tag, optional keys (absent means all).
    let cmd3 = Command::ResetParams {
        ctx: GraphContext::Root,
        node: NodeId(7),
        keys: None,
    };
    let v3 = serde_json::to_value(&cmd3).unwrap();
    assert_eq!(v3["type"], "resetParams");
    let back3: Command = serde_json::from_value(
        serde_json::json!({ "type": "resetParams", "ctx": "root", "node": 7 }),
    )
    .unwrap();
    assert!(matches!(
        back3,
        Command::ResetParams {
            node: NodeId(7),
            keys: None,
            ..
        }
    ));
    let back4: Command = serde_json::from_value(
        serde_json::json!({ "type": "resetParams", "ctx": "root", "node": 7, "keys": ["width"] }),
    )
    .unwrap();
    assert!(matches!(
        back4,
        Command::ResetParams { keys: Some(k), .. } if k == vec!["width".to_string()]
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

/// Undo of a disconnect must restore the edge to its ORIGINAL slot in the
/// variadic port order, not append it to the end.
///
/// `Graph::disconnect` retains the id out of `port_order`, and `Graph::connect`
/// (which the undo path calls) pushes it back on the end. Without an explicit
/// slot the wires silently reorder: `merge` would concatenate in a different
/// order, and `switch` -- which selects BY INDEX -- would read a different
/// branch entirely. `Command::Disconnect` is dispatched by the UI whenever a
/// user deletes an edge or drags one to another handle, so this is reachable.
#[test]
fn undo_of_a_disconnect_restores_the_original_variadic_slot() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "sphere");
    let c = add(&mut e, ctx, "cylinder");
    let sw = add(&mut e, ctx, "switch");

    for src in [a, b, c] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: src,
                port: "geometry".into(),
            },
            to: PortRefDto {
                node: sw,
                port: "inputs".into(),
            },
        })
        .unwrap();
    }

    let before = fingerprint(&e, ctx);
    let first = e
        .document()
        .graph(ctx)
        .unwrap()
        .incoming_to_port(sw, "inputs")[0]
        .id;

    e.apply(Command::Disconnect { ctx, edge: first }).unwrap();
    e.apply(Command::Undo).unwrap();

    assert_eq!(
        fingerprint(&e, ctx),
        before,
        "undo of a disconnect must put the wire back where it was, not at the end"
    );
}

/// The same hazard, stated in terms a user would feel: the switch's index must
/// still select the same geometry after a disconnect-then-undo of an earlier
/// wire.
#[test]
fn a_disconnect_undo_does_not_repoint_a_switch() {
    let (mut e, ctx) = subflow_engine();
    let boxn = add(&mut e, ctx, "box"); // 12 tris
    let plane = add(&mut e, ctx, "plane"); // 2 tris
    let sw = add(&mut e, ctx, "switch");

    for src in [boxn, plane] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: src,
                port: "geometry".into(),
            },
            to: PortRefDto {
                node: sw,
                port: "inputs".into(),
            },
        })
        .unwrap();
    }
    // Index 1 = the second wire = the plane.
    e.apply(Command::SetParam {
        ctx,
        node: sw,
        key: "index".into(),
        value: ParamSource::Literal(ParamValue::Int(1)),
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(sw),
    })
    .unwrap();
    e.cook(&mut || true);

    let tris_before = e
        .cook
        .outputs(sw)
        .unwrap()
        .get("geometry")
        .and_then(crate::registry::coerce::Value::as_geometry)
        .unwrap()
        .triangle_count();
    assert_eq!(tris_before, 2, "index 1 selects the plane");

    // Drop the FIRST wire (the box) and undo it.
    let first = e
        .document()
        .graph(ctx)
        .unwrap()
        .incoming_to_port(sw, "inputs")[0]
        .id;
    e.apply(Command::Disconnect { ctx, edge: first }).unwrap();
    e.apply(Command::Undo).unwrap();
    e.cook(&mut || true);

    let tris_after = e
        .cook
        .outputs(sw)
        .unwrap()
        .get("geometry")
        .and_then(crate::registry::coerce::Value::as_geometry)
        .unwrap()
        .triangle_count();
    assert_eq!(
        tris_after, 2,
        "after disconnect+undo the switch must still select the plane, not the box"
    );
}

/// Asset alias names must survive a `.slxy` round trip. The bytes are stored
/// once (content-addressed) with one blob name, so without replaying the
/// aliases on load the reloaded scene forgets every name but the first and the
/// missing-companion preflight fires on a file it is already holding.
#[test]
fn asset_alias_names_survive_a_slxy_round_trip() {
    let mut e = engine();
    let png = b"\x89PNG fake pixels".to_vec();
    let id = e.stage_asset("albedo.png", "image/png", png.clone());
    e.stage_asset("diffuse.png", "image/png", png);

    // The manifest reports BOTH names (the frontend preflight reads this).
    let names: Vec<String> = e.asset_manifest().into_iter().map(|(_, n)| n).collect();
    assert!(names.contains(&"albedo.png".to_string()));
    assert!(names.contains(&"diffuse.png".to_string()));
    assert_eq!(names.len(), 2, "one row per name, one entry of bytes");

    let bytes = e.save_slxy(&SceneSidecar::default()).expect("save");
    let mut e2 = engine();
    e2.load_slxy(&bytes).expect("load");

    let names2: Vec<String> = e2.asset_manifest().into_iter().map(|(_, n)| n).collect();
    assert!(
        names2.contains(&"diffuse.png".to_string()),
        "the alias survived the round trip: {names2:?}"
    );
    assert_eq!(
        e2.asset_count(),
        1,
        "still one content-addressed entry, not two"
    );
    let _ = id;
}

/// `ReorderVariadicInput` had zero usages anywhere in the repo, so
/// `UndoOp::ReorderVariadic` had never executed.
#[test]
fn undo_of_a_variadic_reorder_restores_the_previous_order() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "sphere");
    let m = add(&mut e, ctx, "merge");
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

    let before = fingerprint(&e, ctx);
    let order: Vec<_> = e
        .document()
        .graph(ctx)
        .unwrap()
        .incoming_to_port(m, "inputs")
        .iter()
        .map(|e| e.id)
        .collect();
    let reversed: Vec<_> = order.iter().rev().copied().collect();

    e.apply(Command::ReorderVariadicInput {
        ctx,
        node: m,
        port: "inputs".into(),
        order: reversed.clone(),
    })
    .unwrap();
    assert_ne!(fingerprint(&e, ctx), before, "the reorder landed");

    e.apply(Command::Undo).unwrap();
    assert_eq!(fingerprint(&e, ctx), before, "undo restored the wire order");
}

/// `EditAnnotation` had zero usages, so `UndoOp::RestoreReview` was never
/// reached through the edit path (only via resolve/delete/reanchor).
#[test]
fn undo_of_an_annotation_edit_restores_the_previous_text_and_category() {
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
    e.apply(Command::AddAnnotation {
        anchor,
        text: "original".into(),
        category: ReviewCategory::Warning,
        author: None,
        created_at: String::new(),
        reply_to: None,
    })
    .unwrap();
    let id = e.document().review().iter().next().unwrap().id;

    e.apply(Command::EditAnnotation {
        id,
        text: "edited".into(),
        category: ReviewCategory::Question,
        updated_at: String::new(),
    })
    .unwrap();
    {
        let a = e.document().review().get(id).unwrap();
        assert_eq!(a.text, "edited");
        assert_eq!(a.category, ReviewCategory::Question);
    }

    e.apply(Command::Undo).unwrap();
    let a = e.document().review().get(id).unwrap();
    assert_eq!(a.text, "original", "undo restored the text");
    assert_eq!(
        a.category,
        ReviewCategory::Warning,
        "undo restored the category"
    );
}

/// `MoveNodes` and `SetSelection` were each used once, never with an `Undo`,
/// so `UndoOp::MoveNodes` and `UndoOp::SetSelection` were never round-tripped.
#[test]
fn undo_of_a_move_and_a_selection_round_trips() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "sphere");

    let before = fingerprint(&e, ctx);
    e.apply(Command::MoveNodes {
        ctx,
        moves: vec![(a, [123.0, 456.0]), (b, [-7.0, 8.0])],
    })
    .unwrap();
    e.apply(Command::Undo).unwrap();
    assert_eq!(fingerprint(&e, ctx), before, "undo restored the positions");

    // Selection lives outside the fingerprint, so assert it directly.
    let sel = |e: &Engine| e.document().graph(ctx).unwrap().selection.clone();
    e.apply(Command::SetSelection {
        ctx,
        ids: vec![a, b],
    })
    .unwrap();
    e.apply(Command::SetSelection { ctx, ids: vec![a] })
        .unwrap();
    assert_eq!(sel(&e), vec![a]);

    e.apply(Command::Undo).unwrap();
    assert_eq!(sel(&e), vec![a, b], "undo restored the previous selection");
}

/// `PasteNodes` was used three times, never with an `Undo`: undo-of-paste (which
/// must remove the freshly-minted ids AND their internal edges) was unverified;
/// only undo-of-duplicate was.
#[test]
fn undo_of_a_paste_removes_the_pasted_nodes_and_their_edges() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let t = add(&mut e, ctx, "transform");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: a,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: t,
            port: "geometry".into(),
        },
    })
    .unwrap();

    let before = fingerprint(&e, ctx);
    let fragment = e.copy_nodes(ctx, &[a, t]);

    e.apply(Command::PasteNodes {
        ctx,
        fragment,
        position: [500.0, 500.0],
    })
    .unwrap();
    assert_ne!(fingerprint(&e, ctx), before, "the paste landed");

    e.apply(Command::Undo).unwrap();
    assert_eq!(
        fingerprint(&e, ctx),
        before,
        "undo of a paste removes the copies and their internal edges"
    );
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
fn reset_params_restores_defaults_in_one_undo_step() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    for (key, v) in [("width", 3.0), ("height", 4.0)] {
        e.apply(Command::SetParam {
            ctx,
            node: box_id,
            key: key.into(),
            value: ParamSource::Literal(ParamValue::Float(v)),
        })
        .unwrap();
    }

    let batch = e
        .apply(Command::ResetParams {
            ctx,
            node: box_id,
            keys: None,
        })
        .unwrap();

    // The stored overrides are gone (the document is honestly unset)...
    let params = &e
        .document()
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params;
    assert!(params.get("width").is_none());
    assert!(params.get("height").is_none());
    // ...and each removal announced the descriptor default so the mirror
    // repaints without a snapshot.
    let changed: Vec<(String, ParamSource)> = batch
        .events
        .iter()
        .filter_map(|ev| match ev {
            EngineEvent::ParamChanged { key, value, .. } => Some((key.clone(), value.clone())),
            _ => None,
        })
        .collect();
    assert!(changed.contains(&(
        "width".to_string(),
        ParamSource::Literal(ParamValue::Float(1.0))
    )));
    assert!(changed.contains(&(
        "height".to_string(),
        ParamSource::Literal(ParamValue::Float(1.0))
    )));

    // ONE undo restores both stored values; redo re-resets both.
    e.apply(Command::Undo).unwrap();
    let params = &e
        .document()
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params;
    assert_eq!(
        params.get("width"),
        Some(&ParamSource::Literal(ParamValue::Float(3.0)))
    );
    assert_eq!(
        params.get("height"),
        Some(&ParamSource::Literal(ParamValue::Float(4.0)))
    );
    e.apply(Command::Redo).unwrap();
    let params = &e
        .document()
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params;
    assert!(params.get("width").is_none());
    assert!(params.get("height").is_none());
}

#[test]
fn reset_params_with_keys_touches_only_those_and_rejects_unknown_ones() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    for (key, v) in [("width", 3.0), ("height", 4.0)] {
        e.apply(Command::SetParam {
            ctx,
            node: box_id,
            key: key.into(),
            value: ParamSource::Literal(ParamValue::Float(v)),
        })
        .unwrap();
    }

    e.apply(Command::ResetParams {
        ctx,
        node: box_id,
        keys: Some(vec!["width".into()]),
    })
    .unwrap();
    let params = &e
        .document()
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params;
    assert!(params.get("width").is_none());
    assert_eq!(
        params.get("height"),
        Some(&ParamSource::Literal(ParamValue::Float(4.0))),
        "a keyed reset leaves the other overrides alone"
    );

    assert!(
        e.apply(Command::ResetParams {
            ctx,
            node: box_id,
            keys: Some(vec!["no_such_param".into()]),
        })
        .is_err(),
        "an unknown key is a command error, matching set_param"
    );
}

#[test]
fn reset_params_removes_an_expression_and_skips_unstored_keys() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Expression {
            expr: "2 + 2".into(),
        },
    })
    .unwrap();

    let batch = e
        .apply(Command::ResetParams {
            ctx,
            node: box_id,
            keys: None,
        })
        .unwrap();
    let params = &e
        .document()
        .graph(ctx)
        .unwrap()
        .node(box_id)
        .unwrap()
        .params;
    assert!(params.get("width").is_none(), "the expression is removed");
    // Only the one stored key announced a change: unstored params are
    // already at their defaults and stay silent.
    let changed = batch
        .events
        .iter()
        .filter(|ev| matches!(ev, EngineEvent::ParamChanged { .. }))
        .count();
    assert_eq!(changed, 1);
}

#[test]
fn reset_params_with_nothing_stored_emits_nothing() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let batch = e
        .apply(Command::ResetParams {
            ctx,
            node: box_id,
            keys: None,
        })
        .unwrap();
    assert!(
        !batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::ParamChanged { .. })),
        "a pristine node has nothing to reset"
    );
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

// review: anchoring, threading, staleness, markers, detailed picks.

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
    // The exclusive-shadow-caster rule: the handoff cascades
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
    // Root holds no geo/light/camera/environment nodes yet, so only the
    // three whole-value ops are emitted: an empty light list, an empty
    // camera list, and an environment asserting nothing. All three are
    // unconditional, because absence has to be communicated too.
    use solarxy_core::scene::SceneOp;
    let mut e = engine();
    let delta = e.take_scene_delta();
    assert_eq!(delta.ops.len(), 3);
    assert!(matches!(
        delta.ops[0],
        SceneOp::SetLights { ref lights } if lights.is_empty()
    ));
    assert!(matches!(
        delta.ops[1],
        SceneOp::SetCameras { ref cameras } if cameras.is_empty()
    ));
}

#[test]
fn scene_delta_lowers_a_camera_node() {
    use solarxy_core::scene::{CameraKind, SceneObjectId, SceneOp};
    let mut e = engine();
    let cam = add(&mut e, GraphContext::Root, "camera");
    // Set an orthographic projection so we can assert the kind maps through.
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: cam,
        key: "kind".to_string(),
        value: ParamSource::Literal(ParamValue::Enum("orthographic".to_string())),
    })
    .unwrap();
    let delta = e.take_scene_delta();
    let cameras = delta
        .ops
        .iter()
        .find_map(|op| match op {
            SceneOp::SetCameras { cameras } => Some(cameras),
            _ => None,
        })
        .expect("SetCameras op present");
    assert_eq!(cameras.len(), 1);
    let def = &cameras[0];
    assert_eq!(def.id, SceneObjectId(cam.0));
    assert_eq!(def.kind, CameraKind::Orthographic);
}

/// The look reaches the scene contract, and the tone override means
/// "inherit" until it is set.
#[test]
fn scene_delta_lowers_the_cameras_look() {
    use solarxy_core::scene::{SceneOp, ToneCurve};
    let mut e = engine();
    let cam = add(&mut e, GraphContext::Root, "camera");

    let look_of = |e: &mut Engine| {
        e.take_scene_delta()
            .ops
            .into_iter()
            .find_map(|op| match op {
                SceneOp::SetCameras { cameras } => cameras.into_iter().next(),
                _ => None,
            })
            .expect("a camera in the delta")
            .look
    };

    // Fresh out of the palette: neutral, and inheriting.
    let fresh = look_of(&mut e);
    assert_eq!(fresh.exposure, 1.0);
    assert_eq!(fresh.tone, None, "a new camera must not restyle the scene");
    assert_eq!(fresh.lift, [0.0; 3]);
    assert_eq!(fresh.gamma, [1.0; 3]);
    assert_eq!(fresh.gain, [1.0; 3]);
    assert!(fresh.lut_a.is_none() && fresh.lut_b.is_none());

    for (key, value) in [
        ("exposure", ParamValue::Float(2.5)),
        ("tone", ParamValue::Enum("reinhard".to_string())),
        ("lift", ParamValue::Vec3([0.1, 0.0, -0.05])),
        ("gamma", ParamValue::Vec3([1.2, 1.0, 0.8])),
        ("gain", ParamValue::Vec3([1.0, 1.1, 1.3])),
        ("lut_b_strength", ParamValue::Float(0.25)),
    ] {
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: cam,
            key: key.to_string(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }
    let set = look_of(&mut e);
    assert_eq!(set.exposure, 2.5);
    assert_eq!(set.tone, Some(ToneCurve::Reinhard));
    assert_eq!(set.lift, [0.1, 0.0, -0.05]);
    assert_eq!(set.gamma, [1.2, 1.0, 0.8]);
    assert_eq!(set.gain, [1.0, 1.1, 1.3]);
    assert_eq!(set.lut_b_strength, 0.25);
}

/// A staged `.cube` reaches `CameraDef` as a decoded table.
///
/// The table travels on the cook's side cache rather than on a wire, so
/// this exercises the whole chain the way the environment node's HDRI test
/// does: stage bytes, cook, and read the lowered scene op.
#[test]
fn a_staged_cube_reaches_the_camera_look() {
    use solarxy_core::scene::SceneOp;
    let mut e = engine();
    let cam = add(&mut e, GraphContext::Root, "camera");

    // A 2-cubed identity, which is the smallest legal table.
    let mut src = String::from("LUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                src.push_str(&format!("{r}.0 {g}.0 {b}.0\n"));
            }
        }
    }
    let id = e.stage_asset("look.cube", "text/plain", src.into_bytes());
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: cam,
        key: "lut_b".to_string(),
        value: ParamSource::Literal(ParamValue::Asset(id)),
    })
    .unwrap();
    e.cook(&mut || true);

    let look = e
        .take_scene_delta()
        .ops
        .into_iter()
        .find_map(|op| match op {
            SceneOp::SetCameras { cameras } => cameras.into_iter().next(),
            _ => None,
        })
        .expect("a camera in the delta")
        .look;
    let table = look
        .lut_b
        .expect("the display-referred slot carries a table");
    assert_eq!(table.size, 2);
    assert_eq!(table.data.len(), 8 * 3);
    assert!(
        look.lut_a.is_none(),
        "only the slot that was pointed at a file may carry one"
    );
}

/// A malformed table is a cook error on the node, not a silent no-op.
///
/// This is the payoff of decoding engine-side rather than host-side: the
/// node that references the bad file is the node that reports it.
#[test]
fn a_malformed_cube_fails_the_cameras_cook() {
    let mut e = engine();
    let cam = add(&mut e, GraphContext::Root, "camera");
    let id = e.stage_asset("broken.cube", "text/plain", b"LUT_3D_SIZE 8\n1 2".to_vec());
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: cam,
        key: "lut_a".to_string(),
        value: ParamSource::Literal(ParamValue::Asset(id)),
    })
    .unwrap();
    e.cook(&mut || true);

    let status = e.cook.status(cam).expect("the camera cooked");
    assert!(
        matches!(status, crate::cook::state::CookStatus::Error { .. }),
        "a malformed table must surface on the node, got {status:?}"
    );
}

/// The camera's look survives a scene-file save and load.
///
/// Through the real seam rather than by cloning a struct: to JSON, back
/// out, into a fresh engine, and cooked. This is the criterion that makes
/// the look worth putting on the camera at all, since a look that does not
/// travel with the document is just application state with extra steps.
#[test]
fn the_cameras_look_survives_a_scene_file_round_trip() {
    use solarxy_core::scene::{SceneOp, ToneCurve};
    let mut e = engine();
    let cam = add(&mut e, GraphContext::Root, "camera");
    for (key, value) in [
        ("exposure", ParamValue::Float(0.75)),
        ("tone", ParamValue::Enum("none".to_string())),
        ("lift", ParamValue::Vec3([0.02, 0.0, 0.03])),
        ("gain", ParamValue::Vec3([1.4, 1.0, 0.9])),
        ("lut_a_strength", ParamValue::Float(0.5)),
    ] {
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: cam,
            key: key.to_string(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }

    let scene = crate::engine::scenefile::document_to_scene(
        &e.doc.to_data(),
        e.cook_mode,
        &crate::runtime::RuntimeSettings::default(),
        &SceneSidecar::default(),
        Vec::new(),
    );
    let (document, warnings) = crate::engine::scenefile::scene_to_document(&scene, &e.registry);
    assert!(
        warnings.is_empty(),
        "a clean save must not warn: {warnings:?}"
    );

    let mut reopened = engine();
    reopened.load_document(&DocumentFile {
        format_version: 1,
        document,
        cook_mode: e.cook_mode,
    });
    reopened.cook(&mut || true);

    let look = reopened
        .take_scene_delta()
        .ops
        .into_iter()
        .find_map(|op| match op {
            SceneOp::SetCameras { cameras } => cameras.into_iter().next(),
            _ => None,
        })
        .expect("a camera in the reopened delta")
        .look;
    assert_eq!(look.exposure, 0.75);
    assert_eq!(look.tone, Some(ToneCurve::None));
    assert_eq!(look.lift, [0.02, 0.0, 0.03]);
    assert_eq!(look.gain, [1.4, 1.0, 0.9]);
    assert_eq!(look.lut_a_strength, 0.5);
}

/// A camera written before the look existed opens neutral rather than dark.
///
/// The v1-to-v2 bump carries no migration hook because every added
/// parameter fills from its registry default and every default is the
/// identity of its effect. That is a claim about the defaults, so it is
/// worth checking against a document that genuinely lacks the keys rather
/// than one that round-tripped through the current writer.
#[test]
fn a_camera_saved_before_the_look_opens_neutral() {
    use solarxy_core::scene::SceneOp;
    let mut e = engine();
    let cam = add(&mut e, GraphContext::Root, "camera");
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: cam,
        key: "fov_y".to_string(),
        value: ParamSource::Literal(ParamValue::Float(60.0)),
    })
    .unwrap();

    let mut scene = crate::engine::scenefile::document_to_scene(
        &e.doc.to_data(),
        e.cook_mode,
        &crate::runtime::RuntimeSettings::default(),
        &SceneSidecar::default(),
        Vec::new(),
    );
    // Age it: strip every look key and stamp the node back to v1, which is
    // exactly what a pre-0.8.2 scene holds on disk.
    let mut aged = 0;
    for node in &mut scene.graph.nodes {
        if node.type_id == "camera" {
            node.type_version = 1;
            for key in [
                "exposure",
                "tone",
                "lift",
                "gamma",
                "gain",
                "lut_a",
                "lut_a_strength",
                "lut_b",
                "lut_b_strength",
            ] {
                node.params.remove(key);
            }
            aged += 1;
        }
    }
    assert_eq!(aged, 1, "the camera node was found and aged");

    let (document, warnings) = crate::engine::scenefile::scene_to_document(&scene, &e.registry);
    assert!(
        warnings.is_empty(),
        "filling from defaults must not warn: {warnings:?}"
    );
    let mut reopened = engine();
    reopened.load_document(&DocumentFile {
        format_version: 1,
        document,
        cook_mode: e.cook_mode,
    });
    reopened.cook(&mut || true);

    let look = reopened
        .take_scene_delta()
        .ops
        .into_iter()
        .find_map(|op| match op {
            SceneOp::SetCameras { cameras } => cameras.into_iter().next(),
            _ => None,
        })
        .expect("a camera in the reopened delta")
        .look;
    assert_eq!(look.exposure, 1.0, "a v1 camera must not open darkened");
    assert_eq!(
        look.tone, None,
        "a v1 camera must inherit the pane's tone mapper, not impose one"
    );
    assert_eq!(look.lift, [0.0; 3]);
    assert_eq!(look.gamma, [1.0; 3]);
    assert_eq!(look.gain, [1.0; 3]);
    assert!(look.lut_a.is_none() && look.lut_b.is_none());
}

#[test]
fn physical_camera_derives_fov_from_focal_and_sensor() {
    let mut e = engine();
    let cam = add(&mut e, GraphContext::Root, "camera");
    for (key, val) in [("kind", ParamValue::Enum("physical".to_string()))] {
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: cam,
            key: key.to_string(),
            value: ParamSource::Literal(val),
        })
        .unwrap();
    }
    // Defaults: 50mm focal, 36mm sensor -> ~39.6 degrees vertical FOV.
    let delta = e.take_scene_delta();
    let cameras = delta
        .ops
        .iter()
        .find_map(|op| match op {
            solarxy_core::scene::SceneOp::SetCameras { cameras } => Some(cameras),
            _ => None,
        })
        .unwrap();
    let fov_deg = cameras[0].fov_y.to_degrees();
    let expected = 2.0 * (36.0f32 / (2.0 * 50.0)).atan().to_degrees();
    assert!(
        (fov_deg - expected).abs() < 0.01,
        "fov {fov_deg} vs {expected}"
    );
}

/// Deleting a geo node must remove its object, not merely stop mentioning
/// it. A rebuilt delta only ever says what exists, so before removals were
/// emitted the renderer kept the GPU object resident and drew it forever.
#[test]
fn deleting_a_geo_node_emits_a_scene_remove() {
    use solarxy_core::scene::{SceneObjectId, SceneOp};
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let _box_id = add(&mut e, sub, "box");
    e.cook(&mut || true);

    // Baseline: the object exists and nothing is being removed.
    let first = e.take_scene_delta();
    let object_id = SceneObjectId(geo.0);
    assert!(
        first
            .ops
            .iter()
            .any(|op| matches!(op, SceneOp::UpsertGeometry { id, .. } if *id == object_id)),
        "the geo should upsert before it is deleted"
    );
    assert!(
        !first
            .ops
            .iter()
            .any(|op| matches!(op, SceneOp::Remove { .. })),
        "nothing has disappeared yet"
    );

    e.apply(Command::RemoveNodes {
        ctx: GraphContext::Root,
        ids: vec![geo],
    })
    .unwrap();
    e.cook(&mut || true);

    let after = e.take_scene_delta();
    assert!(
        after
            .ops
            .iter()
            .any(|op| matches!(op, SceneOp::Remove { id } if *id == object_id)),
        "deleting the geo must emit Remove for its object, got {:?}",
        after.ops
    );

    // The removal is emitted once. A later pass has nothing left to remove,
    // so a deleted object cannot keep generating ops for the rest of the
    // session.
    let third = e.take_scene_delta();
    assert!(
        !third
            .ops
            .iter()
            .any(|op| matches!(op, SceneOp::Remove { .. })),
        "Remove should not repeat once the renderer has been told"
    );
}

/// Clearing a geo's display flag is indistinguishable from deleting it, as
/// far as the renderer is concerned: in both cases the object must stop
/// being drawn.
#[test]
fn clearing_the_display_flag_emits_a_scene_remove() {
    use solarxy_core::scene::{SceneObjectId, SceneOp};
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box_id = add(&mut e, sub, "box");
    e.cook(&mut || true);
    let _ = e.take_scene_delta();

    e.apply(Command::SetActiveOutput {
        ctx: sub,
        node: None,
    })
    .unwrap();
    e.cook(&mut || true);

    let after = e.take_scene_delta();
    assert!(
        after
            .ops
            .iter()
            .any(|op| matches!(op, SceneOp::Remove { id } if *id == SceneObjectId(geo.0))),
        "clearing display must remove the object, got {:?}",
        after.ops
    );
    let _ = box_id;
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

// root visibility: hidden-but-cooked, picking and marker gates.

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
    e.doc.create_subflow(geo, ContextKind::Geo);
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

// Picking, document save/load, host-clocked durations.

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
    // The native/test default (no clock) preserves the behavior.
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

#[test]
fn rect_area_light_reaches_the_scene_with_its_extent_and_orientation() {
    // `light_from_node` fails SILENTLY: a resolve error drops the light out
    // of the delta entirely, and the symptom is a scene that renders
    // exactly as if the light had never been added. That is very hard to
    // tell from "the shading is wrong", so this pins the lowering itself.
    use solarxy_core::scene::{LightKind, SceneOp};
    let mut e = engine();
    let id = add(&mut e, GraphContext::Root, "rect_area_light");
    for (key, value) in [
        ("width", ParamValue::Float(6.0)),
        ("height", ParamValue::Float(2.0)),
        // Degrees in, radians out: the resolver owns the conversion.
        ("rotate", ParamValue::Vec3([90.0, 0.0, 0.0])),
        ("two_sided", ParamValue::Bool(true)),
    ] {
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: id,
            key: key.into(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }
    e.cook(&mut || true);
    let delta = e.take_scene_delta();
    let lights = delta
        .ops
        .iter()
        .find_map(|op| match op {
            SceneOp::SetLights { lights } => Some(lights),
            _ => None,
        })
        .expect("SetLights op present");
    assert_eq!(lights.len(), 1, "the rect-area light vanished: {lights:?}");
    let light = &lights[0];
    assert_eq!(light.kind, LightKind::RectArea);
    assert!((light.area_extent[0] - 6.0).abs() < 1e-5);
    assert!((light.area_extent[1] - 2.0).abs() < 1e-5);
    assert!((light.rotate[0] - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    assert!(light.two_sided);

    // A quarter turn about x tips the emitting face from -y to -z, and the
    // helper draws its arrow along `direction`, so that has to follow.
    let basis = light.rect_basis();
    assert!((basis.normal[2] + 1.0).abs() < 1e-4, "{:?}", basis.normal);
    assert!(
        (light.direction[2] + 1.0).abs() < 1e-4,
        "{:?}",
        light.direction
    );
}

// .slxy round-trip fidelity (graph, params, positions, view/camera,
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
    // bumped the subflow geometry nodes to v2 (rendering-group
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

// Validation systems: implicit import validation, the
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
    // The realtime contract (section 17 item 1): a param drag must reach
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

// ---- the gizmo policy ----

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
    // serde shapes for the additions, the same way the other boundary
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

// ---- import_image persistence + async decode round trips ----

/// A valid 1x1 red PNG (identical to the `import_image` unit fixture).
fn red_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

/// The async web path end to end: the cook parks the node on a
/// `DecodeImage` job, the drained job resolves (as the worker would), and
/// the submitted result commits the Image output under the generation
/// guard.
#[test]
fn import_image_async_decode_round_trip() {
    let (mut e, ctx) = subflow_engine();
    e.set_async_jobs(true);
    let asset = e.stage_asset("red.png", "image/png", red_png());

    let img = add(&mut e, ctx, "import_image");
    e.apply(Command::SetParam {
        ctx,
        node: img,
        key: "file".into(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();
    e.cook(&mut || true);

    let jobs = e.take_jobs();
    assert_eq!(jobs.len(), 1, "one decode job spawned");
    let (job_ctx, job_id, request) = &jobs[0];
    assert!(matches!(request, JobRequest::DecodeImage { .. }));

    let result = e.resolve_job(request);
    e.submit_job_result(*job_ctx, *job_id, result);

    let outputs = e.cook.outputs(img).expect("image committed");
    let image = outputs
        .get("image")
        .and_then(crate::registry::coerce::Value::as_image)
        .expect("Image value");
    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(image.pixels, vec![255, 0, 0, 255]);
}

/// Persistence: only the `AssetRef` serializes; a save/load/recook yields
/// pixel-identical image output (equal content hash), and the staged PNG
/// bytes ride the archive because `referenced_assets` sees the param.
#[test]
fn import_image_slxy_round_trip_recooks_identically() {
    let (mut e, ctx) = subflow_engine();
    let asset = e.stage_asset("red.png", "image/png", red_png());

    let img = add(&mut e, ctx, "import_image");
    e.apply(Command::SetParam {
        ctx,
        node: img,
        key: "file".into(),
        value: ParamSource::Literal(ParamValue::Asset(asset.clone())),
    })
    .unwrap();
    e.cook(&mut || true);
    let first_hash = e
        .cook
        .outputs(img)
        .and_then(|o| {
            o.get("image")
                .and_then(crate::registry::coerce::Value::as_image)
                .map(|i| i.hash)
        })
        .expect("first cook produced an image");

    let bytes = e
        .save_slxy(&SceneSidecar {
            generator: "solarxy-test 0.0.0".into(),
            ..Default::default()
        })
        .expect("save .slxy");

    let mut e2 = engine();
    let loaded = e2.load_slxy(&bytes).expect("load .slxy");
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);

    // The staged bytes traveled inside the archive (GC-at-save kept them).
    assert!(
        e2.asset_manifest()
            .iter()
            .any(|(h, n)| *h == asset.0 && n == "red.png"),
        "asset embedded and restored"
    );

    e2.cook(&mut || true);
    let second = e2
        .cook
        .outputs(img)
        .and_then(|o| {
            o.get("image")
                .and_then(crate::registry::coerce::Value::as_image)
                .cloned()
        })
        .expect("recook produced an image");
    assert_eq!(second.hash, first_hash, "pixel-identical after round trip");
    assert_eq!(second.pixels, vec![255, 0, 0, 255]);
}

/// The exit-criterion chain, engine-level: a primitive through
/// `uv_project` into `material` with an `import_image` wired into the base
/// color map port. The cooked output must carry projected UVs and one
/// override material whose diffuse texture is the imported image with a
/// neutralized (white) base color factor.
#[test]
#[allow(clippy::float_cmp)] // exact values constructed by the test
fn image_material_uv_project_chain_cooks_end_to_end() {
    let (mut e, ctx) = subflow_engine();
    let asset = e.stage_asset("red.png", "image/png", red_png());

    let prim = add(&mut e, ctx, "box");
    let uv = add(&mut e, ctx, "uv_project");
    let img = add(&mut e, ctx, "import_image");
    let mat = add(&mut e, ctx, "material");

    for (from, from_port, to, to_port) in [
        (prim, "geometry", uv, "geometry"),
        (uv, "geometry", mat, "geometry"),
        (img, "image", mat, "base_color_map"),
    ] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: from,
                port: from_port.into(),
            },
            to: PortRefDto {
                node: to,
                port: to_port.into(),
            },
        })
        .unwrap();
    }
    e.apply(Command::SetParam {
        ctx,
        node: img,
        key: "file".into(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: mat,
        key: "material_name".into(),
        value: ParamSource::Literal(ParamValue::Text("textured".into())),
    })
    .unwrap();
    // The cook is display-driven: flag the chain tail as the container's
    // output.
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(mat),
    })
    .unwrap();
    e.cook(&mut || true);

    let outputs = e.cook.outputs(mat).expect("chain cooked");
    let set = outputs
        .get("geometry")
        .and_then(crate::registry::coerce::Value::as_geometry)
        .expect("geometry out");
    assert!(
        set.meshes.iter().all(|m| m.tex_coords.is_some()),
        "uv_project wrote UVs that survive the material node"
    );
    assert_eq!(set.materials.len(), 1, "override-all");
    let m = &set.materials[0];
    assert_eq!(m.name, "textured");
    let tex = m.diffuse_texture_data.as_ref().expect("map landed");
    assert_eq!(tex.pixels, vec![255, 0, 0, 255]);
    assert_eq!(
        m.base_color_factor,
        [1.0, 1.0, 1.0, 1.0],
        "connected map neutralizes the factor"
    );
}

/// All six nodes: they cook, they survive a `.slxy` round trip with
/// their params and the switch's variadic wire order intact, and an undo of the
/// last edit restores the document exactly.
#[test]
fn modeling_nodes_cook_round_trip_and_undo() {
    let (mut e, ctx) = subflow_engine();

    let prim = add(&mut e, ctx, "box");
    let arr = add(&mut e, ctx, "array");
    let mir = add(&mut e, ctx, "mirror");
    let del = add(&mut e, ctx, "delete");
    let bnd = add(&mut e, ctx, "bounds");
    let nul = add(&mut e, ctx, "null");
    let sw = add(&mut e, ctx, "switch");

    // box -> array -> mirror -> delete -> null -> switch (wire 0)
    // box -> bounds ----------------------------> switch (wire 1)
    // Everything sits upstream of the switch, because the cook is demand-driven
    // from the display flag: a dead-end branch would never cook at all.
    for (from, to, to_port) in [
        (prim, arr, "geometry"),
        (arr, mir, "geometry"),
        (mir, del, "geometry"),
        (del, nul, "geometry"),
        (prim, bnd, "geometry"),
        (nul, sw, "inputs"),
        (bnd, sw, "inputs"),
    ] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: from,
                port: "geometry".into(),
            },
            to: PortRefDto {
                node: to,
                port: to_port.into(),
            },
        })
        .unwrap();
    }

    // Non-default params on every new node, so the round trip has something to
    // lose.
    for (node, key, value) in [
        (arr, "count", ParamValue::Int(5)),
        (arr, "mode", ParamValue::Enum("radial".into())),
        (arr, "radius", ParamValue::Float(2.5)),
        (mir, "axis", ParamValue::Enum("z".into())),
        (mir, "offset", ParamValue::Float(1.5)),
        (mir, "keep_original", ParamValue::Bool(false)),
        (del, "mode", ParamValue::Enum("normal".into())),
        (del, "angle", ParamValue::Float(30.0)),
        (del, "invert", ParamValue::Bool(true)),
        (bnd, "mode", ParamValue::Enum("center".into())),
        (sw, "index", ParamValue::Int(1)),
    ] {
        e.apply(Command::SetParam {
            ctx,
            node,
            key: key.into(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }

    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(sw),
    })
    .unwrap();
    e.cook(&mut || true);

    // Every one of the six produced geometry.
    for (node, label) in [
        (arr, "array"),
        (mir, "mirror"),
        (del, "delete"),
        (bnd, "bounds"),
        (nul, "null"),
        (sw, "switch"),
    ] {
        let outputs = e
            .cook
            .outputs(node)
            .unwrap_or_else(|| panic!("{label} cooked"));
        assert!(
            outputs
                .get("geometry")
                .and_then(crate::registry::coerce::Value::as_geometry)
                .is_some(),
            "{label} produced geometry"
        );
    }

    // The switch is on index 1: the SECOND wire, which is bounds in center
    // mode (a single point primitive since v2), not delete. The point-only
    // shape proves which branch came through.
    let picked = e
        .cook
        .outputs(sw)
        .unwrap()
        .get("geometry")
        .and_then(crate::registry::coerce::Value::as_geometry)
        .unwrap()
        .clone();
    assert_eq!(
        (picked.point_count(), picked.triangle_count()),
        (1, 0),
        "switch index 1 selected the bounds center point, not delete"
    );

    // Round trip through .slxy: params, wire order, and the display flag.
    let before = fingerprint(&e, ctx);
    let bytes = e.save_slxy(&SceneSidecar::default()).expect("save .slxy");
    let mut e2 = engine();
    e2.load_slxy(&bytes).expect("load");
    assert_eq!(
        fingerprint(&e2, ctx),
        before,
        ".slxy round trip is lossless"
    );

    // And the loaded document still cooks to the same selection.
    e2.cook(&mut || true);
    let reloaded = e2
        .cook
        .outputs(sw)
        .expect("switch cooked after load")
        .get("geometry")
        .and_then(crate::registry::coerce::Value::as_geometry)
        .unwrap()
        .clone();
    assert_eq!(
        (reloaded.point_count(), reloaded.triangle_count()),
        (1, 0),
        "same branch after reload"
    );

    // Undo the last param edit and land back on the previous document exactly.
    let before_edit = fingerprint(&e, ctx);
    e.apply(Command::SetParam {
        ctx,
        node: arr,
        key: "count".into(),
        value: ParamSource::Literal(ParamValue::Int(9)),
    })
    .unwrap();
    assert_ne!(fingerprint(&e, ctx), before_edit, "the edit landed");
    e.apply(Command::Undo).unwrap();
    assert_eq!(fingerprint(&e, ctx), before_edit, "undo restored it");
}

/// The exit chain, and the reason `material` had to land before the
/// modeling wave: a textured material must survive duplication. box -> material
/// -> array -> mirror, with the material assigned BEFORE the copies are made, so
/// every copy has to carry it and merge's content-hash dedup has to collapse
/// them back to one table entry.
#[test]
fn materials_survive_array_and_mirror() {
    let (mut e, ctx) = subflow_engine();
    let asset = e.stage_asset("red.png", "image/png", red_png());

    let prim = add(&mut e, ctx, "box");
    let img = add(&mut e, ctx, "import_image");
    let mat = add(&mut e, ctx, "material");
    let arr = add(&mut e, ctx, "array");
    let mir = add(&mut e, ctx, "mirror");

    for (from, from_port, to, to_port) in [
        (prim, "geometry", mat, "geometry"),
        (img, "image", mat, "base_color_map"),
        (mat, "geometry", arr, "geometry"),
        (arr, "geometry", mir, "geometry"),
    ] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: from,
                port: from_port.into(),
            },
            to: PortRefDto {
                node: to,
                port: to_port.into(),
            },
        })
        .unwrap();
    }
    e.apply(Command::SetParam {
        ctx,
        node: img,
        key: "file".into(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: mat,
        key: "material_name".into(),
        value: ParamSource::Literal(ParamValue::Text("textured".into())),
    })
    .unwrap();
    // 4 copies, stepped clear of each other on X. Pinned to Bake because
    // this test's subject is material dedup through a real bake and merge;
    // Instance never duplicates a material because it never duplicates a mesh.
    e.apply(Command::SetParam {
        ctx,
        node: arr,
        key: "copy_mode".into(),
        value: ParamSource::Literal(ParamValue::Enum("bake".into())),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: arr,
        key: "count".into(),
        value: ParamSource::Literal(ParamValue::Int(4)),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: arr,
        key: "offset".into(),
        value: ParamSource::Literal(ParamValue::Vec3([3.0, 0.0, 0.0])),
    })
    .unwrap();
    // Mirror the whole row across x = 0, keeping the original.
    e.apply(Command::SetParam {
        ctx,
        node: mir,
        key: "keep_original".into(),
        value: ParamSource::Literal(ParamValue::Bool(true)),
    })
    .unwrap();

    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(mir),
    })
    .unwrap();
    e.cook(&mut || true);

    let outputs = e.cook.outputs(mir).expect("chain cooked");
    let set = outputs
        .get("geometry")
        .and_then(crate::registry::coerce::Value::as_geometry)
        .expect("geometry out");

    // 4 copies from the array, doubled by the mirror.
    assert_eq!(set.mesh_count(), 8, "array x mirror produced 8 meshes");

    // The whole point: one material entry, shared by every copy, still carrying
    // the texture. A merge that failed to dedup would give 8 identical entries;
    // a bake that dropped materials would give 0.
    assert_eq!(
        set.materials.len(),
        1,
        "the 8 copies dedup back to one material"
    );
    let m = &set.materials[0];
    assert_eq!(m.name, "textured");
    let tex = m.diffuse_texture_data.as_ref().expect("texture survived");
    assert_eq!(tex.pixels, vec![255, 0, 0, 255]);
    for mesh in &set.meshes {
        assert_eq!(mesh.material_index, Some(0), "every copy points at it");
    }

    // The mirror half really is mirrored: the row runs +X, so the reflection
    // must reach into -X.
    assert!(
        set.bounds.min.x < -3.0 && set.bounds.max.x > 3.0,
        "both halves present: {:?}",
        set.bounds
    );
}

/// The copy-mode migration proved on a whole document rather than on one
/// node's param map.
///
/// A scene saved before 0.8.2 stores `array` at version 1 with no
/// `copy_mode`, written by an engine that could only bake. Version 2 defaults
/// to Instance, so the migration has to write Bake back in. Get it wrong and
/// the document opens looking entirely plausible: geometry is still there,
/// the node still cooks clean, and every count downstream is quietly
/// different. That is why this goes through the real save and load seam
/// instead of asserting on the hook.
#[test]
fn a_document_saved_before_the_copy_mode_split_opens_still_baked() {
    let (mut e, ctx) = subflow_engine();
    let prim = add(&mut e, ctx, "box");
    let arr = add(&mut e, ctx, "array");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: prim,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: arr,
            port: "geometry".into(),
        },
    })
    .unwrap();
    for (key, value) in [
        ("count", ParamValue::Int(4)),
        ("offset", ParamValue::Vec3([3.0, 0.0, 0.0])),
        // What a pre-0.8.2 document effectively said, and the only thing it
        // could have said.
        ("copy_mode", ParamValue::Enum("bake".into())),
    ] {
        e.apply(Command::SetParam {
            ctx,
            node: arr,
            key: key.into(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(arr),
    })
    .unwrap();
    e.cook(&mut || true);

    /// Everything about the output a wrong migration would move: how the
    /// copies are partitioned, whether they are placements or geometry, and
    /// where the points actually are.
    fn shape(e: &Engine, node: NodeId) -> (u32, Option<usize>, Vec<[f32; 3]>) {
        let set = e
            .cook
            .outputs(node)
            .expect("cooked")
            .get("geometry")
            .and_then(crate::registry::coerce::Value::as_geometry)
            .expect("geometry out");
        (
            set.mesh_count(),
            set.meshes[0].instances.as_ref().map(|i| i.len()),
            set.meshes[0].positions.to_vec(),
        )
    }
    let want = shape(&e, arr);
    assert_eq!(want.0, 4, "the reference cook baked four copies");
    assert_eq!(want.1, None, "baked output carries no placement list");

    // Save, then age the file: strip `copy_mode` and stamp the array node
    // back to version 1, which is what a scene written before this release
    // holds on disk.
    let mut scene = crate::engine::scenefile::document_to_scene(
        &e.doc.to_data(),
        e.cook_mode,
        &crate::runtime::RuntimeSettings::default(),
        &SceneSidecar::default(),
        Vec::new(),
    );
    let mut aged = 0;
    for graph in scene.graph.subflows.values_mut() {
        for node in &mut graph.nodes {
            if node.type_id == "array" {
                node.type_version = 1;
                node.params.remove("copy_mode");
                aged += 1;
            }
        }
    }
    assert_eq!(aged, 1, "the array node was found and aged");

    let (document, warnings) = crate::engine::scenefile::scene_to_document(&scene, &e.registry);
    assert!(
        warnings.is_empty(),
        "a clean migration must not toast: {warnings:?}"
    );

    let mut reopened = engine();
    reopened.load_document(&DocumentFile {
        format_version: 1,
        document,
        cook_mode: e.cook_mode,
    });
    reopened.cook(&mut || true);

    assert_eq!(
        shape(&reopened, arr),
        want,
        "the reopened document must cook to exactly what it cooked before, \
         down to the vertex positions"
    );
}

/// The geo container's rotate-order migration, proved on a whole document
/// by the orientation it produces rather than by the value it stores.
///
/// A container saved before the rotate-order unification composed its
/// rotation as ZYX. The current default is XYZ, so a document reopened
/// without the explicit stamp silently re-orients: nothing errors, nothing
/// warns, and an object is simply facing somewhere its author never chose.
///
/// The unit tests around the hook check the stamp. This checks the thing the
/// stamp exists for, which is the matrix.
#[test]
fn a_geo_saved_before_the_rotate_order_split_opens_facing_the_same_way() {
    /// The world matrix of the one geo in a document whose stored geo node
    /// has been aged to `version`, with `rotate_order` stripped.
    fn reopened_at(version: u32, rotate: [f64; 3]) -> [[f32; 4]; 4] {
        let mut e = engine();
        let geo = add(&mut e, GraphContext::Root, "geo");
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: geo,
            key: "rotate".into(),
            value: ParamSource::Literal(ParamValue::Vec3(rotate)),
        })
        .unwrap();

        let mut scene = crate::engine::scenefile::document_to_scene(
            &e.doc.to_data(),
            e.cook_mode,
            &crate::runtime::RuntimeSettings::default(),
            &SceneSidecar::default(),
            Vec::new(),
        );
        let mut aged = 0;
        for node in &mut scene.graph.nodes {
            if node.type_id == "geo" {
                node.type_version = version;
                node.params.remove("rotate_order");
                aged += 1;
            }
        }
        assert_eq!(aged, 1, "the geo node was found and aged");

        let (document, warnings) = crate::engine::scenefile::scene_to_document(&scene, &e.registry);
        assert!(warnings.is_empty(), "clean migration: {warnings:?}");
        let mut reopened = engine();
        reopened.load_document(&DocumentFile {
            format_version: 1,
            document,
            cook_mode: e.cook_mode,
        });
        crate::engine::scene::geo_world_matrix(
            &reopened.doc,
            &reopened.registry,
            &reopened.previews,
            geo,
        )
        .into()
    }

    // Two nonzero axes: the only case where the composition order is
    // observable at all, so the old order has to be preserved explicitly.
    let observable = [30.0, 40.0, 0.0];
    let aged = reopened_at(2, observable);
    let current = reopened_at(3, observable);
    let differs = aged
        .iter()
        .zip(&current)
        .any(|(a, b)| a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1e-4));
    assert!(
        differs,
        "the fixture has to be a rotation whose order is observable, or this \
         test would pass with the migration deleted"
    );

    // What the aged document must produce: the ZYX composition it was
    // authored against, which is what an explicit stamp gives.
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    for (key, value) in [
        ("rotate", ParamValue::Vec3(observable)),
        ("rotate_order", ParamValue::Enum("zyx".into())),
    ] {
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: geo,
            key: key.into(),
            value: ParamSource::Literal(value),
        })
        .unwrap();
    }
    let want: [[f32; 4]; 4] =
        crate::engine::scene::geo_world_matrix(&e.doc, &e.registry, &e.previews, geo).into();

    for (row, (a, b)) in aged.iter().zip(&want).enumerate() {
        for (col, (x, y)) in a.iter().zip(b).enumerate() {
            assert!(
                (x - y).abs() < 1e-4,
                "a reopened pre-split geo faces somewhere its author never \
                 chose: [{row}][{col}] {x} vs {y}"
            );
        }
    }
}

/// The downstream contract for instanced geometry, node by node.
///
/// Instancing is a representation choice, not a different result, so the
/// same graph must produce the same geometry whether the copy operation
/// placed its copies or baked them. A node that cannot carry placements has
/// to bake them; the failure this guards is the one where it neither
/// carries nor bakes and quietly returns one copy where the user placed
/// four.
///
/// Each case cooks `box -> array -> <node>` twice, once with the array on
/// Instance and once on Bake, and compares the two outputs after baking
/// whatever survives. `carries` records which side of the contract the node
/// is on, so a node silently changing sides fails here rather than in
/// someone's scene.
#[test]
fn every_downstream_node_agrees_with_its_baked_equivalent() {
    // (node type, params, whether it carries placements through)
    let cases: &[(&str, &[(&str, ParamValue)], bool)] = &[
        // Carry: each works in the prototype's own space with a result
        // identical for every copy.
        ("subdivide", &[("iterations", ParamValue::Int(1))], true),
        ("uv_project", &[], true),
        ("compute_normals", &[], true),
        ("material", &[], true),
        // Bake: the meaning is per copy, or real geometry is needed.
        (
            "transform",
            &[("translate", ParamValue::Vec3([1.0, 2.0, 3.0]))],
            false,
        ),
        ("delete", &[], false),
        ("mirror", &[], false),
        ("bounds", &[], false),
        ("attribute_create", &[], false),
        ("points_from_geo", &[], false),
        ("scatter", &[("count", ParamValue::Int(8))], false),
    ];

    for (node_type, params, carries) in cases {
        let mut shapes = Vec::new();
        for mode in ["instance", "bake"] {
            let (mut e, ctx) = subflow_engine();
            let prim = add(&mut e, ctx, "box");
            let arr = add(&mut e, ctx, "array");
            let sink = add(&mut e, ctx, node_type);
            for (from, to) in [(prim, arr), (arr, sink)] {
                e.apply(Command::Connect {
                    ctx,
                    from: PortRefDto {
                        node: from,
                        port: "geometry".into(),
                    },
                    to: PortRefDto {
                        node: to,
                        port: "geometry".into(),
                    },
                })
                .unwrap();
            }
            e.apply(Command::SetParam {
                ctx,
                node: arr,
                key: "copy_mode".into(),
                value: ParamSource::Literal(ParamValue::Enum(mode.into())),
            })
            .unwrap();
            e.apply(Command::SetParam {
                ctx,
                node: arr,
                key: "count".into(),
                value: ParamSource::Literal(ParamValue::Int(4)),
            })
            .unwrap();
            for (key, value) in *params {
                e.apply(Command::SetParam {
                    ctx,
                    node: sink,
                    key: (*key).into(),
                    value: ParamSource::Literal(value.clone()),
                })
                .unwrap();
            }
            e.apply(Command::SetActiveOutput {
                ctx,
                node: Some(sink),
            })
            .unwrap();
            e.cook(&mut || true);

            let set = e
                .cook
                .outputs(sink)
                .unwrap_or_else(|| panic!("{node_type} ({mode}) cooked"))
                .get("geometry")
                .and_then(crate::registry::coerce::Value::as_geometry)
                .unwrap_or_else(|| panic!("{node_type} ({mode}) output geometry"));

            if mode == "instance" {
                assert_eq!(
                    set.is_instanced(),
                    *carries,
                    "{node_type} changed which side of the carry-or-bake \
                     contract it is on"
                );
            }
            // Compare after baking, so a carried list and a baked one are
            // held to the same standard: the geometry the user sees.
            let baked = set.baked().unwrap_or_else(|e| panic!("{node_type}: {e}"));
            let mut points: Vec<[f32; 3]> = baked
                .meshes
                .iter()
                .flat_map(|m| m.positions.iter().copied())
                .collect();
            points.sort_by(|a, b| a.partial_cmp(b).unwrap());
            shapes.push(points);
        }
        assert_eq!(
            shapes[0].len(),
            shapes[1].len(),
            "{node_type}: instanced input produced a different amount of \
             geometry than the baked equivalent"
        );
        for (i, (a, b)) in shapes[0].iter().zip(&shapes[1]).enumerate() {
            for lane in 0..3 {
                assert!(
                    (a[lane] - b[lane]).abs() < 1e-4,
                    "{node_type}: point {i} differs between modes: {a:?} vs {b:?}"
                );
            }
        }
    }
}

/// Merge is the node the placement level was moved for, so it gets its own
/// case: it is variadic, and its whole point is combining an instanced
/// input with a plain one.
#[test]
fn merging_a_scatter_with_its_surface_keeps_the_copies_instanced() {
    let (mut e, ctx) = subflow_engine();
    let surface = add(&mut e, ctx, "sphere");
    let template = add(&mut e, ctx, "box");
    let scatter = add(&mut e, ctx, "scatter");
    let copy = add(&mut e, ctx, "copy_to_points");
    let merge = add(&mut e, ctx, "merge");

    for (from, to, port) in [
        (surface, scatter, "geometry"),
        (scatter, copy, "points"),
        (template, copy, "template"),
        (copy, merge, "inputs"),
        (surface, merge, "inputs"),
    ] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: from,
                port: "geometry".into(),
            },
            to: PortRefDto {
                node: to,
                port: port.into(),
            },
        })
        .unwrap();
    }
    e.apply(Command::SetParam {
        ctx,
        node: scatter,
        key: "count".into(),
        value: ParamSource::Literal(ParamValue::Int(40)),
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(merge),
    })
    .unwrap();
    e.cook(&mut || true);

    let set = e
        .cook
        .outputs(merge)
        .expect("merge cooked")
        .get("geometry")
        .and_then(crate::registry::coerce::Value::as_geometry)
        .expect("geometry out");

    // One instanced prototype plus one plain surface, not one of each
    // baked and not a surface replicated forty times.
    assert!(set.is_instanced(), "the copies survived the merge");
    let placed: Vec<usize> = set.meshes.iter().map(|m| m.instance_count()).collect();
    assert!(
        placed.contains(&40),
        "the prototype keeps its forty placements: {placed:?}"
    );
    assert!(
        placed.contains(&1),
        "the surface is still placed once: {placed:?}"
    );
}

/// The typed-context model generalizes: a fabricated container
/// opens a Mat network, a Mat-placed container opens a Tex network three
/// levels deep, placement is judged by the target graph's KIND (never its
/// address or a special-cased type id), and a removed container's whole
/// child-network tree survives an undo round-trip with its kinds intact.
#[test]
fn typed_contexts_generalize_beyond_geo() {
    use crate::registry::{
        BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, Registry,
    };

    #[allow(clippy::unnecessary_wraps)] // signature matches CookFn
    fn noop_cook(
        _p: &crate::registry::resolve::ResolvedParams,
        _in: &crate::cook::Inputs,
        _cx: &mut crate::cook::CookCtx,
    ) -> Result<crate::cook::CookOutcome, crate::cook::CookError> {
        Ok(crate::cook::CookOutcome::Done(crate::cook::Outputs::empty()))
    }

    let mk = |type_id: &'static str, contexts: ContextSet, opens: Option<ContextKind>| {
        NodeTypeDescriptor {
            type_id,
            version: 1,
            display_name: type_id,
            category: Category::Utility,
            contexts,
            opens,
            inputs: vec![],
            outputs: vec![],
            params: vec![],
            bypass: BypassBehavior::Mute,
            doc: "",
            search_aliases: &[],
            glyph: "probe",
            role: NodeRole::Standard,
            cook: noop_cook,
            migrate: None,
        }
    };
    let registry = Registry::with_descriptors(vec![
        mk("matnet", ContextSet::OBJ, Some(ContextKind::Mat)),
        mk("subtex", ContextSet::MAT, Some(ContextKind::Tex)),
        mk("mat_only", ContextSet::MAT, None),
        mk("geo_only", ContextSet::GEO, None),
    ])
    .expect("test registry satisfies the invariants");
    let mut e = Engine::with_registry(registry);

    // Level 1 -> 2: an obj container opens a Mat canvas, from `opens`.
    let matnet = add(&mut e, GraphContext::Root, "matnet");
    let mat_ctx = GraphContext::Subflow(matnet);
    assert_eq!(e.doc.graph(mat_ctx).unwrap().kind, ContextKind::Mat);

    // Placement judges the target graph's kind: a Mat node lands, a Geo
    // node is refused, and a Mat node cannot sit at root (obj).
    let _mat_node = add(&mut e, mat_ctx, "mat_only");
    let refused = e.apply(Command::AddNode {
        ctx: mat_ctx,
        node_type: "geo_only".to_string(),
        position: [0.0, 0.0],
    });
    assert!(matches!(
        refused,
        Err(EngineError::ContextIllegal { type_id }) if type_id == "geo_only"
    ));
    let refused_root = e.apply(Command::AddNode {
        ctx: GraphContext::Root,
        node_type: "mat_only".to_string(),
        position: [0.0, 0.0],
    });
    assert!(matches!(
        refused_root,
        Err(EngineError::ContextIllegal { .. })
    ));

    // Level 2 -> 3: containers nest; the grandchild kind follows `opens`.
    let subtex = add(&mut e, mat_ctx, "subtex");
    let tex_ctx = GraphContext::Subflow(subtex);
    assert_eq!(e.doc.graph(tex_ctx).unwrap().kind, ContextKind::Tex);

    // Remove the top container: the WHOLE tree goes (no orphaned
    // grandchild network), and undo restores every level with its kind.
    e.apply(Command::RemoveNodes {
        ctx: GraphContext::Root,
        ids: vec![matnet],
    })
    .unwrap();
    assert!(e.doc.graph(mat_ctx).is_err(), "child network removed");
    assert!(e.doc.graph(tex_ctx).is_err(), "grandchild network removed");

    e.apply(Command::Undo).unwrap();
    assert_eq!(e.doc.graph(mat_ctx).unwrap().kind, ContextKind::Mat);
    assert_eq!(e.doc.graph(tex_ctx).unwrap().kind, ContextKind::Tex);
    assert!(
        e.doc.graph(mat_ctx).unwrap().node(subtex).is_some(),
        "the nested container node itself came back"
    );
}

/// The cross-context reference machinery: a geo-side node
/// references a material network by path; editing INSIDE the referenced
/// network re-dirties and recooks the referrer with the fresh value in the
/// SAME pass (the reference-ordered context walk); cycles are refused at
/// set time (direct self-reference and a two-network loop); deleting the
/// referenced network is allowed and the referrer cooks into an error.
#[test]
fn cross_context_references_propagate_and_refuse_cycles() {
    use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
    use crate::registry::coerce::{DataType, Value};
    use crate::registry::param_spec::{NodePathAccept, ParamSpec, ParamType};
    use crate::registry::resolve::ResolvedParams;
    use crate::registry::{
        BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec, Registry,
    };

    // A Mat-context producer: emits its `value` param as a Float.
    #[allow(clippy::unnecessary_wraps)]
    fn mat_const_cook(
        p: &ResolvedParams,
        _in: &Inputs,
        _cx: &mut CookCtx,
    ) -> Result<CookOutcome, CookError> {
        Ok(CookOutcome::Done(Outputs::single(
            "value",
            Value::Float(p.f64("value")),
        )))
    }

    // A consumer with a NodePath param: copies the referenced network's
    // published Float; a set-but-unresolvable reference is a cook error.
    fn ref_consumer_cook(
        p: &ResolvedParams,
        _in: &Inputs,
        cx: &mut CookCtx,
    ) -> Result<CookOutcome, CookError> {
        match p.node_ref("source") {
            None => Ok(CookOutcome::Done(Outputs::single(
                "value",
                Value::Float(0.0),
            ))),
            Some(target) => match cx.referenced(target) {
                Some(Value::Float(v)) => Ok(CookOutcome::Done(Outputs::single(
                    "value",
                    Value::Float(*v),
                ))),
                _ => Err(CookError::Failed {
                    message: format!("reference to node {} does not resolve", target.0),
                }),
            },
        }
    }

    let source_param = || {
        ParamSpec::new(
            "source",
            "Source",
            "general",
            ParamType::NodePath {
                accept: NodePathAccept::Opens(ContextKind::Mat),
            },
            ParamValue::NodeRef(None),
        )
    };
    let float_out =
        || vec![PortSpec::single("value", "Value", DataType::Float, false).default_port()];
    let matnet = NodeTypeDescriptor {
        type_id: "matnet",
        version: 1,
        display_name: "Matnet",
        category: Category::Container,
        contexts: ContextSet::OBJ,
        opens: Some(ContextKind::Mat),
        inputs: vec![],
        outputs: vec![],
        params: vec![],
        bypass: BypassBehavior::Mute,
        doc: "",
        search_aliases: &[],
        glyph: "matnet",
        role: NodeRole::Container,
        cook: |_, _, _| Ok(CookOutcome::Done(Outputs::empty())),
        migrate: None,
    };
    let mat_const = NodeTypeDescriptor {
        type_id: "mat_const",
        version: 1,
        display_name: "Mat Const",
        category: Category::Utility,
        contexts: ContextSet::MAT,
        opens: None,
        inputs: vec![],
        outputs: float_out(),
        params: vec![
            ParamSpec::new(
                "value",
                "Value",
                "general",
                ParamType::Float,
                ParamValue::Float(1.0),
            ),
            // A Mat node may itself reference another material network
            // (the tex_ref pattern); the cycle tests set this.
            source_param(),
        ],
        bypass: BypassBehavior::Mute,
        doc: "",
        search_aliases: &[],
        glyph: "const",
        role: NodeRole::Standard,
        cook: mat_const_cook,
        migrate: None,
    };
    let ref_consumer = NodeTypeDescriptor {
        type_id: "ref_consumer",
        version: 1,
        display_name: "Ref Consumer",
        category: Category::Utility,
        contexts: ContextSet::GEO,
        opens: None,
        inputs: vec![],
        outputs: float_out(),
        params: vec![source_param()],
        bypass: BypassBehavior::Mute,
        doc: "",
        search_aliases: &[],
        glyph: "consumer",
        role: NodeRole::Standard,
        cook: ref_consumer_cook,
        migrate: None,
    };
    let registry = Registry::with_descriptors(vec![matnet, mat_const, ref_consumer])
        .expect("test registry satisfies the invariants");
    let mut e = Engine::with_registry(registry);

    // A geo-side network minted FIRST, so plain id-order context iteration
    // would cook it before the material network and read a stale value;
    // only the reference-ordered walk makes the same-pass assertion hold.
    let geo = e.doc.mint_node_id();
    e.doc.create_subflow(geo, ContextKind::Geo);
    let geo_ctx = GraphContext::Subflow(geo);

    let matnet1 = add(&mut e, GraphContext::Root, "matnet");
    let mat_ctx = GraphContext::Subflow(matnet1);
    let producer = add(&mut e, mat_ctx, "mat_const");
    let consumer = add(&mut e, geo_ctx, "ref_consumer");

    // Point the consumer at the material network.
    e.apply(Command::SetParam {
        ctx: geo_ctx,
        node: consumer,
        key: "source".to_string(),
        value: ParamSource::Literal(ParamValue::NodeRef(Some(matnet1))),
    })
    .unwrap();

    // ONE cook pass: the referenced network cooks first, the consumer
    // reads the fresh published value.
    e.cook(&mut || true);
    let committed = |e: &Engine, node: NodeId| -> Option<f64> {
        e.cook.outputs(node).and_then(|o| match o.get("value") {
            Some(Value::Float(v)) => Some(*v),
            _ => None,
        })
    };
    assert_eq!(committed(&e, consumer), Some(1.0));

    // Editing INSIDE the referenced network dirties the referrer across
    // contexts, and one pass delivers the fresh value.
    e.apply(Command::SetParam {
        ctx: mat_ctx,
        node: producer,
        key: "value".to_string(),
        value: ParamSource::Literal(ParamValue::Float(2.0)),
    })
    .unwrap();
    assert_eq!(
        e.cook.state(consumer),
        crate::cook::state::CookState::Dirty,
        "a /mat edit must re-dirty the geo-side referrer"
    );
    e.cook(&mut || true);
    assert_eq!(committed(&e, consumer), Some(2.0));

    // Cycle refusal, direct: a node inside a network cannot reference its
    // own container.
    let self_ref = e.apply(Command::SetParam {
        ctx: mat_ctx,
        node: producer,
        key: "source".to_string(),
        value: ParamSource::Literal(ParamValue::NodeRef(Some(matnet1))),
    });
    assert!(matches!(self_ref, Err(EngineError::ReferenceCycle { .. })));

    // Cycle refusal, two networks: A's node references B, then B's node
    // referencing A closes the loop and is refused at set time.
    let matnet2 = add(&mut e, GraphContext::Root, "matnet");
    let mat2_ctx = GraphContext::Subflow(matnet2);
    let producer2 = add(&mut e, mat2_ctx, "mat_const");
    e.apply(Command::SetParam {
        ctx: mat_ctx,
        node: producer,
        key: "source".to_string(),
        value: ParamSource::Literal(ParamValue::NodeRef(Some(matnet2))),
    })
    .unwrap();
    let closes_loop = e.apply(Command::SetParam {
        ctx: mat2_ctx,
        node: producer2,
        key: "source".to_string(),
        value: ParamSource::Literal(ParamValue::NodeRef(Some(matnet1))),
    });
    assert!(matches!(
        closes_loop,
        Err(EngineError::ReferenceCycle { .. })
    ));

    // Deleting the referenced network is allowed; the referrer recooks
    // into an error badge (dangling reference), never a stale value.
    e.apply(Command::RemoveNodes {
        ctx: GraphContext::Root,
        ids: vec![matnet1],
    })
    .unwrap();
    assert_eq!(
        e.cook.state(consumer),
        crate::cook::state::CookState::Dirty,
        "removing the target must dirty the referrer"
    );
    e.cook(&mut || true);
    assert!(
        matches!(
            e.cook.status(consumer),
            Some(crate::cook::state::CookStatus::Error { message }) if message.contains("does not resolve")
        ),
        "a dangling reference is an error badge, got {:?}",
        e.cook.status(consumer)
    );
}

/// Network kinds and node references survive a `.slxy` round trip (phase
/// 17d): the file stores the kind string per subflow, and the plain param
/// form stores a reference as the raw id number.
#[test]
fn context_kinds_and_node_refs_survive_a_slxy_round_trip() {
    use crate::engine::scenefile::SceneSidecar;

    let mut e = engine();
    // A real geo container through the command path (kind from `opens`).
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let _box_node = add(&mut e, sub, "box");
    assert_eq!(e.doc.graph(sub).unwrap().kind, ContextKind::Geo);

    let bytes = e.save_slxy(&SceneSidecar::default()).expect("save");
    // The stored form carries the kind string.
    let mut e2 = engine();
    e2.load_slxy(&bytes).expect("load");
    assert_eq!(
        e2.doc.graph(GraphContext::Subflow(geo)).unwrap().kind,
        ContextKind::Geo,
        "the subflow kind survives the archive"
    );

    // The plain JSON form of a NodeRef is the raw id (or null): pin the
    // schema-v1 shape directly.
    use crate::registry::param_spec::{NodePathAccept, ParamType};
    let ty = ParamType::NodePath {
        accept: NodePathAccept::TypeIs("camera".to_string()),
    };
    let set = crate::registry::resolve::param_value_to_json(&ParamValue::NodeRef(Some(geo)));
    assert_eq!(set, serde_json::json!(geo.0));
    let unset = crate::registry::resolve::param_value_to_json(&ParamValue::NodeRef(None));
    assert!(unset.is_null());
    let back = crate::registry::resolve::param_source_from_json(&set, &ty).expect("parse");
    assert_eq!(back, ParamSource::Literal(ParamValue::NodeRef(Some(geo))));
    let back_null = crate::registry::resolve::param_source_from_json(&serde_json::Value::Null, &ty)
        .expect("parse null");
    assert_eq!(back_null, ParamSource::Literal(ParamValue::NodeRef(None)));
}

/// The texture context cooks end to end: a texnet opens a Tex
/// canvas through the command path, generators and filters chain inside
/// it, the display node publishes through `Engine::display_image`, and
/// editing an upstream param recooks the chain (the keep-last-good fix
/// proving out on a real image chain).
#[test]
fn texture_network_cooks_and_publishes_its_display_image() {
    let mut e = engine();
    let texnet = add(&mut e, GraphContext::Root, "texnet");
    let tex_ctx = GraphContext::Subflow(texnet);
    assert_eq!(e.doc.graph(tex_ctx).unwrap().kind, ContextKind::Tex);

    // noise -> blur, display on blur (first node auto-claims, so move it).
    let noise = add(&mut e, tex_ctx, "noise");
    let blur = add(&mut e, tex_ctx, "blur");
    e.apply(Command::Connect {
        ctx: tex_ctx,
        from: PortRefDto {
            node: noise,
            port: "image".into(),
        },
        to: PortRefDto {
            node: blur,
            port: "image".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: tex_ctx,
        node: Some(blur),
    })
    .unwrap();
    e.cook(&mut || true);

    let img = e
        .display_image(texnet)
        .expect("the network publishes an image");
    assert_eq!(
        (img.width, img.height),
        (512, 512),
        "generator default dims"
    );
    let first_hash = img.hash;

    // Edit the generator: the chain recooks and the published image
    // CHANGES (an image-only second commit must not be swallowed).
    e.apply(Command::SetParam {
        ctx: tex_ctx,
        node: noise,
        key: "seed".to_string(),
        value: ParamSource::Literal(ParamValue::Int(42)),
    })
    .unwrap();
    e.cook(&mut || true);
    let img2 = e.display_image(texnet).expect("still publishing");
    assert_ne!(
        img2.hash, first_hash,
        "the re-seeded noise flowed through the blur"
    );

    // A geo network does not publish an image.
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let _box_node = add(&mut e, sub, "box");
    e.cook(&mut || true);
    assert!(e.display_image(geo).is_none());
}

/// The 0.8.0 procedural chain cooks end to end and recooks on reseed:
/// box -> scatter -> copy_to_points with a plane template. This is the
/// milestone's keep-last-good proof on a real graph: a reseeded scatter is
/// a changed Points payload flowing through a downstream copy, and the
/// recook must reach the output rather than being swallowed.
#[test]
fn scatter_copy_chain_recooks_on_reseed() {
    let (mut e, ctx) = subflow_engine();
    let surface = add(&mut e, ctx, "box");
    let scatter = add(&mut e, ctx, "scatter");
    let template = add(&mut e, ctx, "plane");
    let copy = add(&mut e, ctx, "copy_to_points");

    for (from, to, to_port) in [
        (surface, scatter, "geometry"),
        (scatter, copy, "points"),
        (template, copy, "template"),
    ] {
        e.apply(Command::Connect {
            ctx,
            from: PortRefDto {
                node: from,
                port: "geometry".into(),
            },
            to: PortRefDto {
                node: to,
                port: to_port.into(),
            },
        })
        .unwrap();
    }
    e.apply(Command::SetParam {
        ctx,
        node: scatter,
        key: "count".into(),
        value: ParamSource::Literal(ParamValue::Int(25)),
    })
    .unwrap();
    // Pinned to Bake so the assertion below reads the tiled positions
    // directly. The subject here is recook propagation through a downstream
    // copy, not which representation the copy chose.
    e.apply(Command::SetParam {
        ctx,
        node: copy,
        key: "copy_mode".into(),
        value: ParamSource::Literal(ParamValue::Enum("bake".into())),
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(copy),
    })
    .unwrap();
    e.cook(&mut || true);

    let positions_of = |e: &Engine| {
        let set = e
            .cook
            .outputs(copy)
            .unwrap()
            .get("geometry")
            .and_then(crate::registry::coerce::Value::as_geometry)
            .unwrap();
        assert_eq!(
            set.meshes[0].primitive_count(),
            2 * 25,
            "the plane template tiled onto every scattered point"
        );
        std::sync::Arc::clone(&set.meshes[0].positions)
    };
    let before = positions_of(&e);

    e.apply(Command::SetParam {
        ctx,
        node: scatter,
        key: "seed".into(),
        value: ParamSource::Literal(ParamValue::Int(11)),
    })
    .unwrap();
    e.cook(&mut || true);
    let after = positions_of(&e);
    assert_ne!(
        before, after,
        "the reseeded cloud flowed through copy_to_points to the output"
    );
}

/// The full material pipeline: a texture network feeds a
/// material network through `tex_ref`, whose `principled` output a
/// geo-side `material` node consumes in Reference mode; editing the
/// TEXTURE recooks the whole chain across three contexts in one pass, and
/// per-slot targeting leaves unmatched meshes on their old material.
#[test]
fn material_network_references_flow_across_three_contexts() {
    let mut e = engine();

    // /tex: a texnet publishing a constant image.
    let texnet = add(&mut e, GraphContext::Root, "texnet");
    let tex_ctx = GraphContext::Subflow(texnet);
    let constant = add(&mut e, tex_ctx, "constant");

    // /mat: a matnet whose principled surface pulls the texture by path.
    let matnet = add(&mut e, GraphContext::Root, "matnet");
    let mat_ctx = GraphContext::Subflow(matnet);
    let tex_ref = add(&mut e, mat_ctx, "tex_ref");
    let principled = add(&mut e, mat_ctx, "principled");
    e.apply(Command::SetParam {
        ctx: mat_ctx,
        node: tex_ref,
        key: "texture_path".to_string(),
        value: ParamSource::Literal(ParamValue::NodeRef(Some(texnet))),
    })
    .unwrap();
    e.apply(Command::Connect {
        ctx: mat_ctx,
        from: PortRefDto {
            node: tex_ref,
            port: "image".into(),
        },
        to: PortRefDto {
            node: principled,
            port: "base_color_map".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: mat_ctx,
        node: Some(principled),
    })
    .unwrap();

    // /geo: a box whose material node references the matnet.
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box_node = add(&mut e, sub, "box");
    let material = add(&mut e, sub, "material");
    e.apply(Command::Connect {
        ctx: sub,
        from: PortRefDto {
            node: box_node,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: material,
            port: "geometry".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: sub,
        node: Some(material),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx: sub,
        node: material,
        key: "mode".to_string(),
        value: ParamSource::Literal(ParamValue::Enum("reference".to_string())),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx: sub,
        node: material,
        key: "material_path".to_string(),
        value: ParamSource::Literal(ParamValue::NodeRef(Some(matnet))),
    })
    .unwrap();

    e.cook(&mut || true);

    // The geo's displayed geometry carries the referenced material with
    // the texture from /tex riding the base-color role.
    let displayed = |e: &Engine| {
        e.cook
            .outputs(material)
            .and_then(|o| o.get("geometry").and_then(|v| v.as_geometry().cloned()))
            .expect("material node committed")
    };
    let set = displayed(&e);
    assert_eq!(set.materials.len(), 1);
    let tex_hash = set.materials[0]
        .diffuse_texture_data
        .as_ref()
        .expect("texture flowed from /tex through /mat into /geo")
        .hash;

    // Edit the TEXTURE (three contexts away from the geometry): the whole
    // chain recooks in one pass and the material's texture changes.
    e.apply(Command::SetParam {
        ctx: tex_ctx,
        node: constant,
        key: "color".to_string(),
        value: ParamSource::Literal(ParamValue::Color([1.0, 0.0, 0.0, 1.0])),
    })
    .unwrap();
    e.cook(&mut || true);
    let set = displayed(&e);
    assert_ne!(
        set.materials[0].diffuse_texture_data.as_ref().unwrap().hash,
        tex_hash,
        "a /tex edit must repaint the referencing geometry in one pass"
    );

    // Per-slot targeting: a named target leaves unmatched meshes on the
    // OLD material (appended table, not override-all).
    e.apply(Command::SetParam {
        ctx: sub,
        node: material,
        key: "target".to_string(),
        value: ParamSource::Literal(ParamValue::Text("no_such_mesh".to_string())),
    })
    .unwrap();
    e.cook(&mut || true);
    let set = displayed(&e);
    assert!(
        set.meshes
            .iter()
            .all(|m| m.material_index != Some(set.materials.len() - 1)),
        "an unmatched target assigns nothing"
    );
}

/// The export nodes: a geo_export taps the chain, its Save
/// action encodes the committed geometry, and the bytes reimport through
/// this workspace's own loaders (the round-trip acceptance criterion).
#[test]
fn geo_export_action_round_trips_through_the_loaders() {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box_node = add(&mut e, sub, "box");
    let export = add(&mut e, sub, "geo_export");
    e.apply(Command::Connect {
        ctx: sub,
        from: PortRefDto {
            node: box_node,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: export,
            port: "geometry".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: sub,
        node: Some(export),
    })
    .unwrap();
    e.cook(&mut || true);

    for (format, check) in [("obj", 24usize), ("stl", 0), ("ply", 24), ("glb", 24)] {
        e.apply(Command::SetParam {
            ctx: sub,
            node: export,
            key: "format".to_string(),
            value: ParamSource::Literal(ParamValue::Enum(format.to_string())),
        })
        .unwrap();
        let result = e.invoke_action(sub, export, "save").expect(format);
        assert!(result.filename.ends_with(format));
        assert!(!result.bytes.is_empty());
        // Reimport through our own loaders.
        let model = match format {
            "obj" => {
                solarxy_formats::obj::load_obj_bytes(&result.bytes, &mut solarxy_formats::NoAssets)
                    .expect("obj reimport")
            }
            "stl" => {
                solarxy_formats::stl::load_stl_bytes(&result.bytes, "x.stl").expect("stl reimport")
            }
            "ply" => {
                solarxy_formats::ply::load_ply_bytes(&result.bytes, "x.ply").expect("ply reimport")
            }
            _ => solarxy_formats::gltf::load_gltf_bytes(
                &result.bytes,
                &mut solarxy_formats::NoAssets,
            )
            .expect("glb reimport"),
        };
        let tris: usize = model.meshes.iter().map(|m| m.indices.len() / 3).sum();
        assert_eq!(tris, 12, "a box is 12 triangles in every format");
        if check > 0 {
            let verts: usize = model.meshes.iter().map(|m| m.positions.len()).sum();
            assert_eq!(verts, check, "{format} vertex count");
        }
    }

    // An action on a node that has none is a clean error, not a panic.
    assert!(e.invoke_action(sub, box_node, "save").is_err());
}

/// An OBJ export of a set that carries materials is the multi-file form:
/// a Stored zip of `.obj` + `.mtl`. GLB stays a single
/// file, and a material-less OBJ stays the classic single `.obj`.
#[test]
fn geo_export_obj_with_materials_delivers_a_zip() {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let sub = GraphContext::Subflow(geo);
    let box_node = add(&mut e, sub, "box");
    let material = add(&mut e, sub, "material");
    let export = add(&mut e, sub, "geo_export");
    for (from, to) in [(box_node, material), (material, export)] {
        e.apply(Command::Connect {
            ctx: sub,
            from: PortRefDto {
                node: from,
                port: "geometry".into(),
            },
            to: PortRefDto {
                node: to,
                port: "geometry".into(),
            },
        })
        .unwrap();
    }
    e.apply(Command::SetParam {
        ctx: sub,
        node: material,
        key: "mode".to_string(),
        value: ParamSource::Literal(ParamValue::Enum("inline".to_string())),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx: sub,
        node: export,
        key: "format".to_string(),
        value: ParamSource::Literal(ParamValue::Enum("obj".to_string())),
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx: sub,
        node: Some(export),
    })
    .unwrap();
    e.cook(&mut || true);

    let result = e.invoke_action(sub, export, "save").expect("obj zip");
    assert_eq!(result.mime, "application/zip");
    assert!(result.filename.ends_with("_obj.zip"), "{}", result.filename);
    assert!(result.bytes.starts_with(b"PK"), "a real zip container");
    let haystack = result.bytes.as_slice();
    for needle in [b"export.obj".as_slice(), b"export.mtl".as_slice()] {
        assert!(
            haystack.windows(needle.len()).any(|w| w == needle),
            "the archive names its {} entry",
            String::from_utf8_lossy(needle)
        );
    }
}

#[test]
fn attribute_summary_and_page_read_a_cooked_chain() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let rand = add(&mut e, ctx, "attribute_randomize");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: box_id,
            port: "geometry".to_string(),
        },
        to: PortRefDto {
            node: rand,
            port: "geometry".to_string(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(rand),
    })
    .unwrap();
    e.cook(&mut || true);

    // The randomize node's defaults write the vec4 `color` lane; the
    // box's fixed normals and UVs surface as the N/uv pseudo-lanes.
    let summary = e.attribute_summary(rand).expect("cooked geometry");
    assert_eq!(summary.points, 24);
    assert_eq!(
        summary
            .point
            .iter()
            .map(|l| (l.name.as_str(), l.ty, l.len))
            .collect::<Vec<_>>(),
        vec![("N", "vec3", 24), ("color", "vec4", 24), ("uv", "vec2", 24)],
    );
    assert!(summary.primitive.is_empty());

    // A window of the point table: P leads, the lane follows, and the
    // serde form is camelCase like every boundary DTO.
    let page = e
        .attribute_page(rand, solarxy_kernel::AttributeDomain::Point, 4, 2)
        .expect("cooked geometry");
    assert_eq!(page.total, 24);
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.rows[0].len(), 3 + 3 + 4 + 2);
    let json = serde_json::to_string(&page).unwrap();
    assert!(json.contains("\"total\":24"));
    assert!(json.contains("\"columns\""));
    assert!(json.contains("\"components\""));

    // No committed output: a fresh unconnected node id yields None.
    assert!(e.attribute_summary(NodeId(u64::MAX)).is_none());
}

#[test]
fn cook_warnings_read_back_after_a_cook_and_clear_when_fixed() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let rand = add(&mut e, ctx, "attribute_randomize");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: box_id,
            port: "geometry".to_string(),
        },
        to: PortRefDto {
            node: rand,
            port: "geometry".to_string(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(rand),
    })
    .unwrap();
    // Writing `uv` as vec4 warns twice: the reserved contract wants vec2,
    // and the box's fixed uv buffer is being replaced with a new type.
    e.apply(Command::SetParam {
        ctx,
        node: rand,
        key: "attr_name".into(),
        value: ParamSource::Literal(ParamValue::Text("uv".into())),
    })
    .unwrap();
    e.cook(&mut || true);
    let warnings = e.cook_warnings(rand);
    assert_eq!(warnings.len(), 2, "{warnings:?}");

    // A quiet recook clears the set: the vec4 default under a fresh name
    // matches nothing on the input and no reserved contract.
    e.apply(Command::SetParam {
        ctx,
        node: rand,
        key: "attr_name".into(),
        value: ParamSource::Literal(ParamValue::Text("tint".into())),
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(e.cook_warnings(rand).is_empty());
    assert!(e.cook_warnings(box_id).is_empty(), "quiet nodes stay empty");
}

// Node naming: expressions resolve by name, so a name has to be
// stored, graph-unique, and stable across paste, rename and reset.

/// The name a node answers to, through the same rule the resolver uses.
fn name_of(e: &Engine, ctx: GraphContext, id: NodeId) -> String {
    let node = e.document().graph(ctx).unwrap().node(id).unwrap();
    crate::naming::node_name(node, e.registry())
}

#[test]
fn every_added_node_stores_a_unique_auto_numbered_name() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let c = add(&mut e, ctx, "sphere");
    assert_eq!(name_of(&e, ctx, a), "box1");
    assert_eq!(name_of(&e, ctx, b), "box2");
    // Numbering is per type, not a global counter.
    assert_eq!(name_of(&e, ctx, c), "sphere1");
    // Stored, not merely derived: a per-instance name cannot be a
    // descriptor default, and the .slxy round trip carries the stored one.
    let node = e.document().graph(ctx).unwrap().node(a).unwrap();
    assert!(matches!(
        node.params.get("name"),
        Some(ParamSource::Literal(ParamValue::Text(t))) if t == "box1"
    ));
}

#[test]
fn the_added_name_rides_the_node_added_mirror() {
    let (mut e, ctx) = subflow_engine();
    let batch = e
        .apply(Command::AddNode {
            ctx,
            node_type: "box".into(),
            position: [0.0, 0.0],
        })
        .unwrap();
    // The frontend renders from the mirror, so a name minted after the
    // mirror was taken would show the display name until the next snapshot.
    let mirrored = batch.events.iter().find_map(|ev| match ev {
        EngineEvent::NodeAdded { node, .. } => node.params.get("name").cloned(),
        _ => None,
    });
    assert!(matches!(
        mirrored,
        Some(ParamSource::Literal(ParamValue::Text(t))) if t == "box1"
    ));
}

#[test]
fn a_rename_to_a_taken_name_is_suffixed_not_refused() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "name".into(),
        value: ParamSource::Literal(ParamValue::Text("body".into())),
    })
    .unwrap();
    let batch = e
        .apply(Command::SetParam {
            ctx,
            node: b,
            key: "name".into(),
            value: ParamSource::Literal(ParamValue::Text("body".into())),
        })
        .unwrap();
    assert_eq!(name_of(&e, ctx, a), "body");
    assert_eq!(name_of(&e, ctx, b), "body2");
    // The event must carry what was STORED, not what was requested, or the
    // mirror and the document disagree about a name expressions resolve by.
    let announced = batch.events.iter().find_map(|ev| match ev {
        EngineEvent::ParamChanged { key, value, .. } if key == "name" => Some(value.clone()),
        _ => None,
    });
    assert!(matches!(
        announced,
        Some(ParamSource::Literal(ParamValue::Text(t))) if t == "body2"
    ));
}

#[test]
fn a_node_may_be_renamed_to_the_name_it_already_has() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "name".into(),
        value: ParamSource::Literal(ParamValue::Text("box1".into())),
    })
    .unwrap();
    assert_eq!(name_of(&e, ctx, a), "box1", "no self-collision");
}

#[test]
fn pasting_into_the_source_graph_renames_the_copies() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "name".into(),
        value: ParamSource::Literal(ParamValue::Text("body".into())),
    })
    .unwrap();
    let fragment = e.copy_nodes(ctx, &[a]);
    e.apply(Command::PasteNodes {
        ctx,
        fragment: fragment.clone(),
        position: [10.0, 10.0],
    })
    .unwrap();
    e.apply(Command::PasteNodes {
        ctx,
        fragment,
        position: [20.0, 20.0],
    })
    .unwrap();
    let names: Vec<String> = e
        .document()
        .graph(ctx)
        .unwrap()
        .nodes()
        .map(|n| crate::naming::node_name(n, e.registry()))
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "names collided: {names:?}");
    assert!(names.contains(&"body".to_string()));
    assert!(names.contains(&"body2".to_string()));
    assert!(names.contains(&"body3".to_string()));
}

#[test]
fn duplicating_renames_the_copy() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    e.apply(Command::DuplicateNodes { ctx, ids: vec![a] })
        .unwrap();
    let names: Vec<String> = e
        .document()
        .graph(ctx)
        .unwrap()
        .nodes()
        .map(|n| crate::naming::node_name(n, e.registry()))
        .collect();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1], "a duplicate must not share the name");
}

#[test]
fn undoing_a_rename_restores_the_previous_name() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "name".into(),
        value: ParamSource::Literal(ParamValue::Text("body".into())),
    })
    .unwrap();
    assert_eq!(name_of(&e, ctx, a), "body");
    e.apply(Command::Undo).unwrap();
    assert_eq!(name_of(&e, ctx, a), "box1");
    e.apply(Command::Redo).unwrap();
    assert_eq!(name_of(&e, ctx, a), "body");
}

#[test]
fn two_graphs_may_each_hold_the_same_name() {
    let mut e = engine();
    let geo_a = e.doc.mint_node_id();
    e.doc.create_subflow(geo_a, ContextKind::Geo);
    let geo_b = e.doc.mint_node_id();
    e.doc.create_subflow(geo_b, ContextKind::Geo);
    let a = add(&mut e, GraphContext::Subflow(geo_a), "box");
    let b = add(&mut e, GraphContext::Subflow(geo_b), "box");
    // Uniqueness is per network, exactly as two directories may each hold
    // a `readme`; paths resolve relative to a context.
    assert_eq!(name_of(&e, GraphContext::Subflow(geo_a), a), "box1");
    assert_eq!(name_of(&e, GraphContext::Subflow(geo_b), b), "box1");
}

#[test]
fn resetting_the_name_re_mints_rather_than_collapsing_to_the_display_name() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "name".into(),
        value: ParamSource::Literal(ParamValue::Text("body".into())),
    })
    .unwrap();
    e.apply(Command::ResetParams {
        ctx,
        node: a,
        keys: Some(vec!["name".into()]),
    })
    .unwrap();
    // Not "Box": that is shared by every box, which is the state minting
    // exists to escape.
    assert_eq!(name_of(&e, ctx, a), "box1");
}

// Expressions: the seam is only real if a stored expression
// actually changes what a cook produces.

#[test]
fn an_expression_drives_a_cooked_parameter_end_to_end() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Expression {
            expr: "2 * 3".into(),
        },
    })
    .unwrap();
    e.cook(&mut || true);
    let set = e.geometry_output(box_id).expect("box cooked");
    let width = set.bounds.max.x - set.bounds.min.x;
    assert!((width - 6.0).abs() < 1e-5, "width was {width}");
}

#[test]
fn a_bad_expression_badges_the_node_instead_of_cooking() {
    // A broken expression is a value the user can fix by
    // editing, so it is a COOK error (the node badges), not a command
    // error that would refuse the keystroke.
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Expression { expr: "1 +".into() },
    })
    .unwrap();
    let events = e.cook(&mut || true);
    let errored = events.iter().any(|ev| {
        matches!(ev, EngineEvent::CookStatus { node, status }
            if *node == box_id && matches!(status, crate::cook::state::CookStatus::Error { .. }))
    });
    assert!(errored, "a parse error must badge the node: {events:?}");
}

#[test]
fn an_expression_error_names_the_param_it_is_on() {
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Expression {
            expr: "wobble(1)".into(),
        },
    })
    .unwrap();
    let events = e.cook(&mut || true);
    let message = events.iter().find_map(|ev| match ev {
        EngineEvent::CookStatus {
            node,
            status: crate::cook::state::CookStatus::Error { message },
        } if *node == box_id => Some(message.clone()),
        _ => None,
    });
    let message = message.expect("an error status");
    assert!(message.contains("width"), "{message}");
    assert!(message.contains("unknown function"), "{message}");
}

#[test]
fn a_geometry_query_reads_the_nodes_own_gathered_input() {
    // npoints() reads cook OUTPUT, unlike ch(); it works because gather
    // runs before resolve, and the wire topology already cooked upstream.
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    let xform = add(&mut e, ctx, "transform");
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(xform),
    })
    .unwrap();
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
    e.apply(Command::SetParam {
        ctx,
        node: xform,
        key: "translate".into(),
        value: ParamSource::Expression {
            expr: "set(npoints(), 0, 0)".into(),
        },
    })
    .unwrap();
    e.cook(&mut || true);
    let set = e.geometry_output(xform).expect("transform cooked");
    // A default box is 24 points, so the whole thing shifts to x = 24.
    let cx = f64::midpoint(f64::from(set.bounds.min.x), f64::from(set.bounds.max.x));
    assert!((cx - 24.0).abs() < 1e-4, "centre x was {cx}");

    // The parameter panel must agree with the cook. It resolves off the
    // same cached inputs, so a geometry query that cooks is a geometry
    // query the readout can show; reporting it as unavailable here while
    // the node badges green is the disagreement this asserts against.
    let shown = e
        .resolved_param(ctx, xform, "translate")
        .expect("the readout resolves what the cook resolved");
    assert_eq!(shown, ParamValue::Vec3([24.0, 0.0, 0.0]), "{shown:?}");
}

#[test]
fn the_readout_and_the_cook_agree_when_a_geometry_query_cannot_answer() {
    // The other half of the contract: with nothing connected, BOTH refuse.
    // A silent 0 in either one is how a box ends up cooking at the hard
    // clamp floor with nothing on screen saying why.
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Expression {
            expr: "npoints()".into(),
        },
    })
    .unwrap();
    assert!(e.resolved_param(ctx, box_id, "width").is_err());
    let events = e.cook(&mut || true);
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            EngineEvent::CookStatus {
                node,
                status: crate::cook::state::CookStatus::Error { .. }
            } if *node == box_id
        )),
        "the cook must refuse it too"
    );
}

#[test]
fn a_geometry_query_on_a_node_without_inputs_says_so_rather_than_reading_zero() {
    // A box has no geometry input. Answering 0 would be a plausible wrong
    // number; naming the problem is the only useful answer.
    //
    // Every query, not just `bbox`. An earlier version of this test checked
    // `bbox` alone while the counting queries answered 0, so `npoints()`
    // clamped to the hard floor and cooked green while the parameter
    // panel showed red for the same expression.
    for expr in [
        "bbox(\"size\").x",
        "npoints()",
        "nprims()",
        "nmeshes()",
        "centroid().x",
    ] {
        let (mut e, ctx) = subflow_engine();
        let box_id = add(&mut e, ctx, "box");
        e.apply(Command::SetParam {
            ctx,
            node: box_id,
            key: "width".into(),
            value: ParamSource::Expression {
                expr: expr.to_string(),
            },
        })
        .unwrap();
        let events = e.cook(&mut || true);
        let message = events.iter().find_map(|ev| match ev {
            EngineEvent::CookStatus {
                node,
                status: crate::cook::state::CookStatus::Error { message },
            } if *node == box_id => Some(message.clone()),
            _ => None,
        });
        assert!(
            message.is_some_and(|m| m.contains("not connected")),
            "`{expr}` should have produced a 'not connected' cook error"
        );
    }
}

#[test]
fn time_is_stopped_so_a_cook_is_reproducible() {
    // Until the runtime lands, $T and $F are zero everywhere. Golden
    // captures and CLI cooks depend on it.
    let (mut e, ctx) = subflow_engine();
    let box_id = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: box_id,
        key: "width".into(),
        value: ParamSource::Expression {
            expr: "2 + $T * 100 + $F * 100".into(),
        },
    })
    .unwrap();
    e.cook(&mut || true);
    let set = e.geometry_output(box_id).expect("box cooked");
    let width = set.bounds.max.x - set.bounds.min.x;
    assert!((width - 2.0).abs() < 1e-5, "width was {width}");
}

#[test]
fn an_expression_driven_transform_keeps_its_gizmo() {
    // `node_transform` (the gizmo read path) is one of the four resolve
    // sites that fail SILENTLY to a fallback. Before the resolver took an
    // evaluation context, any expression on
    // a transform param made it return None and the gizmo just vanished;
    // it now resolves like any other value.
    let mut e = engine();
    let ctx = GraphContext::Root;
    let geo = add(&mut e, ctx, "geo");
    e.apply(Command::SetParam {
        ctx,
        node: geo,
        key: "translate".into(),
        value: ParamSource::Expression {
            expr: "set(5, 0, 0)".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetSelection {
        ctx,
        ids: vec![geo],
    })
    .unwrap();
    let target = e
        .gizmo_target(ctx)
        .expect("the gizmo must survive an expression");
    assert!(
        (target.translate[0] - 5.0).abs() < 1e-6,
        "the gizmo must read the EVALUATED value, not the default: {:?}",
        target.translate
    );
}

#[test]
fn an_expression_drives_the_geo_container_world_matrix() {
    // `geo_world_matrix` is another of the silent-fallback sites: it
    // returned the identity on any expression, so an expression-driven
    // object would render at the origin while the node badged elsewhere.
    let mut e = engine();
    let ctx = GraphContext::Root;
    let geo = add(&mut e, ctx, "geo");
    e.apply(Command::SetParam {
        ctx,
        node: geo,
        key: "translate".into(),
        value: ParamSource::Expression {
            expr: "set(0, 3, 0)".into(),
        },
    })
    .unwrap();
    let m = e.geo_world_matrix(geo).expect("a geo has a world matrix");
    // Column-major: the translation is the fourth column.
    assert!((m[3][1] - 3.0).abs() < 1e-6, "matrix was {m:?}");
}

// ch() cross-node references.

/// Renames a node, returning the name actually stored.
fn rename(e: &mut Engine, ctx: GraphContext, node: NodeId, to: &str) -> String {
    e.apply(Command::SetParam {
        ctx,
        node,
        key: "name".into(),
        value: ParamSource::Literal(ParamValue::Text(to.into())),
    })
    .unwrap();
    name_of(e, ctx, node)
}

fn set_expr(e: &mut Engine, ctx: GraphContext, node: NodeId, key: &str, expr: &str) {
    e.apply(Command::SetParam {
        ctx,
        node,
        key: key.into(),
        value: ParamSource::Expression { expr: expr.into() },
    })
    .unwrap();
}

/// Makes `node` the subflow's display node. Only the display node's
/// predecessor cone cooks, so a test asserting on an unconnected node has
/// to claim the flag first.
fn set_display(e: &mut Engine, ctx: GraphContext, node: NodeId) {
    e.apply(Command::SetActiveOutput {
        ctx,
        node: Some(node),
    })
    .unwrap();
}

fn cooked_width(e: &Engine, node: NodeId) -> f32 {
    let set = e.geometry_output(node).expect("cooked");
    set.bounds.max.x - set.bounds.min.x
}

#[test]
fn ch_reads_a_sibling_in_the_same_network() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    rename(&mut e, ctx, a, "source");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(4.0)),
    })
    .unwrap();
    set_expr(&mut e, ctx, b, "width", "ch(\"source/width\") * 2");
    set_display(&mut e, ctx, b);
    e.cook(&mut || true);
    assert!((cooked_width(&e, b) - 8.0).abs() < 1e-4);
}

#[test]
fn ch_reads_a_param_on_its_own_node_with_a_single_segment() {
    // One segment is always a param on self, never a node name: that is
    // what makes `ch("height")` unambiguous.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "height".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    set_expr(&mut e, ctx, a, "width", "ch(\"height\") * 2");
    e.cook(&mut || true);
    assert!((cooked_width(&e, a) - 6.0).abs() < 1e-4);
}

#[test]
fn ch_climbs_to_the_container_and_to_its_siblings() {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let ctx = GraphContext::Subflow(geo);
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: geo,
        key: "uniform_scale".into(),
        value: ParamSource::Literal(ParamValue::Float(5.0)),
    })
    .unwrap();
    let b = add(&mut e, ctx, "box");
    // `../param` is the container's own param.
    set_expr(&mut e, ctx, b, "width", "ch(\"../uniform_scale\")");
    e.cook(&mut || true);
    assert!((cooked_width(&e, b) - 5.0).abs() < 1e-4);
}

#[test]
fn ch_resolves_an_absolute_path_from_anywhere() {
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let name = name_of(&e, GraphContext::Root, geo);
    let ctx = GraphContext::Subflow(geo);
    let b = add(&mut e, ctx, "box");
    let b_name = name_of(&e, ctx, b);
    let c = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: b,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(7.0)),
    })
    .unwrap();
    set_expr(
        &mut e,
        ctx,
        c,
        "width",
        &format!("ch(\"/{name}/{b_name}/width\")"),
    );
    set_display(&mut e, ctx, c);
    e.cook(&mut || true);
    assert!((cooked_width(&e, c) - 7.0).abs() < 1e-4);
}

#[test]
fn a_reference_reads_the_authoring_space_not_radians() {
    // The 57x trap. A Degrees param stores degrees and resolves to
    // radians; ch() must hand back degrees so copying a rotation into
    // another rotation round-trips instead of converting twice.
    let mut e = engine();
    let ctx = GraphContext::Root;
    let a = add(&mut e, ctx, "geo");
    let b = add(&mut e, ctx, "geo");
    let a_name = rename(&mut e, ctx, a, "source");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "rotate".into(),
        value: ParamSource::Literal(ParamValue::Vec3([90.0, 0.0, 0.0])),
    })
    .unwrap();
    set_expr(
        &mut e,
        ctx,
        b,
        "rotate",
        &format!("ch(\"{a_name}/rotate\")"),
    );
    // Both containers must end up at the same world rotation.
    let ma = e.geo_world_matrix(a).expect("a");
    let mb = e.geo_world_matrix(b).expect("b");
    for (ca, cb) in ma.iter().zip(mb.iter()) {
        for (va, vb) in ca.iter().zip(cb.iter()) {
            assert!((va - vb).abs() < 1e-5, "{ma:?} vs {mb:?}");
        }
    }
}

#[test]
fn a_reference_sees_the_hard_clamp_the_target_itself_obeys() {
    // A reader must never observe a value the target's own cook does not
    // use, or the same param means two different things.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let a_name = rename(&mut e, ctx, a, "source");
    // width's hard range tops out well below this.
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(1.0e9)),
    })
    .unwrap();
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{a_name}/width\")"));
    set_display(&mut e, ctx, a);
    e.cook(&mut || true);
    let clamped = cooked_width(&e, a);
    set_display(&mut e, ctx, b);
    e.cook(&mut || true);
    assert!(
        (cooked_width(&e, b) - clamped).abs() < 1e-3,
        "reader saw {} but the target uses {clamped}",
        cooked_width(&e, b)
    );
}

#[test]
fn a_reference_chain_evaluates_without_any_cook_ordering() {
    // b reads a, c reads b. Cook order is irrelevant because ch() reads
    // document state and recurses on demand.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let c = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "aa");
    let bn = rename(&mut e, ctx, b, "bb");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(2.0)),
    })
    .unwrap();
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{an}/width\") * 2"));
    set_expr(&mut e, ctx, c, "width", &format!("ch(\"{bn}/width\") + 1"));
    set_display(&mut e, ctx, c);
    e.cook(&mut || true);
    assert!((cooked_width(&e, c) - 5.0).abs() < 1e-4, "2*2+1");
}

#[test]
fn a_reference_follows_a_rename_of_its_target() {
    // The promise `NodeRef` already makes by storing an id, kept for a
    // reference form that is inherently by name.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "source");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{an}/width\") * 2"));
    set_display(&mut e, ctx, b);
    e.cook(&mut || true);
    assert!((cooked_width(&e, b) - 6.0).abs() < 1e-4);

    rename(&mut e, ctx, a, "renamed");
    e.cook(&mut || true);
    assert!(
        (cooked_width(&e, b) - 6.0).abs() < 1e-4,
        "the expression must follow the rename, not break: got {}",
        cooked_width(&e, b)
    );
    // And the stored text names the new node, not the old one.
    let stored = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(b)
        .unwrap()
        .params
        .get("width")
        .cloned();
    let ParamSource::Expression { expr } = stored.unwrap() else {
        panic!("still an expression");
    };
    assert!(expr.contains("renamed/width"), "{expr}");
}

#[test]
fn a_rename_and_its_rewrites_are_one_undo_step() {
    // Two commands would let a user undo the rename and strand the
    // rewritten paths pointing at a name that no longer exists.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "source");
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{an}/width\")"));
    rename(&mut e, ctx, a, "renamed");

    e.apply(Command::Undo).unwrap();
    assert_eq!(name_of(&e, ctx, a), "source", "the name came back");
    let stored = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(b)
        .unwrap()
        .params
        .get("width")
        .cloned();
    let ParamSource::Expression { expr } = stored.unwrap() else {
        panic!("still an expression");
    };
    assert!(
        expr.contains("source/width"),
        "one undo must restore BOTH the name and the paths: {expr}"
    );
}

#[test]
fn a_rename_rewrites_only_node_segments_not_identically_named_params() {
    // `ch("width")` is a param on this node. Renaming a node to `width`
    // must not rewrite it, which is why the rewrite is positional rather
    // than a text substitution.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    set_expr(&mut e, ctx, b, "width", "ch(\"height\") * 2");
    rename(&mut e, ctx, a, "height");
    let stored = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(b)
        .unwrap()
        .params
        .get("width")
        .cloned();
    let ParamSource::Expression { expr } = stored.unwrap() else {
        panic!("still an expression");
    };
    assert_eq!(
        expr, "ch(\"height\") * 2",
        "a param segment must survive a same-named node rename"
    );
}

#[test]
fn a_rename_preserves_the_rest_of_the_expression_byte_for_byte() {
    // The rewrite edits user text, so it must touch only the quoted path.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "src");
    let original = format!("  ch(\"{an}/width\")*2 + bbox(\"xmin\") // note\n");
    set_expr(&mut e, ctx, b, "width", &original);
    rename(&mut e, ctx, a, "dest");
    let stored = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(b)
        .unwrap()
        .params
        .get("width")
        .cloned();
    let ParamSource::Expression { expr } = stored.unwrap() else {
        panic!("still an expression");
    };
    assert_eq!(expr, original.replace("src/width", "dest/width"), "{expr}");
}

#[test]
fn a_rename_rewrites_across_contexts() {
    // An absolute path from inside one network naming a node in another.
    let mut e = engine();
    let geo = add(&mut e, GraphContext::Root, "geo");
    let geo_name = rename(&mut e, GraphContext::Root, geo, "obj");
    let ctx = GraphContext::Subflow(geo);
    let inner = add(&mut e, ctx, "box");
    let inner_name = name_of(&e, ctx, inner);
    let other = add(&mut e, GraphContext::Root, "geo");
    set_expr(
        &mut e,
        GraphContext::Root,
        other,
        "uniform_scale",
        &format!("ch(\"/{geo_name}/{inner_name}/width\")"),
    );
    // Renaming the CONTAINER must rewrite the first segment.
    rename(&mut e, GraphContext::Root, geo, "renamed");
    let stored = e
        .document()
        .graph(GraphContext::Root)
        .unwrap()
        .node(other)
        .unwrap()
        .params
        .get("uniform_scale")
        .cloned();
    let ParamSource::Expression { expr } = stored.unwrap() else {
        panic!("still an expression");
    };
    assert!(
        expr.contains(&format!("/renamed/{inner_name}/width")),
        "{expr}"
    );
}

#[test]
fn a_rename_that_auto_suffixes_rewrites_to_the_stored_name() {
    // uniquify may not give the requested name. Rewriting to the REQUESTED
    // one would point every reference at a node that does not exist.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let taken = rename(&mut e, ctx, b, "taken");
    let an = rename(&mut e, ctx, a, "source");
    let c = add(&mut e, ctx, "box");
    set_expr(&mut e, ctx, c, "width", &format!("ch(\"{an}/width\")"));

    let stored_name = rename(&mut e, ctx, a, &taken);
    assert_ne!(stored_name, taken, "the name collided and was suffixed");
    let stored = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(c)
        .unwrap()
        .params
        .get("width")
        .cloned();
    let ParamSource::Expression { expr } = stored.unwrap() else {
        panic!("still an expression");
    };
    assert!(
        expr.contains(&format!("{stored_name}/width")),
        "must follow the STORED name `{stored_name}`: {expr}"
    );
}

#[test]
fn a_dangling_reference_badges_with_a_message_naming_the_path() {
    let (mut e, ctx) = subflow_engine();
    let b = add(&mut e, ctx, "box");
    set_expr(&mut e, ctx, b, "width", "ch(\"nope/width\")");
    set_display(&mut e, ctx, b);
    let events = e.cook(&mut || true);
    let message = events.iter().find_map(|ev| match ev {
        EngineEvent::CookStatus {
            node,
            status: crate::cook::state::CookStatus::Error { message },
        } if *node == b => Some(message.clone()),
        _ => None,
    });
    let message = message.expect("an error status");
    assert!(message.contains("no node named `nope`"), "{message}");
}

#[test]
fn an_unknown_param_on_a_real_node_names_both() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "source");
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{an}/nosuch\")"));
    set_display(&mut e, ctx, b);
    let events = e.cook(&mut || true);
    let message = events.iter().find_map(|ev| match ev {
        EngineEvent::CookStatus {
            node,
            status: crate::cook::state::CookStatus::Error { message },
        } if *node == b => Some(message.clone()),
        _ => None,
    });
    let message = message.expect("an error status");
    assert!(message.contains("nosuch"), "{message}");
    assert!(message.contains("source"), "{message}");
}

#[test]
fn a_reference_cycle_is_refused_at_set_time() {
    // The cook never has to detect a loop, because one can never be
    // written. Refusing at SetParam is what keeps the document always
    // evaluable.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "aa");
    let bn = rename(&mut e, ctx, b, "bb");
    set_expr(&mut e, ctx, a, "width", &format!("ch(\"{bn}/width\")"));
    let err = e
        .apply(Command::SetParam {
            ctx,
            node: b,
            key: "width".into(),
            value: ParamSource::Expression {
                expr: format!("ch(\"{an}/width\")"),
            },
        })
        .unwrap_err();
    assert!(
        matches!(err, EngineError::ExpressionCycle { .. }),
        "expected a cycle refusal, got {err:?}"
    );
}

#[test]
fn a_param_referencing_itself_is_refused() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let err = e
        .apply(Command::SetParam {
            ctx,
            node: a,
            key: "width".into(),
            value: ParamSource::Expression {
                expr: "ch(\"width\") + 1".into(),
            },
        })
        .unwrap_err();
    assert!(
        matches!(err, EngineError::ExpressionCycle { .. }),
        "{err:?}"
    );
}

#[test]
fn one_param_referencing_another_on_the_same_node_is_legal() {
    // The cycle check is over (node, key) pairs, not nodes. A node-level
    // check would refuse this, and it is real, useful work.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "height".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Expression {
            expr: "ch(\"height\") * 2".into(),
        },
    })
    .expect("cross-param self reference must be allowed");
    e.cook(&mut || true);
    assert!((cooked_width(&e, a) - 6.0).abs() < 1e-4);
}

#[test]
fn a_cycle_smuggled_past_set_param_badges_rather_than_overflowing() {
    // A hand-edited document (or a crafted paste) can hold a loop that
    // SetParam would have refused. The depth backstop is what turns that
    // into a message instead of a stack overflow.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "aa");
    let bn = rename(&mut e, ctx, b, "bb");
    // Written straight into the document, bypassing every command check.
    for (node, expr) in [
        (a, format!("ch(\"{bn}/width\")")),
        (b, format!("ch(\"{an}/width\")")),
    ] {
        e.doc
            .graph_mut(ctx)
            .unwrap()
            .node_mut(node)
            .unwrap()
            .params
            .insert("width".into(), ParamSource::Expression { expr });
    }
    set_display(&mut e, ctx, b);
    e.apply(Command::CookNow).unwrap();
    let events = e.cook(&mut || true);
    let errored = events.iter().any(|ev| {
        matches!(ev, EngineEvent::CookStatus { status, .. }
            if matches!(status, crate::cook::state::CookStatus::Error { .. }))
    });
    assert!(errored, "a smuggled cycle must badge, not recurse forever");
}

#[test]
fn editing_a_referenced_param_recooks_the_referrer() {
    // The whole point of the index: ch() reads document state, so there is
    // no wire to carry the change and the reader would otherwise keep a
    // stale value forever.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "source");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(2.0)),
    })
    .unwrap();
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{an}/width\") * 2"));
    set_display(&mut e, ctx, b);
    e.cook(&mut || true);
    assert!((cooked_width(&e, b) - 4.0).abs() < 1e-4, "before the edit");

    // Edit the SOURCE only. Nothing wires a to b.
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(5.0)),
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(
        (cooked_width(&e, b) - 10.0).abs() < 1e-4,
        "the referrer must recook: got {}",
        cooked_width(&e, b)
    );
}

#[test]
fn propagation_is_transitive_through_a_reference_chain() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let c = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "aa");
    let bn = rename(&mut e, ctx, b, "bb");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(1.0)),
    })
    .unwrap();
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{an}/width\") * 2"));
    set_expr(&mut e, ctx, c, "width", &format!("ch(\"{bn}/width\") * 3"));
    set_display(&mut e, ctx, c);
    e.cook(&mut || true);
    assert!((cooked_width(&e, c) - 6.0).abs() < 1e-4);

    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(2.0)),
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(
        (cooked_width(&e, c) - 12.0).abs() < 1e-4,
        "two hops must propagate: got {}",
        cooked_width(&e, c)
    );
}

#[test]
fn undo_restores_the_dependency_graph_not_just_the_text() {
    // The index is rebuilt from the document on undo, so an undone
    // reference stops propagating. A maintained index is exactly where
    // this would rot.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "box");
    let an = rename(&mut e, ctx, a, "source");
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(2.0)),
    })
    .unwrap();
    set_expr(&mut e, ctx, b, "width", &format!("ch(\"{an}/width\") * 2"));
    set_display(&mut e, ctx, b);
    e.cook(&mut || true);
    assert!((cooked_width(&e, b) - 4.0).abs() < 1e-4);

    // Undo the expression: b returns to its literal default.
    e.apply(Command::Undo).unwrap();
    e.cook(&mut || true);
    let after_undo = cooked_width(&e, b);

    // Editing the old source must no longer move b.
    e.apply(Command::SetParam {
        ctx,
        node: a,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(9.0)),
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(
        (cooked_width(&e, b) - after_undo).abs() < 1e-4,
        "an undone reference must stop propagating"
    );
}

#[test]
fn climbing_above_the_root_is_refused_by_name() {
    let mut e = engine();
    let ctx = GraphContext::Root;
    let geo = add(&mut e, ctx, "geo");
    set_expr(&mut e, ctx, geo, "uniform_scale", "ch(\"../nope\")");
    // Root has no parent; the geo world matrix falls back rather than
    // resolving, and the message is what the cook badge would carry.
    let previews = crate::previews::Previews::new();
    let refs = crate::refs::DocRefs::new(
        e.document(),
        e.registry(),
        &previews,
        ctx,
        geo,
        crate::expr::SceneTime::default(),
    );
    let err = crate::expr::ParamRefs::read(&refs, "../nope").unwrap_err();
    assert!(err.contains("no parent"), "{err}");
}

// Param-type gating: only the numeric types accept an expression.

#[test]
fn expressions_are_accepted_on_every_numeric_param_type() {
    let (mut e, ctx) = subflow_engine();
    let b = add(&mut e, ctx, "box");
    // Float and Int.
    for (key, expr) in [("width", "1 + 1"), ("width_segments", "2 * 2")] {
        e.apply(Command::SetParam {
            ctx,
            node: b,
            key: key.into(),
            value: ParamSource::Expression { expr: expr.into() },
        })
        .unwrap_or_else(|err| panic!("`{key}` should accept an expression: {err}"));
    }
    // Vec3 and Bool, on a geo container.
    let geo = add(&mut e, GraphContext::Root, "geo");
    for (key, expr) in [("translate", "set(1, 2, 3)"), ("visible", "1 > 0")] {
        e.apply(Command::SetParam {
            ctx: GraphContext::Root,
            node: geo,
            key: key.into(),
            value: ParamSource::Expression { expr: expr.into() },
        })
        .unwrap_or_else(|err| panic!("`{key}` should accept an expression: {err}"));
    }
    // Colour, on a light.
    let light = add(&mut e, GraphContext::Root, "point_light");
    e.apply(Command::SetParam {
        ctx: GraphContext::Root,
        node: light,
        key: "color".into(),
        value: ParamSource::Expression {
            expr: "set(1, 0, 0, 1)".into(),
        },
    })
    .expect("a colour accepts an expression");
}

#[test]
fn expressions_are_refused_on_text_and_menu_params() {
    // There is no string type in the value lattice, so an expression could
    // never produce anything these params could hold.
    let (mut e, ctx) = subflow_engine();
    let b = add(&mut e, ctx, "box");
    let err = e
        .apply(Command::SetParam {
            ctx,
            node: b,
            key: "name".into(),
            value: ParamSource::Expression {
                expr: "1 + 1".into(),
            },
        })
        .unwrap_err();
    assert!(
        matches!(&err, EngineError::InvalidParam { reason, .. } if reason.contains("text")),
        "{err:?}"
    );
}

#[test]
fn a_refused_expression_leaves_the_param_untouched() {
    // A command error must not half-apply: the old value stands.
    let (mut e, ctx) = subflow_engine();
    let b = add(&mut e, ctx, "box");
    let before = name_of(&e, ctx, b);
    let _ = e.apply(Command::SetParam {
        ctx,
        node: b,
        key: "name".into(),
        value: ParamSource::Expression {
            expr: "1 + 1".into(),
        },
    });
    assert_eq!(name_of(&e, ctx, b), before);
}

#[test]
fn a_parse_error_is_stored_rather_than_refused() {
    // A half-typed expression is a value the user is in the
    // middle of fixing. Refusing the keystroke would make the field
    // unusable; the node badges instead.
    let (mut e, ctx) = subflow_engine();
    let b = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node: b,
        key: "width".into(),
        value: ParamSource::Expression { expr: "1 +".into() },
    })
    .expect("a syntax error must still be storable");
    let stored = e
        .document()
        .graph(ctx)
        .unwrap()
        .node(b)
        .unwrap()
        .params
        .get("width")
        .cloned();
    assert!(matches!(stored, Some(ParamSource::Expression { .. })));
}

#[test]
fn the_drag_lane_never_carries_an_expression() {
    let (mut e, ctx) = subflow_engine();
    let b = add(&mut e, ctx, "box");
    e.preview_param(
        ctx,
        b,
        "width",
        ParamSource::Expression {
            expr: "1 + 1".into(),
        },
    );
    assert!(
        !e.has_active_previews(),
        "an expression must not enter the preview overlay"
    );
    // A literal still does.
    e.preview_param(
        ctx,
        b,
        "width",
        ParamSource::Literal(ParamValue::Float(2.0)),
    );
    assert!(e.has_active_previews());
}

// ---- F3: the runtime foundation ----------------------------------------

use crate::runtime::LoopMode;

/// A subflow holding one box whose `width` is driven by a time expression.
fn time_driven_engine() -> (Engine, GraphContext, NodeId) {
    let (mut e, ctx) = subflow_engine();
    let node = add(&mut e, ctx, "box");
    e.apply(Command::SetParam {
        ctx,
        node,
        key: "width".to_string(),
        value: ParamSource::Expression {
            expr: "1 + sin($T)".to_string(),
        },
    })
    .unwrap();
    (e, ctx, node)
}

#[test]
fn a_tick_on_a_stopped_clock_does_nothing() {
    let (mut e, _, _) = time_driven_engine();
    let before = e.revision();
    let batch = e.tick();
    assert!(batch.events.is_empty());
    assert_eq!(e.revision(), before, "a paused editor costs nothing");
}

#[test]
fn playing_advances_the_frame_one_step_per_tick() {
    let (mut e, _, _) = time_driven_engine();
    e.apply(Command::Play).unwrap();
    assert_eq!(e.clock().frame, 1);
    e.tick();
    assert_eq!(e.clock().frame, 2, "fixed step: one tick is one frame");
    e.tick();
    assert_eq!(e.clock().frame, 3);
}

#[test]
fn a_tick_emits_the_new_frame() {
    let (mut e, _, _) = time_driven_engine();
    e.apply(Command::Play).unwrap();
    let batch = e.tick();
    let frames: Vec<i64> = batch
        .events
        .iter()
        .filter_map(|ev| match ev {
            EngineEvent::FrameChanged { frame } => Some(*frame),
            _ => None,
        })
        .collect();
    assert_eq!(frames, vec![2]);
}

#[test]
fn a_scene_with_no_time_expression_dirties_nothing_on_a_tick() {
    // The whole reason the index tracks time-dependence: a static scene
    // must pay nothing per frame.
    let (mut e, ctx) = subflow_engine();
    let node = add(&mut e, ctx, "box");
    e.cook(&mut || true);
    assert_eq!(e.cook_state(node), CookState::Clean, "cooked to start with");
    assert!(!e.expr_index.has_time_dependency());

    e.apply(Command::Play).unwrap();
    e.tick();
    // Still clean: nothing time-dependent existed to dirty.
    assert_eq!(
        e.cook_state(node),
        CookState::Clean,
        "a static node must not re-cook because the clock moved"
    );
}

#[test]
fn a_time_expression_registers_as_time_dependent() {
    let (e, ctx, node) = time_driven_engine();
    assert!(e.expr_index.has_time_dependency());
    assert!(e.expr_index.time_dependent().contains(&(ctx, node)));
}

#[test]
fn a_wrangle_program_reading_time_is_time_dependent_too() {
    // The easy one to miss: a Snippet is stored as plain Text, so nothing
    // about its ParamSource says "expression".
    let (mut e, ctx) = subflow_engine();
    let node = add(&mut e, ctx, "attribute_wrangle");
    assert!(
        !e.expr_index.has_time_dependency(),
        "the default program does not read the clock"
    );
    e.apply(Command::SetParam {
        ctx,
        node,
        key: "program".to_string(),
        value: ParamSource::Literal(ParamValue::Text(
            "@P = set(@P.x, @P.y + sin($T), @P.z);".to_string(),
        )),
    })
    .unwrap();
    assert!(e.expr_index.time_dependent().contains(&(ctx, node)));
}

#[test]
fn stop_rewinds_and_reports_both_facts() {
    let (mut e, _, _) = time_driven_engine();
    e.apply(Command::Play).unwrap();
    e.tick();
    e.tick();
    let batch = e.apply(Command::Stop).unwrap();
    assert_eq!(e.clock().frame, 1);
    assert!(!e.clock().playing);
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::PlaybackChanged { playing } if !*playing))
    );
    assert!(
        batch
            .events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::FrameChanged { frame } if *frame == 1))
    );
}

#[test]
fn transport_is_not_undoable_but_settings_are() {
    let (mut e, _, _) = time_driven_engine();
    let before = e.clock().effective_range();

    // Transport: no undo entry, so undo must reach past it.
    e.apply(Command::Play).unwrap();
    e.apply(Command::SetFrame { frame: 5 }).unwrap();

    // Settings: document state, so this IS undoable.
    e.apply(Command::SetFrameRange { start: 10, end: 20 })
        .unwrap();
    assert_eq!(e.clock().effective_range(), (10, 20));
    e.apply(Command::Undo).unwrap();
    assert_eq!(
        e.clock().effective_range(),
        before,
        "the range came back, so the transport commands left no undo steps in the way"
    );
}

#[test]
fn changing_fps_retimes_because_seconds_are_derived_from_the_frame() {
    let (mut e, _, _) = time_driven_engine();
    e.apply(Command::SetFrame { frame: 24 }).unwrap();
    assert!((e.clock().scene_time().seconds - 1.0).abs() < 1e-12);
    e.apply(Command::SetFps { fps: 48.0 }).unwrap();
    assert!(
        (e.clock().scene_time().seconds - 0.5).abs() < 1e-12,
        "the same frame is half the time at twice the rate"
    );
}

#[test]
fn the_runtime_section_round_trips_and_never_carries_session_state() {
    let (mut e, _, _) = time_driven_engine();
    e.apply(Command::SetFps { fps: 30.0 }).unwrap();
    e.apply(Command::SetFrameRange { start: 5, end: 60 })
        .unwrap();
    e.apply(Command::SetLoopMode {
        mode: LoopMode::PingPong,
    })
    .unwrap();
    e.apply(Command::SetAutoplay { autoplay: true }).unwrap();
    e.apply(Command::Play).unwrap();
    e.apply(Command::SetFrame { frame: 42 }).unwrap();

    let sidecar = crate::engine::scenefile::SceneSidecar::default();
    let bytes = e.save_slxy(&sidecar).expect("saves");

    let mut loaded = engine();
    loaded.load_slxy(&bytes).expect("loads");

    assert!((loaded.clock().fps - 30.0).abs() < 1e-12);
    assert_eq!(loaded.clock().effective_range(), (5, 60));
    assert_eq!(loaded.clock().loop_mode, LoopMode::PingPong);
    assert!(loaded.clock().autoplay);
    // The reproducibility contract.
    assert!(!loaded.clock().playing, "a loaded scene is never playing");
    assert_eq!(loaded.clock().frame, 5, "and sits at the range start");
}

#[test]
fn a_scene_written_before_the_runtime_existed_reads_a_default_clock() {
    // schema_version stays 1 and the section is serde-defaulted, so a
    // pre-0.8.1 file is not a migration: it is a file without the section.
    // Built by DELETING the key rather than by saving, because a save can
    // only ever produce the current shape and would test nothing.
    let (e, _, _) = time_driven_engine();
    let sidecar = crate::engine::scenefile::SceneSidecar::default();
    let scene = crate::engine::scenefile::document_to_scene(
        &e.doc.to_data(),
        e.cook_mode,
        &e.clock.settings(),
        &sidecar,
        Vec::new(),
    );

    let mut raw: serde_json::Value = serde_json::to_value(&scene).expect("serializes");
    assert!(raw.get("runtime").is_some(), "the current shape has it");
    raw.as_object_mut().expect("object").remove("runtime");

    let old: solarxy_scenefile::SceneJson =
        serde_json::from_value(raw).expect("a file without the section still parses");
    let settings = crate::engine::scenefile::runtime_from_scene(&old);
    assert!((settings.fps - 24.0).abs() < 1e-12);
    assert_eq!((settings.frame_start, settings.frame_end), (1, 240));
    assert_eq!(settings.loop_mode, LoopMode::Loop);
    assert!(!settings.autoplay);
}

#[test]
fn an_unknown_loop_mode_falls_back_instead_of_failing_the_load() {
    // A file written by a later version must still open; a playback mode is
    // not worth refusing a document over.
    let json = serde_json::json!({
        "fps": 30.0,
        "frameStart": 2,
        "frameEnd": 50,
        "loopMode": "someFutureMode",
        "autoplay": true,
    });
    let runtime: solarxy_scenefile::RuntimeJson = serde_json::from_value(json).expect("parses");
    let scene = solarxy_scenefile::SceneJson {
        runtime,
        ..serde_json::from_str::<solarxy_scenefile::SceneJson>(
            r#"{"schema_version":1,"min_reader":1,"generator":"t","graph":{}}"#,
        )
        .expect("minimal scene")
    };
    let settings = crate::engine::scenefile::runtime_from_scene(&scene);
    assert_eq!(
        settings.loop_mode,
        LoopMode::Loop,
        "the default, not a panic"
    );
    assert!(
        (settings.fps - 30.0).abs() < 1e-12,
        "the rest still survives"
    );
}

/// A fixed, obviously-not-1970 epoch stamp: 2026-07-28T12:00:00Z.
fn fixed_epoch_ms() -> f64 {
    1_785_240_000_000.0
}

#[test]
fn node_timestamps_stay_unknown_without_a_host_epoch_clock() {
    // Native cooks, the CLI and every test run with no epoch clock. They
    // must leave the fields alone rather than write a zero that renders as
    // "1 Jan 1970".
    let (mut e, ctx) = subflow_engine();
    let id = add(&mut e, ctx, "box");
    let report = e.node_report(ctx, id).expect("node exists");
    assert_eq!(report.created_ms, None);
    assert_eq!(report.modified_ms, None);
}

#[test]
fn creating_and_editing_a_node_stamps_it() {
    let (mut e, ctx) = subflow_engine();
    e.set_epoch_clock(fixed_epoch_ms);
    let id = add(&mut e, ctx, "box");
    let created = e.node_report(ctx, id).unwrap().created_ms;
    assert_eq!(created, Some(fixed_epoch_ms()));

    e.apply(Command::SetParam {
        ctx,
        node: id,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    let after = e.node_report(ctx, id).unwrap();
    assert_eq!(after.modified_ms, Some(fixed_epoch_ms()));
    assert_eq!(after.created_ms, created, "creation time never moves");
}

#[test]
fn moving_a_node_on_the_canvas_is_not_a_modification() {
    // The rule that makes the timestamp mean something: auto-layout, or
    // tidying a graph by hand, must not restamp every node in the scene.
    let (mut e, ctx) = subflow_engine();
    e.set_epoch_clock(fixed_epoch_ms);
    let id = add(&mut e, ctx, "box");
    // Clear the creation stamp so a move is the only thing that could set it.
    e.doc
        .graph_mut(ctx)
        .unwrap()
        .node_mut(id)
        .unwrap()
        .modified_ms = None;

    e.apply(Command::MoveNodes {
        ctx,
        moves: vec![(id, [42.0, 17.0])],
    })
    .unwrap();
    assert_eq!(
        e.node_report(ctx, id).unwrap().modified_ms,
        None,
        "a canvas move must leave `modified` alone"
    );
}

#[test]
fn connecting_stamps_both_ends() {
    // A wire changes what BOTH nodes do, so both are modified.
    let (mut e, ctx) = subflow_engine();
    e.set_epoch_clock(fixed_epoch_ms);
    let src = add(&mut e, ctx, "box");
    let dst = add(&mut e, ctx, "transform");
    for id in [src, dst] {
        e.doc
            .graph_mut(ctx)
            .unwrap()
            .node_mut(id)
            .unwrap()
            .modified_ms = None;
    }
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: src,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: dst,
            port: "geometry".into(),
        },
    })
    .unwrap();
    assert_eq!(
        e.node_report(ctx, src).unwrap().modified_ms,
        Some(fixed_epoch_ms())
    );
    assert_eq!(
        e.node_report(ctx, dst).unwrap().modified_ms,
        Some(fixed_epoch_ms())
    );
}

#[test]
fn cook_totals_accumulate_across_cooks() {
    let (mut e, ctx) = subflow_engine();
    e.set_clock(tick_now);
    let id = add(&mut e, ctx, "box");
    e.cook(&mut || true);
    let first = e.node_report(ctx, id).unwrap();
    assert_eq!(first.cook_count, 1);

    e.apply(Command::SetParam {
        ctx,
        node: id,
        key: "width".into(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    e.cook(&mut || true);
    let second = e.node_report(ctx, id).unwrap();
    assert_eq!(second.cook_count, 2, "a recook counts");
    assert!(
        second.total_cook_us >= first.total_cook_us,
        "the total only grows"
    );
}

// ---- playback pacing (round 2) ----
//
// The bug these pin: the clock used to advance every tick regardless of
// whether the cook had finished, so on a scene whose cook exceeds one budget
// slice the retime re-dirtied the head of the chain before the tail was
// reached and the display node never cooked at all.

/// A cook budget that permits `n` budget CHECKS, then refuses.
///
/// Not the same as "n nodes": `cook_until` always cooks the first eligible
/// node without asking (the forward-progress rule at `driver.rs`), so
/// `budget_of(0)` still cooks exactly one node and stops.
fn budget_of(n: usize) -> impl FnMut() -> bool {
    let mut left = n;
    move || {
        let ok = left > 0;
        left = left.saturating_sub(1);
        ok
    }
}

#[test]
fn the_clock_waits_for_an_unfinished_cook() {
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    let b = add(&mut e, ctx, "transform");
    e.apply(Command::Connect {
        ctx,
        from: PortRefDto {
            node: a,
            port: "geometry".into(),
        },
        to: PortRefDto {
            node: b,
            port: "geometry".into(),
        },
    })
    .unwrap();
    e.apply(Command::SetActiveOutput { ctx, node: Some(b) })
        .unwrap();

    // Drain, then start the clock from a clean slate.
    e.cook(&mut || true);
    e.apply(Command::Play).unwrap();
    let start = e.clock().frame;

    // Dirty the chain and cook it only PARTWAY: one node cooks, the rest
    // is left over. That is exactly the budget-exhausted state.
    // One node cooks (forward progress), the second is left Dirty. That is
    // exactly the state a budget-exhausted frame is in.
    e.mark_dirty(ctx, a);
    e.cook(&mut budget_of(0));

    e.tick();
    assert_eq!(
        e.clock().frame,
        start,
        "the clock must not advance while a cook is still outstanding"
    );

    // Once the cook drains, it moves again.
    e.cook(&mut || true);
    e.tick();
    assert_eq!(
        e.clock().frame,
        start + 1,
        "a drained cook releases the clock"
    );
}

#[test]
fn pacing_cannot_deadlock_the_clock() {
    // The property that makes the gate safe: a cook always drains given
    // passes, because every exit from `cook_one` leaves a node Clean or
    // Pending and only `retime` re-dirties -- which cannot run while the
    // gate is closed. A node that ERRORS must not be an exception.
    let (mut e, ctx) = subflow_engine();
    let p = add(&mut e, ctx, "transform"); // required input, unconnected -> errors
    e.apply(Command::SetActiveOutput { ctx, node: Some(p) })
        .unwrap();

    e.cook(&mut || true);
    assert!(
        matches!(e.cook.status(p), Some(CookStatus::Error { .. })),
        "the fixture needs an erroring node to be meaningful"
    );

    e.apply(Command::Play).unwrap();
    let start = e.clock().frame;
    // Several frames of the real host loop: tick, then cook.
    for _ in 0..3 {
        e.tick();
        e.cook(&mut || true);
    }
    assert!(
        e.clock().frame > start,
        "an erroring node must not freeze the clock: a failed cook is a \
         FINISHED cook, so the dirty set still drains"
    );
}

#[test]
fn a_static_scene_still_plays_at_full_rate() {
    // Nothing time-dependent: retime dirties nothing, every cook drains
    // trivially, and the gate must never close.
    let (mut e, ctx) = subflow_engine();
    let b = add(&mut e, ctx, "box");
    e.apply(Command::SetActiveOutput { ctx, node: Some(b) })
        .unwrap();
    e.cook(&mut || true);

    e.apply(Command::Play).unwrap();
    let start = e.clock().frame;
    for _ in 0..5 {
        e.tick();
        e.cook(&mut || true);
    }
    assert_eq!(
        e.clock().frame,
        start + 5,
        "a scene with nothing to recook must advance one frame per tick"
    );
}

#[test]
fn scrubbing_is_not_gated_by_an_unfinished_cook() {
    // Pacing governs PLAYBACK. Someone dragging the playhead should land on
    // the frame they asked for, not the one the cook is ready to give.
    let (mut e, ctx) = subflow_engine();
    let a = add(&mut e, ctx, "box");
    e.apply(Command::SetActiveOutput { ctx, node: Some(a) })
        .unwrap();
    e.cook(&mut || true);

    e.mark_dirty(ctx, a);
    e.cook(&mut budget_of(0));

    e.apply(Command::SetFrame { frame: 42 }).unwrap();
    assert_eq!(e.clock().frame, 42, "an explicit seek is never gated");
}

/// A tiny synthetic 4x2 Radiance HDR file, the fixture shape the formats
/// decoder tests use.
fn tiny_hdr_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n");
    out.extend_from_slice(b"-Y 2 +X 4\n");
    for _ in 0..8 {
        out.extend_from_slice(&[128, 64, 32, 128]);
    }
    out
}

#[test]
fn an_environment_node_lowers_its_hdri_into_the_scene_delta() {
    // The headline of this feature: an HDRI chosen in the graph reaches
    // the renderer through the scene contract rather than through host
    // state, which is what makes it survive a reload at all.
    let mut e = engine();
    let ctx = GraphContext::Root;
    let env = add(&mut e, ctx, "environment");
    let asset = e.stage_asset("sky.hdr", "image/vnd.radiance", tiny_hdr_bytes());
    e.apply(Command::SetParam {
        ctx,
        node: env,
        key: "hdri".to_string(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: env,
        key: "rotation".to_string(),
        value: ParamSource::Literal(ParamValue::Float(90.0)),
    })
    .unwrap();
    e.cook(&mut || true);

    let delta = e.take_scene_delta();
    let op = delta
        .ops
        .iter()
        .find_map(|op| match op {
            solarxy_core::scene::SceneOp::SetEnvironment {
                hdri,
                rotation,
                intensity,
                background,
            } => Some((hdri, rotation, intensity, background)),
            _ => None,
        })
        .expect("the delta carries an environment op");
    let image = op.0.as_ref().expect("the decoded HDRI rides the op");
    assert_eq!((image.width, image.height), (4, 2));
    // Degrees in the param, radians on the contract.
    assert!(
        (op.1 - std::f32::consts::FRAC_PI_2).abs() < 1e-5,
        "{}",
        op.1
    );
    assert!((op.2 - 1.0).abs() < 1e-6);
    assert_eq!(*op.3, solarxy_core::scene::BackgroundKind::Keep);
}

#[test]
fn an_environment_survives_a_scene_file_round_trip() {
    // Journey J3-a's last step: close the tab, reopen the file, and the
    // lighting is exactly as you left it. Before the node existed the
    // HDRI was host state and this was impossible.
    let mut e = engine();
    let ctx = GraphContext::Root;
    let env = add(&mut e, ctx, "environment");
    let asset = e.stage_asset("sky.hdr", "image/vnd.radiance", tiny_hdr_bytes());
    e.apply(Command::SetParam {
        ctx,
        node: env,
        key: "hdri".to_string(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: env,
        key: "intensity".to_string(),
        value: ParamSource::Literal(ParamValue::Float(2.5)),
    })
    .unwrap();
    e.cook(&mut || true);
    let before = e.take_scene_delta();
    let hash_before = environment_hash(&before).expect("an environment before saving");

    let bytes = e.save_slxy(&SceneSidecar::default()).expect("save .slxy");

    let mut e2 = engine();
    let loaded = e2.load_slxy(&bytes).expect("load .slxy");
    assert!(
        loaded.warnings.is_empty(),
        "clean round-trip has no warnings: {:?}",
        loaded.warnings
    );
    assert!(
        e2.has_environment_node(),
        "the reloaded document still holds the node, which is what makes it win over the sidecar"
    );
    e2.cook(&mut || true);
    let after = e2.take_scene_delta();

    // The same bytes decode to the same content hash, so the reloaded
    // scene lights identically rather than merely similarly.
    assert_eq!(environment_hash(&after), Some(hash_before));
    let intensity = after.ops.iter().find_map(|op| match op {
        solarxy_core::scene::SceneOp::SetEnvironment { intensity, .. } => Some(*intensity),
        _ => None,
    });
    assert_eq!(intensity, Some(2.5));
}

/// The content hash of the HDRI a delta's environment op carries, if any.
fn environment_hash(delta: &solarxy_core::scene::SceneDelta) -> Option<u64> {
    delta.ops.iter().find_map(|op| match op {
        solarxy_core::scene::SceneOp::SetEnvironment { hdri, .. } => hdri.as_ref().map(|h| h.hash),
        _ => None,
    })
}

#[test]
fn deleting_the_environment_node_clears_the_environment() {
    // The op is emitted unconditionally for exactly this: absence has to
    // be communicated, or a deleted node's HDRI lights the scene forever.
    let mut e = engine();
    let ctx = GraphContext::Root;
    let env = add(&mut e, ctx, "environment");
    let asset = e.stage_asset("sky.hdr", "image/vnd.radiance", tiny_hdr_bytes());
    e.apply(Command::SetParam {
        ctx,
        node: env,
        key: "hdri".to_string(),
        value: ParamSource::Literal(ParamValue::Asset(asset)),
    })
    .unwrap();
    e.cook(&mut || true);
    assert!(environment_hash(&e.take_scene_delta()).is_some());

    e.apply(Command::RemoveNodes {
        ctx,
        ids: vec![env],
    })
    .unwrap();
    e.cook(&mut || true);
    assert_eq!(environment_hash(&e.take_scene_delta()), None);
}

#[test]
fn only_the_first_environment_node_wins() {
    // There is exactly one environment. The node's own help promises
    // document order decides, so a second node must change nothing.
    let mut e = engine();
    let ctx = GraphContext::Root;
    let first = add(&mut e, ctx, "environment");
    let second = add(&mut e, ctx, "environment");
    e.apply(Command::SetParam {
        ctx,
        node: first,
        key: "intensity".to_string(),
        value: ParamSource::Literal(ParamValue::Float(3.0)),
    })
    .unwrap();
    e.apply(Command::SetParam {
        ctx,
        node: second,
        key: "intensity".to_string(),
        value: ParamSource::Literal(ParamValue::Float(9.0)),
    })
    .unwrap();
    e.cook(&mut || true);
    let delta = e.take_scene_delta();
    let intensity = delta.ops.iter().find_map(|op| match op {
        solarxy_core::scene::SceneOp::SetEnvironment { intensity, .. } => Some(*intensity),
        _ => None,
    });
    assert_eq!(
        intensity,
        Some(3.0),
        "the first node in document order wins"
    );
}
