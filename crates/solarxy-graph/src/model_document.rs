//! The smallest document that renders a bare model, and the loop that cooks
//! a document until it stops moving.
//!
//! # One synthesis, shared by every shell
//!
//! A scene file already is a document. A model file is turned into a geometry
//! container holding one import node, flagged as what to display: the
//! smallest thing that is a real document rather than a special case, which
//! is what lets it enter the identical render path. The terminal's render
//! command and the desktop's still render both build it here, so the two
//! shells answer a bare model identically by construction rather than by
//! care.
//!
//! This module takes bytes and a file name, never a path: the crate has no
//! filesystem dependency and must keep none, so the caller does the reading.
//! The files a model names beside itself are the caller's to collect too
//! (natively through `solarxy_formats::companions`) and to stage before the
//! synthesis, so the import's parse can resolve them from the asset table.
//!
//! # Quiescence is not one cook
//!
//! [`Engine::cook`] is resumable and budget-bounded, and it queues
//! asynchronous work rather than doing it: an import parses in a job, and
//! the node downstream of it cannot cook until that job comes back. A single
//! pass therefore leaves an imported model absent, silently, and the
//! engine's own sample-scene tests do not catch it because every bundled
//! sample is fully parametric. So [`cook_to_quiescence`] alternates cooking
//! with draining jobs, and stops only when a pass produces neither.
//! Natively the jobs resolve synchronously, which is what makes this a loop
//! rather than an event system.

use crate::cook::state::CookStatus;
use crate::document::{GraphContext, NodeId};
use crate::engine::{Command, Engine, EngineEvent, EventBatch};
use crate::params::{ParamSource, ParamValue};

/// The most passes the cook loop will make before calling it stuck.
///
/// Generous: a deep chain of imports resolves one job layer per pass, and the
/// cost of a wasted pass is nothing. It exists so a cyclic wedge fails with a
/// message rather than hanging a caller forever.
const MAX_COOK_PASSES: usize = 256;

/// Why a model could not become a document.
#[derive(Debug, thiserror::Error)]
pub enum ModelDocumentError {
    #[error("nothing imports a .{0}")]
    UnsupportedExtension(String),
    /// An engine command failed, or did not produce the node it should have.
    #[error("{0}")]
    Engine(String),
}

/// Why a document did not cook to quiescence.
#[derive(Debug, thiserror::Error)]
pub enum QuiescenceError {
    #[error("cancelled")]
    Cancelled,
    /// One or more nodes reported a cook error, joined with "; ".
    #[error("{0}")]
    Cook(String),
    #[error("the scene was still changing after {passes} cook passes")]
    Unsettled { passes: usize },
}

/// Builds the one-node document a bare model renders through.
///
/// A geometry container with an import inside it, displayed. `file_name` is
/// the asset's staged name (the model's own file name); `ext` its lowercased
/// extension, which picks the import node type. Companions must already be
/// staged by the caller.
///
/// # Errors
/// An extension nothing imports, or an engine command failing.
pub fn synthesize_model_document(
    engine: &mut Engine,
    file_name: &str,
    ext: &str,
    bytes: Vec<u8>,
) -> Result<(), ModelDocumentError> {
    let node_type = import_type_for(ext)
        .ok_or_else(|| ModelDocumentError::UnsupportedExtension(ext.to_string()))?;
    let asset = engine.stage_asset(file_name.to_string(), String::new(), bytes);

    let engine_err = |e: &dyn std::fmt::Display| ModelDocumentError::Engine(e.to_string());

    let geo = added_node(
        &engine
            .apply(Command::AddNode {
                ctx: GraphContext::Root,
                node_type: "geo".to_string(),
                position: [0.0, 0.0],
            })
            .map_err(|e| engine_err(&e))?,
    )
    .ok_or_else(|| ModelDocumentError::Engine("the geometry container was not created".into()))?;

    let inner = GraphContext::Subflow(geo);
    let import = added_node(
        &engine
            .apply(Command::AddNode {
                ctx: inner,
                node_type: node_type.to_string(),
                position: [0.0, 0.0],
            })
            .map_err(|e| engine_err(&e))?,
    )
    .ok_or_else(|| ModelDocumentError::Engine("the import node was not created".into()))?;

    engine
        .apply(Command::SetParam {
            ctx: inner,
            node: import,
            key: "file".to_string(),
            value: ParamSource::Literal(ParamValue::Asset(asset)),
        })
        .map_err(|e| engine_err(&e))?;
    engine
        .apply(Command::SetActiveOutput {
            ctx: inner,
            node: Some(import),
        })
        .map_err(|e| engine_err(&e))?;

    Ok(())
}

/// Which import node reads a given extension.
///
/// One node type per format rather than one that sniffs, which is the
/// registry's own arrangement: each declares the options its format actually
/// has. So the adapter picks, and a format the registry gains an importer for
/// needs a line here before a shell can open it as a document.
#[must_use]
pub fn import_type_for(ext: &str) -> Option<&'static str> {
    match ext {
        "obj" => Some("import_obj"),
        "gltf" | "glb" => Some("import_gltf"),
        "stl" => Some("import_stl"),
        "ply" => Some("import_ply"),
        _ => None,
    }
}

/// The id of the node an `AddNode` batch reports as added.
fn added_node(batch: &EventBatch) -> Option<NodeId> {
    batch.events.iter().find_map(|e| match e {
        EngineEvent::NodeAdded { node, .. } => Some(node.id),
        _ => None,
    })
}

/// Cooks, drains jobs, and repeats until a pass does neither.
///
/// `on_pass` is told each pass as it starts, one-based, with the cap;
/// `cancelled` is also handed to the engine's own cook so a long cook stops
/// between nodes rather than at the end of one. Jobs are resolved on this
/// thread, which is the engine's documented native path.
///
/// # Errors
/// Cancellation, a node reporting a cook error, or the loop failing to
/// settle.
pub fn cook_to_quiescence(
    engine: &mut Engine,
    cancelled: &mut dyn FnMut() -> bool,
    on_pass: &mut dyn FnMut(u32, u32),
) -> Result<(), QuiescenceError> {
    let mut failures: Vec<String> = Vec::new();
    for pass in 0..MAX_COOK_PASSES {
        on_pass(
            u32::try_from(pass).unwrap_or(u32::MAX).saturating_add(1),
            u32::try_from(MAX_COOK_PASSES).unwrap_or(u32::MAX),
        );
        if cancelled() {
            return Err(QuiescenceError::Cancelled);
        }
        let events = engine.cook(&mut || !cancelled());
        collect_failures(&events, &mut failures);

        let jobs = engine.take_jobs();
        let resolved = !jobs.is_empty();
        for (ctx, id, request) in jobs {
            let result = engine.resolve_job(&request);
            let events = engine.submit_job_result(ctx, id, result);
            collect_failures(&events, &mut failures);
        }

        if !failures.is_empty() {
            return Err(QuiescenceError::Cook(failures.join("; ")));
        }
        if events.is_empty() && !resolved {
            return Ok(());
        }
    }
    Err(QuiescenceError::Unsettled {
        passes: MAX_COOK_PASSES,
    })
}

/// Pulls cook errors out of an event batch.
///
/// The engine reports a failed node as a status rather than as an error
/// return, because a shell wants to keep running and badge the node. A
/// caller cooking to quiescence does not: there is nobody to see the badge,
/// and the result would be of a scene that did not build.
fn collect_failures(events: &[EngineEvent], into: &mut Vec<String>) {
    for event in events {
        if let EngineEvent::CookStatus {
            node,
            status: CookStatus::Error { message },
        } = event
        {
            into.push(format!("node {}: {message}", node.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The end to end: a bare OBJ becomes a cooked document whose snapshot
    /// carries its triangle. This is the path both the terminal's render
    /// command and the desktop's still render stand on.
    #[test]
    fn a_bare_model_synthesizes_and_cooks_to_a_scene() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec();
        let mut engine = Engine::new().expect("engine");
        synthesize_model_document(&mut engine, "tri.obj", "obj", obj).expect("synthesis");
        let mut passes = 0u32;
        cook_to_quiescence(&mut engine, &mut || false, &mut |_, _| passes += 1)
            .expect("quiescence");
        assert!(passes >= 2, "an import needs a job pass, got {passes}");
        let delta = engine.scene_snapshot();
        assert!(
            delta
                .ops
                .iter()
                .any(|op| matches!(op, solarxy_core::scene::SceneOp::UpsertGeometry { .. })),
            "the cooked snapshot carries no geometry"
        );
    }

    #[test]
    fn an_extension_nothing_imports_is_refused_by_name() {
        let mut engine = Engine::new().expect("engine");
        let err = synthesize_model_document(&mut engine, "notes.txt", "txt", Vec::new())
            .expect_err("txt is not a model");
        assert!(err.to_string().contains(".txt"), "{err}");
    }

    #[test]
    fn cancellation_wins_over_cooking() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec();
        let mut engine = Engine::new().expect("engine");
        synthesize_model_document(&mut engine, "tri.obj", "obj", obj).expect("synthesis");
        let err = cook_to_quiescence(&mut engine, &mut || true, &mut |_, _| {})
            .expect_err("a cancelled cook must not report success");
        assert!(matches!(err, QuiescenceError::Cancelled));
    }
}
