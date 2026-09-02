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
//! - **The interface palette** ([`theme::Palette`]) shared by the egui GUI,
//!   the analyze TUI, and — through `examples/gen_tokens.rs` — the web
//!   frontend's `tokens.generated.css`.
//!
//! No GPU types, no winit, no egui — depend on this crate from anywhere
//! without pulling wgpu/egui/winit into the build graph.
//!
//! # Feature flags
//!
//! - `serde`: wasm-safe serde derives + JSON — gates `preferences` (types
//!   only) and `view_config`, and enables the serde derives on validation
//!   types. No filesystem access.
//! - `fs` (implies `serde`): file IO — preferences `config_path`/`load`/
//!   `save` (dirs + toml) and `install_source`. Off for wasm builds.
//! - `serialization` (default; implies `serde` + `fs`): the historical
//!   umbrella — additionally gates `json`, `report`, `project_config`, and
//!   `review`. Disable everything for a pure-computation build — only
//!   [`aabb`], [`geometry`], and [`validation`] remain.
//! - `schemars-gen`: adds `schemars::JsonSchema` derives on the public
//!   on-disk types (used to regenerate `schemas/*.json`).
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
pub mod gizmo;
#[cfg(feature = "fs")]
pub mod install_source;
#[cfg(feature = "serialization")]
pub mod json;
#[cfg(feature = "serde")]
pub mod preferences;
#[cfg(feature = "serialization")]
pub mod project_config;
pub mod raycast;
#[cfg(feature = "serialization")]
pub mod report;
#[cfg(feature = "serialization")]
pub mod review;
pub mod scene;
pub mod theme;
pub mod validation;
#[cfg(feature = "serde")]
pub mod view_config;

pub use aabb::AABB;
pub use geometry::{
    AlphaMode, LUT_LOG_MAX_STOP, LUT_LOG_MIN_STOP, LUT_MAX_SIZE, LUT_MIN_SIZE, LutCube,
    MeshTopology, RawImageData, RawImageHdr, RawMaterialData, RawMeshData, RawModelData,
};

pub const WIKI_URL: &str = "https://github.com/marko-koljancic/solarxy/wiki";
pub use validation::{
    IssueKind, IssueScope, Severity, ValidationConfig, ValidationIssue, ValidationReport,
    ValidationResult, ValidationThresholds,
};

#[cfg(feature = "serialization")]
pub use project_config::{
    AssetCategory, Budgets, ClassifierRule, FilenameClassifier, ProjectConfig, ProjectConfigError,
    ReviewSettings, classify_compiled, discover as discover_project_config,
};

#[cfg(feature = "serialization")]
pub use review::{
    AnchorPosition, AnnotationCategory, ReviewAnnotation, ReviewError, ReviewFile, hash_bytes,
    hash_file, hash_mesh, hash_meshes, hash_positions_indices, sidecar_path_for,
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
