//! Getting a document into an engine, and cooking it until it stops moving.
//!
//! # Two inputs, one document
//!
//! A scene file already is a document. A model file is turned into the smallest
//! document that renders it: a geometry container holding one import node,
//! flagged as what to display. Both then leave here as an `Engine` that has
//! cooked, and everything downstream is written once.
//!
//! The alternative was a second render path for bare models, which is two sets
//! of bugs, two answers to every question about lighting and framing, and two
//! places to fix anything found in either.
//!
//! # Quiescence is not one cook
//!
//! `cook` is resumable and budget-bounded, and it queues asynchronous work
//! rather than doing it: an import parses in a job, and the node downstream of
//! it cannot cook until that job comes back. A single pass therefore leaves an
//! imported model absent, silently, and the engine's own sample-scene tests do
//! not catch it because every bundled sample is fully parametric.
//!
//! So the loop below alternates cooking with draining jobs, and stops only when
//! a pass produces neither. Natively the jobs resolve synchronously, which is
//! what makes this a loop rather than an event system.

use std::path::Path;

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::{Command, Engine, EngineEvent};
use solarxy_graph::params::{ParamSource, ParamValue};

use crate::error::RenderError;

/// The most passes the cook loop will make before calling it stuck.
///
/// Generous: a deep chain of imports resolves one job layer per pass, and the
/// cost of a wasted pass is nothing. It exists so a cyclic wedge fails with a
/// message rather than hanging a build agent forever.
const MAX_COOK_PASSES: usize = 256;

/// A loaded, cooked document plus what loading it had to say.
pub struct Loaded {
    pub engine: Engine,
    pub warnings: Vec<String>,
}

/// Loads whatever `path` is and cooks it to quiescence.
///
/// # Errors
/// The file being absent, unreadable, of an unknown kind, failing to parse, or
/// a node failing to cook.
pub fn load(
    path: &Path,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    sink: &mut dyn crate::RenderSink,
) -> Result<Loaded, RenderError> {
    sink.report(&crate::RenderProgress::Loading);
    if !path.exists() {
        return Err(RenderError::InputMissing(path.to_path_buf()));
    }
    let bytes = std::fs::read(path).map_err(|source| RenderError::InputUnreadable {
        path: path.to_path_buf(),
        source,
    })?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut engine = Engine::new().map_err(|e| RenderError::InputInvalid {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let mut warnings = Vec::new();

    if ext == "slxy" {
        let loaded = engine
            .load_slxy(&bytes)
            .map_err(|e| RenderError::InputInvalid {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
        warnings.extend(loaded.warnings);
    } else if solarxy_core::SUPPORTED_EXTENSIONS
        .iter()
        .any(|e| e.eq_ignore_ascii_case(&ext))
    {
        synthesize_model_document(&mut engine, path, &ext, bytes)?;
    } else {
        return Err(RenderError::InputUnsupported {
            path: path.to_path_buf(),
        });
    }

    cook_to_quiescence(&mut engine, cancel, sink)?;
    Ok(Loaded { engine, warnings })
}

/// Builds the one-node document a bare model renders through.
///
/// A geometry container with an import inside it, displayed. That is the
/// smallest thing that is a real document rather than a special case, which is
/// what lets it enter the identical render path.
fn synthesize_model_document(
    engine: &mut Engine,
    path: &Path,
    ext: &str,
    bytes: Vec<u8>,
) -> Result<(), RenderError> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model")
        .to_string();
    let asset = engine.stage_asset(name, String::new(), bytes);

    let invalid = |message: String| RenderError::InputInvalid {
        path: path.to_path_buf(),
        message,
    };

    let geo = added_node(
        &engine
            .apply(Command::AddNode {
                ctx: GraphContext::Root,
                node_type: "geo".to_string(),
                position: [0.0, 0.0],
            })
            .map_err(|e| invalid(e.to_string()))?,
    )
    .ok_or_else(|| invalid("the geometry container was not created".into()))?;

    let inner = GraphContext::Subflow(geo);
    let import = added_node(
        &engine
            .apply(Command::AddNode {
                ctx: inner,
                node_type: import_type_for(ext)
                    .ok_or_else(|| invalid(format!("nothing imports a .{ext}")))?
                    .to_string(),
                position: [0.0, 0.0],
            })
            .map_err(|e| invalid(e.to_string()))?,
    )
    .ok_or_else(|| invalid("the import node was not created".into()))?;

    engine
        .apply(Command::SetParam {
            ctx: inner,
            node: import,
            key: "file".to_string(),
            value: ParamSource::Literal(ParamValue::Asset(asset)),
        })
        .map_err(|e| invalid(e.to_string()))?;
    engine
        .apply(Command::SetActiveOutput {
            ctx: inner,
            node: Some(import),
        })
        .map_err(|e| invalid(e.to_string()))?;

    Ok(())
}

/// Which import node reads a given extension.
///
/// One node type per format rather than one that sniffs, which is the registry's
/// own arrangement: each declares the options its format actually has. So the
/// adapter picks, and a format the registry gains an importer for needs a line
/// here before the command can open it.
fn import_type_for(ext: &str) -> Option<&'static str> {
    match ext {
        "obj" => Some("import_obj"),
        "gltf" | "glb" => Some("import_gltf"),
        "stl" => Some("import_stl"),
        "ply" => Some("import_ply"),
        _ => None,
    }
}

/// The id of the node an `AddNode` batch reports as added.
fn added_node(batch: &solarxy_graph::engine::EventBatch) -> Option<NodeId> {
    batch.events.iter().find_map(|e| match e {
        EngineEvent::NodeAdded { node, .. } => Some(node.id),
        _ => None,
    })
}

/// Cooks, drains jobs, and repeats until a pass does neither.
///
/// # Errors
/// A node reporting a cook error, or the loop failing to settle.
pub fn cook_to_quiescence(
    engine: &mut Engine,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    sink: &mut dyn crate::RenderSink,
) -> Result<(), RenderError> {
    let stopped = || cancel.is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed));
    let mut failures: Vec<String> = Vec::new();
    for pass in 0..MAX_COOK_PASSES {
        sink.report(&crate::RenderProgress::Cooking {
            pass: u32::try_from(pass).unwrap_or(u32::MAX).saturating_add(1),
            passes: u32::try_from(MAX_COOK_PASSES).unwrap_or(u32::MAX),
        });
        if stopped() {
            return Err(RenderError::Cancelled);
        }
        // The engine's own cancellation hook, which its documentation offers a
        // native caller for exactly this. A long cook stops between nodes
        // rather than at the end of it.
        let events = engine.cook(&mut || !stopped());
        collect_failures(&events, &mut failures);

        // Natively there is no worker: a job is resolved on this thread and
        // handed straight back, which is what the engine's own documentation
        // says the native path is for.
        let jobs = engine.take_jobs();
        let resolved = !jobs.is_empty();
        for (ctx, id, request) in jobs {
            let result = engine.resolve_job(&request);
            let events = engine.submit_job_result(ctx, id, result);
            collect_failures(&events, &mut failures);
        }

        if !failures.is_empty() {
            return Err(RenderError::Cook(failures.join("; ")));
        }
        if events.is_empty() && !resolved {
            return Ok(());
        }
    }
    Err(RenderError::Cook(format!(
        "the scene was still changing after {MAX_COOK_PASSES} cook passes"
    )))
}

/// Pulls cook errors out of an event batch.
///
/// The engine reports a failed node as a status rather than as an error return,
/// because a shell wants to keep running and badge the node. A render does not:
/// there is nobody to see the badge, and the image would be of a scene that did
/// not build.
fn collect_failures(events: &[EngineEvent], into: &mut Vec<String>) {
    use solarxy_graph::cook::state::CookStatus;
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
