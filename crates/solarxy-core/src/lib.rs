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
pub mod geometry;
#[cfg(feature = "config")]
pub mod json;
#[cfg(feature = "config")]
pub mod preferences;
#[cfg(feature = "config")]
pub mod report;
pub mod validation;

pub use aabb::AABB;
pub use geometry::{RawImageData, RawMaterialData, RawMeshData, RawModelData};
pub use validation::{
    IssueKind, IssueScope, Severity, ValidationIssue, ValidationReport, ValidationResult,
};

pub const SUPPORTED_EXTENSIONS: &[&str] = &["obj", "stl", "ply", "gltf", "glb"];

pub fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }

    result
}
