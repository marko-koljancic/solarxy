//! Shared plumbing for the texture-context image nodes (phase 19): port
//! constructors, the working-resolution cap, and the input accessor every
//! image cook funnels through.

use std::sync::Arc;

use solarxy_core::RawImageData;

use crate::cook::{CookError, Inputs, Outputs};
use crate::registry::PortSpec;
use crate::registry::coerce::{DataType, Value};

/// The working-resolution cap (long edge, pixels): every image op clamps
/// its inputs here so a 4K source cannot hitch the single-threaded wasm
/// cook (decision C-5). Full-resolution evaluation is the export node's
/// re-cook concern (phase 21).
pub const WORKING_EDGE: u32 = 2048;

/// The default single Image input.
pub fn image_in(required: bool) -> PortSpec {
    PortSpec::single("image", "Image", DataType::Image, required).default_port()
}

/// The default Image output.
pub fn image_out() -> PortSpec {
    PortSpec::single("image", "Image", DataType::Image, false).default_port()
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
