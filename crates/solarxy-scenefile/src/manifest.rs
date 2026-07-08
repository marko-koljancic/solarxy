//! The `manifest.json` schema: the byte-level asset index of a `.slxy`
//! archive. Where `scene.json`'s `assets[]` carries semantic records (role,
//! import settings), the manifest carries the physical records (size, mime,
//! content hash, and the archive path), keyed by the same content hash.

use serde::{Deserialize, Serialize};

/// The archive manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ManifestJson {
    /// Mirrors `scene.json`'s `schema_version` (a fast pre-open check).
    pub schema_version: u32,
    /// The writing tool + version.
    pub generator: String,
    /// ISO-8601 write time supplied by the host (empty when unknown).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    #[serde(default)]
    pub assets: Vec<AssetManifestEntry>,
}

/// One embedded asset's physical record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct AssetManifestEntry {
    /// The asset id (the content hash today).
    pub id: String,
    /// The original file name (display + sidecar resolution).
    pub name: String,
    /// The reported MIME type (may be empty).
    #[serde(default)]
    pub mime: String,
    /// The blob size in bytes.
    pub size: u64,
    /// The SHA-256 hex digest of the bytes (the integrity check on read).
    pub sha256: String,
    /// The archive path, always `assets/<sha256>`.
    pub path: String,
}
