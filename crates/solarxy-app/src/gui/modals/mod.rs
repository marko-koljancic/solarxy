//! The dialogs, one module each.
//!
//! A modal owns its own state on the renderer and is drained by the state
//! layer through a `take_*` accessor rather than through the intent queue: a
//! single-value handle is already the right shape for one, and a queue would
//! be indirection with no criterion behind it.

pub(in crate::gui) mod about;
pub(in crate::gui) mod preferences;
pub(in crate::gui) mod screenshot;
pub(in crate::gui) mod shortcuts;
pub(in crate::gui) mod still;
pub(crate) mod unsaved;
pub(in crate::gui) mod update;
