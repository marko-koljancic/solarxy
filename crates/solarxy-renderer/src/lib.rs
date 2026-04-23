#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
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
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unreadable_literal,
    clippy::used_underscore_binding,
    clippy::wildcard_imports
)]

pub mod bind_groups;
pub mod bloom;
pub mod camera;
pub mod camera_state;
pub mod composite;
pub mod frame;
pub mod geometry;
pub mod ibl;
pub mod light;
pub mod material;
pub mod model;
pub mod pipeline_builder;
pub mod pipelines;
pub mod resources;
pub mod scene;
pub mod shadow;
pub mod ssao;
pub mod texture;
pub mod uv_camera;
pub mod validation;
pub mod visualization;
