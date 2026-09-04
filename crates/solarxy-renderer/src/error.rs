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
    /// LDR texture decode failed (`image` crate). High-dynamic-range
    /// decode failures arrive as `Formats` instead: `.hdr` and `.exr` are
    /// read by `solarxy-formats`, not here.
    #[error("image decode error: {0}")]
    Image(#[from] image::ImageError),
    /// Model or HDRI parsing failed in `solarxy-formats`.
    #[error(transparent)]
    Formats(#[from] solarxy_formats::FormatsError),
    /// Unsupported input (e.g. a malformed prepared-HDRI worker blob).
    #[error("{0}")]
    Unsupported(String),
    /// A mesh's GPU buffers exceed what the device can allocate.
    ///
    /// `bytes` is the allocation rather than the payload: buffers are
    /// created with growth headroom, so the figure a person needs to see
    /// is the one that was actually refused. Raised before any buffer is
    /// created, which is what leaves the previous scene intact.
    #[error(
        "mesh {mesh} needs {} for its {what}, and this device permits at most {}",
        crate::limits::format_bytes(*bytes),
        crate::limits::format_bytes(*limit)
    )]
    MeshTooLarge {
        mesh: String,
        what: &'static str,
        bytes: u64,
        limit: u64,
    },
}
