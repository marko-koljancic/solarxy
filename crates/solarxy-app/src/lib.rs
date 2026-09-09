//! The desktop shell: a winit window, an egui interface and the shared render
//! host, over one open document held in the engine.
//!
//! The shape to know before anything else: **panels never write.** A panel
//! draws against read-only views and raises an `Intent`; one drain applies
//! the whole queue after the interface pass. That is what keeps the engine
//! the single writer. `gui/intent.rs` states the two rules a raise obeys and
//! `state/intents.rs` is the exhaustive drain.

#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::fn_params_excessive_bools,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::used_underscore_binding,
    clippy::wildcard_imports
)]

mod app;
pub mod console;
pub mod gui;
mod state;

pub use app::run_viewer;
pub use solarxy_core::SUPPORTED_EXTENSIONS;
