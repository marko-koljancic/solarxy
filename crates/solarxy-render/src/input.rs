//! Getting a document into an engine, and cooking it until it stops moving.
//!
//! # Two inputs, one document
//!
//! A scene file already is a document. A model file is turned into the
//! smallest document that renders it, through
//! [`solarxy_graph::model_document`], which the desktop's still render calls
//! too: one synthesis in the product, so the two shells answer a bare model
//! identically by construction. What this adapter owns is the filesystem
//! half the engine crate must not have: reading the file, collecting the
//! companions it names beside itself (`solarxy_formats::companions`), and
//! mapping every failure onto the command's error taxonomy.
//!
//! # Quiescence
//!
//! The cook loop lives beside the synthesis in `solarxy_graph`, for the same
//! sharing reason; the wrapper here reports each pass to the progress sink
//! and wires the interrupt flag through.

use std::path::Path;

use solarxy_graph::engine::Engine;
use solarxy_graph::model_document::{self, QuiescenceError};

use crate::error::RenderError;

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

    let invalid = |message: String| RenderError::InputInvalid {
        path: path.to_path_buf(),
        message,
    };

    if ext == "slxy" {
        let loaded = engine
            .load_slxy(&bytes)
            .map_err(|e| invalid(e.to_string()))?;
        warnings.extend(loaded.warnings);
    } else if solarxy_core::SUPPORTED_EXTENSIONS
        .iter()
        .any(|e| e.eq_ignore_ascii_case(&ext))
    {
        // Companions before the primary, so a required companion that cannot
        // be read fails as an input problem naming the file rather than
        // later, inside a cook job, as a parse failure naming something
        // else. Their warnings join the channel the scene-file branch
        // already fills, so a missing texture reaches the reader through
        // `tracing` and the JSON report by the one route.
        let companions = solarxy_formats::companions::collect(path, &ext, &bytes)
            .map_err(|e| invalid(e.to_string()))?;
        warnings.extend(companions.warnings);
        for asset in companions.assets {
            engine.stage_asset(asset.name, String::new(), asset.bytes);
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("model")
            .to_string();
        model_document::synthesize_model_document(&mut engine, &name, &ext, bytes)
            .map_err(|e| invalid(e.to_string()))?;
    } else {
        return Err(RenderError::InputUnsupported {
            path: path.to_path_buf(),
        });
    }

    cook_to_quiescence(&mut engine, cancel, sink)?;
    Ok(Loaded { engine, warnings })
}

/// Cooks to quiescence, reporting each pass to the sink.
///
/// The loop itself is [`model_document::cook_to_quiescence`]; this wrapper
/// owns the progress reporting and the interrupt flag, and maps the outcome
/// onto the command's taxonomy.
///
/// # Errors
/// A node reporting a cook error, cancellation, or the loop failing to
/// settle.
pub fn cook_to_quiescence(
    engine: &mut Engine,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    sink: &mut dyn crate::RenderSink,
) -> Result<(), RenderError> {
    let mut stopped = || cancel.is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed));
    model_document::cook_to_quiescence(engine, &mut stopped, &mut |pass, passes| {
        sink.report(&crate::RenderProgress::Cooking { pass, passes });
    })
    .map_err(|e| match e {
        QuiescenceError::Cancelled => RenderError::Cancelled,
        QuiescenceError::Cook(message) => RenderError::Cook(message),
        unsettled @ QuiescenceError::Unsettled { .. } => RenderError::Cook(unsettled.to_string()),
    })
}
