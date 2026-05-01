//! GPU-free CPU-side type home for the Solarxy workspace.
//!
//! Foundation crate shared by `solarxy-renderer`, `solarxy-app`, and
//! `solarxy-cli`:
//!
//! - **Geometry primitives** ([`AABB`], [`geometry::compute_normals`],
//!   [`geometry::compute_tangent_basis`]) used by every loader and the
//!   renderer.
//! - **The raw model I/O type** ([`RawModelData`]) that loaders in
//!   `solarxy-formats` produce and the renderer consumes.
//! - **Validation** ([`validation::validate_raw_model`], [`ValidationReport`])
//!   shared by the CLI's `analyze` mode and the GUI's validation overlay.
//! - **Preferences** (`preferences::Preferences`, plus cycle-able enums like
//!   `preferences::IblMode`) loaded from `~/.config/solarxy/config.toml` via
//!   `preferences::load`.
//! - **Reporting** (`report::AnalysisReport`, `json::report_to_json`).
//!
//! No GPU types, no winit, no egui — depend on this crate from anywhere
//! without pulling wgpu/egui/winit into the build graph.
//!
//! # Feature flags
//!
//! - `serialization` (default): gates `preferences`, `json`, `report`,
//!   `install_source`, and `view_config`. Disable for a pure-computation
//!   build — only [`aabb`], [`geometry`], and [`validation`] remain.
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
#[cfg(feature = "serialization")]
pub mod install_source;
#[cfg(feature = "serialization")]
pub mod json;
#[cfg(feature = "serialization")]
pub mod preferences;
#[cfg(feature = "serialization")]
pub mod report;
pub mod validation;
#[cfg(feature = "serialization")]
pub mod view_config;

pub use aabb::AABB;
pub use geometry::{AlphaMode, RawImageData, RawMaterialData, RawMeshData, RawModelData};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_number_boundaries() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(1), "1");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1001), "1,001");
        assert_eq!(format_number(1_234_567), "1,234,567");
        assert_eq!(format_number(1_000_000_000), "1,000,000,000");
    }
}
