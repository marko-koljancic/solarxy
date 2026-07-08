//! The ZIP framing layer: turning a set of named byte entries into a `.slxy`
//! archive and back, plus the SHA-256 helper the integrity check uses.
//!
//! Entries are Stored (uncompressed) so the whole path is pure Rust and
//! compiles to wasm32 without a C compression backend; asset blobs (glb,
//! png) are typically already compressed, and `scene.json` is small.
//! Deflate is a later size optimization, not a v0 requirement.

use std::io::{Cursor, Read, Write};

use sha2::{Digest, Sha256};
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, ZipArchive};

use crate::SceneFileError;

/// The lowercase SHA-256 hex digest of `bytes` (the content id and the
/// integrity check).
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Writes named byte entries into a ZIP archive, in the given order. Each
/// entry is Stored (uncompressed).
pub fn zip_bytes(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, SceneFileError> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::<u8>::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in entries {
        writer.start_file(name, options)?;
        writer.write_all(bytes)?;
    }
    let cursor = writer.finish()?;
    Ok(cursor.into_inner())
}

/// Reads every entry of a ZIP archive into `(name, bytes)` pairs, preserving
/// archive order.
pub fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, SceneFileError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        // Skip directory entries (a `.slxy` has none, but be defensive).
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_string();
        let mut buf = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        file.read_to_end(&mut buf)?;
        out.push((name, buf));
    }
    Ok(out)
}
