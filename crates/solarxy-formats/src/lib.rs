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

pub mod gltf;
pub mod obj;
pub mod ply;
pub mod stl;

pub use solarxy_core::{RawImageData, RawMaterialData, RawMeshData, RawModelData};

pub fn load_model(path: &str) -> anyhow::Result<RawModelData> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "stl" => stl::load_stl(path),
        "ply" => ply::load_ply(path),
        "gltf" | "glb" => gltf::load_gltf(path),
        _ => obj::load_obj(path),
    }
}
