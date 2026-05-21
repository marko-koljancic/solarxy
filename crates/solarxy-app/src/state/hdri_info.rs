//! [`HdriInfo`]: descriptive metadata for the loaded HDRI, shown in the
//! Properties panel's HDRI section. Built when an HDRI finishes loading
//! (`state/update.rs`) and stored on the GUI side (`EguiRenderer`),
//! mirroring how [`crate::gui`]'s `ModelInfo` is handled.

/// Metadata for the currently-loaded HDRI environment map.
#[derive(Debug, Clone)]
pub(crate) struct HdriInfo {
    /// File name (no directory).
    pub filename: String,
    /// Full path as displayed to the user.
    pub path: String,
    /// Equirectangular source resolution, `(width, height)` in pixels.
    pub resolution: (u32, u32),
    /// On-disk file size in bytes.
    pub file_size: u64,
}
