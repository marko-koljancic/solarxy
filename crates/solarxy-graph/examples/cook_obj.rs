//! The native smoke tool: builds a box -> transform -> merge graph
//! through the direct document/cook API, cooks it, and writes the merged
//! result as OBJ to stdout (or to a path argument).
//!
//! Usage: `cargo run -p solarxy-graph --example cook_obj [out.obj]`
//!
//! This is the phase-3 exit-criterion proof that the kernel + document +
//! registry + cook core produce valid geometry end to end without any UI.

use std::fmt::Write as _;

use solarxy_graph::assets::AssetTable;
use solarxy_graph::builtin_registry;
use solarxy_graph::cook::CookEngine;
use solarxy_graph::cook::state::CookStatus;
use solarxy_graph::document::{ContextKind, Document, Edge, GraphContext, NodeData, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::registry::coerce::Value;
use solarxy_kernel::GeometrySet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = builtin_registry()?;
    let mut doc = Document::new();
    let mut engine = CookEngine::new();

    // One subflow to work in (owned by a synthetic geo container id).
    let geo = doc.mint_node_id();
    doc.create_subflow(geo, ContextKind::Geo);
    let ctx = GraphContext::Subflow(geo);

    // Helpers over the direct API.
    let add = |doc: &mut Document, engine: &mut CookEngine, type_id: &str| -> NodeId {
        let id = doc.mint_node_id();
        let g = doc.graph_mut(ctx).unwrap();
        g.add_node(NodeData::new(id, type_id, 1));
        engine.insert_node(id);
        engine.mark_dirty(doc.graph(ctx).unwrap(), id);
        id
    };

    // box -> transform (shift +X by 2) ; box2 ; both -> merge.
    let box_a = add(&mut doc, &mut engine, "box");
    let box_b = add(&mut doc, &mut engine, "box");
    let xform = add(&mut doc, &mut engine, "transform");
    let merge = add(&mut doc, &mut engine, "merge");

    // Shift the transform's box to the right so the two boxes are distinct.
    {
        let g = doc.graph_mut(ctx).unwrap();
        g.node_mut(xform).unwrap().params.insert(
            "translate".to_string(),
            ParamSource::Literal(ParamValue::Vec3([2.0, 0.0, 0.0])),
        );
    }

    // Wire: box_a -> transform.geometry ; transform -> merge.inputs ;
    //       box_b -> merge.inputs.
    connect(
        &mut doc,
        &mut engine,
        ctx,
        box_a,
        "geometry",
        xform,
        "geometry",
        false,
    );
    connect(
        &mut doc,
        &mut engine,
        ctx,
        xform,
        "geometry",
        merge,
        "inputs",
        true,
    );
    connect(
        &mut doc,
        &mut engine,
        ctx,
        box_b,
        "geometry",
        merge,
        "inputs",
        true,
    );

    // Display the merge node so its cone cooks.
    doc.graph_mut(ctx).unwrap().active_output = Some(merge);

    // Cook to completion (unbounded budget).
    let report = engine.cook_until(
        &doc,
        &registry,
        &AssetTable::new(),
        &solarxy_graph::previews::Previews::new(),
        ctx,
        &mut || true,
    );
    for (node, status) in &report.status_changed {
        if let CookStatus::Error { message } = status {
            return Err(format!("node {node:?} failed to cook: {message}").into());
        }
    }

    // Pull the merged geometry off the display node.
    let outputs = engine.outputs(merge).ok_or("merge produced no output")?;
    let Some(Value::Geometry(set)) = outputs.get("geometry") else {
        return Err("merge output is not geometry".into());
    };

    let obj = to_obj(set);
    match std::env::args().nth(1) {
        Some(path) => {
            std::fs::write(&path, obj)?;
            eprintln!(
                "wrote {} ({} verts, {} tris) to {path}",
                set.mesh_count(),
                set.point_count(),
                set.triangle_count()
            );
        }
        None => print!("{obj}"),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn connect(
    doc: &mut Document,
    engine: &mut CookEngine,
    ctx: GraphContext,
    from: NodeId,
    from_port: &str,
    to: NodeId,
    to_port: &str,
    to_variadic: bool,
) {
    let eid = doc.mint_edge_id();
    let g = doc.graph_mut(ctx).unwrap();
    g.connect(
        Edge {
            id: eid,
            from,
            from_port: from_port.to_string(),
            to,
            to_port: to_port.to_string(),
        },
        to_variadic,
    )
    .expect("smoke-tool wiring is valid");
    engine.mark_dirty(doc.graph(ctx).unwrap(), to);
}

/// Minimal Wavefront OBJ writer over a merged geometry set. Emits one
/// group per mesh; positions, normals (when present), and 1-based faces.
fn to_obj(set: &GeometrySet) -> String {
    let mut out = String::new();
    out.push_str("# solarxy-graph cook_obj smoke output\n");
    let mut vertex_base: u32 = 1;
    for mesh in &set.meshes {
        let _ = writeln!(out, "g {}", mesh.name);
        for p in mesh.positions.iter() {
            let _ = writeln!(out, "v {} {} {}", p[0], p[1], p[2]);
        }
        let has_normals = mesh.normals.is_some();
        if let Some(normals) = &mesh.normals {
            for n in normals.iter() {
                let _ = writeln!(out, "vn {} {} {}", n[0], n[1], n[2]);
            }
        }
        for tri in mesh.indices.chunks_exact(3) {
            let (a, b, c) = (
                vertex_base + tri[0],
                vertex_base + tri[1],
                vertex_base + tri[2],
            );
            if has_normals {
                let _ = writeln!(out, "f {a}//{a} {b}//{b} {c}//{c}");
            } else {
                let _ = writeln!(out, "f {a} {b} {c}");
            }
        }
        vertex_base += mesh.positions.len() as u32;
    }
    out
}
