//! Transient param previews: the drag lane.
//!
//! During a drag (a parameter slider, a viewport gizmo) the value changes tens
//! of times a second. Committing each step would spam the undo stack and the
//! event batch, so instead the host streams the intermediate values through
//! [`crate::Engine::preview_param`], which parks them here, dirties the node,
//! and lets the ordinary budgeted cook pick them up. On pointer-up the host
//! sends one authoritative `SetParam`, which clears the preview and produces
//! exactly one undo entry.
//!
//! The overlay is deliberately NOT the document: it produces no event, no undo
//! entry, and no write, so a cancelled drag leaves nothing behind (the host
//! calls [`crate::Engine::clear_preview`]).
//!
//! Every consumer of a node's stored params -- the cook, the scene lowering
//! (the geo's object transform, the render flags, the lights) and picking --
//! must resolve through [`effective_params`], or a drag would be invisible in
//! exactly the surface it is meant to drive.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::document::NodeId;
use crate::params::ParamSource;

/// Pending drag values, keyed by the node and param they override.
pub type Previews = BTreeMap<(NodeId, String), ParamSource>;

/// A node's stored params with any in-flight preview values laid over the top.
///
/// Borrows in the common case (nothing being dragged) and only clones for the
/// one node that is actually under the pointer.
#[must_use]
pub fn effective_params<'a>(
    previews: &Previews,
    node: NodeId,
    stored: &'a BTreeMap<String, ParamSource>,
) -> Cow<'a, BTreeMap<String, ParamSource>> {
    if previews.is_empty() {
        return Cow::Borrowed(stored);
    }
    // Previews are keyed (node, key); collect this node's without scanning the
    // whole map twice.
    let mut overlaid: Option<BTreeMap<String, ParamSource>> = None;
    for ((n, key), value) in previews {
        if *n != node {
            continue;
        }
        overlaid
            .get_or_insert_with(|| stored.clone())
            .insert(key.clone(), value.clone());
    }
    match overlaid {
        Some(map) => Cow::Owned(map),
        None => Cow::Borrowed(stored),
    }
}
