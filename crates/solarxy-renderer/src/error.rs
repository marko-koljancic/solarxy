//! The renderer's public error type. Library convention: `thiserror` here,
//! `anyhow` only in binary shells.

/// Errors surfaced by renderer resource loading and GPU upload paths.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    /// Filesystem read failed (path-based entry points only).
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// LDR/HDR image decode failed (`image` crate).
    #[error("image decode error: {0}")]
    Image(#[from] image::ImageError),
    /// EXR decode failed.
    #[error("EXR decode error: {0}")]
    Exr(#[from] exr::error::Error),
    /// Model parsing failed in `solarxy-formats`.
    #[error(transparent)]
    Formats(#[from] solarxy_formats::FormatsError),
    /// Unsupported input (e.g. an HDRI extension that is neither .hdr nor
    /// .exr).
    #[error("{0}")]
    Unsupported(String),
}
