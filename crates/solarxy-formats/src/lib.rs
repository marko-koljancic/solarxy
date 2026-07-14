//! Format loaders (OBJ, STL, PLY, glTF/GLB) producing
//! [`solarxy_core::RawModelData`].
//!
//! The API is byte-first: `load_*_bytes` functions parse in-memory slices and
//! resolve sidecar assets (`.mtl` files, `.bin` buffers, textures) through an
//! [`AssetResolver`], so they run anywhere — including wasm, where there is
//! no filesystem. The path-based loaders ([`load_model`] and the per-format
//! `load_*` functions) are thin filesystem wrappers behind the default-on
//! `std-fs` feature.
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

/// Errors produced by the format loaders.
#[derive(Debug, thiserror::Error)]
pub enum FormatsError {
    /// Filesystem read failed (path-based loaders only).
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// OBJ/MTL parse failure.
    #[error("OBJ parse error: {0}")]
    Obj(#[from] tobj::LoadError),
    /// STL parse failure.
    #[error("STL parse error: {0}")]
    Stl(#[source] std::io::Error),
    /// PLY parse failure.
    #[error("PLY parse error: {0}")]
    Ply(#[source] std::io::Error),
    /// glTF/GLB parse or import failure.
    #[error("glTF error: {0}")]
    Gltf(#[from] ::gltf::Error),
    /// A required external asset (buffer, MTL) was not provided by the
    /// [`AssetResolver`]. The message is user-facing (import error badges
    /// and toasts), so it says what to do about it.
    #[error(
        "missing external asset '{0}': the model references this companion \
         file; import or place it alongside the model"
    )]
    MissingAsset(String),
    /// Structurally valid file with unusable content (no geometry, missing
    /// required elements).
    #[error("{0}")]
    Invalid(String),
}

/// Supplies the bytes of sidecar assets referenced by a model file:
/// `.mtl` material libraries, `.bin` glTF buffers, and textures.
///
/// Paths are the reference strings as they appear in the model file
/// (percent-decoded for glTF URIs), relative to wherever the model came
/// from. Return `None` when the asset cannot be provided; loaders degrade
/// per format (missing MTL means default materials, missing glTF buffers
/// are an error).
pub trait AssetResolver {
    fn read(&mut self, rel_path: &str) -> Option<Vec<u8>>;
}

/// An [`AssetResolver`] that has nothing: every lookup returns `None`.
/// For self-contained formats (STL, PLY, GLB) and tests.
pub struct NoAssets;

impl AssetResolver for NoAssets {
    fn read(&mut self, _rel_path: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Filesystem-backed [`AssetResolver`] rooted at a base directory —
/// the resolver the path-based loaders use.
#[cfg(feature = "std-fs")]
pub struct DirResolver {
    base: std::path::PathBuf,
}

#[cfg(feature = "std-fs")]
impl DirResolver {
    pub fn new(base: impl Into<std::path::PathBuf>) -> Self {
        Self { base: base.into() }
    }
}

#[cfg(feature = "std-fs")]
impl AssetResolver for DirResolver {
    fn read(&mut self, rel_path: &str) -> Option<Vec<u8>> {
        // `rel_path` is attacker-controlled: it is a raw reference out of a model
        // file (an OBJ `mtllib`/`map_Kd`, a glTF buffer/image `uri`). Joining it
        // unchecked lets a malicious model read arbitrary local files:
        // `../../../../etc/passwd` escapes the base, and an absolute path makes
        // `PathBuf::join` discard the base entirely. On the desktop viewer that
        // is arbitrary local-file read; for the `solarxy-validate` library run
        // server-side over uploaded models it is an LFI / file-existence oracle.
        //
        // A companion always sits beside (or below) the model it belongs to, so
        // a legitimate reference never needs `..`, a root, or a drive prefix.
        // Reject all three, then confirm the resolved path is still contained by
        // the base after canonicalization (defeats symlink escapes too).
        let rel = std::path::Path::new(rel_path);
        if rel.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        }) {
            return None;
        }
        let base = self.base.canonicalize().ok()?;
        let full = base.join(rel).canonicalize().ok()?;
        if !full.starts_with(&base) {
            return None;
        }
        std::fs::read(&full).ok()
    }
}

/// Decode an encoded image (PNG, JPEG, WebP, ...) into RGBA8
/// [`RawImageData`]. The shared decode path for every texture that enters
/// through bytes: OBJ `map_Kd` sidecars, glTF external images, and the
/// `import_image` node's native cook.
pub fn decode_image_bytes(bytes: &[u8]) -> Result<RawImageData, FormatsError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| FormatsError::Invalid(format!("image decode failed: {e}")))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(RawImageData::new(rgba.into_raw(), width, height))
}

/// Parse a model from bytes, dispatching on the (lowercase, dot-free)
/// extension exactly like [`load_model`] does on paths. `name` seeds mesh
/// names for formats that carry none (STL, PLY); `resolver` supplies
/// sidecars for OBJ and glTF.
pub fn load_model_bytes(
    bytes: &[u8],
    ext: &str,
    name: &str,
    resolver: &mut dyn AssetResolver,
) -> Result<RawModelData, FormatsError> {
    match ext.to_ascii_lowercase().as_str() {
        "stl" => stl::load_stl_bytes(bytes, name),
        "ply" => ply::load_ply_bytes(bytes, name),
        "gltf" | "glb" => gltf::load_gltf_bytes(bytes, resolver),
        _ => obj::load_obj_bytes(bytes, resolver),
    }
}

/// Load a model from a filesystem path, dispatching on extension.
/// OBJ is the fallback for unknown extensions (historical behavior).
#[cfg(feature = "std-fs")]
pub fn load_model(path: &str) -> Result<RawModelData, FormatsError> {
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

#[cfg(feature = "std-fs")]
pub(crate) fn read_file(path: &str) -> Result<Vec<u8>, FormatsError> {
    std::fs::read(path).map_err(|source| FormatsError::Io {
        path: path.to_string(),
        source,
    })
}
