//! Shared plumbing for the texture-context image nodes: port
//! constructors, the working-resolution cap, and the input accessor every
//! image cook funnels through.

use std::sync::Arc;

use solarxy_core::RawImageData;

use crate::cook::{CookError, Inputs, Outputs};
use crate::registry::PortSpec;
use crate::registry::coerce::{DataType, Value};

/// The working-resolution cap (long edge, pixels): every image op clamps
/// its inputs here so a large source cannot hitch the single-threaded wasm
/// cook.
///
/// This cap is terminal today: `image_export` encodes the node's
/// already-cooked, working-resolution output (`engine/mod.rs`, the
/// `("image_export", "save")` arm), so a source larger than this edge still
/// exports at this edge. A full-resolution re-cook on export is a
/// later-milestone item, not built here.
pub const WORKING_EDGE: u32 = 2048;

/// The default single Image input. Carries the generic port doc; a node
/// whose input means something more specific than "the image to operate
/// on" (a height field, a base layer) chains its own `.doc(...)` over it.
pub fn image_in(required: bool) -> PortSpec {
    PortSpec::single("image", "Image", DataType::Image, required)
        .default_port()
        .doc(format!(
            "The image to operate on. Being the default input, a drag from \
             an upstream node's body wires here, and dropping this node on \
             an existing wire splices it in. Whatever arrives is clamped to \
             the working resolution ({WORKING_EDGE} px on the long edge) \
             before the operator runs, so a large source cannot stall the \
             cook."
        ))
}

/// The default Image output.
pub fn image_out() -> PortSpec {
    PortSpec::single("image", "Image", DataType::Image, false)
        .default_port()
        .doc(format!(
            "The resulting image: RGBA8, and never more than {WORKING_EDGE} \
             px on the long edge, because that is the resolution the \
             texture context cooks at. Being the default output, a drag \
             from the node's body wires from here."
        ))
}

/// One image output under the catalog's default key.
pub fn image_outputs(img: RawImageData) -> Outputs {
    Outputs::single("image", Value::Image(Arc::new(img)))
}

/// The connected image on `key`, clamped to the working resolution
/// (an in-bounds image rides by refcount bump, never a copy).
pub fn working_image(inputs: &Inputs, key: &str) -> Option<Arc<RawImageData>> {
    let img = inputs.image(key)?;
    if img.width.max(img.height) <= WORKING_EDGE {
        Some(Arc::clone(img))
    } else {
        Some(Arc::new(solarxy_imaging::clamp_to_edge(img, WORKING_EDGE)))
    }
}

/// The required default image input, as a cook error when the wire is
/// connected but the upstream has no committed value yet.
pub fn require_image(inputs: &Inputs) -> Result<Arc<RawImageData>, CookError> {
    working_image(inputs, "image").ok_or_else(|| CookError::Failed {
        message: "no image on the input yet".to_string(),
    })
}
