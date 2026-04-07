#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::fn_params_excessive_bools,
    clippy::many_single_char_names,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::pub_underscore_fields,
    clippy::return_self_not_must_use,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unreadable_literal,
    clippy::used_underscore_binding,
    clippy::wildcard_imports
)]

pub mod aabb;
#[cfg(feature = "viewer")]
mod app;
#[cfg(any(feature = "viewer", feature = "analyzer"))]
pub mod cgi;
pub mod preferences;
#[cfg(feature = "viewer")]
mod state;
#[cfg(any(feature = "viewer", feature = "analyzer"))]
pub mod validation;

pub use solarxy_core::format_number;
pub use solarxy_core::SUPPORTED_EXTENSIONS;

#[cfg(feature = "viewer")]
pub use app::run_viewer;
